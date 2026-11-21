//! The tasks, and planning and running them. Tasks run in the order they are
//! defined here, whatever order they are given in, each at most once. A task
//! that takes inputs receives the flake inputs named among the tasks. A
//! shortcut names a set of tasks, which run in their own places of that order.

use std::path::{Path, PathBuf};

use crate::output::Mark;
use crate::results::{Kind, Outcome};
use crate::strategy::{Strategies, Strategy};
use crate::text::strip_ansi;
use crate::{flake, git, nixos, output, process, state};

/// Runs a task with the inputs it takes. The outcomes it has besides its own,
/// such as one for an input of the automatic update, are added to the last argument.
type Runner = fn(&Run, &[String], &mut Vec<Outcome>) -> Result<(), Stop>;

/// What a task is, does and says. A new task is one entry of `TASKS`.
struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub takes_inputs: bool,
    /// Phase headings the task prints: one for the build, one for activation.
    phases: usize,
    /// Whether the task changes the lockfile. When it leaves it as it was,
    /// its outcome is `unchanged`.
    changes_lockfile: bool,
    run: Runner,
}

/// What the tasks that build the configuration say differently.
struct Build {
    /// What follows the build, if anything.
    activation: Option<Activation>,
    /// The outcome of a run that ends with the task.
    closing: &'static str,
    /// The same when the built system is the running one, if it differs.
    closing_unchanged: Option<&'static str>,
    /// What the run says beneath the outcome.
    effect: &'static str,
}

struct Activation {
    heading: &'static str,
    failure: &'static str,
    row: &'static str,
    /// Whether it changes the running system. If it fails, some changes may be active.
    live: bool,
}

struct Shortcut {
    pub name: &'static str,
    pub description: &'static str,
    pub expands: &'static [&'static str],
}

/// The names of the tasks that other code reads results of or runs.
pub const UPDATE: &str = "update";
pub const SWITCH: &str = "switch";

const TASKS: &[Task] = &[
    Task {
        name: UPDATE,
        description: "Update all inputs, or only the named ones, one at a time. Each update is built, then committed; one that does not build is restored.",
        takes_inputs: true,
        phases: 1,
        changes_lockfile: true,
        run: update,
    },
    Task {
        name: "build",
        description: "Build the configuration and show what would change.",
        takes_inputs: false,
        phases: 1,
        changes_lockfile: false,
        run: |run, _, _| {
            system(
                run,
                &Build {
                    activation: None,
                    closing: "Configuration built",
                    closing_unchanged: Some("Configuration built; system unchanged"),
                    effect: "Ready to use. The running system was not switched.",
                },
            )
        },
    },
    Task {
        name: "test",
        description: "Activate the configuration until the next reboot.",
        takes_inputs: false,
        phases: 2,
        changes_lockfile: false,
        run: |run, _, _| {
            system(
                run,
                &Build {
                    activation: Some(Activation {
                        heading: "Activating configuration",
                        failure: "Activation did not finish successfully",
                        row: "Activated until reboot, boot default not changed",
                        live: true,
                    }),
                    closing: "Configuration active until reboot",
                    closing_unchanged: None,
                    effect: "The boot default was not changed.",
                },
            )
        },
    },
    Task {
        name: "boot",
        description: "Make the configuration the boot default.",
        takes_inputs: false,
        phases: 2,
        changes_lockfile: false,
        run: |run, _, _| {
            system(
                run,
                &Build {
                    activation: Some(Activation {
                        heading: "Configuring next boot",
                        failure: "Could not configure the next boot",
                        row: "Boot default configured",
                        live: false,
                    }),
                    closing: "Next boot configured",
                    closing_unchanged: None,
                    effect: "The current system continues running.",
                },
            )
        },
    },
    Task {
        name: SWITCH,
        description: "Activate the configuration and make it the boot default.",
        takes_inputs: false,
        phases: 2,
        changes_lockfile: false,
        run: |run, _, _| {
            system(
                run,
                &Build {
                    activation: Some(Activation {
                        heading: "Activating configuration",
                        failure: "Activation did not finish successfully",
                        row: "Activated, boot default updated",
                        live: true,
                    }),
                    closing: "Configuration activated",
                    closing_unchanged: None,
                    effect: "Active now and configured for the next boot.",
                },
            )
        },
    },
];

const SHORTCUTS: &[Shortcut] = &[Shortcut {
    name: "upgrade",
    description: "Update each input, then switch.",
    expands: &[UPDATE, SWITCH],
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
/// that take inputs. `commands` are named in the error of an unknown word.
pub fn plan(given: &[String], inputs: &[String], commands: &[String]) -> Result<Plan, String> {
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
            "Unknown task or input: {}\n{:<width$}{}\n{:<width$}{}\n{:<width$}{}",
            unknown.join(", "),
            "Tasks",
            names.join(", "),
            "Inputs",
            inputs.join(", "),
            "Commands",
            commands.join(", "),
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
    /// The name of the task that runs.
    task: &'static str,
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

/// Where a run stopped, which says what the failure left behind.
enum Stopped {
    Update,
    Build,
    /// `live`: the activation changes the running system.
    Activation {
        live: bool,
    },
}

/// Reports a failure: the cause first, what it means for the run in muted
/// text, then the command to retry. It does not promise an activation rollback.
fn stopped(run: &Run, title: &str, causes: &[String], retry: &str, at: Stopped) -> Stop {
    let title = if run.upgrade {
        format!("Upgrade stopped: {}", title.to_lowercase())
    } else {
        title.to_string()
    };
    output::failure(&title);
    let mut exits: Vec<&str> = Vec::new();
    for error in causes {
        let (lines, exit) = process::split(error);
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
    match at {
        Stopped::Activation { live: true } => {
            output::effect_err("Some changes may already be active.")
        }
        Stopped::Activation { live: false } => output::effect_err(
            "The running system was not switched; verify boot entries before rebooting.",
        ),
        Stopped::Build => output::note_err("Activation was not started. Later tasks were not run."),
        Stopped::Update => output::note_err(
            "The lockfile was restored for these inputs. Later tasks were not run.",
        ),
    }
    output::line_err("");
    output::effect_err(if matches!(at, Stopped::Activation { .. }) {
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

/// What a failed input update said, one entry for each line.
fn said(error: &str) -> Vec<String> {
    strip_ansi(error)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
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
    let width = output::widened(selected.iter().map(String::as_str));
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
        let Some(kind) = run.strategy(&result.name).and_then(Strategy::outcome) else {
            failed.push(result);
            continue;
        };
        noted.push(Outcome::new(
            run.task,
            vec![result.name.clone()],
            kind,
            said(result.error.as_deref().unwrap_or_default()),
        ));
    }
    if !failed.is_empty() {
        let causes: Vec<String> = failed
            .iter()
            .map(|result| {
                let error = result.error.as_deref().unwrap_or_default();
                let (lines, exit) = process::split(error);
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
            Stopped::Update,
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
fn system(run: &Run, build: &Build) -> Result<(), Stop> {
    let dir = run.dir;
    let tree = git::clean_tree(dir);
    let running = nixos::running();
    output::start("Building configuration");
    let retry = format!("nxf {}", run.task);
    let built = nixos::build(dir, run.configuration).map_err(|err| {
        stopped(
            run,
            "Configuration build failed",
            &[err],
            &retry,
            Stopped::Build,
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
    if let Some(activation) = &build.activation {
        output::start(activation.heading);
        nixos::activate(run.task, &built).map_err(|err| {
            stopped(
                run,
                activation.failure,
                &[err],
                &retry,
                Stopped::Activation {
                    live: activation.live,
                },
            )
        })?;
        output::row(Mark::Done, activation.row);
    }
    if run.last {
        output::success(if run.upgrade {
            "Upgrade applied"
        } else if unchanged {
            build.closing_unchanged.unwrap_or(build.closing)
        } else {
            build.closing
        });
        output::effect(build.effect);
        if unchanged
            && build
                .activation
                .as_ref()
                .is_some_and(|activation| activation.live)
        {
            output::note("The system path is unchanged; activation was still performed.");
        }
    }
    Ok(())
}

/// What a failed task said, without its styling and recovery hint. It reads
/// the texts that `stopped` prints, and stops at the first of its hints.
fn diagnostics(stderr: &str) -> Vec<String> {
    strip_ansi(stderr)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take_while(|line| {
            !line.starts_with("Retry after fixing") && !line.starts_with("Inspect the diagnostics")
        })
        .map(str::to_string)
        .collect()
}

/// Runs the given tasks in order and returns one outcome per task that ran.
/// A failure stops later tasks. Quiet runs print nothing.
pub fn run(given: &[String], quiet: bool, commands: &[String]) -> Result<Vec<Outcome>, String> {
    run_with(given, quiet, None, commands)
}

/// What the automatic update settles once for all its tasks.
pub struct Unattended {
    pub dir: PathBuf,
    /// The configuration of the flake that is built.
    pub configuration: String,
    pub strategies: Strategies,
}

/// Runs the tasks for the automatic update, quietly and with its strategies:
/// an input that is off is not updated, and every other input whose update
/// fails adds an outcome of its own, `flagged` or `reported`, while the update
/// goes on.
pub fn run_unattended(
    given: &[String],
    setup: &Unattended,
    commands: &[String],
) -> Result<Vec<Outcome>, String> {
    run_with(given, true, Some(setup), commands)
}

fn run_with(
    given: &[String],
    quiet: bool,
    setup: Option<&Unattended>,
    commands: &[String],
) -> Result<Vec<Outcome>, String> {
    let dir = match setup {
        Some(setup) => setup.dir.clone(),
        None => flake::checkout()?,
    };
    let plan = plan(given, &flake::root_inputs(&flake::lock(&dir)?), commands)?;
    let chosen: Vec<&Task> = plan.tasks.iter().filter_map(|name| task(name)).collect();
    let configuration = match setup {
        Some(setup) => setup.configuration.clone(),
        None => flake::configuration(&dir)?,
    };
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
            task: task.name,
            dir: &dir,
            configuration: &configuration,
            last: index + 1 == chosen.len(),
            upgrade: given.iter().any(|word| word == "upgrade"),
            update_commit: &update_commit,
            strategies: setup.map(|setup| &setup.strategies),
        };
        let mut noted = Vec::new();
        output::phases(offset, total);
        if quiet {
            output::capture();
        }
        let result = (task.run)(&context, inputs, &mut noted);
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
        let kind = if result.is_err() {
            Kind::Failed
        } else if task.changes_lockfile && !changed {
            Kind::Unchanged
        } else {
            Kind::Done
        };
        let details = if result.is_err() {
            diagnostics(&captured.stderr)
        } else {
            Vec::new()
        };
        outcomes.push(Outcome::new(task.name, inputs.to_vec(), kind, details));
        outcomes.extend(noted);
        if result.is_err() {
            break;
        }
        if task.changes_lockfile && changed {
            update_commit = git::commit_line(&dir, "HEAD");
        }
        offset += task.phases;
    }
    Ok(outcomes)
}

/// The tasks a run would perform, each with its inputs.
pub fn dry_run(given: &[String], commands: &[String]) -> Result<Vec<String>, String> {
    let dir = flake::checkout()?;
    let plan = plan(given, &flake::root_inputs(&flake::lock(&dir)?), commands)?;
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
