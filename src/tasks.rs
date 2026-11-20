//! The tasks, and planning and running them. Tasks run in the order they are
//! defined here, whatever order they are given in, each at most once. A task
//! that takes inputs receives the flake inputs named among the tasks. A
//! shortcut names a set of tasks, which run in their own places of that order.

use std::path::Path;

use serde_json::{json, Value};

use crate::output::Mark;
use crate::strategy::{Strategies, Strategy};
use crate::text::strip_ansi;
use crate::{flake, git, nixos, output, process, state};

pub struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub takes_inputs: bool,
    /// Phase headings the task prints: one for the build, one for activation.
    phases: usize,
}

pub struct Shortcut {
    pub name: &'static str,
    pub description: &'static str,
    pub expands: &'static [&'static str],
}

pub const TASKS: &[Task] = &[
    Task {
        name: "update",
        description: "Update all inputs, or only the named ones, one at a time. Each update is built, then committed; one that does not build is restored.",
        takes_inputs: true,
        phases: 1,
    },
    Task {
        name: "build",
        description: "Build the configuration and show what would change.",
        takes_inputs: false,
        phases: 1,
    },
    Task {
        name: "test",
        description: "Activate the configuration until the next reboot.",
        takes_inputs: false,
        phases: 2,
    },
    Task {
        name: "boot",
        description: "Make the configuration the boot default.",
        takes_inputs: false,
        phases: 2,
    },
    Task {
        name: "switch",
        description: "Activate the configuration and make it the boot default.",
        takes_inputs: false,
        phases: 2,
    },
];

pub const SHORTCUTS: &[Shortcut] = &[Shortcut {
    name: "upgrade",
    description: "Update each input, then switch.",
    expands: &["update", "switch"],
}];

fn task(name: &str) -> Option<&'static Task> {
    TASKS.iter().find(|task| task.name == name)
}

/// A task or shortcut as the help and the planner see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defined {
    pub name: &'static str,
    pub description: String,
    pub takes_inputs: bool,
    pub shortcut: bool,
    pub expands: Vec<&'static str>,
}

/// The tasks in definition order, then the shortcuts.
pub fn defined() -> Vec<Defined> {
    let tasks = TASKS.iter().map(|task| Defined {
        name: task.name,
        description: task.description.to_string(),
        takes_inputs: task.takes_inputs,
        shortcut: false,
        expands: vec![task.name],
    });
    let shortcuts = SHORTCUTS.iter().map(|shortcut| Defined {
        name: shortcut.name,
        description: format!(
            "{} ({})",
            shortcut.description,
            shortcut.expands.join(" + ")
        ),
        takes_inputs: shortcut
            .expands
            .iter()
            .any(|name| task(name).is_some_and(|task| task.takes_inputs)),
        shortcut: true,
        expands: shortcut.expands.to_vec(),
    });
    tasks.chain(shortcuts).collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub tasks: Vec<&'static str>,
    pub inputs: Vec<String>,
}

/// Turns the given words into the tasks to run, in definition order, after
/// expanding shortcuts. Words naming one of the given inputs go to the tasks
/// that take inputs.
pub fn plan(given: &[String], inputs: &[String]) -> Result<Plan, String> {
    let defined = defined();
    let names: Vec<&str> = defined.iter().map(|entry| entry.name).collect();
    let named: Vec<&String> = given
        .iter()
        .filter(|word| !names.contains(&word.as_str()))
        .collect();
    let unknown: Vec<&str> = named
        .iter()
        .filter(|word| !inputs.contains(word))
        .map(|word| word.as_str())
        .collect();
    if !unknown.is_empty() {
        let width = output::LABEL;
        return Err(format!(
            "Unknown task or input: {}\n{:<width$}{}\n{:<width$}{}\n{:<width$}status, doctor, version, init, install, rollout, hold, release, auto-update",
            unknown.join(", "),
            "Tasks",
            names.join(", "),
            "Inputs",
            inputs.join(", "),
            "Commands",
        ));
    }
    let wanted: Vec<&str> = defined
        .iter()
        .filter(|entry| given.iter().any(|word| word == entry.name))
        .flat_map(|entry| entry.expands.iter().copied())
        .collect();
    let selected: Vec<&'static Task> = TASKS
        .iter()
        .filter(|task| wanted.contains(&task.name))
        .collect();
    if !named.is_empty() && !selected.iter().any(|task| task.takes_inputs) {
        let takers: Vec<&str> = defined
            .iter()
            .filter(|entry| entry.takes_inputs)
            .map(|entry| entry.name)
            .collect();
        return Err(format!(
            "Inputs {} need the {} task.",
            named
                .iter()
                .map(|word| word.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            takers.join(" or ")
        ));
    }
    let mut chosen: Vec<String> = Vec::new();
    for word in named {
        if !chosen.contains(word) {
            chosen.push(word.clone());
        }
    }
    Ok(Plan {
        tasks: selected.iter().map(|task| task.name).collect(),
        inputs: chosen,
    })
}

/// What a task needs to know about the run it is part of.
struct Run<'a> {
    dir: &'a Path,
    /// The configuration of the flake that is built.
    configuration: &'a str,
    last: bool,
    upgrade: bool,
    /// The commit an earlier update task made, if any.
    update_commit: &'a str,
    /// How the automatic update treats each input. None for a run by hand,
    /// which updates what it is asked to and reports every failure.
    strategies: Option<&'a Strategies>,
}

impl Run<'_> {
    fn strategy(&self, input: &str) -> Option<Strategy> {
        self.strategies.map(|strategies| strategies.of(input))
    }
}

/// A failed task has either printed its own failure, or left an error to print.
enum Stop {
    Reported,
    Error(String),
}

impl From<String> for Stop {
    fn from(error: String) -> Self {
        Stop::Error(error)
    }
}

#[derive(Default)]
struct Stopped {
    activation: bool,
    boot: bool,
    build: bool,
}

/// The cause in a tool's error, and the exit status line that led it.
fn cause(error: &str) -> (Vec<&str>, Option<&str>) {
    let mut lines: Vec<&str> = error.lines().collect();
    let exit = lines
        .first()
        .is_some_and(|line| process::is_failure_line(line))
        .then(|| lines.remove(0));
    (lines, exit)
}

/// Reports a failure: the cause first, what it means for the run in muted
/// text, then the command to retry. It does not promise an activation rollback.
fn stopped(run: &Run, title: &str, causes: &[String], retry: &str, kind: Stopped) -> Stop {
    let title = if run.upgrade {
        format!("Upgrade stopped: {}", title.to_lowercase())
    } else {
        title.to_string()
    };
    output::failure(&title);
    let mut exits: Vec<&str> = Vec::new();
    for error in causes {
        let (lines, exit) = cause(error);
        if lines.is_empty() {
            output::effect_err(error);
        }
        for line in lines {
            output::effect_err(line);
        }
        exits.extend(exit.filter(|exit| !exits.contains(exit)));
    }
    for exit in exits {
        output::note_err(exit);
    }
    if !run.update_commit.is_empty() {
        output::effect_err(&format!(
            "Input updates were committed: {}.",
            run.update_commit
        ));
    }
    if kind.activation {
        output::effect_err(if kind.boot {
            "The running system was not switched; verify boot entries before rebooting."
        } else {
            "Some changes may already be active."
        });
    } else if kind.build {
        output::note_err("Activation was not started. Later tasks were not run.");
    } else {
        output::note_err("The lockfile was restored for these inputs. Later tasks were not run.");
    }
    output::line_err("");
    output::effect_err(if kind.activation {
        "Inspect the diagnostics before retrying:"
    } else {
        "Retry after fixing the error:"
    });
    output::command_err(retry);
    Stop::Reported
}

struct InputResult {
    name: String,
    mark: Mark,
    /// The failure of the update, when there was one.
    error: Option<String>,
    summary: String,
}

/// Leaves the lockfile committed or restored, never changed.
fn update_input(dir: &Path, configuration: &str, input: &str) -> Result<InputResult, String> {
    let before = flake::lock(dir)?;
    let attempt = || -> Result<(), String> {
        let base = git::clean_tree(dir);
        flake::update(dir, &[input.to_string()])?;
        if git::changed(dir, "flake.lock") {
            let source = base.as_ref().and_then(|tree| git::worktree_diff(dir, tree));
            let system = nixos::build(dir, configuration)?;
            git::commit_file(dir, "flake.lock", &format!("flake.lock: update {input}"))?;
            if base.is_none() {
                output::warning(
                    "uncommitted changes: this build cannot be identified by nxf status.",
                );
            } else if source.is_some()
                && base.as_ref().and_then(|tree| git::worktree_diff(dir, tree)) == source
            {
                if let Some(tree) = git::clean_tree(dir) {
                    state::remember(&tree, &system);
                }
            } else {
                output::warning("checkout changed during the update build: this build cannot be identified by nxf status.");
            }
        }
        Ok(())
    };
    let error = attempt().err();
    if git::changed(dir, "flake.lock") {
        git::restore(dir, "flake.lock")?;
    }
    let after = flake::lock(dir)?;
    let change = flake::input_updates(&before, &after, &[input.to_string()])
        .into_iter()
        .next();
    let name = input.to_string();
    Ok(if error.is_some() {
        InputResult {
            name,
            mark: Mark::Failed,
            error,
            summary: "failed, lockfile restored".to_string(),
        }
    } else if before == after {
        InputResult {
            name,
            mark: Mark::Quiet,
            error: None,
            summary: format!(
                "{}   unchanged",
                change.map(|change| change.after).unwrap_or_default()
            ),
        }
    } else {
        InputResult {
            name,
            mark: Mark::Done,
            error: None,
            summary: match change {
                Some(change) if change.changed => format!("{} → {}", change.before, change.after),
                _ => "lockfile updated".to_string(),
            },
        }
    })
}

/// The sources of the locked inputs stay in the Nix store, so that the next
/// build, by whoever, needs no access to where an input came from.
fn keep_inputs(dir: &Path) {
    let kept = flake::archive(dir).and_then(|paths| state::keep_sources(&paths));
    if let Err(err) = kept {
        let cause = err.lines().last().unwrap_or_default().trim();
        output::warning(&format!(
            "the sources of the inputs could not be kept in the Nix store: {cause}"
        ));
    }
}

/// What a failed input update said, on one line.
fn said(error: &str) -> String {
    strip_ansi(error)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn update(run: &Run, inputs: &[String], noted: &mut Vec<Outcome>) -> Result<(), Stop> {
    let dir = run.dir;
    output::start("Checking input updates");
    // Never built, so never committed: left by an interrupted update.
    if git::changed(dir, "flake.lock") {
        git::restore(dir, "flake.lock")?;
        output::warning("flake.lock had uncommitted changes; they were restored.");
    }
    let before = flake::lock(dir)?;
    let selected: Vec<String> = if inputs.is_empty() {
        // The automatic update leaves alone what is updated by hand.
        flake::root_inputs(&before)
            .into_iter()
            .filter(|input| run.strategy(input) != Some(Strategy::Off))
            .collect()
    } else {
        inputs.to_vec()
    };
    // Input names share the label column, widened for long names.
    let width = selected
        .iter()
        .map(|name| name.chars().count() + 2)
        .max()
        .unwrap_or(0)
        .max(output::LABEL);
    let mut results = Vec::new();
    for input in &selected {
        let pending = output::pending(&format!("Checking input {input}"));
        let result = update_input(dir, run.configuration, input);
        output::settle(pending);
        let result = result?;
        output::row(
            result.mark,
            &format!("{:<width$}{}", result.name, result.summary),
        );
        results.push(result);
    }
    let unchanged = flake::lock(dir)? == before;
    if !unchanged {
        output::sub("Commit", &git::commit_line(dir, "HEAD"));
        keep_inputs(dir);
    }
    let mut failed: Vec<&InputResult> = Vec::new();
    for result in results.iter().filter(|result| result.mark == Mark::Failed) {
        // To the automatic update a failed input is a record, not a failure of the run.
        let outcome = match run.strategy(&result.name) {
            Some(Strategy::Flag) => "flagged",
            Some(Strategy::Report) => "reported",
            Some(Strategy::Off) | None => {
                failed.push(result);
                continue;
            }
        };
        noted.push(Outcome {
            task: "update".to_string(),
            inputs: vec![result.name.clone()],
            outcome: outcome.to_string(),
            detail: said(result.error.as_deref().unwrap_or_default()),
        });
    }
    if !failed.is_empty() {
        let causes: Vec<String> = failed
            .iter()
            .map(|result| {
                let error = result.error.as_deref().unwrap_or_default();
                let (lines, exit) = cause(error);
                // The first line of the cause names its input.
                match lines.split_first() {
                    Some((first, rest)) => exit
                        .map(str::to_string)
                        .into_iter()
                        .chain([format!("{}: {first}", result.name)])
                        .chain(rest.iter().map(|line| line.to_string()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    None => format!("{}: {error}", result.name),
                }
            })
            .collect();
        let names: Vec<&str> = failed.iter().map(|result| result.name.as_str()).collect();
        return Err(stopped(
            run,
            "Input update failed",
            &causes,
            &format!("nxf update {}", names.join(" ")),
            Stopped::default(),
        ));
    }
    if run.last {
        if unchanged {
            output::success("All selected inputs are up to date");
            output::effect("No lockfile commit was needed.");
        } else {
            let count = results
                .iter()
                .filter(|result| result.mark == Mark::Done)
                .count();
            let noun = if count == 1 { "input" } else { "inputs" };
            output::success(&format!("Updated {count} {noun}"));
            output::effect("Each update was built and committed.");
        }
    }
    Ok(())
}

/// nh evaluates the flake once. Activation then uses that exact store path,
/// keeping nh's validation, elevation and specialisation handling.
fn system(run: &Run, action: &str) -> Result<(), Stop> {
    let dir = run.dir;
    let tree = git::clean_tree(dir);
    let running = nixos::running();
    output::start("Building configuration");
    let retry = format!("nxf {action}");
    let built = nixos::build(dir, run.configuration).map_err(|err| {
        stopped(
            run,
            "Configuration build failed",
            &[err],
            &retry,
            Stopped {
                build: true,
                ..Stopped::default()
            },
        )
    })?;
    match tree.filter(|before| git::clean_tree(dir).as_ref() == Some(before)) {
        Some(tree) => {
            state::remember(&tree, &built);
            keep_inputs(dir);
        }
        None => {
            output::warning("uncommitted changes or checkout changed during the build: this build cannot be identified by nxf status.")
        }
    }
    let unchanged = built == running;
    output::row(
        Mark::Done,
        if unchanged {
            "Built, system unchanged"
        } else {
            "Built"
        },
    );
    output::sub("System", &built.to_string_lossy());
    if !unchanged && running.exists() {
        match nixos::changes(&running, &built) {
            Ok(facts) => facts
                .iter()
                .for_each(|(label, value)| output::sub(label, value)),
            Err(err) => output::sub("Packages", &format!("summary unavailable: {err}")),
        }
    }
    if action != "build" {
        output::start(if action == "boot" {
            "Configuring next boot"
        } else {
            "Activating configuration"
        });
        nixos::activate(action, &built).map_err(|err| {
            stopped(
                run,
                if action == "boot" {
                    "Could not configure the next boot"
                } else {
                    "Activation did not finish successfully"
                },
                &[err],
                &retry,
                Stopped {
                    activation: true,
                    boot: action == "boot",
                    ..Stopped::default()
                },
            )
        })?;
        output::row(
            Mark::Done,
            match action {
                "test" => "Activated until reboot, boot default not changed",
                "boot" => "Boot default configured",
                _ => "Activated, boot default updated",
            },
        );
    }
    if run.last {
        output::success(if run.upgrade {
            "Upgrade applied"
        } else {
            match action {
                "build" if unchanged => "Configuration built; system unchanged",
                "build" => "Configuration built",
                "test" => "Configuration active until reboot",
                "boot" => "Next boot configured",
                _ => "Configuration activated",
            }
        });
        output::effect(match action {
            "build" => "Ready to use. The running system was not switched.",
            "test" => "The boot default was not changed.",
            "boot" => "The current system continues running.",
            _ => "Active now and configured for the next boot.",
        });
        if matches!(action, "test" | "switch") && unchanged {
            output::note("The system path is unchanged; activation was still performed.");
        }
    }
    Ok(())
}

/// The result of one task, as `--json` and the automatic updater report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub task: String,
    pub inputs: Vec<String>,
    /// `done`, `unchanged`, `failed`, or the updater's `skipped`, `flagged` and `reported`.
    pub outcome: String,
    pub detail: String,
}

impl Outcome {
    pub fn failed(&self) -> bool {
        self.outcome == "failed"
    }

    /// An input the automatic update could not update, and is to report.
    pub fn reported(&self) -> bool {
        self.outcome == "reported"
    }

    pub fn to_json(&self) -> Value {
        json!({
            "task": self.task,
            "inputs": self.inputs,
            "outcome": self.outcome,
            "detail": self.detail,
        })
    }
}

/// What a failed task said, without its styling and recovery hint.
fn diagnostics(stderr: &str) -> String {
    strip_ansi(stderr)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take_while(|line| {
            !line.starts_with("Retry after fixing") && !line.starts_with("Inspect the diagnostics")
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Runs the given tasks in order and returns one outcome per task that ran.
/// A failure stops later tasks. Quiet runs print nothing.
pub fn run(given: &[String], quiet: bool) -> Result<Vec<Outcome>, String> {
    run_with(given, quiet, None)
}

/// Runs the tasks for the automatic update, quietly and with its strategies:
/// an input that is off is not updated, and every other input whose update
/// fails adds an outcome of its own, `flagged` or `reported`, while the update
/// goes on.
pub fn run_unattended(given: &[String], strategies: &Strategies) -> Result<Vec<Outcome>, String> {
    run_with(given, true, Some(strategies))
}

fn run_with(
    given: &[String],
    quiet: bool,
    strategies: Option<&Strategies>,
) -> Result<Vec<Outcome>, String> {
    let dir = flake::checkout()?;
    let plan = plan(given, &flake::root_inputs(&flake::lock(&dir)?))?;
    let chosen: Vec<&Task> = plan.tasks.iter().filter_map(|name| task(name)).collect();
    let configuration = flake::configuration(&dir)?;
    let total: usize = chosen.iter().map(|task| task.phases).sum();
    let mut offset = 0;
    let mut update_commit = String::new();
    let mut outcomes = Vec::new();
    if !quiet {
        output::line("");
        output::context(&format!("{} · {}", nixos::hostname(), dir.display()));
    }
    for (index, task) in chosen.iter().enumerate() {
        let inputs: &[String] = if task.takes_inputs { &plan.inputs } else { &[] };
        let before = flake::lock(&dir)?;
        let context = Run {
            dir: &dir,
            configuration: &configuration,
            last: index + 1 == chosen.len(),
            upgrade: given.iter().any(|word| word == "upgrade"),
            update_commit: &update_commit,
            strategies,
        };
        let mut noted = Vec::new();
        output::phases(offset, total);
        if quiet {
            output::capture();
        }
        let result = match task.name {
            "update" => update(&context, inputs, &mut noted),
            action => system(&context, action),
        };
        if let Err(Stop::Error(error)) = &result {
            output::failure("Task failed");
            output::effect_err(error);
        }
        let captured = if quiet {
            output::release()
        } else {
            output::Captured::default()
        };
        let changed = flake::lock(&dir).map_or(true, |lock| lock != before);
        outcomes.push(Outcome {
            task: task.name.to_string(),
            inputs: inputs.to_vec(),
            outcome: if result.is_err() {
                "failed"
            } else if task.name == "update" && !changed {
                "unchanged"
            } else {
                "done"
            }
            .to_string(),
            detail: if result.is_err() {
                diagnostics(&captured.stderr)
            } else {
                String::new()
            },
        });
        outcomes.extend(noted);
        if result.is_err() {
            break;
        }
        if task.name == "update" && changed {
            update_commit = git::commit_line(&dir, "HEAD");
        }
        offset += task.phases;
    }
    Ok(outcomes)
}

/// The tasks a run would perform, each with its inputs.
pub fn dry_run(given: &[String]) -> Result<Vec<String>, String> {
    let dir = flake::checkout()?;
    let plan = plan(given, &flake::root_inputs(&flake::lock(&dir)?))?;
    Ok(plan
        .tasks
        .iter()
        .filter_map(|name| task(name))
        .map(|task| {
            let mut words = vec![task.name.to_string()];
            if task.takes_inputs {
                words.extend(plan.inputs.iter().cloned());
            }
            words.join(" ")
        })
        .collect())
}

pub fn to_json(outcomes: &[Outcome]) -> String {
    let list: Vec<Value> = outcomes.iter().map(Outcome::to_json).collect();
    serde_json::to_string_pretty(&list).unwrap_or_default()
}
