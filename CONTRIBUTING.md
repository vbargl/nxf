# Contributing to nxf

Rules agreed while building nxf and its predecessor glue. Keep to them, or
change them here first.

## Scope

- nxf needs no changes to the machine flake: no wrapper around its
  configurations, no extra outputs, no evaluation-time hooks.
- nxf has no dependency on any particular configuration repository. The
  package and the NixOS module in this flake are all a machine needs.
- Reuse data that already exists instead of generating more: the lockfile in
  Git history for input revisions, nh and dix for builds and package diffs,
  `nix path-info` for sizes.
- nxf finds its flake like nh: `NXF_FLAKE`, defaulting to `/etc/nixos`. It
  needs no registry entry.
- `nxf status` and `nxf doctor` build nothing, and status evaluates nothing.
- Never leave the checkout with uncommitted changes of nxf's own: `update`
  commits the lockfile only after it built, and restores it otherwise.
- A failed update never changes the running system.
- The machine is never restarted by nxf, except by an installation:
  `nxf install --reboot` restarts into the new system, and `rollout`
  restarts each machine it installs, into the installer when the machine
  runs another system, and into the new system.
- nxf is a general tool. It requires a local Git flake with a `flake.lock`
  and one configuration under `nixosConfigurations`, and nothing else: no particular
  files, identities, presets or cluster. What is specific to one environment
  enters through general interfaces only: the options of the module, the
  health checks, and a template given by reference.
- Remote work is nxf running itself on each machine over ssh. `status`,
  `hold`, `release`, `version` and `doctor` take `--config`; tasks never do.
  `rollout` is the one command that changes the system of machines; `hold`
  and `release` with `--config` write only the hold of each machine. Without
  `--upgrade` rollout changes only the machines that nxf does not manage:
  it must stay repeatable,
  and it must ask before it erases a disk. With `--upgrade` it runs the task
  `upgrade` on each machine, the first alone and then in waves of `--batch`,
  and a failure starts no further wave. No other task runs on a set of
  machines. The hosts file stays a list of SSH destinations: nxf reads
  `hosts.<name>.ssh` and must keep ignoring everything else.
- `nxf init` does not edit the files of a template. It adds `nxf.nix` and
  reports whether the configuration imports the module.
- Health checks and the two hooks come from the configuration. nxf ships
  none and knows nothing about what they do: no URLs, protocols or services
  of its own.
- There are two hooks and no more: the check hook before an automatic update
  and the failure hook after a failed one, each one script. The check hook
  can only skip the automatic run, and the failure hook can only fail as a
  result of its own. Neither changes what the tasks do, and tasks run by hand
  use neither.
- `nxf status` is the only way nxf brings results to people: no notification
  services, queues or channels. The failure hook is the configuration's, and
  what it tells to whom is not nxf's.

## Layout

| Path | Holds |
| --- | --- |
| `docs/features/` | Short feature briefs and use-case scenarios for acceptance tests |
| `src/main.rs` | The program; it only calls `cli::main` |
| `src/lib.rs` | The list of the modules |
| `src/cli.rs` | The commands and their routing; no logic |
| `src/tasks.rs` | The task table, and planning and running tasks |
| `src/results.rs` | What nxf says to programs with `--json`, and reads back: a task's outcome, a refusal, the machines of a rollout, an installation |
| `src/auto_update.rs` | The unattended run: which task comes next, verifying its switch, and saving the results |
| `src/health.rs` | Running the configuration's health checks until they pass or time runs out |
| `src/strategy.rs` | How the automatic update treats each input: `off`, `flag` or `report`, read from the configuration in the checkout |
| `src/hold.rs` | The hold on automatic updates |
| `src/hooks.rs` | The configuration's check hook before an automatic update and its failure hook after a failed one |
| `src/hosts.rs` | The hosts file: SSH destinations, with everything else ignored |
| `src/fleet.rs` | Running nxf on the machines of a hosts file over ssh, and the upgrade in waves |
| `src/init.rs` | Founding a machine flake from a template |
| `src/install.rs` | Installing from an installer: disks, nixos-install, host key, records |
| `src/rollout.rs` | Installing the machines of a hosts file that nxf does not manage |
| `src/status.rs` | The status report, the running system's flake commit and the updater's latest result |
| `src/doctor.rs` | The doctor's checks |
| `src/output.rs` | Terminal styles, rows, label columns, phase headings, outcomes, streaming diagnostics, the one document of a run (`data`) and the diversion of its text to standard error (`divert`) |
| `src/progress.rs` | Nix activity decoding, derivation counts and active package names |
| `src/git.rs` | Git helpers |
| `src/flake.rs` | Flake helpers: the checkout, updating inputs, reading the lockfile |
| `src/nixos.rs` | NixOS helpers: nh, dix, the running system, package changes, sizes, the update service |
| `src/env.rs` | The environment: the name of each variable nxf reads, and the readers of its values |
| `src/process.rs` | Child processes: running them, with a deadline, reading their output and error together or apart, the one shape of their failures, and the atomic write of a file |
| `src/ssh.rs` | Reaching a machine over ssh: the options, the trust of its host key, the quoting of the remote command |
| `src/state.rs` | The state: the per-user and the updater's directories, build records, marks of unhealthy sources, source roots and the marks of the trees whose sources are kept, the places of the hold and of the updater's results |
| `src/time.rs` | The clock, a time as `date` reads and writes it, and how long ago a time was |
| `src/text.rs` | Escape-sequence removal and clipping to terminal cells |
| `nix/package.nix` | The package |
| `nix/module.nix` | The NixOS module |
| `nix/module-check.nix` | The check that evaluates the module |
| `nix/shared-records-check.nix` | The NixOS VM test of the shared build records: users of the group, a user outside it, root |
| `nix/init-module.nix` | The generator of `nix/nxf.nix` from the options of the module |
| `nix/nxf.nix` | The module file `nxf init` writes; generated, renew it with `just generate` |

- The version lives in `Cargo.toml`; the package reads it. Bump it with every
  change of behaviour.
- `git.rs`, `flake.rs` and `nixos.rs` each wrap one tool and do not use each
  other. They may use `output.rs` for progress and failure diagnostics, and
  `env.rs`, `process.rs` and `state.rs`, which uses none of the three modules. The
  tasks and the status report wire them together.
- Every variable of the environment that nxf reads is named in `env.rs` and read through it.
  Every child process is started through `process.rs`, and every call of ssh
  is made through `ssh.rs`. Only `rollout::upload`, which pipes the output of
  `tar` into `ssh`, starts its processes itself. A new variable, command or
  program is one edit there, and not a copy of what another module does.
- A failure of a program has the shape `<program> exited with status N.` with
  the last eight lines the program said beneath it; an outcome prints those
  lines first and the exit line after them. A failure of a step of the remote
  installation names the step and not a program: it has the shape
  `<step> failed`, with the last lines beneath.
- nxf calls the `git`, `nix`, `nh` and `dix` programs. It does not link their
  libraries, so hooks, identities and configuration behave as on the command
  line. It also calls `ssh` for every command with `--config` and for
  rollout, `ssh-keygen` for install and rollout, `tar` for rollout, `cp` for
  init and install, `date` for the times of a hold, and `systemctl` for
  status and `install --reboot`. The package brings these ten programs: `cp`
  and `date` as coreutils, `systemctl` as systemd. `nixos-install`,
  `nixos-generate-config` and `nixos-version` are the machine's own, and so
  is the `sudo` that nh calls.

## Feature descriptions

- Keep a short brief for each new or changed feature in
  `docs/features/<feature>.md`, and keep it current when the behaviour
  changes.
- A brief holds, in this order: a one-line summary; `Brief`, with the problem
  and who has it, the resulting behaviour and a `Boundaries:` list;
  `Acceptance scenarios`; `Existing verification`.
- `Acceptance scenarios` is a table of ID, Given, When and Then, a template
  for acceptance tests. A When is one action. A Then can be observed: the
  text of a row or of the outcome, the exit status, a state that a command
  shows, or what a stand-in program was asked to run.
- `Existing verification` maps every scenario ID to the test that covers it,
  by name, or says that no automated test covers it. A run by hand is
  recorded with the nxf version and the commit it ran on, and with what was
  seen.

## Commands and tasks

- `status`, `doctor`, `version`, `init`, `install`, `rollout`, `hold`,
  `release` and `auto-update` are commands: the variants of `Cmd` in
  `src/cli.rs`, which clap lists with `help`. A new command is a variant, an
  arm of `run_command` and an arm of `Cmd::on_checkout`; nothing else names
  it. A command is recognised only as the first word, so `nxf test status`
  is an unknown task. A command refuses a word it does not take: `nxf version update`
  fails instead of ignoring `update`. `status` and `rollout --upgrade` take
  the names of inputs, so `nxf status update` fails as
  `Unknown input: update`.
- Everything else is a list of tasks. Unknown words are an error and nothing
  runs.
- Tasks are the entries of `TASKS` in `src/tasks.rs` and run in that order,
  each at most once. Place a new task where it belongs in the order. An entry
  holds what the task is, does and says, so a new task is one entry. No code
  matches on the name of a task, except `auto-update`, which decides which
  comes next, and the readers of results, which use `tasks::UPDATE` and
  `tasks::SWITCH`.
- A set of tasks given one name is an entry of `SHORTCUTS`, not a task calling
  other tasks: that keeps each task in its own place of the order.
- Scripts get results from `--json`, never from terminal output, and so do
  the programs of nxf that run each other: `rollout` reads `install --json`,
  and the commands with `--config` read `--json` of the machines. A word of a
  result is a value of an enum. The shapes that `src/results.rs` owns, which
  are the outcomes of tasks, a refusal, the machines of a rollout and an
  installation, have one writer there, and one reader there where a program
  of nxf reads them. The document of `status --json` is written by `src/status.rs`, and `fleet.rs` and
  `rollout.rs` read the fields they need from it. Every task reports through
  `--json` without code of its own; keep it that way.
- `auto-update` decides which task comes next from the task results, and
  verifies the switch it made: the health checks, the mark of a bad
  revision and the return to the previous system. Everything else is a task.

## Output

- Four levels, each with one indentation and one style: the outcome (column
  0, bold and coloured, once, last), phase and section headings (column 0,
  bold), facts (indented by 2, plain), and support (muted). Use the functions
  of `src/output.rs`; do not print with other indentation or styles.
- What matters most is plain or stronger; what merely accompanies it is muted.
  The cause of a failure is never muted.
- One run has one outcome line. Do not repeat it as a phase result.
- Labels are capitalised words without a colon, padded to `output::LABEL`.
  Names that sit in the label column, such as inputs, widen it together.
- State is a mark, and colour only repeats the mark. The output must read the
  same with `NO_COLOR`.
- A row says what happened to one thing. Do not announce work that a row will
  report; show it with `output::pending` while it runs.
- Reformat what other tools print into facts; do not pass their layout on.
- Errors of the command line have the shape of every failure: the cause as
  the outcome, then facts that help to resolve it. What clap refuses is
  printed in that shape too and exits with status 2. Help that is asked for
  is printed as clap prints it. With `--json` the same cause and facts are
  also the one object `{"error", "details"}` on standard output.
- A command with `--json` that shows progress keeps standard output for its
  one JSON document (`output::data`) and sends the rest, as it comes, to
  standard error (`output::divert`). A test holds a tool back and asserts its
  line before releasing it.

## Tests

- `just test` runs all tests; `just check` also checks the format of the Rust
  and the Nix sources and runs the lints and the flake checks. CI runs
  `just check`.
- Each file in `tests/` mirrors a source file. Tests that change the process
  environment are in `tests/environment.rs`; they hold one lock and start no
  nxf process. Shared fixtures are in `tests/common/mod.rs`.
- Give a test a short name that does not repeat the file name, and a doc
  comment that says what must hold.
- Add or update tests with every change of behaviour.
- The installation of a real machine cannot be a test here. After a change to
  `install.rs` or `rollout.rs`, run `nxf rollout` against a virtual
  machine that starts as another system, and run it a second time.
- `tests/rollout.rs` plays an installation without a machine. Its mock ssh
  keeps the files it is sent, reports a host key as the JSON of `nxf install
  --json` and answers as a managed machine from then on. A mock `nix` names
  the source of the template and does nothing on `nix copy`; a test that looks
  at what `nix copy` is given puts a `nix` of its own in its place that
  records it. This does not replace the run against a virtual machine.
- Tests never touch the machine. End-to-end tests use the throwaway flake and
  the mock nh, and variables of the environment stand in for what belongs to
  the machine. The shipped program reads them too.

  | Stand-in | Stands in for |
  | --- | --- |
  | `NXF_SYSTEM` | The running system, `/run/current-system` |
  | `NXF_BOOTED` | The booted system, `/run/booted-system` |
  | `NXF_UPDATER_STATE` | The updater's state, `/var/lib/nxf` |
  | `NXF_SYSTEMCTL` | The `systemctl` that status asks about the update service and that `install --reboot` runs |
  | `NXF_HEALTH_DIR` | The directory of the health checks, `/etc/nxf/health.d`; `health.conf` is read beside it |
  | `NXF_HOOKS_DIR` | The directory of the two hooks, `/etc/nxf/hooks` |
  | `NXF_SSH` | The `ssh` program |
  | `NXF_INSTALL_ROOT` | Where the new system is mounted, `/mnt`; when it is set, install does not require root |
  | `NXF_INSTALLER` | The answer whether the system runs from memory: `yes`, anything else is no |
  | `NXF_DISKO` | The script of the disk layout |
  | `NXF_PACKAGE` | The package that rollout copies to the machines, for an nxf that does not run from the Nix store; `tests/rollout.rs` sets it |

- These variables are settings, for an operator or the module:

  | Setting | Default | Sets |
  | --- | --- | --- |
  | `NXF_FLAKE` | `/etc/nixos` | The checkout of the machine flake |
  | `NXF_STATE` | `$XDG_STATE_HOME/nxf`, then `~/.local/state/nxf` | The per-user state |
  | `NXF_HOOK_TIMEOUT` | 60 | Seconds each hook has to end; the module sets it from `hooks.timeout` |
  | `NXF_SSH_CONNECT_TIMEOUT` | 10 | Seconds ssh has to connect to a machine |
  | `NXF_SSH_TIMEOUT` | 120 | Seconds a machine has to answer a command with `--config` |
  | `NXF_UPGRADE_TIMEOUT` | 10800 | Seconds a machine has for `rollout --upgrade` |
  | `NXF_ROLLOUT_WAIT` | 600 | Seconds rollout waits for the installer, and then for the installed system, to answer |

  nxf sets `NXF_RESULTS` for the failure hook and does not read it.
- Tests of live progress hold the mock nh back and assert that the output is
  there before it is released. Tests of in-place updates run in a
  pseudo-terminal and assert the final screen.
- When trying nxf by hand, use `--dry-run` or the mock nh. Never run a real
  `build`, `test`, `boot`, `switch`, `upgrade` or `auto-update` to check a
  change.

## Done

A change of behaviour is done when all of this holds:

- The version is bumped; see [Layout](#layout).
- Tests are added or updated, and `just check` passes; see [Tests](#tests).
- A change to `install.rs` or `rollout.rs` ran against a virtual machine;
  see [Tests](#tests).
- The brief of the feature is current; see
  [Feature descriptions](#feature-descriptions).
- The [README](README.md) describes the behaviour.
- `nix/nxf.nix` is renewed when the options of the module changed; see
  [Layout](#layout).
