//! Checks that nxf can work on this machine, and says how to fix what not.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::status::system_commit;
use crate::{flake, git, nixos, output};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Fail => "fail",
        }
    }
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
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

fn writable(path: &Path) -> bool {
    CString::new(path.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}

fn user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "you".to_string())
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
pub fn checkout(location: &flake::Location, host: &str) -> Vec<Check> {
    let dir = &location.dir;
    let shown = dir.display();
    let origin = if location.source == "default" {
        "default; set NXF_FLAKE to change"
    } else {
        "from NXF_FLAKE"
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
            "host",
            "cannot evaluate nixosConfigurations of the flake".to_string(),
        ),
        Ok(hosts) if hosts.iter().any(|known| known == host) => check(
            Level::Ok,
            "host",
            format!("nixosConfigurations.{host} exists"),
        ),
        Ok(hosts) => check(
            Level::Fail,
            "host",
            format!(
                "nixosConfigurations has no {host}; it has {}",
                hosts.join(", ")
            ),
        ),
    });
    results
}

fn state() -> Check {
    let dir = nixos::state_dir();
    // The state directory may not exist yet; its nearest existing parent decides.
    let mut existing: PathBuf = dir.clone();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent.to_path_buf(),
            None => break,
        }
    }
    if writable(&existing) {
        check(
            Level::Ok,
            "state",
            format!("builds are remembered in {}", dir.display()),
        )
    } else {
        check(
            Level::Fail,
            "state",
            format!(
                "{} is not writable; builds cannot be remembered",
                existing.display()
            ),
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
    match system_commit(dir, &nixos::running(), &nixos::remembered()) {
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
pub fn checks() -> Vec<Check> {
    let checkout = checkout(&flake::location(), &nixos::hostname());
    let passed = checkout.iter().all(|result| result.level != Level::Fail);
    let dir = passed.then(|| flake::location().dir);
    let mut results = vec![tools()];
    results.extend(checkout);
    results.push(state());
    results.push(running(dir.as_deref()));
    results
}

/// Prints one line for each check.
pub fn print(results: &[Check]) {
    for result in results {
        output::line(&format!(
            "{:<6}{:<10}{}",
            result.level.label(),
            result.name,
            result.message
        ));
    }
}

pub fn report() -> Result<(), String> {
    let results = checks();
    print(&results);
    let failed = results
        .iter()
        .filter(|result| result.level == Level::Fail)
        .count();
    if failed > 0 {
        return Err(format!("{failed} check(s) failed."));
    }
    Ok(())
}
