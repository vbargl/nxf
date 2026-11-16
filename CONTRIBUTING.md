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
- Health checks come from the configuration. nxf ships none and knows nothing
  about what they check.
- `nxf status` is the only way results reach people: no notification
  services, queues or channels.

## Layout

| Path | Holds |
| --- | --- |
| `src/cli.rs` | The commands and their routing; no logic |
| `src/tasks.rs` | The task table, and planning and running tasks |
| `src/auto_update.rs` | The unattended run: which task comes next, and saving the results |
| `src/health.rs` | Running the configuration's health checks until they pass or time runs out |
| `src/hold.rs` | The hold on automatic updates |
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

- The version lives in `Cargo.toml`; the package reads it. Bump it with every
  change of behaviour.
- `git.rs`, `flake.rs` and `nixos.rs` each wrap one tool and do not use each
  other. They may use `output.rs` for progress and failure diagnostics. The
  tasks and the status report wire them together.
- nxf calls the `git`, `nix`, `nh` and `dix` programs. It does not link their
  libraries, so hooks, identities and configuration behave as on the command
  line.

## Commands and tasks

- `status`, `doctor`, `version`, `hold`, `release` and `auto-update` are
  commands. A command is
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

## Tests

- `just test` runs all tests; `just check` also runs the formatter, the lints
  and the flake checks. CI runs `just check`.
- Each file in `tests/` mirrors a source file. Tests that change the process
  environment are in `tests/environment.rs`; they hold one lock and start no
  process. Shared fixtures are in `tests/common/mod.rs`.
- Give a test a short name that does not repeat the file name, and a doc
  comment that says what must hold.
- Add or update tests with every change of behaviour.
- Tests never touch the machine. End-to-end tests use the throwaway flake and
  the mock nh; `NXF_SYSTEM`, `NXF_BOOTED`, `NXF_UPDATER_STATE`,
  `NXF_SYSTEMCTL` and `NXF_HEALTH_DIR` stand in for the running and the booted
  system, the updater, systemd and the health checks.
- Tests of live progress hold the mock nh back and assert that the output is
  there before it is released. Tests of in-place updates run in a
  pseudo-terminal and assert the final screen.
- When trying nxf by hand, use `--dry-run` or the mock nh. Never run a real
  `build`, `test`, `boot`, `switch`, `upgrade` or `auto-update` to check a
  change.
