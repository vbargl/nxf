//! Health checks after a switch. The configuration supplies them as
//! executables in one directory; all must pass before the time runs out.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::text::plain;

pub const DEFAULT_DIR: &str = "/etc/nxf/health.d";
const TIMED_OUT: &str = "timed out";

#[derive(Debug, PartialEq, Eq)]
pub enum Health {
    /// The configuration has no checks.
    Unchecked,
    Healthy,
    /// The checks that still failed when the time ran out, with what they said.
    Unhealthy(String),
}

fn seconds(name: &str, default: u64) -> Duration {
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
    let dir = std::env::var_os("NXF_HEALTH_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from);
    let Ok(entries) = std::fs::read_dir(dir) else {
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

/// Runs one check until the deadline; an error says why it did not pass.
fn passes(check: &Path, deadline: Instant) -> Result<(), String> {
    let (mut reader, writer) = os_pipe::pipe().map_err(|err| err.to_string())?;
    let stderr = writer.try_clone().map_err(|err| err.to_string())?;
    let mut command = Command::new(check);
    let mut child = command
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
        return Err(TIMED_OUT.to_string());
    };
    if status.success() {
        return Ok(());
    }
    let said = plain(&said.join().unwrap_or_default());
    let last = said.lines().map(str::trim).rfind(|line| !line.is_empty());
    Err(match (last, status.code()) {
        (Some(line), _) => line.to_string(),
        (None, Some(code)) => format!("exit status {code}"),
        (None, None) => "killed by a signal".to_string(),
    })
}

/// Runs all checks, again and again, until all pass or the time runs out.
pub fn check() -> Health {
    if checks().is_empty() {
        return Health::Unchecked;
    }
    let deadline = Instant::now() + seconds("NXF_HEALTH_TIMEOUT", 300);
    let interval = seconds("NXF_HEALTH_INTERVAL", 10);
    // What each check said when it last ran to its end. A run that the
    // deadline cut short says less than the one before it.
    let mut reasons: Vec<(String, String)> = Vec::new();
    loop {
        let mut failed = Vec::new();
        for check in checks() {
            let name = check
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let Err(why) = passes(&check, deadline) else {
                continue;
            };
            let known = reasons.iter().position(|(check, _)| *check == name);
            let why = match known {
                Some(index) if why == TIMED_OUT => reasons[index].1.clone(),
                Some(index) => {
                    reasons[index].1 = why.clone();
                    why
                }
                None => {
                    if why != TIMED_OUT {
                        reasons.push((name.clone(), why.clone()));
                    }
                    why
                }
            };
            failed.push(format!("{name}: {why}"));
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
