//! The unattended update the timer starts. The tasks do every operation and
//! report their results; this only decides what comes next, and saves the
//! results for `nxf status`.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::tasks::{self, Outcome};
use crate::{output, status};

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn decide() -> Result<Vec<Outcome>, String> {
    // A lockfile left by an interrupted run is not work in progress; update restores it.
    let foreign = status::compare()?
        .changes
        .iter()
        .any(|file| file != "flake.lock");
    if foreign {
        return Ok(vec![Outcome {
            task: "update".to_string(),
            inputs: Vec::new(),
            outcome: "skipped".to_string(),
            detail: "the checkout has uncommitted changes".to_string(),
        }]);
    }
    // Every input that builds is committed; the others stay as they were.
    let mut results = tasks::run(&words(&["update"]), true)?;
    // Whatever is committed now has been built, by this run or an earlier one.
    if !status::compare()?.matches {
        results.extend(tasks::run(&words(&["switch"]), true)?);
    }
    Ok(results)
}

fn save(state: &Path, results: &[Outcome]) -> Result<(), String> {
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
        .map_err(|err| format!("cannot save {}: {err}", file.display()))
}

/// Runs one update and returns whether every task succeeded.
pub fn run(state: &Path) -> Result<bool, String> {
    let results = decide()?;
    save(state, &results)?;
    for result in &results {
        let name = [result.task.clone()]
            .into_iter()
            .chain(result.inputs.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        let detail = if result.detail.is_empty() {
            String::new()
        } else {
            format!(". {}", result.detail)
        };
        output::line(&format!("{name}: {}{detail}", result.outcome));
    }
    Ok(!results.iter().any(Outcome::failed))
}
