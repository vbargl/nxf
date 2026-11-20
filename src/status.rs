//! Reports what the machine runs and how it compares with the checkout.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::output::{self, Mark, Style};
use crate::state::Remembered;
use crate::text::plain;
use crate::{flake, git, hold, nixos, process, state, time};

/// A size in the largest binary unit that keeps it above one.
fn human(bytes: u64) -> String {
    let units = ["bytes", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < units.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{size:.2} {}", units[unit])
    }
}

/// A task result as a row: its state, its name with what happened, and
/// supporting lines. Results that changed nothing are left out.
pub struct ResultRow {
    pub mark: Mark,
    pub text: String,
    pub more: Vec<String>,
}

pub fn result_rows(results: &[Value]) -> Vec<ResultRow> {
    let field = |result: &Value, name: &str| -> String {
        result
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    results
        .iter()
        .filter(|result| field(result, "outcome") != "unchanged")
        .map(|result| {
            let mut name = vec![field(result, "task")];
            if let Some(inputs) = result.get("inputs").and_then(Value::as_array) {
                name.extend(inputs.iter().filter_map(Value::as_str).map(str::to_string));
            }
            let name = name.join(" ");
            let detail = plain(&field(result, "detail"));
            // The title of a failure repeats what the mark already says.
            let mut pieces = detail
                .split(" | ")
                .filter(|piece| !piece.is_empty() && !piece.starts_with('✗'))
                .map(str::to_string);
            let outcome = field(result, "outcome");
            let (mark, said) = match outcome.as_str() {
                "done" => (Mark::Done, pieces.next().unwrap_or_default()),
                "failed" => (Mark::Failed, pieces.next().unwrap_or_default()),
                other => {
                    let first = pieces.next();
                    (
                        Mark::Quiet,
                        match first {
                            Some(first) => format!("{other}, {first}"),
                            None => other.to_string(),
                        },
                    )
                }
            };
            ResultRow {
                mark,
                // A long name keeps its distance from what follows.
                text: format!(
                    "{name:<width$}{said}",
                    width = output::widened([name.as_str()])
                )
                .trim_end()
                .to_string(),
                more: pieces.collect(),
            }
        })
        .collect()
}

/// Prints result rows, with their supporting lines beneath the text column.
pub fn print_rows(rows: &[ResultRow]) {
    for row in rows {
        output::row(row.mark, &row.text);
        for more in &row.more {
            output::beneath(more);
        }
    }
}

/// The task results the updater saved last, with their time.
fn saved_update() -> Result<Option<Value>, String> {
    let file = state::update_file(&state::updater_dir());
    if !file.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file)
        .map_err(|err| format!("cannot read {}: {err}", file.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|err| format!("cannot parse {}: {err}", file.display()))
}

/// The latest automatic update: the saved task results, and the service's
/// failure if it ended after them, as when it was killed before saving. The id
/// changes whenever either does. None when there is nothing to report.
fn update_result() -> Result<Option<Value>, String> {
    let update = saved_update()?;
    let saved_at = update
        .as_ref()
        .and_then(|update| update.get("time"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let failure = nixos::service_failure()
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

/// Prints the latest unread summary, then acknowledges this exact snapshot for
/// the invoking user. The updater's result is shared and is never modified.
fn update_summary() {
    let snapshot = match update_result() {
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
    let marker = state::read_marker();
    let seen = std::fs::read_to_string(&marker).unwrap_or_default();
    if seen.trim() == id {
        return;
    }

    let update = &snapshot["update"];
    let when = update.get("time").and_then(Value::as_u64).map(time::age);
    output::section("Last automatic update", &when.unwrap_or_default());
    if !update.is_null() {
        let results = update.get("results").and_then(Value::as_array);
        let rows = result_rows(results.map_or(&[][..], Vec::as_slice));
        if rows.is_empty() {
            output::note("Nothing changed.");
        }
        print_rows(&rows);
    }
    if let Some(failure) = snapshot.get("failure").filter(|failure| !failure.is_null()) {
        output::row(
            Mark::Failed,
            &format!(
                "{:<width$}{}",
                "service",
                failure["result"].as_str().unwrap_or("failed"),
                width = output::LABEL
            ),
        );
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
    process::write_atomic(marker, id).map_err(|err| format!("{}: {err}", marker.display()))
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
    system_commit(&dir, &nixos::running(), &state::remembered())
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
    let remembered = state::remembered();
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

/// The inputs of a lock as facts. Names share the label column, widened for
/// long names; a difference from the other lock draws attention, the source
/// is supporting text.
fn input_table(lock: &Value, inputs: &[String], other: Option<&Value>) {
    let rows = flake::input_rows(lock, inputs, other);
    let names = output::widened(rows.iter().map(|row| row.name.as_str()));
    let revs = rows
        .iter()
        .map(|row| row.rev.chars().count())
        .max()
        .unwrap_or(0);
    for row in rows {
        let mut text = format!("{:<revs$}", row.rev);
        if let Some(change) = &row.change {
            text.push_str(&format!("  {}", output::styled(change, Style::Yellow)));
        }
        if !row.url.is_empty() {
            text.push_str(&format!("  {}", output::dim(&row.url)));
        }
        output::fact_in(names, &row.name, text.trim_end());
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
            "revision": status.running_commit,
            "rebootRequired": nixos::reboot_required(),
            "hold": hold::active().map(|hold| hold.to_json()),
            "bad": status.tree.as_deref().and_then(state::bad),
            "lastUpdate": update_result()
                .ok()
                .flatten()
                .map(|result| result["update"].clone()),
        });
        output::line(&serde_json::to_string_pretty(&comparison).unwrap_or_default());
        return Ok(());
    }

    let running = status.running.to_string_lossy();
    if status.matches {
        output::verdict("Running system matches the checkout", &running, true);
    } else if status.tree.is_none() {
        output::verdict("Checkout has uncommitted changes", &running, false);
        output::note("Commit them to compare with the running system.");
    } else if status.running_trees.is_empty() && status.built.is_none() {
        output::verdict(
            "Cannot compare the running system with the checkout",
            &running,
            false,
        );
        output::note("No build record identifies the running system or the checkout.");
    } else {
        output::verdict("Running system differs from the checkout", &running, false);
    }
    if nixos::reboot_required() {
        output::attention("Restart needed: the kernel or initrd changed since the machine booted");
    }
    if let Some(hold) = hold::active() {
        output::attention(&format!(
            "Automatic updates are held: {}",
            plain(&hold.describe())
        ));
    }
    if let Some(reason) = status.tree.as_deref().and_then(state::bad) {
        output::attention(&format!(
            "The checkout failed its health check and is not activated automatically: {}",
            plain(&reason)
        ));
    }

    if options.detailed || !options.checkedout {
        output::section("Running", "");
        match (&status.running_commit, &running_lock) {
            (Some(commit), Some(lock)) => {
                output::fact("Flake", &git::commit_line(dir, commit));
                output::fact("Checkout", &dir.to_string_lossy());
                if options.detailed {
                    let size = nixos::size(&status.running)?;
                    output::fact("Closure", &human(size));
                }
                input_table(lock, &options.inputs, Some(&status.lock));
            }
            _ => {
                let revision = process::capture(
                    std::process::Command::new("nixos-version").arg("--configuration-revision"),
                )
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .filter(|revision| !revision.is_empty())
                .unwrap_or_else(|| "unknown".to_string());
                output::fact(
                    "Flake",
                    "unknown; the system was not built by nxf or the updater",
                );
                output::fact("Revision", &revision);
                if options.detailed {
                    let size = nixos::size(&status.running)?;
                    output::fact("Closure", &human(size));
                }
            }
        }
    }
    if options.detailed || options.checkedout {
        output::section("Checkout", "");
        let dirty = if status.tree.is_none() {
            ", with uncommitted changes"
        } else {
            ""
        };
        output::fact(
            "Flake",
            &format!("{}{dirty}", git::commit_line(dir, "HEAD")),
        );
        output::fact("Path", &dir.to_string_lossy());
        input_table(&status.lock, &options.inputs, None);
    }

    if !status.matches && status.tree.is_some() {
        output::section("Changes when switched", "");
        match &status.built {
            Some(built) if options.detailed => {
                output::stream(&mut nixos::diff(&status.running, built), false, &[])?;
            }
            Some(built) => {
                for (label, value) in nixos::changes(&status.running, built)? {
                    output::fact(label, &value);
                }
                output::effect(&format!(
                    "{} {}",
                    output::dim("Package list:"),
                    output::styled("nxf status --detailed", Style::Bold)
                ));
            }
            None => output::effect(&format!(
                "{} {}",
                output::dim("The checkout has not been built yet:"),
                output::styled("nxf build", Style::Bold)
            )),
        }
    }
    update_summary();
    Ok(())
}
