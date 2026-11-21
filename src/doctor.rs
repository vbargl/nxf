//! Checks that nxf can work on this machine, and says how to fix what not.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::output::Mark;
use crate::status::system_commit;
use crate::{env, flake, git, nixos, output, state};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

pub struct Check {
    pub level: Level,
    pub name: &'static str,
    pub message: String,
}

pub fn check(level: Level, name: &'static str, message: String) -> Check {
    Check {
        level,
        name,
        message,
    }
}

fn on_path(tool: &str) -> bool {
    env::var(env::PATH)
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

fn writable(path: &Path) -> bool {
    // SAFETY: the path is a C string that lives through the call.
    CString::new(path.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}

fn user() -> String {
    env::text(env::USER).unwrap_or_else(|| "you".to_string())
}

pub fn tools() -> Check {
    let missing: Vec<&str> = ["nix", "nh", "git", "dix"]
        .into_iter()
        .filter(|tool| !on_path(tool))
        .collect();
    if missing.is_empty() {
        check(
            Level::Ok,
            "tools",
            "nix, nh, git and dix are available".to_string(),
        )
    } else {
        check(
            Level::Fail,
            "tools",
            format!(
                "{} not found; install nxf as a package to get them",
                missing.join(", ")
            ),
        )
    }
}

/// Checks of the checkout, ending at the first one later checks depend on.
pub fn checkout(location: &flake::Location) -> Vec<Check> {
    let dir = &location.dir;
    let shown = dir.display();
    let origin = if location.source == "default" {
        format!("default; set {} to change", env::FLAKE)
    } else {
        format!("from {}", env::FLAKE)
    };
    if !dir.join("flake.nix").exists() {
        return vec![check(
            Level::Fail,
            "flake",
            format!("{shown} has no flake.nix ({origin})"),
        )];
    }
    let mut results = vec![check(Level::Ok, "flake", format!("{shown} ({origin})"))];
    if !git::is_repo(dir) {
        results.push(check(
            Level::Fail,
            "git",
            format!("{shown} is not a Git repository with a commit; nxf reads its history"),
        ));
        return results;
    }
    results.push(check(
        Level::Ok,
        "git",
        format!("at {}", git::commit_line(dir, "HEAD")),
    ));
    results.push(match flake::lock(dir) {
        Ok(lock) => check(
            Level::Ok,
            "lockfile",
            format!("{} root inputs", flake::root_inputs(&lock).len()),
        ),
        Err(_) => check(
            Level::Fail,
            "lockfile",
            format!("{shown}/flake.lock is missing; run 'nix flake lock' there"),
        ),
    });
    results.push(if writable(dir) && writable(&dir.join(".git")) {
        check(
            Level::Ok,
            "writable",
            format!("{shown} is writable by {}", user()),
        )
    } else {
        check(
            Level::Fail,
            "writable",
            format!(
                "{shown} is not writable by {}; update cannot commit the lockfile",
                user()
            ),
        )
    });
    results.push(match git::identity(dir) {
        Some(identity) => check(Level::Ok, "identity", format!("commits as {identity}")),
        None => check(
            Level::Fail,
            "identity",
            "Git has no identity to commit with; set user.name and user.email".to_string(),
        ),
    });
    results.push(if git::clean_tree(dir).is_some() {
        check(Level::Ok, "changes", "no uncommitted changes".to_string())
    } else {
        check(
            Level::Warn,
            "changes",
            "uncommitted changes; builds of them are not remembered for status".to_string(),
        )
    });
    results.push(match flake::configurations(dir) {
        Err(_) => check(
            Level::Fail,
            "config",
            "cannot evaluate nixosConfigurations of the flake".to_string(),
        ),
        Ok(names) => match flake::choose(&names) {
            Ok(name) if name == flake::CONFIGURATION => {
                check(Level::Ok, "config", format!("nixosConfigurations.{name}"))
            }
            Ok(name) => check(
                Level::Ok,
                "config",
                format!("nixosConfigurations.{name}, the only configuration"),
            ),
            Err(_) => check(
                Level::Fail,
                "config",
                format!(
                    "{} configurations ({}) and none is named {}; nxf works with one",
                    names.len(),
                    names.join(", "),
                    flake::CONFIGURATION
                ),
            ),
        },
    });
    results
}

fn state() -> Check {
    let dir = state::dir();
    // The state directory may not exist yet; its nearest existing parent decides.
    let mut existing: PathBuf = dir.clone();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent.to_path_buf(),
            None => break,
        }
    }
    if state::writable_directory(&existing) {
        check(
            Level::Ok,
            "state",
            format!(
                "result links, source roots and read markers in {}",
                dir.display()
            ),
        )
    } else {
        check(
            Level::Fail,
            "state",
            format!(
                "{} is not writable/searchable; per-user state cannot be saved",
                existing.display()
            ),
        )
    }
}

fn records() -> Check {
    let shared = state::shared_records();
    let destination = state::record_dir();
    if shared.exists() && !state::writable_directory(&shared) {
        let effect = if destination == shared {
            "build records cannot be saved".to_string()
        } else {
            format!("build records fall back to {}", destination.display())
        };
        check(
            Level::Warn,
            "records",
            format!("{} is not writable/searchable by this process; {effect}; check permissions and active nxf group membership (a new login may be needed)", shared.display()),
        )
    } else {
        check(
            Level::Ok,
            "records",
            format!("build records go to {}", destination.display()),
        )
    }
}

fn running(dir: Option<&Path>) -> Check {
    let Some(dir) = dir else {
        return check(
            Level::Warn,
            "running",
            "skipped until the checkout checks pass".to_string(),
        );
    };
    let tree = git::clean_tree(dir);
    match system_commit(dir, tree.as_ref(), &nixos::running(), &state::remembered()) {
        Some(commit) => check(
            Level::Ok,
            "running",
            format!("built from flake commit {}", git::commit_line(dir, &commit)),
        ),
        None => check(
            Level::Warn,
            "running",
            "not built by nxf or the updater, so status cannot name its commit; run 'nxf switch' once"
                .to_string(),
        ),
    }
}

/// Runs all checks; a failure makes the doctor exit non-zero.
fn checks() -> Vec<Check> {
    let checkout = checkout(&flake::location());
    let passed = checkout.iter().all(|result| result.level != Level::Fail);
    let dir = passed.then(|| flake::location().dir);
    let mut results = vec![tools()];
    results.extend(checkout);
    results.push(state());
    results.push(records());
    results.push(running(dir.as_deref()));
    results
}

/// Prints one row for each check. Only the state has colour; what passed is
/// supporting text, what did not stands out.
pub fn print(results: &[Check]) {
    for result in results {
        let (mark, message) = match result.level {
            Level::Ok => (Mark::Done, output::dim(&result.message)),
            Level::Warn => (Mark::Attention, result.message.clone()),
            Level::Fail => (Mark::Failed, result.message.clone()),
        };
        output::row(
            mark,
            &format!("{:<width$}{message}", result.name, width = output::LABEL),
        );
    }
}

fn counted(count: usize, noun: &str) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {noun}{plural}")
}

/// The one outcome of the checks: `Err` when a check failed.
pub fn verdict(results: &[Check]) -> Result<String, String> {
    let count = |level: Level| {
        results
            .iter()
            .filter(|result| result.level == level)
            .count()
    };
    let (failed, warned) = (count(Level::Fail), count(Level::Warn));
    let warnings = if warned > 0 {
        format!(", {}", counted(warned, "warning"))
    } else {
        String::new()
    };
    if failed > 0 {
        Err(format!("{} failed{warnings}", counted(failed, "check")))
    } else if warned > 0 {
        Ok(format!("Checks passed{warnings}"))
    } else {
        Ok("All checks passed".to_string())
    }
}

pub fn report() -> Result<(), String> {
    let results = checks();
    print(&results);
    output::success(&verdict(&results)?);
    Ok(())
}
