//! The command line: commands are declared here and routed to their modules.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

use crate::{
    auto_update, doctor, env, fleet, hold, hosts, init, install, output, rollout, state, status,
    tasks,
};

/// Manage the machine's flake: NXF_FLAKE, or /etc/nixos.
///
/// Tasks run in the order they are defined, whatever order they are given in.
/// Name flake inputs next to update or upgrade to update only those:
/// nxf update nixpkgs home-manager
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

/// The machines a command runs on instead of this one.
#[derive(clap::Args)]
struct Machines {
    /// Run on every machine of this hosts file, over ssh, instead of here.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
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
        #[command(flatten)]
        machines: Machines,
    },
    /// Check that nxf can work on this machine.
    Doctor {
        #[command(flatten)]
        machines: Machines,
    },
    /// Print the version of nxf, or with --system the running system's flake commit.
    Version {
        /// Print the flake commit of the running system.
        #[arg(long)]
        system: bool,
        #[command(flatten)]
        machines: Machines,
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
        /// Lay the content of this directory over the template: the machine's own values.
        #[arg(long, value_name = "DIR")]
        files: Option<PathBuf>,
    },
    /// Install the machine a flake describes, from a NixOS installer on that machine.
    Install {
        /// The machine flake, as `nxf init` founds it.
        dir: PathBuf,
        /// Do not partition: the target is already mounted at /mnt.
        #[arg(long)]
        mounted: bool,
        /// Restart into the new system when it is installed.
        #[arg(long)]
        reboot: bool,
    },
    /// Install every machine of a hosts file that nxf has not installed, or with --upgrade upgrade them in waves.
    Rollout {
        /// The hosts file.
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        /// The template of the machine flake, as `<flake>#<name>`.
        #[arg(long, required_unless_present = "upgrade")]
        template: Option<String>,
        /// Run `nxf upgrade` on the machines instead: the first alone, then in waves.
        #[arg(long, conflicts_with_all = ["template", "files", "yes", "kexec_url", "known_hosts"])]
        upgrade: bool,
        /// How many machines upgrade at once after the first; 1 when not given.
        #[arg(long, value_name = "N",
              value_parser = clap::value_parser!(u32).range(1..))]
        batch: Option<u32>,
        /// Upgrade only these flake inputs.
        #[arg(value_name = "INPUT")]
        inputs: Vec<String>,
        /// A directory with one directory of files for each machine, by name.
        #[arg(long, value_name = "DIR")]
        files: Option<PathBuf>,
        /// Erase and install without asking.
        #[arg(long)]
        yes: bool,
        /// The installer image for machines that run another system; `{arch}` is replaced.
        #[arg(long, value_name = "URL")]
        kexec_url: Option<String>,
        /// A known_hosts file that gets the host keys of the installed machines.
        #[arg(long, value_name = "FILE")]
        known_hosts: Option<PathBuf>,
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
        #[command(flatten)]
        machines: Machines,
    },
    /// Release the hold, so that automatic updates continue.
    Release {
        #[command(flatten)]
        machines: Machines,
    },
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
    "install",
    "rollout",
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

/// What clap refused, in the same shape and with clap's exit status of a
/// wrong call. Help that was asked for is printed as it is.
fn refuse(error: clap::Error) -> ExitCode {
    if !error.use_stderr() {
        let _ = error.print();
        return ExitCode::SUCCESS;
    }
    let text = error.render().to_string();
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let cause = lines.next().unwrap_or("Failed");
    output::failure(cause.strip_prefix("error: ").unwrap_or(cause));
    lines.for_each(output::effect_err);
    ExitCode::from(2)
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
            machines,
        } => match machines.config {
            Some(file) => {
                if checkedout || detailed || !inputs.is_empty() {
                    return Err(
                        "With --config, status shows one row for each machine\nOnly --json can be added."
                            .to_string(),
                    );
                }
                fleet::status(&file, json)
            }
            None => status::report(&status::Options {
                checkedout,
                detailed,
                json,
                inputs,
            })
            .map(|()| true),
        },
        Cmd::Doctor { machines } => match machines.config {
            Some(file) => fleet::each(&file, &["doctor"], "Checks passed", "Checks failed"),
            None => doctor::report().map(|()| true),
        },
        Cmd::Version { system, machines } => match machines.config {
            Some(file) if system => {
                fleet::each(&file, &["version", "--system"], "Answered", "No answer")
            }
            Some(file) => fleet::each(&file, &["version"], "Answered", "No answer"),
            None if system => {
                output::line(&status::running_commit()?);
                Ok(true)
            }
            None => {
                output::line(&format!("nxf {}", env!("CARGO_PKG_VERSION")));
                Ok(true)
            }
        },
        Cmd::Init {
            dir,
            template,
            hardware,
            files,
        } => init::run(&init::Options {
            dir,
            template,
            hardware,
            files,
        })
        .map(|()| true),
        Cmd::Install {
            dir,
            mounted,
            reboot,
        } => install::run(&install::Options {
            dir,
            mounted,
            reboot,
        })
        .map(|()| true),
        Cmd::Rollout {
            config,
            template,
            upgrade,
            batch,
            inputs,
            files,
            yes,
            kexec_url,
            known_hosts,
        } => {
            if !upgrade && (batch.is_some() || !inputs.is_empty()) {
                return Err(
                    "Inputs and --batch belong to the upgrade\nAdd --upgrade, or leave them out to install."
                        .to_string(),
                );
            }
            let machines = hosts::load(&config)?;
            if upgrade {
                return Ok(fleet::upgrade(
                    &config,
                    &machines,
                    &inputs,
                    batch.unwrap_or(1) as usize,
                ));
            }
            rollout::run(
                &rollout::Options {
                    file: config,
                    template: template.unwrap_or_default(),
                    files,
                    yes,
                    kexec_url,
                    known_hosts,
                },
                &machines,
            )
        }
        Cmd::Hold {
            reason,
            until,
            duration,
            machines,
        } => {
            let until = hold::end(until, duration)?;
            if let Some(file) = machines.config {
                return fleet::hold(&file, &reason, until);
            }
            let hold = hold::set(&reason, until)?;
            output::line(&format!("Automatic updates are held: {}", hold.describe()));
            Ok(true)
        }
        Cmd::Release { machines } => {
            if let Some(file) = machines.config {
                return fleet::each(
                    &file,
                    &["release"],
                    "Automatic updates continue",
                    "Not released",
                );
            }
            output::line(if hold::clear()? {
                "Automatic updates continue."
            } else {
                "No hold is set."
            });
            Ok(true)
        }
        Cmd::AutoUpdate { state } => auto_update::run(&state.unwrap_or_else(state::updater_dir)),
    }
}

impl Cmd {
    /// Whether the command works on the checkout of this machine's flake,
    /// which the group of the machine may share.
    fn on_checkout(&self) -> bool {
        match self {
            Cmd::Status { machines, .. } | Cmd::Doctor { machines } => machines.config.is_none(),
            Cmd::Version { system, machines } => *system && machines.config.is_none(),
            Cmd::Init { dir, .. } => dir.is_none(),
            Cmd::AutoUpdate { .. } => true,
            Cmd::Install { .. } | Cmd::Rollout { .. } | Cmd::Hold { .. } | Cmd::Release { .. } => {
                false
            }
        }
    }
}

pub fn main() -> ExitCode {
    env::enable_flakes();
    // A command is recognised only in first place, so that `nxf test status`
    // is an unknown task instead of a status report.
    let first = std::env::args().nth(1).unwrap_or_default();
    let wants_command = COMMANDS.contains(&first.as_str()) || first == "-h" || first == "--help";
    if !wants_command {
        return match TaskCli::try_parse() {
            Ok(cli) => {
                state::share_checkout();
                run_tasks(cli.run)
            }
            Err(error) => refuse(error),
        };
    }
    let cli = Cli::command().after_help(task_list());
    let matches = match cli.try_get_matches() {
        Ok(matches) => matches,
        Err(error) => return refuse(error),
    };
    let cli = match <Cli as clap::FromArgMatches>::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match cli.command {
        Some(command) => {
            if command.on_checkout() {
                state::share_checkout();
            }
            match run_command(command) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::FAILURE,
                Err(error) => fail(&error),
            }
        }
        None => {
            state::share_checkout();
            run_tasks(cli.run)
        }
    }
}
