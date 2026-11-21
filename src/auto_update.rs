//! The unattended update the timer starts. It decides which task comes next,
//! checks the health of the system it switched to and returns to the one before
//! when that fails, and saves the results for `nxf status`.

use std::path::{Path, PathBuf};

use crate::health::{self, Health};
use crate::results::{Kind, Outcome, Saved};
use crate::state::Tree;
use crate::tasks::{self, Unattended, SWITCH, UPDATE};
use crate::{flake, git, hold, hooks, nixos, output, process, state, status, strategy, time};

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn outcome(task: &str, kind: Kind, details: Vec<String>) -> Outcome {
    Outcome::new(task, Vec::new(), kind, details)
}

/// Checks the health of the system just switched to. An unhealthy system is
/// replaced by the previous one, and its source is not tried again.
fn verify(tree: Option<&Tree>, previous: &Path) -> Option<Outcome> {
    let failed = match health::check() {
        Health::Unchecked => return None,
        Health::Healthy => return Some(outcome("health", Kind::Done, Vec::new())),
        Health::Unhealthy(failed) => failed,
    };
    let mut said = vec![failed.clone()];
    // A mark that cannot be written is said, and never keeps the unhealthy system.
    if let Some(Err(err)) = tree.map(|tree| state::mark_bad(tree, &failed)) {
        said.push(err);
    }
    output::capture();
    let returned = nixos::activate(SWITCH, previous);
    output::release();
    match returned {
        Ok(_) => said.push(format!("Returned to {}.", previous.display())),
        Err(err) => {
            let cause = process::cause_first(&err);
            let mut lines = cause.lines();
            said.push(format!(
                "Could not return to {}: {}",
                previous.display(),
                lines.next().unwrap_or_default()
            ));
            said.extend(lines.map(str::to_string));
        }
    }
    Some(outcome("health", Kind::Failed, said))
}

/// Runs what comes next and adds its results as they come, so that a run
/// that cannot go on keeps the results of what ran before.
fn decide(results: &mut Vec<Outcome>) -> Result<(), String> {
    hold::forget_ended();
    if let Some(hold) = hold::active() {
        results.push(outcome(
            UPDATE,
            Kind::Skipped,
            vec![format!("held: {}", hold.describe())],
        ));
        return Ok(());
    }
    let dir = flake::checkout()?;
    // A lockfile left by an interrupted run is not work in progress; update restores it.
    let foreign = git::changed_files(&dir)
        .iter()
        .any(|file| file != "flake.lock");
    if foreign {
        results.push(outcome(
            UPDATE,
            Kind::Skipped,
            vec!["the checkout has uncommitted changes".to_string()],
        ));
        return Ok(());
    }
    if let Some(refused) = hooks::refused() {
        results.push(outcome(UPDATE, Kind::Skipped, vec![refused]));
        return Ok(());
    }
    // What is committed says how each input is treated.
    let configuration = flake::configuration(&dir)?;
    let strategies = strategy::read(&dir, &configuration)?;
    let setup = Unattended {
        dir,
        configuration,
        strategies,
    };
    // Every input that builds is committed; the others stay as they were.
    // Only tasks are given, so no word is unknown and no command is named.
    results.extend(tasks::run_unattended(&words(&[UPDATE]), &setup, &[])?);
    if results.iter().any(Outcome::failed) {
        return Ok(());
    }
    // Whatever is committed now has been built, by this run or an earlier one.
    let checkout = status::compare()?;
    if checkout.matches {
        return Ok(());
    }
    if let Some(reason) = checkout.tree.as_ref().and_then(state::bad) {
        results.push(outcome(
            SWITCH,
            Kind::Skipped,
            vec![format!(
                "this revision failed its health check ({reason}); waiting for a newer one"
            )],
        ));
        return Ok(());
    }
    let switched = tasks::run_unattended(&words(&[SWITCH]), &setup, &[])?;
    let activated = !switched.iter().any(Outcome::failed);
    results.extend(switched);
    if activated {
        results.extend(verify(checkout.tree.as_ref(), &checkout.running));
    }
    Ok(())
}

fn save(dir: &Path, results: &[Outcome]) -> Result<PathBuf, String> {
    let report = Saved {
        time: time::now(),
        results: results.to_vec(),
    };
    let file = state::update_file(dir);
    process::write_atomic(&file, &format!("{:#}\n", report.to_json()))
        .map_err(|err| format!("cannot save {}: {err}", file.display()))?;
    Ok(file)
}

/// What failed after the run did, beneath the cause of the run; alone when
/// the run has none.
fn beneath(cause: Option<&str>, then: String) -> String {
    match cause {
        Some(cause) => format!("{cause}\n{then}"),
        None => then,
    }
}

/// Runs one update and returns whether every task succeeded.
pub fn run(state: &Path) -> Result<bool, String> {
    // A run that could not go on failed too: it is saved, and the hook runs.
    let mut results = Vec::new();
    let mut error = decide(&mut results).err();
    if let Some(error) = &error {
        let cause = process::cause_first(error);
        results.push(outcome(
            UPDATE,
            Kind::Failed,
            cause.lines().map(str::to_string).collect(),
        ));
    }
    let saved = save(state, &results).map_err(|unsaved| beneath(error.as_deref(), unsaved))?;
    // The hook reads the saved results; when it fails, that is a result too.
    if results
        .iter()
        .any(|result| result.failed() || result.reported())
    {
        if let Some(failed) = hooks::failure(&saved) {
            if let Some(error) = &mut error {
                error.push_str(&format!("\nThe failure hook failed too: {failed}"));
            }
            results.push(outcome("hook", Kind::Failed, vec![failed]));
            save(state, &results).map_err(|unsaved| beneath(error.as_deref(), unsaved))?;
        }
    }
    if let Some(error) = error {
        return Err(error);
    }
    let rows = status::result_rows(&results);
    if rows.is_empty() {
        output::note("Nothing changed.");
    }
    status::print_rows(&rows);
    Ok(!results.iter().any(Outcome::failed))
}
