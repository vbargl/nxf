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

/// The host name nh selects the configuration by.
pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_string())
        .unwrap_or_default()
}

pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Where nxf keeps its state: remembered builds and the read marker.
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

/// Runs nh on a flake installable or an already built store path.
fn nh(action: &str, installable: &str) -> Result<PathBuf, String> {
    let state = state_dir();
    std::fs::create_dir_all(&state)
        .map_err(|err| format!("cannot create {}: {err}", state.display()))?;
    let result = state.join("result");
    let mut nh = Command::new("nh");
    nh.args(["os", action, installable])
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
pub fn build(flake: &Path) -> Result<PathBuf, String> {
    nh("build", &format!("{}#", flake.display()))
}

/// Runs test, boot or switch on a built system.
pub fn activate(action: &str, system: &Path) -> Result<PathBuf, String> {
    nh(action, &system.to_string_lossy())
}

/// The command printing the full package diff between two systems.
pub fn diff(old: &Path, new: &Path) -> Command {
    let mut dix = Command::new("dix");
    dix.arg("diff").arg(old).arg(new);
    dix
}

fn is_count(line: &str) -> bool {
    // "CHANGED (12)"
    let Some((word, count)) = line.split_once(" (") else {
        return false;
    };
    let Some(count) = count.strip_suffix(')') else {
        return false;
    };
    !word.is_empty()
        && word.bytes().all(|byte| byte.is_ascii_uppercase())
        && !count.is_empty()
        && count.bytes().all(|byte| byte.is_ascii_digit())
}

/// The changed, added and removed package counts and the path and size
/// totals between two systems, without the package lines.
pub fn changes(old: &Path, new: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("dix")
        .arg("--color=never")
        .arg("diff")
        .arg(old)
        .arg(new)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run dix: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| is_count(line) || line.starts_with("PATHS:") || line.starts_with("SIZE:"))
        .map(str::to_string)
        .collect())
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

/// How and when the update service last ended, if it did not succeed.
fn service_failure() -> Option<Value> {
    let systemctl = var("NXF_SYSTEMCTL").unwrap_or_else(|| PathBuf::from("systemctl"));
    let mut child = Command::new(systemctl)
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

/// Remembers that the source with the given key built a system.
/// A plain link, not a GC root: the record disappears with the system.
pub fn remember(key: &str, system: &Path) -> Result<(), String> {
    let systems = state_dir().join("systems");
    std::fs::create_dir_all(&systems)
        .map_err(|err| format!("cannot create {}: {err}", systems.display()))?;
    let link = systems.join(key);
    let temporary = systems.join(format!(".{key}.{}", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    std::os::unix::fs::symlink(system, &temporary)
        .and_then(|()| std::fs::rename(&temporary, &link))
        .map_err(|err| format!("cannot write {}: {err}", link.display()))
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
        .filter(|entry| entry.path().exists())
        .map(|entry| Remembered {
            key: entry.file_name().to_string_lossy().into_owned(),
            system: expand(&entry.path()),
        })
        .collect();
    records.sort_by(|a, b| a.key.cmp(&b.key));
    records
}

/// Remembered systems by source key: this user's and the automatic
/// updater's, which keys its builds the same way.
pub fn remembered() -> Vec<Remembered> {
    let mut all = records(&state_dir().join("systems"));
    for record in records(&updater_dir().join("systems")) {
        if !all.contains(&record) {
            all.push(record);
        }
    }
    all
}
