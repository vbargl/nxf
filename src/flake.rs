//! Helpers for the machine flake: its checkout and its lockfile.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::{env, output, process};

pub const DEFAULT_CHECKOUT: &str = "/etc/nixos";

#[derive(Debug, PartialEq, Eq)]
pub struct Location {
    pub dir: PathBuf,
    pub source: &'static str,
}

/// Where the flake checkout is said to be, and what said so.
pub fn location() -> Location {
    match env::var(env::FLAKE) {
        Some(dir) => Location {
            dir: std::path::absolute(&dir).unwrap_or(dir),
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
    let output = process::run(
        Command::new("nix")
            .args(["eval", "--json"])
            .arg(format!("{}#nixosConfigurations", dir.display()))
            .args(["--apply", "builtins.attrNames"]),
    )?;
    serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())
}

/// What the configuration in the checkout says about the automatic update of
/// its inputs: the strategy for all of them and those of single ones, from
/// `services.nxf.autoUpdate`. Null and empty where it does not have the option.
pub fn update_settings(dir: &Path, configuration: &str) -> Result<Value, String> {
    let output = process::run(
        Command::new("nix")
            .args(["eval", "--json"])
            .arg(format!(
                "{}#nixosConfigurations.\"{configuration}\"",
                dir.display()
            ))
            .args([
                "--apply",
                "c: { strategy = c.config.services.nxf.autoUpdate.strategy or null; inputs = c.config.services.nxf.autoUpdate.inputs or { }; }",
            ]),
    )?;
    serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())
}

/// The store paths of an archived flake's inputs, and of theirs.
fn archived(node: &Value, paths: &mut Vec<PathBuf>) {
    let Some(inputs) = node.get("inputs").and_then(Value::as_object) else {
        return;
    };
    for input in inputs.values() {
        if let Some(path) = input.get("path").and_then(Value::as_str) {
            let path = PathBuf::from(path);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        archived(input, paths);
    }
}

/// Copies every locked input of the flake, and theirs, into the Nix store,
/// and returns where they are. Nix reads a locked input from the store when
/// it is there, and asks its origin only when it is not.
pub fn archive(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let said = nix(dir, &["flake", "archive", "--json"])?;
    let tree: Value = serde_json::from_str(&said).map_err(|err| err.to_string())?;
    let mut paths = Vec::new();
    archived(&tree, &mut paths);
    Ok(paths)
}

/// The name nxf expects the machine's configuration to have.
pub const CONFIGURATION: &str = "default";

/// Chooses the configuration nxf works with: the one named `default`, or the
/// only one. Several without that name are not supported by design.
pub fn choose(names: &[String]) -> Result<String, String> {
    if names.iter().any(|name| name == CONFIGURATION) {
        return Ok(CONFIGURATION.to_string());
    }
    match names {
        [] => Err("The flake has no nixosConfigurations.".to_string()),
        [only] => Ok(only.clone()),
        several => Err(format!(
            "The flake has {} nixosConfigurations and none is named {CONFIGURATION}\n{:<LABEL$}{}\nnxf works with one configuration for each flake; name it {CONFIGURATION} or keep only one.",
            several.len(),
            "Found",
            several.join(", "),
            LABEL = output::LABEL
        )),
    }
}

/// The configuration of a flake directory that nxf works with.
pub fn configuration(dir: &Path) -> Result<String, String> {
    choose(&configurations(dir).map_err(|err| {
        format!(
            "Cannot evaluate nixosConfigurations of {}\n{err}",
            dir.display()
        )
    })?)
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

/// A root input as status shows it.
#[derive(Debug, PartialEq, Eq)]
pub struct InputRow {
    pub name: String,
    pub rev: String,
    /// Where the input comes from; empty for an input that follows another.
    pub url: String,
    /// How the other lock differs: its revision, or that it lacks the input.
    pub change: Option<String>,
}

/// Root inputs, all or only the given ones, with their revisions and
/// sources. With another lock, says which inputs differ from it.
pub fn input_rows(lock: &Value, only: &[String], other: Option<&Value>) -> Vec<InputRow> {
    root_inputs(lock)
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
                None => None,
                Some(other) if !root_inputs(other).contains(&name) => {
                    Some("not in the checkout".to_string())
                }
                Some(other) => {
                    let next = input_rev(other, &name);
                    (next != rev).then(|| format!("→ {next} in the checkout"))
                }
            };
            InputRow {
                name,
                rev,
                url,
                change,
            }
        })
        .collect()
}

fn nix(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = process::run(Command::new("nix").args(args).current_dir(dir))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Unpacks a flake template, given as `<flake>#<name>`, into the directory.
pub fn unpack(dir: &Path, template: &str) -> Result<(), String> {
    nix(dir, &["flake", "init", "--template", template]).map(drop)
}

/// Writes the lockfile of the flake in the directory.
pub fn write_lock(dir: &Path) -> Result<(), String> {
    nix(dir, &["flake", "lock"]).map(drop)
}

/// Whether the host's configuration has the options of the nxf module.
pub fn uses_module(dir: &Path, host: &str) -> Result<bool, String> {
    let options = format!(
        "{}#nixosConfigurations.\"{host}\".options.services",
        dir.display()
    );
    let found = nix(
        dir,
        &[
            "eval",
            "--json",
            &options,
            "--apply",
            "services: services ? nxf",
        ],
    )?;
    Ok(found.trim() == "true")
}
