//! Health checks after a switch. The configuration supplies them as
//! executables in one directory, with their times in a file beside them; all
//! must pass before the time runs out.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::env;
use crate::process::{self, TIMED_OUT};

pub const DEFAULT_DIR: &str = "/etc/nxf/health.d";
/// The file beside the directory of the checks that holds their times.
const SETTINGS: &str = "health.conf";

#[derive(Debug, PartialEq, Eq)]
pub enum Health {
    /// The configuration has no checks.
    Unchecked,
    Healthy,
    /// The checks that still failed when the time ran out, with what they said.
    Unhealthy(String),
}

fn dir() -> PathBuf {
    env::path(env::HEALTH_DIR, DEFAULT_DIR)
}

/// A time in seconds from the `name=seconds` lines of the file beside the
/// checks. A switch replaces it too, so the new configuration decides.
fn time(name: &str, default: u64) -> Duration {
    let settings = std::fs::read_to_string(dir().with_file_name(SETTINGS)).unwrap_or_default();
    let value = settings.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| value.trim().parse().ok())?
    });
    Duration::from_secs(value.unwrap_or(default))
}

/// The checks in name order. They are read again on every round, because a
/// switch replaces them with those of the new configuration.
fn checks() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut checks: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let hidden = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'));
            let executable = std::fs::metadata(path)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
            !hidden && executable
        })
        .collect();
    checks.sort();
    checks
}

/// Runs all checks, again and again, until all pass or the time runs out.
pub fn check() -> Health {
    if checks().is_empty() {
        return Health::Unchecked;
    }
    let deadline = Instant::now() + time("timeout", 300);
    let interval = time("interval", 10);
    // How each check ended when it last ran to its end: passed, or why not.
    // A run that the deadline cut short says less than the one before it.
    let mut ended: Vec<(String, Option<String>)> = Vec::new();
    loop {
        let mut failed = Vec::new();
        for check in checks() {
            let name = check
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let (cut, result) = match process::script(&check, deadline, &[]) {
                Ok(()) => (false, None),
                Err(Some(why)) => (false, Some(why)),
                Err(None) => (true, Some(TIMED_OUT.to_string())),
            };
            let known = ended.iter().position(|(check, _)| *check == name);
            let result = match (known, cut) {
                (Some(index), true) => ended[index].1.clone(),
                (Some(index), false) => {
                    ended[index].1 = result.clone();
                    result
                }
                (None, true) => result,
                (None, false) => {
                    ended.push((name.clone(), result.clone()));
                    result
                }
            };
            if let Some(why) = result {
                failed.push(format!("{name}: {why}"));
            }
        }
        if failed.is_empty() {
            return Health::Healthy;
        }
        if Instant::now() + interval >= deadline {
            return Health::Unhealthy(failed.join("; "));
        }
        std::thread::sleep(interval.max(Duration::from_millis(50)));
    }
}
