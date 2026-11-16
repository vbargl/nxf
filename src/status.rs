//! Reports what the machine runs and how it compares with the checkout.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::nixos::Remembered;
use crate::output::{self, Style};
use crate::text::plain;
use crate::{flake, git, nixos};

/// Prints the latest unread summary, then acknowledges this exact snapshot for
/// the invoking user. The updater's result is shared and is never modified.
fn update_summary() {
    let snapshot = match nixos::update_result() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return,
        Err(err) => {
            output::warning(&format!(
                "Cannot read the automatic update summary: {}",
                plain(&err)
            ));
            return;
        }
    };
    let id = snapshot["id"].as_str().unwrap_or_default();
    let marker = nixos::state_dir().join("update-read");
    let seen = std::fs::read_to_string(&marker).unwrap_or_default();
    if seen.trim() == id {
        return;
    }

    output::line("");
    output::effect(&output::styled("Automatic update summary", Style::Bold));
    if let Some(results) = snapshot["update"].get("results").and_then(Value::as_array) {
        let field = |result: &Value, name: &str| -> String {
            result
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let notable: Vec<&Value> = results
            .iter()
            .filter(|result| field(result, "outcome") != "unchanged")
            .collect();
        if notable.is_empty() {
            output::detail("Nothing changed.");
        }
        for result in notable {
            let mut name = vec![field(result, "task")];
            if let Some(inputs) = result.get("inputs").and_then(Value::as_array) {
                name.extend(inputs.iter().filter_map(Value::as_str).map(str::to_string));
            }
            let detail = match field(result, "detail") {
                detail if detail.is_empty() => String::new(),
                detail => format!(". {detail}"),
            };
            output::detail(&plain(&format!(
                "{}: {}{detail}",
                name.join(" "),
                field(result, "outcome")
            )));
        }
    } else if !snapshot["update"].is_null() {
        output::detail("Nothing changed.");
    }
    if let Some(failure) = snapshot.get("failure").filter(|failure| !failure.is_null()) {
        output::detail(&format!(
            "Update service: {}.",
            failure["result"].as_str().unwrap_or("failed")
        ));
        output::command(&format!(
            "journalctl -u {}",
            failure["service"].as_str().unwrap_or(nixos::UPDATE_SERVICE)
        ));
    }
    if let Err(err) = mark_read(&marker, id) {
        output::warning(&format!(
            "Could not mark the update summary as read: {}",
            plain(&err)
        ));
    }
}

fn mark_read(marker: &Path, id: &str) -> Result<(), String> {
    let dir = marker.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let temporary = dir.join(format!("update-read.{}.tmp", std::process::id()));
    let written = std::fs::write(&temporary, id).and_then(|()| std::fs::rename(&temporary, marker));
    written.map_err(|err| {
        let _ = std::fs::remove_file(&temporary);
        format!("{}: {err}", marker.display())
    })
}

/// The flake commit that built a system, if one is remembered. Several commits
/// can build the same system; the checkout's wins, then the newest.
pub fn system_commit(dir: &Path, system: &Path, remembered: &[Remembered]) -> Option<String> {
    let trees: Vec<String> = remembered
        .iter()
        .filter(|record| record.system == system)
        .map(|record| record.key.clone())
        .collect();
    match git::clean_tree(dir) {
        Some(checkout) if trees.contains(&checkout) => git::commit_of(dir, &[checkout]),
        _ if !trees.is_empty() => git::commit_of(dir, &trees),
        _ => None,
    }
}

/// The flake commit of the running system.
pub fn running_commit() -> Result<String, String> {
    let dir = flake::checkout()?;
    system_commit(&dir, &nixos::running(), &nixos::remembered())
        .ok_or_else(|| "The running system was not built by nxf or the updater.".to_string())
}

/// How the committed checkout relates to the running system.
pub struct Comparison {
    pub dir: PathBuf,
    pub lock: Value,
    pub changes: Vec<String>,
    pub running: PathBuf,
    pub running_commit: Option<String>,
    pub running_trees: Vec<String>,
    pub tree: Option<String>,
    pub built: Option<PathBuf>,
    pub matches: bool,
}

pub fn compare() -> Result<Comparison, String> {
    let dir = flake::checkout()?;
    let lock = flake::lock(&dir)?;
    let running = nixos::running();
    let remembered = nixos::remembered();
    let tree = git::clean_tree(&dir);
    let built = tree.as_ref().and_then(|tree| {
        remembered
            .iter()
            .find(|record| record.key == *tree)
            .map(|record| record.system.clone())
    });
    let running_trees: Vec<String> = remembered
        .iter()
        .filter(|record| record.system == running)
        .map(|record| record.key.clone())
        .collect();
    let matches = built.as_ref() == Some(&running)
        || tree
            .as_ref()
            .is_some_and(|tree| running_trees.contains(tree));
    Ok(Comparison {
        running_commit: system_commit(&dir, &running, &remembered),
        changes: git::changed_files(&dir),
        dir,
        lock,
        running,
        running_trees,
        tree,
        built,
        matches,
    })
}

pub struct Options {
    pub checkedout: bool,
    pub detailed: bool,
    pub json: bool,
    pub inputs: Vec<String>,
}

fn input_table(lock: &Value, inputs: &[String], other: Option<&Value>) {
    for line in flake::input_lines(lock, inputs, other) {
        output::detail(line.trim_start());
    }
}

/// With `json` only the comparison is printed, and no summary is marked as read.
pub fn report(options: &Options) -> Result<(), String> {
    let status = compare()?;
    let dir = &status.dir;
    let running_lock: Option<Value> = match &status.running_commit {
        Some(commit) => Some(
            serde_json::from_str(&git::show_file(dir, commit, "flake.lock")?)
                .map_err(|err| format!("cannot parse flake.lock of {commit}: {err}"))?,
        ),
        None => None,
    };

    let mut known = flake::root_inputs(&status.lock);
    if let Some(lock) = &running_lock {
        for name in flake::root_inputs(lock) {
            if !known.contains(&name) {
                known.push(name);
            }
        }
    }
    let unknown: Vec<&str> = options
        .inputs
        .iter()
        .filter(|name| !known.contains(name))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "Unknown input: {}. Inputs are {}.",
            unknown.join(", "),
            known.join(", ")
        ));
    }

    if options.json {
        let comparison = json!({
            "checkout": dir,
            "changes": status.changes,
            "inputs": flake::root_inputs(&status.lock),
            "running": status.running,
            "built": status.built,
            "matches": status.matches,
        });
        output::line(&serde_json::to_string_pretty(&comparison).unwrap_or_default());
        return Ok(());
    }

    let running = status.running.to_string_lossy();
    if status.matches {
        output::verdict("Running system matches the checkout.", &running, true);
    } else if status.tree.is_none() {
        output::verdict("Checkout has uncommitted changes.", &running, false);
        output::effect("Commit them to compare with the running system.");
    } else if status.running_trees.is_empty() && status.built.is_none() {
        output::verdict(
            "Cannot compare the running system with the checkout.",
            &running,
            false,
        );
        output::effect("No build record identifies the running system or the checkout.");
    } else {
        output::verdict("Running system differs from the checkout.", &running, false);
        if status.built.is_none() {
            output::effect("The checkout has not been built yet.");
        }
    }
    output::line("");

    if options.detailed || !options.checkedout {
        if options.detailed {
            let size = nixos::size(&status.running)?;
            output::detail(&format!(
                "Closure size: {:.2} GiB",
                size as f64 / (1u64 << 30) as f64
            ));
        }
        match (&status.running_commit, &running_lock) {
            (Some(commit), Some(lock)) => {
                output::detail(&format!("Flake: {}", git::commit_line(dir, commit)));
                output::detail(&format!("Checkout: {}", dir.display()));
                output::line("");
                input_table(lock, &options.inputs, Some(&status.lock));
            }
            _ => {
                let revision = std::process::Command::new("nixos-version")
                    .arg("--configuration-revision")
                    .output()
                    .ok()
                    .filter(|output| output.status.success())
                    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                    .filter(|revision| !revision.is_empty())
                    .unwrap_or_else(|| "unknown".to_string());
                output::detail("Flake: unknown; the system was not built by nxf or the updater.");
                output::detail(&format!("Configuration revision: {revision}"));
            }
        }
    }
    if options.detailed {
        output::line("");
    }
    if options.detailed || options.checkedout {
        output::detail(&format!("Checkout: {}", dir.display()));
        let dirty = if status.tree.is_none() {
            ", with uncommitted changes"
        } else {
            ""
        };
        output::detail(&format!("Flake: {}{dirty}", git::commit_line(dir, "HEAD")));
        output::line("");
        input_table(&status.lock, &options.inputs, None);
    }

    if !status.matches && status.tree.is_some() {
        output::line("");
        match &status.built {
            Some(built) if options.detailed => {
                output::stream(&mut nixos::diff(&status.running, built), false, &[])?;
            }
            Some(built) => {
                for line in nixos::changes(&status.running, built)? {
                    output::detail(&line);
                }
                output::line("");
                output::effect("Show the package list:");
                output::command("nxf status --detailed");
            }
            None => {
                output::effect("Build the checkout to compare configurations:");
                output::command("nxf build");
            }
        }
    }
    update_summary();
    Ok(())
}
