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
| `nxf init --template <flake>#<name>` | Found a machine flake; see [Founding a machine flake](#founding-a-machine-flake) |
| `nxf <command> --config <file>` | Run `status`, `hold`, `release`, `version` or `doctor` on a set of machines; see [Several machines](#several-machines) |
| `nxf hold --reason <text>` | Hold automatic updates back; see [Holding updates](#holding-updates) |
| `nxf release` | Release the hold |
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

## One configuration

A machine flake holds the configuration of one machine. nxf builds
`nixosConfigurations.default`. When the flake has no configuration of that
name and exactly one of another name, nxf builds that one, so a flake that
names its configuration after the machine also works. Several configurations
without one named `default` are an error: nxf does not choose between them.

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

The terminal output is the log; no log files are written. Raw Nix, Git and nh
output stays hidden during successful tasks.

```
ash-twin · /etc/nixos

[1/3] Checking input updates
  ✓ nixpkgs   ab40acb1b3b0 → ca6fc441d5a6
  · presets   ab40acb1b3b0   unchanged
    Commit    75821d9 flake.lock: update nixpkgs (0 seconds ago)

[2/3] Building configuration
  ✓ Built
    System    /nix/store/…-nixos-system-ash-twin
    Packages  3 changed, 1 added, 1 removed
    Size      9.31 GiB → 9.36 GiB (+52.1 MiB)

[3/3] Activating configuration
  ✓ Activated, boot default updated

✓ Upgrade applied
  Active now and configured for the next boot.
```

Every line is on one of four levels, each with one indentation and one style:

| Level | Looks like | Holds |
| --- | --- | --- |
| Outcome | Column 0, bold, coloured | The one result of the run, always last |
| Phase or section | Column 0, bold | The heading of a block |
| Fact | Indented by 2, plain | A row with its state, or a label and its value |
| Support | Muted | Details beneath a row, times, sources, notes |

Labels and the names of inputs share one column. A mark carries the state of a
row: `✓` done, `✗` failed, `!` needs attention, `·` nothing happened. Colour
only repeats the state: green, red and yellow for the marks, blue for the
phase headings. Without colour the levels still read from the marks, the
indentation and the blank lines.

Build progress comes from Nix's structured activity events. On a terminal, two
rows update in place: the derivation count, and the current package with its
latest builder log line. Parallel builds show the package that most recently
produced output and how many others are running. Long lines are clipped to the
terminal width. A new input or derivation and the first builder line appear at
once; later log lines and counts are limited to one update per second. The two
rows are cleared when each tool exits. Counts are never guessed.

One green outcome ends a successful run. A failure ends with a red one,
followed by the cause in plain text, what it means for the run in muted text,
and the command to retry on its own line. The cause is the last diagnostic
lines of the failing tool, at most eight. A failed build does not start
activation. An activation failure may leave partial changes, so nxf does not
claim that it rolled back.

On a terminal, the input being checked is shown until its row replaces it.
Redirected output gets each row when it is finished.

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
  the committed checkout, or null) and `matches`. For monitoring it also
  prints `revision` (the flake commit of the running system, or null),
  `rebootRequired`, `hold` (`reason`, `since`, `until`, or null), `bad` (why
  the checkout failed its health check, or null) and `lastUpdate` (the time
  and task results of the last automatic update, or null). It does not mark
  the update summary as read.

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

Below the first line, status says when something needs attention:

- `Restart needed`: the kernel, the initrd or the kernel modules of the
  running system differ from those of the booted system. nxf never reboots
  the machine.
- `Automatic updates are held`, with the reason and the end of the hold.
- `The checkout failed its health check`, with what the failing checks said.

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
- `config`: the flake has the one configuration nxf works with;
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
    nixosConfigurations.default = nixpkgs.lib.nixosSystem {
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

## Several machines

`status`, `hold`, `release`, `version` and `doctor` take `--config <file>` and
then run on every machine of a hosts file instead of this one:

```sh
nxf status --config hosts.toml       # the state of every machine
nxf hold --config hosts.toml --reason "bad update" --for 3d
nxf release --config hosts.toml
```

```
hosts.toml · 3 machines

  ✓ node-1    75821d9f0c1a  updated 2 hours ago
  ! node-2    75821d9f0c1a  updated 2 hours ago  restart needed
  ! node-3    41c0a7e33b02  updated 3 days ago  held: bad update (until 2030-01-31 18:00)

! 2 of 3 machines need attention
```

One command shows which revision every machine runs, when it last updated and
what needs attention, and one command holds automatic updates on all of them,
so that a bad update does not spread. A hold with an end ends at the same
moment on every machine. Tasks such as `switch` do not take `--config`: nxf
never changes several machines with one command.

nxf runs itself on each machine with the system's `ssh`, in batch mode, all
machines at once. It uses your SSH configuration, keys and `known_hosts`, and
needs nxf installed on the machines. A machine that does not answer is a
failed row and makes the command fail. `nxf status --config <file> --json`
prints one entry for each machine: its `status --json`, or an `error`.
`hold` and `release` need an SSH user that may write `/var/lib/nxf`: root or
the update user.

### The hosts file

```toml
[hosts.node-1.ssh]
address = "10.0.0.11"
user = "root"        # default
port = 22            # default: what ssh decides

[hosts.node-1.k3s]   # not nxf's; ignored
role = "server"
```

nxf knows a list of SSH destinations and nothing about what the machines form
together. It reads `hosts.<name>.ssh` and ignores every other key and section
on every level, so another tool can keep its own data in the same file. A
name is unique by the form of the file. One file is one set of machines;
there are no groups, so another selection is another file. The format follows
the file name: `.toml`, `.json`, `.yaml` or `.yml`.

## Founding a machine flake

```sh
nxf init --template github:example/presets#machine                  # into /etc/nixos
nxf init ./staging/node-7 --template ./presets#machine
nxf init --template github:example/presets#machine --hardware
```

nxf does the mechanics; the template supplies the content. The template is
any flake template, named as for `nix flake init --template`, and its files
have no meaning to nxf. All nxf needs from the result is a flake with a
configuration under `nixosConfigurations`; see [One configuration](#one-configuration).

1. The directory is `NXF_FLAKE` or `/etc/nixos` unless one is given. It must
   be empty or not exist; init never writes into existing content.
2. The template is unpacked.
3. With `--hardware`, `hardware.nix` is written by `nixos-generate-config`.
   Use it on the machine itself.
4. `nxf.nix` is added: a module file that enables automatic updates and shows
   every option of the nxf module commented out with its default. It comes
   from the nxf version that runs init. A template that brings its own
   `nxf.nix` keeps it.
5. The directory becomes a Git repository without a remote, the inputs are
   locked, and everything is committed. A repository for which Git has no
   identity gets `nxf <nxf@host>` as its own.
6. The checks of `nxf doctor` that concern the checkout run, and
   `module` says whether the configuration imports the nxf module.

A flake can be founded on another computer, in any directory, and copied to
the machine later: nothing in it has to match the computer that runs init.

`nxf.nix` only has an effect when the flake imports the nxf module. The
template does that with three lines, and keeping the `nxf` input up to date
is up to the owner of the flake, like every other input:

```nix
{
  inputs.nxf.url = "github:vbargl/nxf";

  outputs = { nixpkgs, nxf, ... }: {
    nixosConfigurations.default = nixpkgs.lib.nixosSystem {
      modules = [
        nxf.nixosModules.default
        ./nxf.nix
        ./configuration.nix
      ];
    };
  };
}
```

A template without these lines is founded all the same; the `module` check
then warns and names the lines to add.

## Automatic updates

`nxf-auto-update.timer` starts `nxf-auto-update.service`, which runs
`nxf auto-update`:

1. If updates are held, or files other than `flake.lock` have uncommitted
   changes, the run does nothing and reports `skipped`.
2. `update` handles the inputs one at a time, as described above. A broken or
   unreachable input does not block the others.
3. If the committed flake is not what runs, `switch` builds and activates it:
   a live switch of everything. The machine is never rebooted; a new kernel
   takes effect at the next reboot, and status says that one is needed.
4. After the switch, the [health checks](#health-checks) run.
5. The results are saved to `/var/lib/nxf/update.json`, and the service fails
   if any task failed. `nxf status` shows them once to each user; there is no
   other notification.

| Option under `services.nxf.autoUpdate` | Default | Meaning |
| --- | --- | --- |
| `user` | `root` | The user that runs the update |
| `schedule` | `*-*-* 00/6:00:00` | systemd calendar expression |
| `randomizedDelay` | `1h` | Latest start after the schedule |
| `fixedDelay` | `true` | Keep the same delay on every run, different for each machine |
| `gitIdentity` | `nxf <nxf@host>` for root | Identity of the lockfile commits |
| `gitCredentialsFile` | none | Tokens for Git inputs over HTTPS |
| `nixConfigFile` | none | nix.conf fragment with tokens for other fetchers |
| `healthChecks` | none | Scripts that must succeed after a switch |
| `healthTimeout` | `300` | Seconds the checks have to pass |
| `healthInterval` | `10` | Seconds between two rounds of checks |
| `timeout` | `3h` | Longest run |

### Health checks

The configuration supplies the checks; nxf has none of its own. Each entry of
`healthChecks` is a shell script, and modules can add their own entries:

```nix
services.nxf.autoUpdate.healthChecks = {
  "10-units" = ''test -z "$(systemctl --failed --no-legend)"'';
  "20-k3s" = "systemctl is-active --quiet k3s";
};
```

The scripts become executables in `/etc/nxf/health.d`. After an automatic
switch nxf runs those of the new configuration in name order, and repeats the
round every `healthInterval` seconds until all pass or `healthTimeout` runs
out. A check that needs time, such as a node that becomes ready after a few
minutes, just fails until then.

If checks still fail at the end:

- nxf activates the previous system again;
- the revision is marked as bad in `/var/lib/nxf/bad/<tree>`, with what the
  failing checks said;
- the run fails and reports `health: failed`;
- later runs skip the switch until a newer revision is committed. The bad
  revision stays in the repository, because it built.

Tasks run by hand, such as `nxf switch`, run no health checks.

### Holding updates

```sh
nxf hold --reason "waiting for the vendor fix"         # until released
nxf hold --reason "release week" --for 3d              # m, h, d or w
nxf hold --reason "release week" --until "2030-01-31 18:00"
nxf release
```

A hold stops the automatic update: the run reports `skipped` with the reason.
The hold is the file `/var/lib/nxf/hold.json`, so it survives a reboot, and
only root or the update user can set or release it. `nxf status` shows it. A
hold with an end stops holding at that time. Tasks run by hand are not held.

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
