# nxf

`nxf` manages the local flake of a NixOS machine: the Git checkout in
`NXF_FLAKE`, or `/etc/nixos` when unset, as nh does. It updates the flake
inputs one at a time, builds the system with each update, and commits the
lockfile only after the build passed. It works from any directory and needs no
changes to the flake itself.

`nxf` is the Rust successor of `glue` from `vbargl/nixfiles`.

| Command | Action |
| --- | --- |
| `nxf status` | Show the running system's flake commit and inputs, compare with the checkout |
| `nxf status --checkedout` | Show the checkout's commit and inputs instead |
| `nxf status --detailed` | Show both, the running closure size, and the full package diff |
| `nxf status <input>...` | Show only the named inputs, with any of the flags above |
| `nxf status --json` | Print the comparison as JSON, for scripts |
| `nxf doctor` | Check that nxf can work on this machine |
| `nxf version` | Print the version of nxf |
| `nxf version --system` | Print the running system's flake commit |
| `nxf auto-update` | Update unattended; see [Automatic updates](#automatic-updates) |
| `nxf <task>...` | Run tasks, see below |
| `nxf --json <task>...` | Run tasks quietly and print each one's result as JSON, for scripts |

| Task | Action |
| --- | --- |
| `update` | Update all inputs, or only the named ones, one at a time: build with each update and commit it, or restore the lockfile |
| `build` | Build with nh and summarize package changes |
| `test` | Build, then activate until the next reboot |
| `boot` | Build, then make the configuration the boot default |
| `switch` | Build, activate, and make the configuration the boot default |
| `upgrade` | Shortcut for `update` and `switch`: update each input, then switch |

Tasks run in the order of the table, whatever order they are given in, and
each runs at most once: `nxf test update` and `nxf update test` both update,
then test. Run `nxf` to list them. An unknown task, including `status` or
`version` mixed with tasks, is an error and nothing runs. Add `--dry-run` to
print the tasks. If a task fails, later ones do not run.

A shortcut, such as `upgrade`, stands for a set of tasks, which keep their own
places in the order: `nxf upgrade build test` updates, builds, tests, then
switches. Each task runs exactly what it says, so `nxf test boot` evaluates
the configuration twice; use `switch` instead.

Name root inputs of the `flake.lock` among the tasks to update only those:
`nxf update nixpkgs test` or `nxf upgrade presets`. They go to `update`, also
through `upgrade`, so one of those must be given. A name that is neither a
task nor an input is an error, and a task name wins over an input of the same
name.

## The lockfile is committed or restored

`update` never leaves the lockfile changed. Each input is updated on its own,
the configuration is built with the new lockfile, and only then is the
lockfile committed, alone. If the fetch, the build or the commit fails, for
example because of a hook or the Git identity, that input's update is restored
and the next input is tried; the task then fails, naming the inputs to retry.
Unchanged inputs produce no build and no commit. A lockfile found with
uncommitted changes was never built, so `update` restores it first.

A failed update therefore never changes what the machine runs and never stays
in the repository.

## Output

The terminal output is the log; no log files are written. The current phase
gets a bold cyan heading. Completed phases stay in scrollback with a quiet
check mark. Raw Nix, Git and nh output stays hidden during successful tasks.

Build progress comes from Nix's structured activity events. On a terminal, two
rows update in place: the derivation count, and the current package with its
latest builder log line. Parallel builds show the package that most recently
produced output and how many others are running. Long lines are clipped to the
terminal width. A new input or derivation and the first builder line appear at
once; later log lines and counts are limited to one update per second. The two
rows are cleared when each tool exits. Counts are never guessed.

One bold green outcome ends a successful run. A failure prints a bold red
summary, the effects of earlier work, the last eight diagnostic lines of the
failing tool, and a recovery command on its own line. A failed build does not
start activation. An activation failure may leave partial changes, so nxf does
not claim that it rolled back.

Colour and in-place updates are used on terminals only. Redirected output,
`TERM=dumb`, `NO_COLOR` (also when empty) and `CLICOLOR=0` give append-only
output with the same spacing and symbols and no escape sequences.
`CLICOLOR_FORCE=1` enables colour for captures unless `NO_COLOR` or
`CLICOLOR=0` disables it.

Each nh task evaluates the flake once with `nh os build`; `test`, `boot` and
`switch` then pass the built store path to nh for activation. nh handles
validation, specialisations, privilege elevation and the bootloader. When nxf
runs as root, nh is told not to elevate. dix makes the package summary.

## Scripts

Scripts drive nxf through JSON instead of reading its terminal output:

- `nxf --json <task>...` prints nothing while the tasks run, then a list with
  one record per task that ran: `task`, `inputs`, `outcome` and `detail`. The
  outcome is `done`, `unchanged` for an update that found nothing new, or
  `failed`, also when only some inputs failed; `detail` holds the diagnostics
  of a failure. A failure stops later tasks, which are then not listed, and
  the exit code is non-zero.
- `nxf status --json` prints `checkout`, `changes` (tracked files with
  uncommitted changes), `inputs`, `running`, `built` (the system built from
  the committed checkout, or null) and `matches`. It does not mark the update
  summary as read.

## Status

The nh tasks record which committed Git tree produced each system, as a plain
symlink `<state>/systems/<tree> -> <system>`. The state directory is
`NXF_STATE`, or `$XDG_STATE_HOME/nxf`, or `~/.local/state/nxf`. The automatic
updater keeps its records in `/var/lib/nxf`, and status reads both. That is
all status needs:

- The tree of the running system gives its flake commit, and
  `git show <commit>:flake.lock` gives the exact input revisions it was built
  from. When several commits built the same system, such as commits that only
  touched comments, the checkout's commit is named, otherwise the newest.
- The tree of the checkout gives its built system, if any, and dix reports the
  package changes.

Nothing is evaluated or built, so status returns at once. Builds with
uncommitted changes are not recorded, and a record disappears when its system
is garbage-collected. A system built some other way, such as a plain
`nixos-rebuild`, shows `nixos-version --configuration-revision` instead.

`nxf status` also prints the latest unread automatic update summary: the task
results in `/var/lib/nxf/update.json`, and the result of
`nxf-auto-update.service` when the service was killed before it saved. Each
user sees a summary once; the marker is `<state>/update-read`. Only the latest
summary is kept. Invalid requests and `--json` do not mark anything as read,
and reading status never starts a build, an activation or a retry.

## Doctor

`nxf doctor` checks, without building or changing anything:

- `tools`: nix, nh, git and dix are available;
- `flake`, `git`, `lockfile`: the checkout exists, is a Git repository with a
  commit, and has a lockfile;
- `writable`, `identity`: `update` can write and commit the lockfile;
- `changes`: no uncommitted changes, whose builds would not be remembered;
- `host`: the flake has a configuration for this host name, as nh selects;
- `state`: builds can be remembered;
- `running`: the running system was built by nxf or the updater.

Each line is `ok`, `warn` or `fail`, with how to fix it. Any `fail` makes it
exit non-zero; checks that need the checkout are skipped until it is found.

## Install

The flake provides the package and a NixOS module:

```nix
{
  inputs.nxf.url = "github:vbargl/nxf";

  outputs = { nixpkgs, nxf, ... }: {
    nixosConfigurations.machine = nixpkgs.lib.nixosSystem {
      modules = [
        nxf.nixosModules.default
        { services.nxf.autoUpdate.enable = true; }
      ];
    };
  };
}
```

`services.nxf.enable` installs the command and sets `NXF_FLAKE` to
`services.nxf.flake` (default `/etc/nixos`).
`services.nxf.autoUpdate.enable` does the same and adds the timer. The package
alone is `nxf.packages.<system>.nxf`; it brings nh, dix, Git and Nix for
machines that do not have them on `PATH`.

## Automatic updates

`nxf-auto-update.timer` starts `nxf-auto-update.service`, which runs
`nxf auto-update`:

1. If files other than `flake.lock` have uncommitted changes, the run does
   nothing and reports `skipped`.
2. `update` handles the inputs one at a time, as described above. A broken or
   unreachable input does not block the others.
3. If the committed flake is not what runs, `switch` builds and activates it.
   The machine is never rebooted; a new kernel takes effect at the next
   reboot.
4. The results are saved to `/var/lib/nxf/update.json`, and the service fails
   if any task failed.

| Option under `services.nxf.autoUpdate` | Default | Meaning |
| --- | --- | --- |
| `user` | `root` | The user that runs the update |
| `schedule` | `*-*-* 00/6:00:00` | systemd calendar expression |
| `randomizedDelay` | `1h` | Latest start after the schedule |
| `fixedDelay` | `true` | Keep the same delay on every run, different for each machine |
| `gitIdentity` | `nxf <nxf@host>` for root | Identity of the lockfile commits |
| `gitCredentialsFile` | none | Tokens for Git inputs over HTTPS |
| `nixConfigFile` | none | nix.conf fragment with tokens for other fetchers |
| `timeout` | `3h` | Longest run |

### As root

Root is the default and needs no interactive user: no sudo, no personal SSH
configuration. The flake directory can belong to root or to another user.

Inputs fetched with Git over HTTPS (`git+https://...`) take a token from
`gitCredentialsFile`, a file in the format of
[git-credential-store](https://git-scm.com/docs/git-credential-store), one
line for each host:

```
https://machine:TOKEN@git.example.org
```

`github:`, `gitlab:` and tarball inputs are fetched by Nix itself. Give their
tokens in `nixConfigFile`, a nix.conf fragment:

```
access-tokens = github.com=TOKEN
```

Keep both files out of the Nix store and readable only by the update user, for
example with sops-nix or agenix. Inputs over SSH also work: the service runs
`ssh` in batch mode with root's SSH configuration and keys.

### As a user

With `user = "alice"`, everything runs as if that user ran nxf: their Git
identity makes the commits, their `~/.ssh/config` provides hosts and keys, and
nh elevates with sudo, which must not ask for a password. The user must own
the flake directory.

### Operating it

```sh
systemctl list-timers nxf-auto-update.timer
journalctl -u nxf-auto-update.service
sudo systemctl start nxf-auto-update.service   # update now
sudo systemctl stop nxf-auto-update.timer      # hold until the next boot or activation
nxf status                                     # the latest outcome
```

## Planned

These are not implemented yet:

- A health check after the switch, with a return to the previous generation,
  a mark on the bad revision, and no further attempt until a newer one comes.
- More machine-readable state for monitoring: the running revision, the result
  of the last update, and whether a reboot is needed for a new kernel or
  initrd.

## Development

```sh
nix develop          # Rust, nh, dix, Git and util-linux
just test            # all tests
just check           # formatting, lints, tests and the flake checks
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the rules to follow when changing
nxf.

## License

MIT or Apache-2.0, at your option.
