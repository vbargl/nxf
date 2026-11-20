//! The unattended update the timer starts. The tasks do every operation and
//! report their results; this only decides what comes next, and saves the
//! results for `nxf status`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::health::{self, Health};
use crate::tasks::{self, Outcome};
use crate::{flake, hold, hooks, nixos, output, status, strategy};

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn outcome(task: &str, outcome: &str, detail: String) -> Outcome {
    Outcome {
        task: task.to_string(),
        inputs: Vec::new(),
        outcome: outcome.to_string(),
        detail,
    }
}

/// Checks the health of the system just switched to. An unhealthy system is
/// replaced by the previous one, and its source is not tried again.
fn verify(tree: Option<&str>, previous: &Path) -> Option<Outcome> {
    let mut failed = match health::check() {
        Health::Unchecked => return None,
        Health::Healthy => return Some(outcome("health", "done", String::new())),
        Health::Unhealthy(failed) => failed,
    };
    // A mark that cannot be written is said, and never keeps the unhealthy system.
    if let Some(Err(err)) = tree.map(|tree| nixos::mark_bad(tree, &failed)) {
        failed.push_str(&format!(" | {err}"));
    }
    output::capture();
    let returned = nixos::activate("switch", previous);
    output::release();
    let effect = match returned {
        Ok(_) => format!("Returned to {}.", previous.display()),
        Err(err) => format!(
            "Could not return to {}: {}",
            previous.display(),
            err.replace('\n', " | ")
        ),
    };
    Some(outcome("health", "failed", format!("{failed} | {effect}")))
}

/// Runs what comes next and adds its results as they come, so that a run
/// that cannot go on keeps the results of what ran before.
fn decide(results: &mut Vec<Outcome>) -> Result<(), String> {
    hold::forget_ended();
    if let Some(hold) = hold::active() {
        results.push(outcome(
            "update",
            "skipped",
            format!("held: {}", hold.describe()),
        ));
        return Ok(());
    }
    // A lockfile left by an interrupted run is not work in progress; update restores it.
    let foreign = status::compare()?
        .changes
        .iter()
        .any(|file| file != "flake.lock");
    if foreign {
        results.push(outcome(
            "update",
            "skipped",
            "the checkout has uncommitted changes".to_string(),
        ));
        return Ok(());
    }
    if let Some(refused) = hooks::refused() {
        results.push(outcome("update", "skipped", refused));
        return Ok(());
    }
    // What is committed says how each input is treated.
    let dir = flake::checkout()?;
    let strategies = strategy::read(&dir, &flake::configuration(&dir)?)?;
    // Every input that builds is committed; the others stay as they were.
    results.extend(tasks::run_unattended(&words(&["update"]), &strategies)?);
    if results.iter().any(Outcome::failed) {
        return Ok(());
    }
    // Whatever is committed now has been built, by this run or an earlier one.
    let checkout = status::compare()?;
    if checkout.matches {
        return Ok(());
    }
    if let Some(reason) = checkout.tree.as_deref().and_then(nixos::bad) {
        results.push(outcome(
            "switch",
            "skipped",
            format!("this revision failed its health check ({reason}); waiting for a newer one"),
        ));
        return Ok(());
    }
    let switched = tasks::run(&words(&["switch"]), true)?;
    let activated = !switched.iter().any(Outcome::failed);
    results.extend(switched);
    if activated {
        results.extend(verify(checkout.tree.as_deref(), &checkout.running));
    }
    Ok(())
}

fn save(state: &Path, results: &[Outcome]) -> Result<PathBuf, String> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let list: Vec<Value> = results.iter().map(Outcome::to_json).collect();
    let report = json!({"time": time, "results": list});
    let file = state.join("update.json");
    let temporary = state.join("update.json.tmp");
    std::fs::create_dir_all(state)
        .and_then(|()| std::fs::write(&temporary, format!("{report:#}\n")))
        .and_then(|()| std::fs::rename(&temporary, &file))
        .map_err(|err| format!("cannot save {}: {err}", file.display()))?;
    Ok(file)
}

/// Runs one update and returns whether every task succeeded.
pub fn run(state: &Path) -> Result<bool, String> {
    // A run that could not go on failed too: it is saved, and the hook runs.
    let mut results = Vec::new();
    let error = decide(&mut results).err();
    if let Some(error) = &error {
        results.push(outcome("update", "failed", error.replace('\n', " | ")));
    }
    let saved = save(state, &results)?;
    // The hook reads the saved results; when it fails, that is a result too.
    if results
        .iter()
        .any(|result| result.failed() || result.reported())
    {
        if let Some(failed) = hooks::failure(&saved) {
            results.push(outcome("hook", "failed", failed));
            save(state, &results)?;
        }
    }
    if let Some(mut error) = error {
        if let Some(hook) = results.iter().find(|result| result.task == "hook") {
            error.push_str(&format!("\nThe failure hook failed too: {}", hook.detail));
        }
        return Err(error);
    }
    let list: Vec<Value> = results.iter().map(Outcome::to_json).collect();
    let rows = status::result_rows(&list);
    if rows.is_empty() {
        output::note("Nothing changed.");
    }
    status::print_rows(&rows);
    Ok(!results.iter().any(Outcome::failed))
}
