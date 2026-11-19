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
- The machine is never rebooted by nxf.
- nxf is a general tool. It requires a local Git flake with a `flake.lock`
  and one configuration under `nixosConfigurations`, and nothing else: no particular
  files, identities, presets or cluster. What is specific to one environment
  enters through general interfaces only: the options of the module, the
  health checks, and a template given by reference.
- Remote work is nxf running itself on each machine over ssh. `status`,
  `hold`, `release`, `version` and `doctor` take `--config`; tasks never do.
  `reconcile` is the one command that changes machines, and only those that
  nxf has not installed: it must stay repeatable, and it must ask before it
  erases a disk. The hosts file stays a list of SSH destinations: nxf reads
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
| `src/cli.rs` | The commands and their routing; no logic |
| `src/tasks.rs` | The task table, and planning and running tasks |
| `src/auto_update.rs` | The unattended run: which task comes next, and saving the results |
| `src/health.rs` | Running the configuration's health checks until they pass or time runs out |
| `src/hold.rs` | The hold on automatic updates |
| `src/hooks.rs` | The configuration's check hook before an automatic update and its failure hook after a failed one |
| `src/hosts.rs` | The hosts file: SSH destinations, with everything else ignored |
| `src/fleet.rs` | Running nxf on the machines of a hosts file over ssh |
| `src/init.rs` | Founding a machine flake from a template |
| `src/install.rs` | Installing from an installer: disks, nixos-install, host key, records |
| `src/reconcile.rs` | Installing the machines of a hosts file that nxf has not installed |
| `src/status.rs` | The status report and the running system's flake commit |
| `src/doctor.rs` | The doctor's checks |
| `src/output.rs` | Terminal styles, phase headings, outcomes and streaming diagnostics |
| `src/progress.rs` | Nix activity decoding, derivation counts and active package names |
| `src/git.rs` | Git helpers |
| `src/flake.rs` | Flake helpers: the checkout, updating inputs, reading the lockfile |
| `src/nixos.rs` | NixOS helpers: nh, the running system, package changes, sizes, remembered builds, the updater's results |
| `src/text.rs` | Escape-sequence removal and clipping to terminal cells |
| `nix/package.nix` | The package |
| `nix/module.nix` | The NixOS module |
| `nix/module-check.nix` | The check that evaluates the module |
| `nix/init-module.nix` | The generator of `nix/nxf.nix` from the options of the module |
| `nix/nxf.nix` | The module file `nxf init` writes; generated, renew it with `just generate` |

- The version lives in `Cargo.toml`; the package reads it. Bump it with every
  change of behaviour.
- `git.rs`, `flake.rs` and `nixos.rs` each wrap one tool and do not use each
  other. They may use `output.rs` for progress and failure diagnostics. The
  tasks and the status report wire them together.
- nxf calls the `git`, `nix`, `nh` and `dix` programs. It does not link their
  libraries, so hooks, identities and configuration behave as on the command
  line.

## Feature descriptions

- Keep a short brief for each new or changed feature in
  `docs/features/<feature>.md`: the problem, who uses it, resulting behavior
  and boundaries.
- Include concrete Given/When/Then use cases as templates for acceptance
  tests. Link existing coverage and distinguish scenarios from checks that
  have actually run. Keep the brief current when the behavior changes.

## Commands and tasks

- `status`, `doctor`, `version`, `init`, `hold`, `release` and `auto-update`
  are commands. A command is
  recognised only as the first word and rejects extra arguments, so
  `nxf status update` fails instead of ignoring `update`.
- Everything else is a list of tasks. Unknown words are an error and nothing
  runs.
- Tasks are the entries of `TASKS` in `src/tasks.rs` and run in that order,
  each at most once. Place a new task where it belongs in the order.
- A set of tasks given one name is an entry of `SHORTCUTS`, not a task calling
  other tasks: that keeps each task in its own place of the order.
- Scripts get results from `--json`, never from terminal output. Every task
  reports through it without code of its own; keep it that way.
- `auto-update` only decides which task comes next from the task results. Put
  logic into the tasks.

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
  the outcome, then facts that help to resolve it.

## Tests

- `just test` runs all tests; `just check` also runs the formatter, the lints
  and the flake checks. CI runs `just check`.
- Each file in `tests/` mirrors a source file. Tests that change the process
  environment are in `tests/environment.rs`; they hold one lock and start no
  process. Shared fixtures are in `tests/common/mod.rs`.
- Give a test a short name that does not repeat the file name, and a doc
  comment that says what must hold.
- Add or update tests with every change of behaviour.
- The installation of a real machine cannot be a test here. After a change to
  `install.rs` or `reconcile.rs`, run `nxf reconcile` against a virtual
  machine that starts as another system, and run it a second time.
- Tests never touch the machine. End-to-end tests use the throwaway flake and
  the mock nh; `NXF_SYSTEM`, `NXF_BOOTED`, `NXF_UPDATER_STATE`,
  `NXF_SYSTEMCTL`, `NXF_HEALTH_DIR`, `NXF_HOOKS_DIR` and
  `NXF_SSH` stand in for the running and the booted system, the updater,
  systemd, the health checks, the two hooks and ssh.
- Tests of live progress hold the mock nh back and assert that the output is
  there before it is released. Tests of in-place updates run in a
  pseudo-terminal and assert the final screen.
- When trying nxf by hand, use `--dry-run` or the mock nh. Never run a real
  `build`, `test`, `boot`, `switch`, `upgrade` or `auto-update` to check a
  change.
