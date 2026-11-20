# Bulk Install

Installing the machines of a hosts file from another computer.

## Brief

Whoever brings up several machines wants each one installed the same way,
from one template, without carrying a USB installer from machine to machine,
and wants to run the same command again after fixing a machine that failed.

`nxf rollout --config <file> --template <flake>#<name>` asks every machine
over ssh what it is, all at once, and shows a plan:

- A machine that nxf installed, or whose system has nxf, is left alone,
  whatever its root file system is.
- A NixOS installer is installed.
- Another system, such as Ubuntu, first starts a NixOS installer from memory
  (kexec, from `--kexec-url` or the nixos-images release), and is then
  installed.
- A machine that does not answer is a failed row.
- A machine that an SSH user other than root would install is a failed row
  and is not installed.

Installing erases disks, so rollout asks before it installs anything. `--yes`
confirms without a question, and without a terminal nothing is erased unless
`--yes` is given. Before the question rollout finds its own package and the
source of the template, so a template that cannot be used fails the command
and no question is asked.

The machines are then installed one at a time, in the order of their names.
The order compares bytes, so `node-10` comes before `node-2`.
For each one rollout copies this nxf and the template, stored in the Nix
store with the uncommitted changes of its checkout, to the machine. It runs
`nxf init --hardware` there, with `<files>/<name>/` laid over the template
when `--files <files>` is given, and then runs `nxf install`. It restarts the
machine and waits up to `NXF_ROLLOUT_WAIT` seconds, 600 by default, until the
new system answers with the host key the installation made. A failed machine
does not stop the others.

The outcome names the revision and the `known_hosts` line of every installed
machine. `--known-hosts <file>` writes the lines into that file and replaces
the key of the system that was there before. When the file cannot be written
or the old key cannot be removed, the machine stays installed, its line is
still printed, the other machines are still installed, and the command
fails.

Boundaries:

- The flake never exists on the computer that runs rollout; each machine
  founds and commits its own.
- It does not upgrade machines that are already managed; that is
  [Bulk Upgrade](bulk-upgrade.md).
- A machine is managed when it has `/var/lib/nxf/installed.json` or an
  executable `/run/current-system/sw/bin/nxf`, also when its root file
  system is in memory. nxf on the `PATH` alone counts only on a machine that
  is not a NixOS installer.
- The SSH user must be root. The probe reports the user id, and a machine to
  install that answers with another id is refused in the plan, before
  anything is erased. A managed machine is left alone whatever the user is.
- The machine needs a way to the internet for the installer image and the
  inputs.
- The first connection to a machine nobody knows accepts its host key; a key
  that differs from one in `~/.ssh/known_hosts` is refused. `nix copy` gets
  the same SSH options in `NIX_SSHOPTS` and trusts the same keys; Nix 2.26
  and later split them as a shell does. After the installation only the key
  it made is accepted.
- It does not roll back a machine whose installation failed halfway. Its
  disks may already be erased, and a later run installs it again when it
  still answers as an installer. A machine that no longer does is treated as
  what the probe finds then.

## Acceptance scenarios

| ID | Given | When | Then |
| --- | --- | --- | --- |
| I1: Repeatable | Every machine is installed by nxf | `rollout` runs again | Only the probe runs on each machine; every row is `installed by nxf`; the outcome is `N machines installed by nxf`; the exit status is 0 |
| I2: Asks before erasing | A managed machine, an Ubuntu machine and a NixOS installer, no terminal and no `--yes` | `rollout` runs | The rows are `installed by nxf`, `<system>; it will be erased` and `NixOS installer; its disks will be erased`; only the probe ran on each machine; the outcome is `2 machines would be erased; nothing was changed` with `Add --yes to erase and install <names>.`; the exit status is 1 |
| I3: A machine does not answer | One machine is unreachable and the others are installed by nxf | `rollout --yes` runs | Its row is `unreachable:` with the last line ssh printed; no row says that it will be erased; the outcome is `1 of N machines gave no answer`; the exit status is 1 |
| I4: Install from an installer | A machine booted into a NixOS installer | `rollout --yes` runs | The rows of its phases are `Already in a NixOS installer`, `Founded on the machine`, `Installed` and `Started, and its host key is the one it was given`; `/etc/nixos` on the machine holds the flake with `hardware.nix`; the outcome is `1 machine installed` with the machine's revision and its `known_hosts` line; the exit status is 0 |
| I5: Install over another system | A machine runs Ubuntu | `rollout --yes` runs | The row of the first phase is `Started from memory`, with the image URL in which `{arch}` is the machine's `uname -m`; the rest is as in I4 |
| I6: Machine files | `--files <dir>` with `<dir>/<name>/` for some machines | `rollout --yes` runs | `/etc/nixos` on those machines holds their files over the template; on the others it holds the template alone |
| I7: Known hosts are written | `--known-hosts <file>` holds the key of the system that was there before | `rollout --yes` installs the machine | The file holds one line for the address, with the key the installation made, and keeps the lines of other machines; with a port other than 22 the line starts with `[address]:port`. When `ssh-keygen -R` cannot remove the old key, the machine has the row `✗ Its host key was not written: cannot remove the old key of <address> from <file>:` with the last line `ssh-keygen` printed; the file is unchanged, so the new key is not added; the machine's `known_hosts` line is still printed; the exit status is 1 |
| I8: One machine fails | Two machines to install and the first one's installation fails | `rollout --yes` runs | The first has a `✗` row that names the step that failed, with the last lines of that step beneath; the second is installed as in I4; the outcome is `1 of 2 machines not rolled out` with `Not installed: <first>.`; the exit status is 1 |
| I9: Template with uncommitted changes | The template's checkout has an uncommitted change | `rollout --yes` runs | `/etc/nixos` on each installed machine holds the changed content |
| I10: The erase question is declined | A machine to install, a terminal and no `--yes` | `rollout` runs and the answer to `Type yes:` is not `yes` | Only the probe ran on each machine; the outcome is `Nothing was changed`; the exit status is 1 |
| I11: Unreachable beside a machine to install | One machine is a NixOS installer and another is unreachable | `rollout --yes` runs | The first is installed as in I4; the row of the other stays `unreachable:`; the outcome is `1 of 2 machines not rolled out`, and `Not installed` does not name the unreachable machine; the exit status is 1 |
| I12: A machine does not come up in time | A machine that does not answer as an installer, or later as the installed system, within `NXF_ROLLOUT_WAIT` seconds | `rollout --yes` runs | Its `✗` row is `The installer did not come up in time` or `The installed system did not come up in time`, with `Last seen:` and what the last probe found; its host key is not listed and not written to `--known-hosts`; it is named under `Not installed`; the exit status is 1 |
| I13: A managed machine that runs from memory | A machine whose root file system is a `tmpfs`, with `/etc/NIXOS`, and with `/var/lib/nxf/installed.json` or an executable `/run/current-system/sw/bin/nxf` | `rollout --yes` runs | Its row is `installed by nxf`; only the probe runs on it; no row says that it will be erased |
| I14: Another user than root | Two machines to install, and one of them answers the probe as a user other than root | `rollout --yes` runs | Its row is `✗ <name>  <system>; not installed: the SSH user must be root, and its user id is <id>`; only the probe ran on it; the other machine is installed; the outcome is `1 of 2 machines not rolled out` with `Not installed: <name>.`; the exit status is 1 |
| I15: Known hosts cannot be written | Two machines to install, and a `--known-hosts <file>` that cannot be written | `rollout --yes` runs | Both machines are installed; each has the row `✗ Its host key was not written: cannot write <file>:` with the cause; the outcome is `2 machines installed, but not every host key was written` with `Not written to <file>: <names>.`; the revision and the `known_hosts` line of both are printed; the exit status is 1 |
| I16: tar fails on a machine's files | `--files <dir>` with `<dir>/<name>/`, and `tar` fails while it packs them | `rollout --yes` runs | The machine's `✗` row is `Sending the machine's files failed`, with what `tar` and ssh printed beneath; `nxf init` does not run on the machine; it is named under `Not installed`; the exit status is 1 |
| I17: A template that cannot be used | A machine to install, a terminal, no `--yes`, and a `--template` without `#<name>` | `rollout` runs | The outcome is `The template must be given as <flake>#<name>`; the question `Type yes:` is not asked; only the probe ran on each machine; the exit status is 1 |

## Existing verification

[Rollout fixtures](../../tests/rollout.rs) run through a mock ssh. In the
fixtures of I1 to I3 it only answers the probe. In those that install, it
also plays an installation: it keeps the files it is sent, reports a host
key for `nxf install`, and answers as a managed machine from then on. There
a mock `nix` names the source of the template and records what `nix copy`
is given, and `NXF_PACKAGE` stands in for the package of nxf. No fixture
runs kexec, disko, `nixos-install` or a restart.

- I1: `tests/rollout.rs::repeatable`.
- I2: `tests/rollout.rs::asks_before_erasing`.
- I3: `tests/rollout.rs::unreachable`, with one managed and one unreachable
  machine.
- I4: `tests/rollout.rs::unpacked_files_fail_the_machine` covers only the
  outcome `1 machine installed` of a played installation, not the rows of
  the phases and not what `/etc/nixos` holds.
- I6: `tests/rollout.rs::unpacked_files_fail_the_machine` covers that the
  files of a machine are sent to it and that `nxf init` gets `--files`, not
  what `/etc/nixos` holds and not a machine without files.
- I7: `tests/rollout.rs::known_hosts` covers the form of the line.
  `tests/rollout.rs::old_key_is_replaced_or_said` covers the replacement in
  the file, the kept line of another machine, and an old key that cannot be
  removed.
- I8: `tests/rollout.rs::unpacked_files_fail_the_machine` and
  `tests/rollout.rs::copy_trusts_as_the_link` cover the `✗` row of a step
  that fails and the exit status, with one machine. No test covers a second
  machine that is installed after the first failed.
- I9: `tests/rollout.rs::template_source_is_stored` covers only that the
  source in the Nix store holds the changed content, not that a machine gets
  it.
- I11: `tests/rollout.rs::not_rolled_out` covers that the unreachable
  machine is counted and not named under `Not installed`, beside a machine
  whose installation fails, not beside one that is installed.
- I13: `tests/rollout.rs::managed_from_memory` covers what the probe answers
  on such a machine, not the row. I1 covers the row of a managed machine.
- I14: `tests/rollout.rs::installs_only_as_root`, with two refused machines,
  one managed and one that is installed;
  `tests/rollout.rs::probe_tells_the_user` for the user id in the probe's
  answer.
- I15: `tests/rollout.rs::unwritten_known_hosts`.
- I16: `tests/rollout.rs::unpacked_files_fail_the_machine`. It asserts what
  `tar` printed, not what ssh printed.
- I17: `tests/rollout.rs::bad_template_before_the_question`.
- `tests/rollout.rs::probe` covers how the probe's answer is read, and
  `tests/rollout.rs::copy_trusts_as_the_link` the SSH options that
  `nix copy` gets.

No automated test covers I5, I10 and I12, or the copy of I9. I4, I6 and
I8 are covered only as far as said above: what a machine holds after an
installation needs real machines with kexec, disko, `nixos-install` and a
restart.

I1 and I4 to I8 were run by hand against three Incus Ubuntu 24.04 VMs with
the default installer image, a template with a disko layout and the nxf
module, and SSH as root:

- The first run installed node-2 and node-3 from Ubuntu (I5). node-2 had its
  own files and node-3 the template alone (I6). The `--known-hosts` file had
  their Ubuntu keys replaced by the new ones (I7). node-1's files failed an
  assertion, so its installation failed after disko had erased its disks;
  the others were still installed and the command named node-1 (I8).
- The second run, with node-1's files fixed, found node-1 still in the
  installer and installed it from there (I4). A third run only probed the
  three machines (I1).

That run is recorded in commit 69bb112, where nxf is 0.13.0 and `src/` is
that of commit eb6f355. The record does not name the commit the nxf that ran
was built from, and it keeps none of the output. The run is not repeatable
in CI.

I10 and I11 were run by hand with nxf 0.13.0, built from commit 1905cce,
through a mock ssh that answers the probe. No machine was touched:

- I10, in a pseudo-terminal with the answer `no`: the outcome was
  `Nothing was changed`, the exit status was 1, and ssh had run only the
  probe on each machine.
- I11, with `--yes`, one machine that answered as a NixOS installer and one
  unreachable, both under addresses that do not resolve: the installation of
  the first was started and failed at `Copying nxf to the machine failed`.
  The outcome was `2 of 2 machines not rolled out` with
  `Not installed: node-1.`, and the exit status was 1. This shows that the
  unreachable machine is counted and not named. It does not show a completed
  installation beside it.

I1, I2 and I5 to I7 were run by hand with nxf 0.13.1, built from commit
0226ab0, against one Incus Ubuntu 24.04 VM with SSH as root, and with
`--files` and `--known-hosts`:

- Without `--yes` and without a terminal, rollout showed the plan and failed
  (I2, with one machine to erase).
- With `--yes` it started the installer from memory, founded the flake,
  installed, restarted the machine, and got the reported host key back (I5,
  and I6 for a machine with its own files). The Ubuntu key in the
  `--known-hosts` file was replaced, and the line of another machine was
  kept (I7).
- A second run only probed the machine and ended with
  `1 machine installed by nxf` (I1).

None of the output of that run is kept. The run is not repeatable in CI.

No run covers I12 to I17.

See the [operating documentation](../../README.md#from-another-computer).
