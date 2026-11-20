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
| `nxf install <dir>` | Install the machine a flake describes, from a NixOS installer on it; see [Installing a machine](#installing-a-machine) |
| `nxf rollout --config <file> --template <flake>#<name>` | Install every machine of a hosts file that nxf does not manage yet; see [From another computer](#from-another-computer) |
| `nxf rollout --config <file> --upgrade` | Upgrade every machine of a hosts file, the first alone, then in waves; see [Upgrading machines](#upgrading-machines) |
| `nxf hold --reason <text>` | Hold automatic updates back; see [Holding updates](#holding-updates) |
| `nxf release` | Release the hold |
| `nxf auto-update` | Update unattended; see [Automatic updates](#automatic-updates) |
| `nxf <task>...` | Run tasks, see below |
| `nxf --json <task>...` | Run tasks quietly and print each one's result as JSON, for scripts |

| Task | Action |
| --- | --- |
| `update` | Update all inputs, or only the named ones, one at a time: build with each update and commit it, or restore the lockfile |
| `build` | Build with nh and summarize package changes |
| `test` | Build, then activate until the next restart |
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
only repeats the state: green, red and yellow for the marks, cyan for the
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
  the exit status is 1. A command line that nxf refuses, such as one with an
  unknown option, exits with status 2. A refusal, or an error before the
  first task, prints no JSON.
- `nxf status --json` prints `checkout`, `changes` (tracked files with
  uncommitted changes), `inputs`, `running`, `built` (the system built from
  the committed checkout, or null) and `matches`. For monitoring it also
  prints `revision` (the flake commit of the running system, or null),
  `rebootRequired`, `hold` (`reason`, `since`, `until`, or null), `bad` (why
  the checkout failed its health check, or null) and `lastUpdate` (the time
  and task results of the last automatic update, or null). It does not mark
  the update summary as read.

The results of an automatic update have the same four fields. They are in
`lastUpdate`, in `/var/lib/nxf/update.json` and in the file `$NXF_RESULTS` of
the [failure hook](#hooks). Besides the tasks, a result there is named
`health`, for the [health checks](#health-checks), or `hook`, for the failure
hook. These are all outcomes:

| Outcome | Of | Meaning |
| --- | --- | --- |
| `done` | a task, `health` | The task ran to its end, or the health checks passed |
| `unchanged` | `update` | The update left the lockfile as it was |
| `failed` | a task, `health`, `hook` | The task failed, the checks still failed when their time ran out, or the failure hook failed. `update` `failed` is also the result of a run that cannot go on; see [Automatic updates](#automatic-updates). The run fails |
| `skipped` | `update`, `switch` | Automatic update only. The run did not start, because of a hold, uncommitted changes or the check hook; or the switch was left out, because the revision failed its health check before. `detail` says which |
| `flagged` | `update`, with the input in `inputs` | Automatic update only. The input could not be updated and its [strategy](#strategies) is `flag`. The run goes on and does not fail |
| `reported` | `update`, with the input in `inputs` | The same for the strategy `report`, and the failure hook runs after the run |

## Status

For the feature brief and acceptance scenarios, see
[Shared Builds](docs/features/shared-builds.md).

The nh tasks record which committed Git tree produced each system, as a plain
symlink `/var/lib/nxf/systems/<tree> -> /nix/store/<system>`. The module creates
this directory as `nxf:nxf`, mode `2775`, without the sticky bit. Root, the
user `nxf` and members of the group `nxf` can atomically replace each other's
records. A build is recorded only if the tracked checkout still corresponds
to what was built, including the lockfile built before an update commits it.

Without a provisioned shared directory, records stay in `<state>/systems`.
No task creates the shared directory: the module provisions it, and
`nxf install` creates it on the system it installs. If shared publication
fails, nxf warns and tries the per-user location; record failures do not fail
a successful build, update or activation. `nxf doctor` reports the destination
and warns about an inaccessible shared directory, including group membership
that needs a new login.

The per-user state directory is `NXF_STATE`, or `$XDG_STATE_HOME/nxf`, or
`~/.local/state/nxf`. `NXF_STATE` controls result links, source roots, read
markers and fallback records; it does not disable shared publication.
`NXF_UPDATER_STATE` overrides `/var/lib/nxf` for shared records and updater
state. Status reads shared records first, then per-user records for missing
keys. On successful shared publication, nxf also imports the invoking user's
own old records without replacing existing shared entries. Dangling links,
non-symlinks and targets other than directories under `/nix/store` are ignored.
Status never imports records, including with `--json`.

That is all status needs:

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
  running system differ from those of the booted system. No task and no
  automatic update restarts the machine.
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
- `state`: the per-user state can be written;
- `records`: where build records go, with a warning when the shared
  directory exists and cannot be used;
- `running`: the running system was built by nxf or the updater.

Each line starts with a mark: `✓` passed, `!` a warning, `✗` failed, with how
to fix it. Any `✗` makes it exit non-zero; checks that need the checkout are
skipped until it is found.

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
`services.nxf.flake` (default `/etc/nixos`). It also creates the user and the
group `nxf`, lets the group use sudo without a password, creates
`/var/lib/nxf`, hands the flake directory and that state over to the user and
the group, and makes Git trust the flake directory for every user; see
[Who runs it, and who may work with it](#who-runs-it-and-who-may-work-with-it).
`services.nxf.autoUpdate.enable` does the same and adds the timer. The package
alone is `nxf.packages.<system>.nxf`; it brings nh, dix, Git, Nix, OpenSSH,
tar, coreutils and systemd for machines that do not have them on `PATH`.
[Layout](CONTRIBUTING.md#layout) lists the programs nxf calls.

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
moment on every machine. Tasks such as `switch` do not take `--config`; the
one way to change the system of several machines is
[`rollout`](#from-another-computer).

nxf runs itself on each machine with the system's `ssh`, in batch mode, all
machines at once. It uses your SSH configuration, keys and `known_hosts`, and
needs nxf installed on the machines. A machine that does not answer is a
failed row and makes the command fail. ssh has `NXF_SSH_CONNECT_TIMEOUT`
seconds to connect, 10 by default, also when `rollout` runs it. A machine has
`NXF_SSH_TIMEOUT` seconds to answer one of these commands, 120 by default;
after that its row is `timed out after <N> seconds`, with the limit, and not
`unreachable:`. `nxf status --config <file> --json` prints one entry for each
machine: its `status --json`, or an `error` with the text of its failed row.
With `--config`, status takes only `--json`.
`hold` and `release` need an SSH user that may write `/var/lib/nxf`: root or
a member of the group `nxf`.

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
3. With `--hardware`, `hardware.nix` is written by `nixos-generate-config`,
   without file systems: the flake describes those. Use it on the machine
   itself.
4. With `--files <dir>`, the content of that directory is laid over the
   template: the machine's own values, such as its name or its secrets. nxf
   does not read them. What the template's `.gitignore` names stays out of
   the repository. A `<dir>` that is not a directory fails the command
   before anything is written.
5. `nxf.nix` is added: a module file that enables automatic updates and shows
   every option of the nxf module commented out with its default. It comes
   from the nxf version that runs init. A template that brings its own
   `nxf.nix` keeps it.
6. The directory becomes a Git repository without a remote, the inputs are
   locked, and everything is committed. A repository for which Git has no
   identity gets `nxf`, without an e-mail, as its own.
7. The checks of `nxf doctor` that concern the checkout run, and
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

## Installing a machine

### On the machine

For the feature brief and acceptance scenarios, see
[Install](docs/features/install.md).

```sh
nxf init /tmp/machine --template <flake>#<name> --hardware
nxf install /tmp/machine
```

`nxf install` runs on the machine itself, started from a NixOS installer:

1. It applies the disk layout of the configuration, which the
   [disko](https://github.com/nix-community/disko) module describes: the
   disks are erased, formatted and mounted. With `--mounted` it skips this and
   uses what is mounted at `/mnt`.
2. It copies the flake to `/etc/nixos` of the new system, as the same Git
   repository without a remote, and runs `nixos-install` from it.
3. It makes the SSH host key and prints it, and records the installation in
   `/var/lib/nxf/installed.json` and which system the flake built.

`--reboot` restarts into the new system. Install needs root: another user
gets `Installing needs root`. It refuses outside an installer (a system that
runs from a disk), on a system that nxf manages (one it installed, or one
whose system has nxf, also when it runs from memory), with uncommitted
changes in the flake, and, with `--mounted`, into a target whose `/etc/nixos`
is not empty. A system runs from memory when the last `/` line of
`/proc/mounts` names a `tmpfs`, a `rootfs` or a `ramfs`. Inputs are fetched
on the machine, so a private input needs its credentials in the installer.

### From another computer

For the feature brief and acceptance scenarios, see
[Bulk Install](docs/features/bulk-install.md).

```sh
nxf rollout --config hosts.toml --template <flake>#<name> --files ./machines
```

`rollout` brings every machine of a [hosts file](#the-hosts-file) to the
same state: managed by nxf. It asks each machine what it is, and then:

| The machine is | Rollout |
| --- | --- |
| managed: installed by nxf, or with nxf in its system, whatever its root file system is | leaves it alone |
| a NixOS installer | installs it |
| another system, such as Ubuntu | starts a NixOS installer from memory (kexec), then installs it |
| not reachable | reports it and fails |

A machine with nxf only on its `PATH` is left alone too, unless it is a NixOS
installer.

The command can be repeated: machines that are managed are not touched, so
a second run only installs what the first one could not. Installing erases
disks, so rollout shows the plan and asks before it erases anything; `--yes`
confirms without a question, and without a terminal nothing is erased unless
`--yes` is given. Before the question rollout finds its own package and reads
the template. A template that cannot be read, or an nxf that does not run
from the Nix store, ends the command: nothing is asked and nothing is erased.

For each machine it installs, rollout copies this nxf and the template to
the machine, runs `nxf init --hardware` and `nxf install` there, restarts it,
and waits until the new system answers with the host key the installation
made. It waits up to `NXF_ROLLOUT_WAIT` seconds, 600 by default, for the
installer to answer, and as long again for the new system; a machine that
takes longer counts as failed and is named under `Not installed`. The flake
never exists on your computer. `--files <dir>` gives each
machine the content of `<dir>/<name>/`, laid over the template. A `<dir>`
that is not a directory is refused before any machine is asked.

The outcome lists the host key of every installed machine as a `known_hosts`
line; `--known-hosts <file>` writes them into that file, replacing the key of
the system that was there before. When the file cannot be written, or the old
key cannot be removed, the machine stays installed and the other machines are
still installed. The row `Its host key was not written:` shows the cause, the
line of the machine is still printed, and the command fails. When the old key
cannot be removed, the new key is not added. `--kexec-url` names another
installer image.

Limits: the SSH user must be root. A machine that another user would install
is a failed row of the plan, `not installed: the SSH user must be root, and
its user id is <id>`. It is not installed, the other machines are, and the
command fails. The machine needs a way to the internet for the installer
image and the inputs. The first connection to a machine nobody has connected
to accepts its host key; a key that differs from one you already know is
refused. `nix copy`, which brings nxf and the template to the machine,
trusts the same keys: rollout writes them into one file and names it in
`NIX_SSHOPTS`. A `known_hosts` of yours that is there and cannot be read
fails the machine. The options of ssh name files in the temporary directory,
so rollout refuses one whose path holds a space, a quote or a backslash,
before any machine is asked. It makes a directory of its own there for the
run, for its user alone, and refuses one that is there already.

### Upgrading machines

For the feature brief and acceptance scenarios, see
[Bulk Upgrade](docs/features/bulk-upgrade.md).

```sh
nxf rollout --config hosts.toml --upgrade                 # one machine at a time
nxf rollout --config hosts.toml --upgrade --batch 3       # the first, then three at a time
nxf rollout --config hosts.toml --upgrade nixpkgs         # update only these inputs
```

With `--upgrade`, rollout installs nothing: it runs `nxf upgrade` on each
machine of the hosts file, in the order of their names, as you would by hand.
The order compares the names byte by byte, so `node-10` comes before
`node-2`. The first machine upgrades alone. When it succeeded, the others follow in
waves of `--batch` machines (1 when not given), and each wave starts only when
the one before has ended without a failure.

```
hosts.toml · 3 machines

  ✓ node-1    inputs updated, switched
  ✗ node-2    switch failed: Upgrade stopped: activation did not finish successfully | warning: the following units failed: k3s.service | Command exited with status 1. | Some changes may already be active.
  · node-3    not started

✗ Upgrade failed on 1 of 3 machines
  Not started: node-3. Run the rollout again once the failure is fixed.
```

- A failure, or a machine that does not answer, lets its own wave end and
  starts no other. The machines of a later wave are listed as not started.
  The row of a failed task holds what the task said on the machine, its
  lines joined by ` | `.
- Each machine updates its own flake, so each commits its own `flake.lock`;
  the machines get the same revisions only when they fetch at the same time.
  A machine with no new revision still switches to its committed flake.
- It is a task run by hand on each machine: holds, strategies, the
  [hooks](#hooks) and the [health checks](#health-checks) do not apply, and
  the result is not the automatic update of `nxf status`. A wave stops on
  what `nxf upgrade` reports: a fetch, a build or a switch that fails.
- The SSH user must be able to work with the flake and switch: root, or a
  member of the group `nxf`.
- Each machine has `NXF_UPGRADE_TIMEOUT` seconds, 3 hours by default. A
  machine that takes longer has the row
  `timed out after <N> seconds; the upgrade may still run on the machine`,
  and no further wave starts.
- Named inputs go to every machine; one that a machine's flake does not
  have fails on the first machine, before any other changes.

## Automatic updates

For the feature brief and acceptance scenarios, see
[Automatic Update](docs/features/automatic-update.md).

`nxf-auto-update.timer` starts `nxf-auto-update.service`, which runs
`nxf auto-update`:

1. If updates are held, files other than `flake.lock` have uncommitted
   changes, or the [check hook](#hooks) fails, the run does nothing and
   reports `skipped`.
2. `update` handles the inputs one at a time, as described above. What it
   does with each, and with one that cannot be fetched or does not build, is
   the input's [strategy](#strategies).
3. If the committed flake is not what runs, `switch` builds and activates it:
   a live switch of everything. The machine is never restarted; a new kernel
   takes effect at the next restart, and status says that one is needed.
   Shared records let the updater skip `nh os build` when the committed tree
   is already running, even if someone else built it. Configuration selection
   and input strategies still require evaluation before that comparison.
   A recorded system that is not running is still built by the switch task.
4. After the switch, the [health checks](#health-checks) run.
5. The results are saved to `/var/lib/nxf/update.json`, and the service fails
   if any task failed. `nxf status` shows them once to each user. nxf sends
   no other notification; after a failure, or an input it is to report, it
   runs the [failure hook](#hooks) of the configuration.
6. A run that cannot go on is saved too: the results of the tasks that ran,
   followed by `update` `failed` with the cause. A run that fails before any
   task ran, for example because the checkout names an unknown strategy, has
   that one result. The failure hook runs for it too, and the service fails.
   When the hook fails too, the result `hook` `failed` is saved, and the
   command shows `The failure hook failed too:` with what the hook said.
   When the results cannot be saved, the command fails with the cause and
   the failed save beneath it.

`nxf auto-update --state <dir>` saves `update.json` in `<dir>` instead, and
`$NXF_RESULTS` names the file there. The hold, the bad marks and the build
records stay in `/var/lib/nxf`, and `nxf status` reads the results from
`/var/lib/nxf` only. The service does not use the flag.

| Option under `services.nxf.autoUpdate` | Default | Meaning |
| --- | --- | --- |
| `schedule` | `*-*-* 00/6:00:00` | systemd calendar expression |
| `randomizedDelay` | `1h` | Latest start after the schedule |
| `fixedDelay` | `true` | Keep the same delay on every run, different for each machine |
| `gitIdentity` | `nxf`, no e-mail | Identity of the lockfile commits of the update |
| `gitCredentialsFile` | none | Tokens for Git inputs over HTTPS |
| `nixConfigFile` | none | nix.conf fragment with tokens for other fetchers |
| `strategy` | `report` | What the update does with an input: `off`, `flag` or `report` |
| `inputs` | none | The strategy of single inputs, by name |
| `checkHook` | none | Script that is asked before an update starts |
| `healthChecks` | none | Scripts that must succeed after a switch |
| `healthTimeout` | `300` | Seconds the checks have to pass |
| `healthInterval` | `10` | Seconds between two rounds of checks |
| `failureHook` | none | Script that runs after a failed update, or an input to report |
| `hookTimeout` | `60` | Seconds each of the two hooks has to end |
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
minutes, just fails until then. The two times are those of the new
configuration too: they are in `/etc/nxf/health.conf`, which the switch
replaces together with the scripts.

Each script runs with `set -e`: a check fails when any of its commands does.
Modules can each add lines to one check, and every line must succeed.

If checks still fail at the end:

- the revision is marked as bad in `/var/lib/nxf/bad/<tree>`, with what the
  failing checks said;
- nxf activates the previous system again, also when the revision could not
  be marked as bad. When that activation fails, the result reads
  `Could not return to <system>:` with the cause;
- the run fails and reports `health: failed`. When the revision could not
  be marked, the result names that too;
- later runs skip the switch of a marked revision until a newer revision is
  committed. The bad revision stays in the repository, because it built.

Tasks run by hand, such as `nxf switch`, run no health checks.

### Hooks

The configuration has a say at two points of an automatic update: before it
starts, and after it failed. Each is one shell script. nxf has none of its
own, knows nothing about what they do, and has no other hooks.

```nix
services.nxf.autoUpdate = {
  checkHook = ''
    test "$(curl --fail --silent https://example.org/rollout)" = open
  '';
  failureHook = ''
    curl --fail --silent --request POST --data @"$NXF_RESULTS" https://example.org/rollout/pause
  '';
};
```

- `checkHook` becomes `/etc/nxf/hooks/check` and is asked before the update
  starts. When it fails, the run is skipped: it reports `skipped` with the
  last line the script printed, and nothing is fetched, built or activated.
  The run does not count as failed.
- `failureHook` becomes `/etc/nxf/hooks/failure`. It runs after a run in
  which a task failed, such as a switch or the health checks. It runs after
  a run that cannot go on, also when no task ran. It runs after a run that
  could not update an input whose [strategy](#strategies) is `report`. The
  file `$NXF_RESULTS` holds the results of the run, the same JSON as
  `/var/lib/nxf/update.json`. When the hook fails itself, that is reported,
  as the result `hook`.
- A hook fails when any of its commands does. Modules can each add lines to
  a hook, and the lines run in the order of the configuration.
- Each hook has `hookTimeout` seconds; one that does not end is stopped and
  counts as failed.

Tasks run by hand ask no hook and run none.

#### Stopping a rollout

The two together let one machine stop the others. A machine that updates
earlier than the rest, by its `schedule`, reports a failure with its failure
hook; the others ask with their check hook, before every update, whether to
go on. What stands between them is yours: a file on a server, a small
service, or `nxf hold --config` run from the hook. nxf defines only when the
hooks run.

Know what this does and does not do:

- It stops what breaks the build, the switch or the health checks. An input
  that was tampered with and passes them stops nothing.
- A check hook that cannot reach its answer should fail, as the one above
  does, so that an outage holds updates instead of letting them through. The
  held machines say so in `nxf status`.
- Nothing starts the rollout again by itself. Whoever looked at the failure
  opens it.
- The machine that updates first gets every new revision first. Give its hook
  the right to report, and nothing more.

### Holding updates

```sh
nxf hold --reason "waiting for the vendor fix"         # until released
nxf hold --reason "release week" --for 3d              # m, h, d or w
nxf hold --reason "release week" --until "2030-01-31 18:00"
nxf release
```

A hold stops the automatic update: the run reports `skipped` with the reason.
The hold is the file `/var/lib/nxf/hold.json`, so it survives a restart, and
only root or a member of the group `nxf` can set or release it. `nxf status` shows it. A
hold with an end stops holding at that time. Tasks run by hand are not held.

### Strategies

Not every input can be updated unattended: one needs a VPN, another
credentials that the machine should not hold, a third is moved on purpose
only. Each input has a strategy for the automatic update:

| Strategy | The automatic update |
| --- | --- |
| `off` | leaves the input alone. It is updated by hand, with `nxf update <input>` |
| `flag` | tries. An update that cannot be fetched or does not build is recorded, and the run goes on and does not fail. `nxf status` shows the record |
| `report` | does the same as `flag`, and the [failure hook](#hooks) runs after the run |

```nix
services.nxf.autoUpdate = {
  strategy = "report"; # every input without an entry below
  inputs = {
    work = "off";
    unstable = "flag";
  };
};
```

nxf reads the strategies from the configuration in the checkout, not from
the system that runs: a commit changes them, and no switch is needed. They
concern the automatic update only. A task run by hand updates what it is
asked to and fails when an update does.

No strategy holds back the other inputs or the switch: an input that could
not be updated stays as it was, and what the others brought is rolled out.

A build needs every input, also one the automatic update does not touch.
Nix reads a locked input from the Nix store when it is there, and asks the
input's origin only when it is not. So whenever nxf has built a committed
flake, it copies the locked inputs into the store and keeps them there, as
GC roots in `sources` of its own state. Those roots protect the shared store
for every user and are not moved or shared. Locked inputs that are kept this
way need no fetch access for the next user. Additional sources fetched at
evaluation time outside `flake.lock` may still need network access or
credentials, even when build records are shared. Nix's fetcher cache remains
per-user.

### Who runs it, and who may work with it

The update runs as the system user `nxf`, not as root, and commits with the
identity of `gitIdentity`. Only the switch needs root, and nh gets it with
sudo. The module therefore lets the group `nxf` use sudo without a password:
an unattended run cannot answer a prompt.

That user is the owner of the process and no person: it has no home. What
the update needs to reach private inputs is given to it by path, see below.
What Nix keeps between runs, the clones of Git inputs and what it remembers
of earlier fetches and evaluations, is in `/var/cache/nxf` and can be deleted
at any time; the inputs themselves are in the Nix store. A member of the
group who runs a task by hand does so under their own account, with their
own access to the inputs and Nix's cache in their own home.

The user `nxf` owns the flake directory and the state in `/var/lib/nxf`, and
the group `nxf` may write them. The module creates both. Whoever else is in
the group can work with nxf, with no other setting:

```nix
users.users.alice.extraGroups = [ "nxf" ];
```

A member runs `update`, `build`, `switch` and the other tasks on the checkout
with their own Git identity, and holds or releases updates. The group is
trusted like root: what its members write is what the switch activates, and
sudo asks them for no password.

The module hands over only what nxf changes: the directory, `flake.lock`, the
`.git` repository, which it sets to `core.sharedRepository = group`, and the
state. Other files, such as `secrets/`, keep their owner and mode. nxf creates
its files writable by the group while the checkout is, and the module makes
Git trust the checkout for every user: Git refuses one that another user
owns, which is the case for root and for the other members of the group.

A flake that `nxf init` or `nxf install` founds belongs to whoever ran it,
usually root, because the user `nxf` exists only once the configuration is
active. The module hands it over when the machine starts, and after every
switch that changes the module.

The commits of the update carry the name `nxf` and no e-mail: they stay on the
machine. Set `gitIdentity` for another identity, for example when the machine
flake is pushed to a server that wants an e-mail.

Inputs fetched with Git over HTTPS (`git+https://...`) take a token from
`gitCredentialsFile`, a file in the format of
[git-credential-store](https://git-scm.com/docs/git-credential-store), one
line for each server:

```
https://machine:TOKEN@git.example.org
```

`github:`, `gitlab:` and tarball inputs are fetched by Nix itself. Give their
tokens in `nixConfigFile`, a nix.conf fragment:

```
access-tokens = github.com=TOKEN
```

Where the two files are is yours to choose: a directory beside the flake
that Git ignores, or secrets committed encrypted that sops-nix or agenix
decrypt. They must be outside the Nix store, the user `nxf` must be able to
read them, and nobody else should.

Inputs over SSH also work. The service runs `ssh` in batch mode, and the user
`nxf` has no `~/.ssh`: its key and the host keys it must trust come from the
SSH configuration of the system, for example `programs.ssh.knownHosts` and a
`Match localuser nxf` block with an `IdentityFile` in
`programs.ssh.extraConfig`.

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
nix develop          # Rust, nh, dix, Git, Nix, OpenSSH, util-linux, just and nixfmt
just test            # all tests
just check           # formatting, lints, tests and the flake checks
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the rules to follow when changing
nxf.

## License

MIT or Apache-2.0, at your option.
