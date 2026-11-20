//! Health checks after a switch. The configuration supplies them as
//! executables in one directory, with their times in a file beside them; all
//! must pass before the time runs out.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::text::plain;

pub const DEFAULT_DIR: &str = "/etc/nxf/health.d";
/// The file beside the directory of the checks that holds their times.
const SETTINGS: &str = "health.conf";
const TIMED_OUT: &str = "timed out";

#[derive(Debug, PartialEq, Eq)]
pub enum Health {
    /// The configuration has no checks.
    Unchecked,
    Healthy,
    /// The checks that still failed when the time ran out, with what they said.
    Unhealthy(String),
}

fn dir() -> PathBuf {
    std::env::var_os("NXF_HEALTH_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from)
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

/// Seconds from a variable of the environment.
pub fn seconds(name: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default),
    )
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

/// Runs one script until the deadline, with the given variables added to its
/// environment; an error says why it did not pass.
pub fn passes(check: &Path, deadline: Instant, variables: &[(&str, &Path)]) -> Result<(), String> {
    attempt(check, deadline, variables).map_err(|why| why.unwrap_or_else(|| TIMED_OUT.to_string()))
}

/// The same, with no reason when the deadline cut the script short: what a
/// script says itself is never taken for that.
fn attempt(
    check: &Path,
    deadline: Instant,
    variables: &[(&str, &Path)],
) -> Result<(), Option<String>> {
    let (mut reader, writer) = os_pipe::pipe().map_err(|err| err.to_string())?;
    let stderr = writer.try_clone().map_err(|err| err.to_string())?;
    let mut command = Command::new(check);
    let mut child = command
        .envs(variables.iter().copied())
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(stderr)
        .spawn()
        .map_err(|err| format!("cannot run: {err}"))?;
    drop(command);
    let said = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = reader.read_to_end(&mut text);
        String::from_utf8_lossy(&text).into_owned()
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let Some(status) = status else {
        return Err(None);
    };
    if status.success() {
        return Ok(());
    }
    let said = plain(&said.join().unwrap_or_default());
    let last = said.lines().map(str::trim).rfind(|line| !line.is_empty());
    Err(Some(match (last, status.code()) {
        (Some(line), _) => line.to_string(),
        (None, Some(code)) => format!("exit status {code}"),
        (None, None) => "killed by a signal".to_string(),
    }))
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
            let (cut, result) = match attempt(&check, deadline, &[]) {
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
