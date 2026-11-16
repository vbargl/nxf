//! The tasks, and planning and running them. Tasks run in the order they are
//! defined here, whatever order they are given in, each at most once. A task
//! that takes inputs receives the flake inputs named among the tasks. A
//! shortcut names a set of tasks, which run in their own places of that order.

use std::path::Path;

use serde_json::{json, Value};

use crate::text::strip_ansi;
use crate::{flake, git, nixos, output};

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
        return Err(format!(
            "Unknown task or input: {}. Tasks are {}; inputs are {}; status, doctor, version, hold, release and auto-update are separate commands.",
            unknown.join(", "),
            names.join(", "),
            inputs.join(", ")
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
    last: bool,
    upgrade: bool,
    /// The commit an earlier update task made, if any.
    update_commit: &'a str,
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

/// Failures describe observed effects without promising activation rollback.
fn stopped(run: &Run, title: &str, message: &str, retry: &str, kind: Stopped) -> Stop {
    let title = if run.upgrade {
        format!("Upgrade stopped: {}", title.to_lowercase())
    } else {
        title.to_string()
    };
    output::failure(&title);
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
    } else {
        if kind.build {
            output::effect_err("Activation was not started for this task.");
        }
        output::effect_err("Later tasks were not run.");
    }
    output::line_err("");
    output::detail_err(message);
    output::line_err("");
    output::effect_err(if kind.activation {
        "Inspect the diagnostics above before retrying:"
    } else {
        "Resolve the error above, then retry:"
    });
    output::command_err(retry);
    Stop::Reported
}

struct InputResult {
    name: String,
    outcome: &'static str,
    detail: String,
    summary: String,
}

/// Leaves the lockfile committed or restored, never changed.
fn update_input(dir: &Path, input: &str) -> Result<InputResult, String> {
    let before = flake::lock(dir)?;
    let attempt = || -> Result<(), String> {
        flake::update(dir, &[input.to_string()])?;
        if git::changed(dir, "flake.lock") {
            let system = nixos::build(dir)?;
            git::commit_file(dir, "flake.lock", &format!("flake.lock: update {input}"))?;
            if let Some(tree) = git::clean_tree(dir) {
                nixos::remember(&tree, &system)?;
            }
        }
        Ok(())
    };
    let failure = attempt().err();
    if git::changed(dir, "flake.lock") {
        git::restore(dir, "flake.lock")?;
    }
    let after = flake::lock(dir)?;
    let change = flake::input_updates(&before, &after, &[input.to_string()])
        .into_iter()
        .next();
    let name = input.to_string();
    Ok(if let Some(detail) = failure {
        InputResult {
            name,
            outcome: "failed",
            detail,
            summary: "failed; lockfile restored".to_string(),
        }
    } else if before == after {
        InputResult {
            name,
            outcome: "unchanged",
            detail: String::new(),
            summary: format!(
                "{}  unchanged",
                change.map(|change| change.after).unwrap_or_default()
            ),
        }
    } else {
        InputResult {
            name,
            outcome: "updated",
            detail: String::new(),
            summary: match change {
                Some(change) if change.changed => format!("{} → {}", change.before, change.after),
                _ => "lockfile updated".to_string(),
            },
        }
    })
}

fn update(run: &Run, inputs: &[String]) -> Result<(), Stop> {
    let dir = run.dir;
    output::start("Checking input updates");
    // Never built, so never committed: left by an interrupted update.
    if git::changed(dir, "flake.lock") {
        git::restore(dir, "flake.lock")?;
        output::warning("flake.lock had uncommitted changes; they were restored.");
    }
    output::detail(&format!("Reading inputs from {}/flake.lock", dir.display()));
    let before = flake::lock(dir)?;
    let selected = if inputs.is_empty() {
        flake::root_inputs(&before)
    } else {
        inputs.to_vec()
    };
    output::detail(&format!("Inputs    {}", selected.join(", ")));
    let mut results = Vec::new();
    for input in &selected {
        output::detail(&format!("Checking input {input}"));
        let result = update_input(dir, input)?;
        output::detail(&format!("  {}", result.summary));
        results.push(result);
    }
    let unchanged = flake::lock(dir)? == before;
    output::detail(&if unchanged {
        "No lockfile commit needed".to_string()
    } else {
        format!("Commit    {}", git::commit_line(dir, "HEAD"))
    });
    let failed: Vec<&InputResult> = results
        .iter()
        .filter(|result| result.outcome == "failed")
        .collect();
    if !failed.is_empty() {
        let mut message: Vec<String> = failed
            .iter()
            .map(|result| format!("{}: {}", result.name, result.detail))
            .collect();
        message.push("The lockfile was restored for these inputs.".to_string());
        let names: Vec<&str> = failed.iter().map(|result| result.name.as_str()).collect();
        return Err(stopped(
            run,
            "Input update failed",
            &message.join("\n"),
            &format!("nxf update {}", names.join(" ")),
            Stopped::default(),
        ));
    }
    output::done(if unchanged {
        "Inputs already up to date"
    } else {
        "Inputs updated"
    });
    if run.last {
        if unchanged {
            output::success("All selected inputs are up to date");
            output::effect("No lockfile commit was needed.");
        } else {
            let count = results
                .iter()
                .filter(|result| result.outcome == "updated")
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
    let host = nixos::hostname();
    output::start("Building configuration");
    output::detail(&host);
    output::detail(&format!(
        "Resolving nixosConfigurations.{host}.config.system.build.toplevel"
    ));
    let retry = format!("nxf {action}");
    let built = nixos::build(dir).map_err(|err| {
        stopped(
            run,
            "Configuration build failed",
            &err,
            &retry,
            Stopped {
                build: true,
                ..Stopped::default()
            },
        )
    })?;
    match &tree {
        Some(tree) => nixos::remember(tree, &built)?,
        None => {
            output::warning("uncommitted changes: this build cannot be identified by nxf status.")
        }
    }
    output::done("Configuration built");
    if built == running {
        output::detail("System unchanged · no package changes");
    } else if running.exists() {
        match nixos::changes(&running, &built) {
            Ok(lines) => lines.iter().for_each(|line| output::detail(line)),
            Err(err) => output::detail(&format!("Package summary unavailable: {err}")),
        }
    }
    output::detail(&format!("System    {}", built.display()));
    if action != "build" {
        output::start(if action == "boot" {
            "Configuring next boot"
        } else {
            "Activating configuration"
        });
        if action == "test" {
            output::detail("Boot default will not be changed");
        }
        nixos::activate(action, &built).map_err(|err| {
            stopped(
                run,
                if action == "boot" {
                    "Could not configure the next boot"
                } else {
                    "Activation did not finish successfully"
                },
                &err,
                &retry,
                Stopped {
                    activation: true,
                    boot: action == "boot",
                    ..Stopped::default()
                },
            )
        })?;
        output::done(if action == "boot" {
            "Boot default configured"
        } else {
            "Configuration activated"
        });
        if action == "switch" {
            output::detail("Boot default updated");
        }
    }
    if run.last {
        output::success(if run.upgrade {
            "Upgrade applied"
        } else {
            match action {
                "build" if built == running => "Configuration built; system unchanged",
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
        if matches!(action, "test" | "switch") && built == running {
            output::detail("The system path is unchanged; activation was still performed.");
        }
    }
    Ok(())
}

/// The result of one task, as `--json` and the automatic updater report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub task: String,
    pub inputs: Vec<String>,
    /// `done`, `unchanged`, `failed`, or the updater's `skipped`.
    pub outcome: String,
    pub detail: String,
}

impl Outcome {
    pub fn failed(&self) -> bool {
        self.outcome == "failed"
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
            !line.starts_with("Resolve the error") && !line.starts_with("Inspect the diagnostics")
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Runs the given tasks in order and returns one outcome per task that ran.
/// A failure stops later tasks. Quiet runs print nothing.
pub fn run(given: &[String], quiet: bool) -> Result<Vec<Outcome>, String> {
    let dir = flake::checkout()?;
    let plan = plan(given, &flake::root_inputs(&flake::lock(&dir)?))?;
    let chosen: Vec<&Task> = plan.tasks.iter().filter_map(|name| task(name)).collect();
    let total: usize = chosen.iter().map(|task| task.phases).sum();
    let mut offset = 0;
    let mut update_commit = String::new();
    let mut outcomes = Vec::new();
    if !quiet {
        output::line("");
        output::detail(&format!("{}  {}", nixos::hostname(), dir.display()));
    }
    for (index, task) in chosen.iter().enumerate() {
        let inputs: &[String] = if task.takes_inputs { &plan.inputs } else { &[] };
        let before = flake::lock(&dir)?;
        let context = Run {
            dir: &dir,
            last: index + 1 == chosen.len(),
            upgrade: given.iter().any(|word| word == "upgrade"),
            update_commit: &update_commit,
        };
        output::phases(offset, total);
        if quiet {
            output::capture();
        }
        let result = match task.name {
            "update" => update(&context, inputs),
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
