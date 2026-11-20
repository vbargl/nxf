//! Helpers for managing NixOS: building and activating with nh, reporting on
//! systems, and remembering which source built which system.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::output;

pub const UPDATE_SERVICE: &str = "nxf-auto-update.service";
pub const DEFAULT_UPDATER_STATE: &str = "/var/lib/nxf";

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn expand(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The running system. NXF_SYSTEM stands in for it in tests.
pub fn running() -> PathBuf {
    expand(&var("NXF_SYSTEM").unwrap_or_else(|| PathBuf::from("/run/current-system")))
}

/// The system the machine booted. NXF_BOOTED stands in for it in tests.
pub fn booted() -> PathBuf {
    var("NXF_BOOTED").unwrap_or_else(|| PathBuf::from("/run/booted-system"))
}

/// Whether the running system has another kernel, initrd or kernel modules
/// than the booted one, which only a reboot brings into use.
pub fn reboot_required() -> bool {
    let (booted, running) = (booted(), running());
    ["kernel", "initrd", "kernel-modules"].iter().any(|part| {
        match (
            std::fs::canonicalize(booted.join(part)),
            std::fs::canonicalize(running.join(part)),
        ) {
            (Ok(before), Ok(now)) => before != now,
            _ => false,
        }
    })
}

/// The host name nh selects the configuration by.
pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_string())
        .unwrap_or_default()
}

pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Makes what nxf creates writable by the group when the directory is: the
/// group shares the checkout, and the next member must be able to change the
/// lockfile and the Git objects this run writes. Returns whether it did.
pub fn share_with_group(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let shared = std::fs::metadata(dir).is_ok_and(|meta| meta.permissions().mode() & 0o020 != 0);
    if shared {
        unsafe { libc::umask(0o002) };
    }
    shared
}

/// Per-user state: result links, source roots, fallback records and read markers.
pub fn state_dir() -> PathBuf {
    var("NXF_STATE").unwrap_or_else(|| {
        var("XDG_STATE_HOME")
            .unwrap_or_else(|| var("HOME").unwrap_or_default().join(".local/state"))
            .join("nxf")
    })
}

/// The automatic updater's state. NXF_UPDATER_STATE stands in for it in tests.
pub fn updater_dir() -> PathBuf {
    var("NXF_UPDATER_STATE").unwrap_or_else(|| PathBuf::from(DEFAULT_UPDATER_STATE))
}

/// Whether this process can create and replace entries in a directory.
pub fn writable_directory(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    path.is_dir()
        && CString::new(path.as_os_str().as_bytes())
            .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) == 0 })
}

/// Use shared records only when their directory has already been provisioned.
pub fn record_dir() -> PathBuf {
    let shared = updater_dir().join("systems");
    if writable_directory(&shared) {
        shared
    } else {
        state_dir().join("systems")
    }
}

/// Runs nh on a flake installable or an already built store path.
fn nh(action: &str, installable: &[&str]) -> Result<PathBuf, String> {
    let state = state_dir();
    std::fs::create_dir_all(&state)
        .map_err(|err| format!("cannot create {}: {err}", state.display()))?;
    let result = state.join("result");
    let mut nh = Command::new("nh");
    nh.args(["os", action])
        .args(installable)
        .args(["--no-nom", "--diff", "never", "--quiet"])
        .args(["--log-format", "internal-json", "--print-build-logs"])
        .arg("--out-link")
        .arg(&result);
    if is_root() {
        // nh elevates by itself and refuses root unless told otherwise.
        nh.env("NH_BYPASS_ROOT_CHECK", "true")
            .env("NH_ELEVATION_STRATEGY", "none");
    }
    output::stream(&mut nh, true, &[])?;
    Ok(expand(&result))
}

/// Builds the flake. The `#` distinguishes its directory from a store path.
pub fn build(flake: &Path, configuration: &str) -> Result<PathBuf, String> {
    nh(
        "build",
        &[
            &format!("{}#", flake.display()),
            "--hostname",
            configuration,
        ],
    )
}

/// Runs test, boot or switch on a built system.
pub fn activate(action: &str, system: &Path) -> Result<PathBuf, String> {
    nh(action, &[&system.to_string_lossy()])
}

/// Whether dix takes the two systems after the word `diff`, as it does from
/// dix 3. An unreadable version counts as a current one.
fn dix_has_commands() -> bool {
    let version = Command::new("dix")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let major = version
        .rsplit(' ')
        .next()
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.trim().parse::<u32>().ok());
    major.is_none_or(|major| major >= 3)
}

/// The command printing the full package diff between two systems.
pub fn diff(old: &Path, new: &Path) -> Command {
    let mut dix = Command::new("dix");
    if dix_has_commands() {
        dix.arg("diff");
    }
    dix.arg(old).arg(new);
    dix
}

/// The heading of a group of dix, such as "CHANGED (12)" or, before dix 3,
/// "CHANGED": its word, and the count where dix gives one.
fn heading(line: &str) -> Option<(&str, Option<usize>)> {
    let (word, count) = match line.split_once(" (") {
        Some((word, count)) => (word, Some(count.strip_suffix(')')?.parse().ok()?)),
        None => (line, None),
    };
    let valid = !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_uppercase());
    valid.then_some((word, count))
}

/// The groups of a dix diff as "12 changed". Without a count in the heading
/// the packages listed under it are counted.
fn counts(text: &str) -> Vec<String> {
    let mut groups: Vec<(&str, usize, bool)> = Vec::new();
    for line in text.lines() {
        if let Some((word, count)) = heading(line) {
            groups.push((word, count.unwrap_or(0), count.is_none()));
        } else if line.starts_with('[') {
            if let Some((_, count, true)) = groups.last_mut() {
                *count += 1;
            }
        }
    }
    groups
        .iter()
        .filter(|(_, count, _)| *count > 0)
        .map(|(word, count, _)| format!("{count} {}", word.to_lowercase()))
        .collect()
}

/// What changes between two systems, as labelled facts: the changed, added
/// and removed package counts, and the closure size.
pub fn changes(old: &Path, new: &Path) -> Result<Vec<(&'static str, String)>, String> {
    let output = diff(old, new)
        .arg("--color=never")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run dix: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let counts = counts(&text);
    let mut facts = vec![(
        "Packages",
        if counts.is_empty() {
            "no changes".to_string()
        } else {
            counts.join(", ")
        },
    )];
    if let Some(size) = text.lines().find_map(|line| line.strip_prefix("SIZE:")) {
        facts.push(("Size", size.trim().replace("->", "→")));
    }
    Ok(facts)
}

/// The closure size of a system in bytes.
pub fn size(system: &Path) -> Result<u64, String> {
    let output = Command::new("nix")
        .args([
            "path-info",
            "--json",
            "--json-format",
            "1",
            "--closure-size",
        ])
        .arg(system)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run nix: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let info: Value = serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())?;
    let first = match &info {
        Value::Object(paths) => paths.values().next(),
        Value::Array(paths) => paths.first(),
        _ => None,
    };
    first
        .and_then(|path| path.get("closureSize"))
        .and_then(Value::as_u64)
        .ok_or_else(|| "nix path-info reported no closure size".to_string())
}

/// The task results the updater saved last, with their time.
fn saved_update() -> Result<Option<Value>, String> {
    let file = updater_dir().join("update.json");
    if !file.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file)
        .map_err(|err| format!("cannot read {}: {err}", file.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|err| format!("cannot parse {}: {err}", file.display()))
}

/// The command of systemd. NXF_SYSTEMCTL stands in for it in tests.
pub fn systemctl() -> PathBuf {
    var("NXF_SYSTEMCTL").unwrap_or_else(|| PathBuf::from("systemctl"))
}

/// How and when the update service last ended, if it did not succeed.
fn service_failure() -> Option<Value> {
    let mut child = Command::new(systemctl())
        .args(["show", UPDATE_SERVICE])
        .args([
            "--property=Result,ExecMainExitTimestamp",
            "--timestamp=unix",
        ])
        .arg("--no-pager")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Bounded: an unavailable system manager must not hold status up.
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let shown = child.wait_with_output().ok()?;
    if !shown.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&shown.stdout);
    let property = |name: &str| {
        text.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    };
    let result = property("Result").unwrap_or_else(|| "success".to_string());
    let ended = property("ExecMainExitTimestamp")?;
    let time: u64 = ended.strip_prefix('@')?.parse().ok()?;
    (result != "success")
        .then(|| json!({"service": UPDATE_SERVICE, "result": result, "time": time}))
}

/// The latest automatic update: the saved task results, and the service's
/// failure if it ended after them, as when it was killed before saving. The id
/// changes whenever either does. None when there is nothing to report.
pub fn update_result() -> Result<Option<Value>, String> {
    let update = saved_update()?;
    let saved_at = update
        .as_ref()
        .and_then(|update| update.get("time"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let failure = service_failure()
        .filter(|failure| failure.get("time").and_then(Value::as_u64).unwrap_or(0) > saved_at);
    if update.is_none() && failure.is_none() {
        return Ok(None);
    }
    let mut result = json!({"update": update, "failure": failure});
    let id = Sha256::digest(result.to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    result["id"] = Value::String(id);
    Ok(Some(result))
}

fn bad_mark(tree: &str) -> PathBuf {
    updater_dir().join("bad").join(tree)
}

/// Marks the source with the given key as unhealthy, with the reason.
pub fn mark_bad(tree: &str, reason: &str) -> Result<(), String> {
    let mark = bad_mark(tree);
    let dir = updater_dir().join("bad");
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&mark, format!("{reason}\n")))
        .map_err(|err| format!("cannot write {}: {err}", mark.display()))
}

/// Why the source with the given key is marked as unhealthy, if it is.
pub fn bad(tree: &str) -> Option<String> {
    std::fs::read_to_string(bad_mark(tree))
        .ok()
        .map(|reason| reason.trim().to_string())
}

fn write_record(systems: &Path, key: &str, system: &Path) -> Result<(), String> {
    let link = systems.join(key);
    let temporary = systems.join(format!(".{key}.{}", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let result = std::os::unix::fs::symlink(system, &temporary)
        .and_then(|()| std::fs::rename(&temporary, &link))
        .map_err(|err| format!("cannot write {}: {err}", link.display()));
    let _ = std::fs::remove_file(&temporary);
    result
}

/// Publishes a plain link, not a GC root. Metadata failures never fail a build.
/// Only this caller's legacy records are imported, without replacing shared ones.
pub fn remember(key: &str, system: &Path) {
    let shared = updater_dir().join("systems");
    let local = state_dir().join("systems");
    if shared.exists() {
        let result = if writable_directory(&shared) {
            write_record(&shared, key, system)
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
                        if let Err(err) =
                            std::os::unix::fs::symlink(record.system, shared.join(&record.key))
                        {
                            if err.kind() != std::io::ErrorKind::AlreadyExists {
                                output::warning(&format!(
                                    "cannot import build record {}: {err}",
                                    record.key
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
        .and_then(|()| write_record(&local, key, system));
    if let Err(err) = result {
        output::warning(&format!("build record was not saved: {err}"));
    }
}

/// Keeps the sources of the locked inputs in the Nix store, as GC roots in
/// the state, so that the next build needs no access to their origin. They
/// replace those of the build before, which stay when these cannot be kept.
pub fn keep_sources(paths: &[PathBuf]) -> Result<(), String> {
    let sources = state_dir().join("sources");
    std::fs::create_dir_all(&sources)
        .map_err(|err| format!("cannot create {}: {err}", sources.display()))?;
    if !paths.is_empty() {
        // Nix replaces the roots of the same name, and makes none when it fails.
        let output = Command::new("nix")
            .arg("build")
            .args(paths)
            .arg("--out-link")
            .arg(sources.join("input"))
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("cannot run nix: {err}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
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
    pub key: String,
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
                key: entry.file_name().to_string_lossy().into_owned(),
                system,
            })
        })
        .collect();
    records.sort_by(|a, b| a.key.cmp(&b.key));
    records
}

/// Shared records take precedence over this caller's legacy/fallback records.
pub fn remembered() -> Vec<Remembered> {
    let mut all = records(&updater_dir().join("systems"));
    for record in records(&state_dir().join("systems")) {
        if !all.iter().any(|known| known.key == record.key) {
            all.push(record);
        }
    }
    all
}
