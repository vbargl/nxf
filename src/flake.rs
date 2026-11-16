//! Helpers for the machine flake: its checkout and its lockfile.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::output;

pub const DEFAULT_CHECKOUT: &str = "/etc/nixos";

#[derive(Debug, PartialEq, Eq)]
pub struct Location {
    pub dir: PathBuf,
    pub source: &'static str,
}

/// Where the flake checkout is said to be, and what said so.
pub fn location() -> Location {
    match std::env::var_os("NXF_FLAKE").filter(|value| !value.is_empty()) {
        Some(dir) => Location {
            dir: std::path::absolute(&dir).unwrap_or_else(|_| PathBuf::from(dir)),
            source: "NXF_FLAKE",
        },
        None => Location {
            dir: PathBuf::from(DEFAULT_CHECKOUT),
            source: "default",
        },
    }
}

/// The flake checkout: NXF_FLAKE, or /etc/nixos like nh.
pub fn checkout() -> Result<PathBuf, String> {
    let dir = location().dir;
    if !dir.join("flake.nix").exists() {
        return Err(format!(
            "{} has no flake.nix; set NXF_FLAKE to the machine flake checkout.",
            dir.display()
        ));
    }
    Ok(dir)
}

/// Updates the given inputs, or all of them, in the lockfile, without committing it.
pub fn update(dir: &Path, inputs: &[String]) -> Result<(), String> {
    let mut nix = Command::new("nix");
    nix.args(["flake", "update"])
        .args(inputs)
        .arg("--flake")
        .arg(dir)
        .args(["--log-format", "internal-json"]);
    output::stream(&mut nix, true, inputs)
}

/// The NixOS configurations a flake directory defines.
pub fn configurations(dir: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("nix")
        .args(["eval", "--json"])
        .arg(format!("{}#nixosConfigurations", dir.display()))
        .args(["--apply", "builtins.attrNames"])
        .output()
        .map_err(|err| format!("cannot run nix: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())
}

/// The lockfile of a flake directory.
pub fn lock(dir: &Path) -> Result<Value, String> {
    let file = dir.join("flake.lock");
    let text = std::fs::read_to_string(&file)
        .map_err(|err| format!("cannot read {}: {err}", file.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", file.display()))
}

fn root(lock: &Value) -> Option<&serde_json::Map<String, Value>> {
    let name = lock.get("root")?.as_str()?;
    lock.get("nodes")?.get(name)?.get("inputs")?.as_object()
}

pub fn root_inputs(lock: &Value) -> Vec<String> {
    root(lock)
        .map(|inputs| inputs.keys().cloned().collect())
        .unwrap_or_default()
}

fn node<'a>(lock: &'a Value, reference: &Value) -> Option<&'a Value> {
    lock.get("nodes")?.get(reference.as_str()?)
}

fn follows(reference: &Value) -> Option<String> {
    let path = reference.as_array()?;
    Some(
        path.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn input_url(original: &Value) -> String {
    let field = |name: &str| original.get(name).and_then(Value::as_str).unwrap_or("");
    match field("type") {
        "github" => format!("https://github.com/{}/{}", field("owner"), field("repo")),
        "gitlab" => format!("https://gitlab.com/{}/{}", field("owner"), field("repo")),
        kind => ["url", "path"]
            .iter()
            .map(|name| field(name))
            .find(|value| !value.is_empty())
            .unwrap_or(kind)
            .to_string(),
    }
}

/// A root input's locked revision, or how it follows another input.
pub fn input_rev(lock: &Value, name: &str) -> String {
    let Some(reference) = root(lock).and_then(|inputs| inputs.get(name)) else {
        return String::new();
    };
    if let Some(path) = follows(reference) {
        return format!("follows {path}");
    }
    let locked = node(lock, reference).and_then(|node| node.get("locked"));
    let pin = ["rev", "narHash"]
        .iter()
        .find_map(|field| locked?.get(field)?.as_str())
        .unwrap_or("");
    pin.chars().take(12).collect()
}

/// The complete pin, so that comparisons do not rely on abbreviated revisions.
fn input_pin<'a>(lock: &'a Value, name: &str) -> Option<&'a Value> {
    let reference = root(lock)?.get(name)?;
    if reference.is_array() {
        Some(reference)
    } else {
        node(lock, reference)?.get("locked")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InputUpdate {
    pub name: String,
    pub before: String,
    pub after: String,
    pub changed: bool,
}

pub fn input_updates(before: &Value, after: &Value, only: &[String]) -> Vec<InputUpdate> {
    let (old, new) = (root_inputs(before), root_inputs(after));
    let mut names = old.clone();
    names.extend(new.iter().filter(|name| !old.contains(name)).cloned());
    names
        .into_iter()
        .filter(|name| only.is_empty() || only.contains(name))
        .map(|name| InputUpdate {
            before: if old.contains(&name) {
                input_rev(before, &name)
            } else {
                "new".to_string()
            },
            after: if new.contains(&name) {
                input_rev(after, &name)
            } else {
                "removed".to_string()
            },
            changed: input_pin(before, &name) != input_pin(after, &name),
            name,
        })
        .collect()
}

/// Lines describing root inputs, all or only the given ones, with their
/// revisions and sources. With another lock, marks inputs that differ from it.
pub fn input_lines(lock: &Value, only: &[String], other: Option<&Value>) -> Vec<String> {
    let rows: Vec<[String; 4]> = root_inputs(lock)
        .into_iter()
        .filter(|name| only.is_empty() || only.contains(name))
        .map(|name| {
            let rev = input_rev(lock, &name);
            let reference = root(lock).and_then(|inputs| inputs.get(&name));
            let url = match reference {
                Some(reference) if !reference.is_array() => node(lock, reference)
                    .and_then(|node| node.get("original"))
                    .map(input_url)
                    .unwrap_or_default(),
                _ => String::new(),
            };
            let change = match other {
                None => String::new(),
                Some(other) if !root_inputs(other).contains(&name) => {
                    "  (not in the checkout)".to_string()
                }
                Some(other) => {
                    let next = input_rev(other, &name);
                    if next == rev {
                        String::new()
                    } else {
                        format!("  (checkout: {next})")
                    }
                }
            };
            [name, rev, url, change]
        })
        .collect();
    let width = |column: usize| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (names, revs) = (width(0), width(1));
    rows.iter()
        .map(|[name, rev, url, change]| {
            format!("  {name:<names$}  {rev:<revs$}  {url}{change}")
                .trim_end()
                .to_string()
        })
        .collect()
}
