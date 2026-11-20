//! Helpers for managing NixOS: building and activating with nh, the running
//! system, package changes and sizes with dix and nix, and the update service.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::{env, output, process, state};

pub const UPDATE_SERVICE: &str = "nxf-auto-update.service";

fn expand(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The running system. NXF_SYSTEM stands in for it in tests.
pub fn running() -> PathBuf {
    expand(&env::path(env::SYSTEM, "/run/current-system"))
}

/// The system the machine booted. NXF_BOOTED stands in for it in tests.
pub fn booted() -> PathBuf {
    env::path(env::BOOTED, "/run/booted-system")
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

/// Runs nh on a flake installable or an already built store path.
fn nh(action: &str, installable: &[&str]) -> Result<PathBuf, String> {
    let state = state::dir();
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
    let version = process::capture(Command::new("dix").arg("--version"))
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
    let output = process::run(diff(old, new).arg("--color=never"))?;
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
    let output = process::run(
        Command::new("nix")
            .args([
                "path-info",
                "--json",
                "--json-format",
                "1",
                "--closure-size",
            ])
            .arg(system),
    )?;
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

/// The command of systemd. NXF_SYSTEMCTL stands in for it in tests.
pub fn systemctl() -> PathBuf {
    env::path(env::SYSTEMCTL, "systemctl")
}

/// How and when the update service last ended, if it did not succeed.
pub fn service_failure() -> Option<Value> {
    let mut show = Command::new(systemctl());
    show.args(["show", UPDATE_SERVICE])
        .args([
            "--property=Result,ExecMainExitTimestamp",
            "--timestamp=unix",
        ])
        .arg("--no-pager");
    // Bounded: an unavailable system manager must not hold status up.
    let shown = process::run_within(&mut show, Instant::now() + Duration::from_secs(1), false)
        .ok()
        .filter(|shown| shown.status.is_some_and(|status| status.success()))?;
    let text = shown.stdout;
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
