//! The command line: commands are declared here and routed to their modules.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

use crate::{auto_update, doctor, hold, init, nixos, output, status, tasks};

/// Manage the machine's flake: NXF_FLAKE, or /etc/nixos.
///
/// Tasks run in the order they are defined, whatever order they are given in.
/// Name flake inputs next to update or upgrade to update only those:
/// nxf update personal test
#[derive(Parser)]
#[command(
    name = "nxf",
    disable_version_flag = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    #[command(flatten)]
    run: RunArgs,
}

#[derive(Parser)]
#[command(name = "nxf")]
struct TaskCli {
    #[command(flatten)]
    run: RunArgs,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Print the tasks without running them.
    #[arg(long)]
    dry_run: bool,

    /// Print each task's result as JSON, for scripts.
    #[arg(long)]
    json: bool,

    /// Tasks, or flake input names; run without arguments to list tasks.
    #[arg(value_name = "TASK")]
    words: Vec<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the running system's flake commit and inputs, compared with the checkout.
    Status {
        /// Show the checkout's revisions instead of the running system's.
        #[arg(long)]
        checkedout: bool,
        /// Show both, and the full package diff.
        #[arg(long)]
        detailed: bool,
        /// Print the comparison as JSON, for scripts.
        #[arg(long)]
        json: bool,
        /// Show only these flake inputs.
        inputs: Vec<String>,
    },
    /// Check that nxf can work on this machine.
    Doctor,
    /// Print the version of nxf, or with --system the running system's flake commit.
    Version {
        /// Print the flake commit of the running system.
        #[arg(long)]
        system: bool,
    },
    /// Found a machine flake from a template, as a Git repository without a remote.
    Init {
        /// Where the flake is founded; NXF_FLAKE or /etc/nixos when not given.
        dir: Option<PathBuf>,
        /// The template, as `<flake>#<name>`, like `nix flake init --template`.
        #[arg(long)]
        template: String,
        /// Write hardware.nix with nixos-generate-config, on the machine itself.
        #[arg(long)]
        hardware: bool,
    },
    /// Hold automatic updates back, with a reason, until released or until a time.
    Hold {
        /// Why updates are held; status shows it.
        #[arg(long)]
        reason: String,
        /// When the hold ends, such as 2030-01-31 or "2030-01-31 18:00".
        #[arg(long, conflicts_with = "duration")]
        until: Option<String>,
        /// How long the hold lasts, such as 12h, 3d or 2w.
        #[arg(long = "for", value_name = "DURATION")]
        duration: Option<String>,
    },
    /// Release the hold, so that automatic updates continue.
    Release,
    /// Update unattended: update inputs, switch if needed, save the results.
    AutoUpdate {
        /// Where the results are saved.
        #[arg(long)]
        state: Option<PathBuf>,
    },
}

const COMMANDS: &[&str] = &[
    "status",
    "doctor",
    "version",
    "init",
    "hold",
    "release",
    "auto-update",
    "help",
];

fn task_list() -> String {
    let mut list = String::from("Tasks:\n");
    for task in tasks::defined() {
        list.push_str(&format!("  {:<9}{}\n", task.name, task.description));
    }
    list
}

/// An error in the shape of every failure: the cause as the one outcome,
/// with what helps to resolve it beneath.
fn fail(error: &str) -> ExitCode {
    let mut lines = error.lines();
    output::failure(lines.next().unwrap_or("Failed"));
    for line in lines {
        output::effect_err(line);
    }
    ExitCode::FAILURE
}

fn run_tasks(args: RunArgs) -> ExitCode {
    if args.words.is_empty() {
        let mut help = Cli::command().after_help(task_list());
        let _ = help.print_help();
        return ExitCode::SUCCESS;
    }
    if args.dry_run {
        return match tasks::dry_run(&args.words) {
            Ok(lines) => {
                lines.iter().for_each(|line| output::line(line));
                ExitCode::SUCCESS
            }
            Err(error) => fail(&error),
        };
    }
    match tasks::run(&args.words, args.json) {
        Ok(outcomes) => {
            if args.json {
                output::line(&tasks::to_json(&outcomes));
            }
            if outcomes.iter().any(tasks::Outcome::failed) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => fail(&error),
    }
}

fn run_command(command: Cmd) -> Result<bool, String> {
    match command {
        Cmd::Status {
            checkedout,
            detailed,
            json,
            inputs,
        } => status::report(&status::Options {
            checkedout,
            detailed,
            json,
            inputs,
        })
        .map(|()| true),
        Cmd::Doctor => doctor::report().map(|()| true),
        Cmd::Version { system: true } => {
            output::line(&status::running_commit()?);
            Ok(true)
        }
        Cmd::Version { system: false } => {
            output::line(&format!("nxf {}", env!("CARGO_PKG_VERSION")));
            Ok(true)
        }
        Cmd::Init {
            dir,
            template,
            hardware,
        } => init::run(&init::Options {
            dir,
            template,
            hardware,
        })
        .map(|()| true),
        Cmd::Hold {
            reason,
            until,
            duration,
        } => {
            let until = match (until, duration) {
                (Some(until), _) => Some(hold::time(&until)?),
                (None, Some(duration)) => Some(hold::now() + hold::duration(&duration)?),
                (None, None) => None,
            };
            if until.is_some_and(|until| until <= hold::now()) {
                return Err("The hold would end in the past.".to_string());
            }
            let hold = hold::set(&reason, until)?;
            output::line(&format!("Automatic updates are held: {}", hold.describe()));
            Ok(true)
        }
        Cmd::Release => {
            output::line(if hold::clear()? {
                "Automatic updates continue."
            } else {
                "No hold is set."
            });
            Ok(true)
        }
        Cmd::AutoUpdate { state } => auto_update::run(&state.unwrap_or_else(nixos::updater_dir)),
    }
}

pub fn main() -> ExitCode {
    // A command is recognised only in first place, so that `nxf test status`
    // is an unknown task instead of a status report.
    let first = std::env::args().nth(1).unwrap_or_default();
    let wants_command = COMMANDS.contains(&first.as_str()) || first == "-h" || first == "--help";
    if !wants_command {
        return run_tasks(TaskCli::parse().run);
    }
    let cli = Cli::command().after_help(task_list());
    let matches = cli.get_matches();
    let cli = match <Cli as clap::FromArgMatches>::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match cli.command {
        Some(command) => match run_command(command) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => fail(&error),
        },
        None => run_tasks(cli.run),
    }
}
