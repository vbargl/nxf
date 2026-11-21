//! The state nxf keeps: the per-user state, the updater's state that the
//! users of a machine share, the records of what built which system, the
//! marks of unhealthy sources and the file of the updater's results.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{env, output, process};

/// The default place of the updater's state, and where an installation puts it.
pub const UPDATER: &str = "/var/lib/nxf";
/// The directory of the records of the systems that were built, by the tree of their source.
pub const SYSTEMS: &str = "systems";
/// The file of the results the updater saved last.
const UPDATE: &str = "update.json";
/// The mark of a machine that nxf installed, in the updater's state.
pub const INSTALLED: &str = "installed.json";
/// The link to the system that the last build made, in the per-user state.
const RESULT: &str = "result";
/// The directory of the source roots, in the per-user state.
const SOURCES: &str = "sources";
/// The file of the hold on automatic updates, in the updater's state.
const HOLD: &str = "hold.json";
/// The marker of the last update summary that this user has read.
const READ: &str = "update-read";
/// The directory of the marks of the sources that failed their health check.
const BAD: &str = "bad";

/// The hash of a committed Git tree. A build record and a mark of an
/// unhealthy source are named by it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tree(String);

impl Tree {
    pub fn new(hash: impl Into<String>) -> Tree {
        Tree(hash.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Tree {
    fn fmt(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}

/// Makes what nxf creates writable by the group when the directory is: the
/// group shares the checkout, and the next member must be able to change the
/// lockfile and the Git objects this run writes. Returns whether it did.
pub fn share_with_group(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let shared = std::fs::metadata(dir).is_ok_and(|meta| meta.permissions().mode() & 0o020 != 0);
    if shared {
        // SAFETY: umask takes a number and cannot fail. The mask belongs to the
        // process, so this must run before any thread is started.
        unsafe { libc::umask(0o002) };
    }
    shared
}

/// Per-user state: result links, source roots, fallback records and read markers.
pub fn dir() -> PathBuf {
    env::var(env::STATE).unwrap_or_else(|| {
        env::var(env::XDG_STATE_HOME)
            .unwrap_or_else(|| env::var(env::HOME).unwrap_or_default().join(".local/state"))
            .join("nxf")
    })
}

/// The automatic updater's state. NXF_UPDATER_STATE stands in for it in tests.
pub fn updater_dir() -> PathBuf {
    env::path(env::UPDATER_STATE, UPDATER)
}

/// Whether this process can create and replace entries in a directory.
pub fn writable_directory(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: the path is a C string that lives through the call.
    path.is_dir()
        && CString::new(path.as_os_str().as_bytes())
            .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) == 0 })
}

/// The records of the systems that the updater and the users of the machine share.
pub fn shared_records() -> PathBuf {
    updater_dir().join(SYSTEMS)
}

/// The records of this user.
fn local_records() -> PathBuf {
    dir().join(SYSTEMS)
}

/// Use shared records only when their directory has already been provisioned.
pub fn record_dir() -> PathBuf {
    let shared = shared_records();
    if writable_directory(&shared) {
        shared
    } else {
        local_records()
    }
}

/// The link to the system that the last build made.
pub fn result_link() -> PathBuf {
    dir().join(RESULT)
}

/// The file of the hold on automatic updates.
pub fn hold_file() -> PathBuf {
    updater_dir().join(HOLD)
}

/// The marker of the last update summary that this user has read.
pub fn read_marker() -> PathBuf {
    dir().join(READ)
}

/// The file of the results the updater saved last, in its state directory.
pub fn update_file(dir: &Path) -> PathBuf {
    dir.join(UPDATE)
}

fn bad_dir() -> PathBuf {
    updater_dir().join(BAD)
}

/// Marks the source with the given tree as unhealthy, with the reason.
pub fn mark_bad(tree: &Tree, reason: &str) -> Result<(), String> {
    let mark = bad_dir().join(tree.as_str());
    std::fs::create_dir_all(bad_dir())
        .and_then(|()| std::fs::write(&mark, format!("{reason}\n")))
        .map_err(|err| format!("cannot write {}: {err}", mark.display()))
}

/// Why the source with the given tree is marked as unhealthy, if it is.
pub fn bad(tree: &Tree) -> Option<String> {
    std::fs::read_to_string(bad_dir().join(tree.as_str()))
        .ok()
        .map(|reason| reason.trim().to_string())
}

fn write_record(systems: &Path, tree: &Tree, system: &Path) -> Result<(), String> {
    let link = systems.join(tree.as_str());
    let temporary = systems.join(format!(".{tree}.{}", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let result = std::os::unix::fs::symlink(system, &temporary)
        .and_then(|()| std::fs::rename(&temporary, &link))
        .map_err(|err| format!("cannot write {}: {err}", link.display()));
    let _ = std::fs::remove_file(&temporary);
    result
}

/// Publishes a plain link, not a GC root. Metadata failures never fail a build.
/// Only this caller's legacy records are imported, without replacing shared ones.
pub fn remember(tree: &Tree, system: &Path) {
    let (shared, local) = (shared_records(), local_records());
    if shared.exists() {
        let result = if writable_directory(&shared) {
            write_record(&shared, tree, system)
        } else {
            Err(format!(
                "{} is not writable/searchable by this process",
                shared.display()
            ))
        };
        match result {
            Ok(()) => {
                if local != shared {
                    for record in records(&local) {
                        // Creating the final symlink is atomic and refuses any existing entry,
                        // including a dangling link. Never replace a newer shared record.
                        if let Err(err) = std::os::unix::fs::symlink(
                            record.system,
                            shared.join(record.tree.as_str()),
                        ) {
                            if err.kind() != std::io::ErrorKind::AlreadyExists {
                                output::warning(&format!(
                                    "cannot import build record {}: {err}",
                                    record.tree
                                ));
                            }
                        }
                    }
                }
                return;
            }
            Err(err) => {
                output::warning(&format!("{err}; shared build records unavailable; check permissions and active nxf group membership."));
                if local == shared {
                    return;
                }
            }
        }
    }
    let result = std::fs::create_dir_all(&local)
        .map_err(|err| format!("cannot create {}: {err}", local.display()))
        .and_then(|()| write_record(&local, tree, system));
    if let Err(err) = result {
        output::warning(&format!("build record was not saved: {err}"));
    }
}

/// Keeps the sources of the locked inputs in the Nix store, as GC roots in
/// the state, so that the next build needs no access to their origin. They
/// replace those of the build before, which stay when these cannot be kept.
pub fn keep_sources(paths: &[PathBuf]) -> Result<(), String> {
    let sources = dir().join(SOURCES);
    std::fs::create_dir_all(&sources)
        .map_err(|err| format!("cannot create {}: {err}", sources.display()))?;
    if !paths.is_empty() {
        // Nix replaces the roots of the same name, and makes none when it fails.
        process::run(
            Command::new("nix")
                .arg("build")
                .args(paths)
                .arg("--out-link")
                .arg(sources.join("input")),
        )?;
    }
    for root in std::fs::read_dir(&sources).into_iter().flatten().flatten() {
        if std::fs::read_link(root.path()).is_ok_and(|source| !paths.contains(&source)) {
            let _ = std::fs::remove_file(root.path());
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remembered {
    pub tree: Tree,
    pub system: PathBuf,
}

fn records(systems: &Path) -> Vec<Remembered> {
    let Ok(entries) = std::fs::read_dir(systems) else {
        return Vec::new();
    };
    let mut records: Vec<Remembered> = entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_symlink() {
                return None;
            }
            let system = std::fs::canonicalize(entry.path()).ok()?;
            (system.starts_with("/nix/store") && system.is_dir()).then(|| Remembered {
                tree: Tree::new(entry.file_name().to_string_lossy()),
                system,
            })
        })
        .collect();
    records.sort_by(|a, b| a.tree.cmp(&b.tree));
    records
}

/// Shared records take precedence over this caller's legacy/fallback records.
pub fn remembered() -> Vec<Remembered> {
    let mut all = records(&shared_records());
    for record in records(&local_records()) {
        if !all.iter().any(|known| known.tree == record.tree) {
            all.push(record);
        }
    }
    all
}
