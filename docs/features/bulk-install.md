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
machine and waits until the new system answers with the host key the
installation made. It waits up to `NXF_ROLLOUT_WAIT` seconds, 600 by default,
for the installer to answer, and as long again for the new system. A failed
machine does not stop the others.

The outcome names the revision and the `known_hosts` line of every installed
machine. `--known-hosts <file>` writes the lines into that file and replaces
the key of the system that was there before. When the file cannot be written
or the old key cannot be removed, the machine stays installed, its line is
still printed, the other machines are still installed, and the command
fails.

`nxf install` on a machine is run with `--json`, and rollout reads the host
key of the machine from the object it prints. A failed installation is the
refusal object that it prints on standard output, and its cause and lines
are the row of the machine; the machine shows no live line while the
installation runs. With `--json` rollout prints no text on standard output and
asks nothing: it prints one JSON object at the end, with `ok`, `counts` of the
machines in each state and one entry for each machine, in the order of the
hosts file. A machine is `managed`, `installed`, `failed` or `not started`,
with its `error` and `details` when something went wrong, and with its
`revision` and `hostKey` when it was installed. The README section
[Scripts](../../README.md#scripts) has the shape.

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
  anything is erased. A probe answer in which `id` is missing reports
  `uid=unknown`, and the machine is refused with
  `the SSH user must be root, and its user id is unknown`. A managed machine
  is left alone whatever the user is.
- The machine needs a way to the internet for the installer image and the
  inputs.
- The first connection to a machine nobody knows accepts its host key; a key
  that differs from one in `~/.ssh/known_hosts` is refused. `nix copy`
  trusts the same keys: rollout writes them into one file and names it in
  `NIX_SSHOPTS`. After the installation only the key it made is accepted.
- The options of ssh name files in a directory that rollout makes for the run
  in the temporary directory; the rule is in the
  [operating documentation](../../README.md#from-another-computer).
- A `~/.ssh/known_hosts` that does not exist is left out. One that is there
  and cannot be read fails the command before any machine is asked, also
  when no machine is to be installed: a run in which every machine is managed
  fails on an unreadable file. A file of keys that the run wrote itself and
  cannot read fails that machine at the copy.
- When `ssh-keygen -R` removes the old key, it leaves the original content of
  the `--known-hosts` file as `<file>.old` beside it.
- `--files <dir>` takes a directory. Anything else is refused before any
  machine is asked. A machine without `<dir>/<name>/` gets the template
  alone.
- It does not roll back a machine whose installation failed halfway. Its
  disks may already be erased, and a later run installs it again when it
  still answers as an installer. A machine that answers as something else
  is treated as what the probe finds.
- A machine whose installation completed and whose system did not answer
  within `NXF_ROLLOUT_WAIT` seconds is a failed machine. When its system
  answers later and ssh accepts its key, the next run finds it managed and
  leaves it alone. Its host key is never printed or written, and
  `--known-hosts` keeps the key of the system that was there before.

## Acceptance scenarios

| ID | Given | When | Then |
| --- | --- | --- | --- |
| I1: Repeatable | Every machine is managed | `rollout` runs again | Only the probe runs on each machine; every row is `installed by nxf`; the outcome is `N machines installed by nxf`; the exit status is 0 |
| I2: Asks before erasing | A managed machine, an Ubuntu machine and a NixOS installer, no terminal and no `--yes` | `rollout` runs | The rows are `installed by nxf`, `<system>; it will be erased` and `NixOS installer; its disks will be erased`; only the probe ran on each machine; the outcome is `2 machines would be erased; nothing was changed` with `Add --yes to erase and install <names>.`; the exit status is 1 |
| I3: A machine does not answer | One machine is unreachable and the others are managed | `rollout --yes` runs | Its row is `unreachable:` with the last line ssh printed; no row says that it will be erased; the outcome is `1 of N machines gave no answer`; the exit status is 1 |
| I4: Install from an installer | A machine booted into a NixOS installer | `rollout --yes` runs | The rows of its phases are `Already in a NixOS installer`, `Founded on the machine`, `Installed` and `Started, and its host key is the one it was given`; `/etc/nixos` on the machine holds the flake with `hardware.nix`; the outcome is `1 machine installed` with the machine's revision and its `known_hosts` line; the exit status is 0 |
| I5: Install over another system | A machine runs Ubuntu | `rollout --yes` runs | The row of the first phase is `Started from memory`, with the image URL in which `{arch}` is the machine's `uname -m`; the rest is as in I4 |
| I6: Machine files | `--files <dir>` with `<dir>/<name>/` for some machines | `rollout --yes` runs | `/etc/nixos` on those machines holds their files over the template; on the others it holds the template alone |
| I7: Known hosts are written | `--known-hosts <file>` holds the key of the system that was there before | `rollout --yes` installs the machine | The file holds one line for the address, with the key the installation made, and keeps the lines of other machines; with a port other than 22 the line starts with `[address]:port` |
| I8: One machine fails | Two machines to install and the first one's installation fails | `rollout --yes` runs | The first has a `✗` row that names the step that failed, with the last lines of that step beneath; the second is installed as in I4; the outcome is `1 of 2 machines not rolled out` with `Not installed: <first>.`; the exit status is 1 |
| I9: Template with uncommitted changes | The template's checkout has an uncommitted change | `rollout --yes` runs | `/etc/nixos` on each installed machine holds the changed content |
| I10: The erase question is declined | A machine to install, no `--yes`, and a terminal that answers `no` to `Type yes:` | `rollout` runs | Only the probe ran on each machine; the outcome is `Nothing was changed`; the exit status is 1 |
| I11: Unreachable beside a machine to install | One machine is a NixOS installer and another is unreachable | `rollout --yes` runs | The first is installed as in I4; the row of the other stays `unreachable:`; the outcome is `1 of 2 machines not rolled out`, without a `Not installed` line; the exit status is 1 |
| I12: A machine does not come up in time | A machine that does not answer as an installer, or later as the installed system, within `NXF_ROLLOUT_WAIT` seconds | `rollout --yes` runs | Its `✗` row is `The installer did not come up in time` or `The installed system did not come up in time`, with `Last seen:` and what the last probe found; its host key is not listed and not written to `--known-hosts`; it is named under `Not installed`; the exit status is 1 |
| I13: A managed machine that runs from memory | A machine whose root file system is a `tmpfs`, with `/etc/NIXOS`, and with `/var/lib/nxf/installed.json` or an executable `/run/current-system/sw/bin/nxf` | `rollout --yes` runs | Its row is `installed by nxf`; only the probe runs on it; no row says that it will be erased |
| I14: Another user than root | Two machines to install, and one of them answers the probe as a user other than root | `rollout --yes` runs | Its row is `✗ <name>  <system>; not installed: the SSH user must be root, and its user id is <id>`; only the probe ran on it; the other machine is installed; the outcome is `1 of 2 machines not rolled out` with `Not installed: <name>.`; the exit status is 1 |
| I15: Known hosts cannot be written | Two machines to install, and a `--known-hosts <file>` that cannot be written | `rollout --yes` runs | Both machines are installed; each has the row `✗ Its host key was not written: cannot write <file>:` with the cause; the outcome is `2 machines installed, but not every host key was written` with `Not written to <file>: <names>.`; the revision and the `known_hosts` line of both are printed; the exit status is 1 |
| I16: tar fails on a machine's files | `--files <dir>` with `<dir>/<name>/`, and `tar` fails while it packs them, or the machine cannot unpack them | `rollout --yes` runs | The machine's `✗` row is `Sending the machine's files failed`, with what `tar` and ssh printed beneath; `nxf init` does not run on the machine; it is named under `Not installed`; the exit status is 1 |
| I17: A template that cannot be used | A machine to install, a terminal, no `--yes`, and a `--template` without `#<name>` | `rollout` runs | The outcome is `The template must be given as <flake>#<name>`; the question `Type yes:` is not asked; only the probe ran on each machine; the exit status is 1 |
| I18: The old key cannot be removed | `--known-hosts <file>` holds the key of the system that was there before, and `ssh-keygen -R` fails | `rollout --yes` installs the machine | The machine has the row `✗ Its host key was not written: cannot remove the old key of <address> from <file>:` (`[<address>]:<port>` takes the place of `<address>` with a port other than 22) with the last line `ssh-keygen` printed; the file is unchanged; the machine's `known_hosts` line is printed; the exit status is 1 |
| I19: The files are not a directory | `--files <dir>`, where `<dir>` is a file or does not exist | `rollout --yes` runs | No machine is asked; the outcome is `<dir> is not a directory`; the exit status is 1 |
| I20: A temporary directory that an option cannot name | A temporary directory whose path holds whitespace, a quote or a backslash | `rollout --yes` runs | No machine is asked; the outcome is `The temporary directory <dir> has a space, a quote or a backslash in its path`, with `The options of ssh cannot name a file there. Set TMPDIR to a directory without one.` beneath it; the exit status is 1 |
| I21: A system that comes up late | An earlier run installed a machine whose system did not answer within `NXF_ROLLOUT_WAIT` seconds; the system answers, and `~/.ssh/known_hosts` holds no other key for its address | `rollout --known-hosts <file>` runs again | Its row is `installed by nxf`; only the probe runs on it; no `known_hosts` line is printed for it; `<file>` keeps the key of the system that was there before |
| I22: A directory of the first name is there before | A machine to install, and `<temporary directory>/nxf-rollout.<pid>.0` with the mode 755, made by a `sh` that then starts rollout with `exec`, so that `<pid>` is the process id of rollout | `rollout --yes` runs | `nix copy` is given a file in `<temporary directory>/nxf-rollout.<pid>.1`, a directory with the mode 700; the directory of the run is removed; `<temporary directory>/nxf-rollout.<pid>.0` is left as it was; the exit status is 0 |
| I23: Known hosts cannot be read | `~/.ssh/known_hosts` is there and cannot be read, and a machine to install | `rollout --yes` runs | No machine is asked; the outcome is `cannot read <file>: <cause>`, with `rollout checks the host key of a machine against the keys in this file before it installs it. Make it a file that can be read, or move it away.` beneath it; the exit status is 1 |
| I24: Every name of the directory is taken | A machine to install, and the directories `<temporary directory>/nxf-rollout.<pid>.0` to `<temporary directory>/nxf-rollout.<pid>.99` exist, made by a `sh` that then starts rollout with `exec`, so that `<pid>` is the process id of rollout | `rollout --yes` runs | No machine is asked; none of the directories is removed; the outcome is `<temporary directory>/nxf-rollout.<pid>.0 to <temporary directory>/nxf-rollout.<pid>.99 are all there already`, with `rollout keeps the keys of a run in a directory it makes itself. Remove them, or set TMPDIR to another directory.` beneath it; the exit status is 1 |
| I25: The temporary directory is not there | A machine to install, and `TMPDIR` names a directory that does not exist | `rollout --yes` runs | No machine is asked; the outcome is `cannot create <TMPDIR>/nxf-rollout.<pid>.0: No such file or directory (os error 2)`, with `rollout keeps the keys of a run there. TMPDIR names the directory it is made in.` beneath it; the exit status is 1 |
| I26: JSON | Two machines, a NixOS installer and a managed one | `rollout --yes --json` runs | Standard output is one JSON object: `ok` is true; `counts` has 1 `installed` and 1 `managed`; the installed machine has its `revision` and its `hostKey`, which is the key that `nxf install --json` printed on the machine; the managed machine has only its name and state; standard error is empty; the exit status is 0; the remote command is `nxf install /tmp/nxf-machine --json` |
| I27: JSON never asks | A managed machine, an Ubuntu machine, a NixOS installer and an unreachable machine, and no `--yes` | `rollout --json` runs | No machine is changed and nothing is asked; `ok` is false; the Ubuntu machine and the installer are `not started`, each with its row text as `error`; the unreachable machine is `failed` with `unreachable:` and the last line ssh printed as `error`; the text of the outcome is on standard error; the exit status is 1 |
| I28: JSON refusal | `--json` and a `--files` that is not a directory | `rollout --yes --json` runs | No machine is asked; standard output is the object `{"error", "details"}` with `<dir> is not a directory`; the exit status is 1 |
| I29: The installation is refused on the machine | A machine to install whose `nxf install --json` prints a refusal object on standard output, exits with status 1, and prints other text on standard error | `rollout --yes` runs | The `✗` row of the machine is `The installation failed`, with the `error` of the object and its `details` beneath it, and not the text on standard error; it is named under `Not installed`; the exit status is 1 |

## Existing verification

[Rollout fixtures](../../tests/rollout.rs) run through a mock ssh and a mock
`nix`; [Tests](../../CONTRIBUTING.md#tests) says what they play. Fixtures
that play no installation use a mock ssh that answers every command with
the answer of the probe. No fixture runs disko, `nixos-install` or a
restart, and none runs kexec: the script that starts the installer is kept,
not run.

- I1: `tests/rollout.rs::repeatable`. It asserts the row of the first of two
  machines.
- I2: `tests/rollout.rs::asks_before_erasing`. It asserts that ssh ran once
  for each machine, not what it ran.
- I3: `tests/rollout.rs::unreachable`, with one managed and one unreachable
  machine. No run by hand covers it.
- I4: `tests/rollout.rs::half_packed_files_fail_the_machine` covers the outcome
  `1 machine installed` and the exit status of a played installation.
  `tests/rollout.rs::not_rolled_out` covers the row
  `Already in a NixOS installer`, `tests/rollout.rs::unwritten_known_hosts`
  the revision and the `known_hosts` line of an installed machine, and
  `tests/rollout.rs::names_reach_the_machine_whole` that `nxf init` gets
  the template with its name as one word. No test covers the rows of the
  other phases or what `/etc/nixos` holds.
- I5: `tests/rollout.rs::names_reach_the_machine_whole` covers that the
  script that starts the installer names the image with `{arch}` replaced,
  and the exit status of a played installation over another system. No
  test covers the row `Started from memory` or runs kexec.
- I6: `tests/rollout.rs::half_packed_files_fail_the_machine` covers that the
  files of a machine are sent to it and that `nxf init` gets `--files`.
  `tests/rollout.rs::files_must_be_a_directory` covers that `nxf init` gets
  no `--files` for a machine without files. No test covers what
  `/etc/nixos` holds.
- I7: `tests/rollout.rs::known_hosts` covers the form of the line, also with
  a port. `tests/rollout.rs::old_key_is_replaced_or_said` covers the
  replacement in the file and the kept line of another machine.
- I8: `tests/rollout.rs::half_packed_files_fail_the_machine`,
  `tests/rollout.rs::unreceived_files_fail_the_machine` and
  `tests/rollout.rs::copy_trusts_as_the_link` cover the `✗` row of a step
  that fails, the lines beneath it and the exit status, with one machine.
  No test covers a second machine that is installed after the first
  failed, or the outcome `1 of 2 machines not rolled out`.
- I9: `tests/rollout.rs::template_source_is_stored` covers only that the
  source in the Nix store holds the changed content. No test and no run by
  hand covers that a machine gets it.
- I10: no automated test covers it.
- I11: `tests/rollout.rs::not_rolled_out` covers that the unreachable
  machine is counted and not named under `Not installed`, beside a machine
  whose installation fails. No test covers it beside a machine that is
  installed, where the outcome has no `Not installed` line.
- I12: no automated test and no run by hand covers it.
- I13: `tests/rollout.rs::managed_from_memory` covers what the probe answers
  on such a machine, not the row. `tests/rollout.rs::repeatable` covers the
  row of a managed machine. No run by hand covers it.
- I14: `tests/rollout.rs::installs_only_as_root`, with two refused machines,
  one managed and one that is installed;
  `tests/rollout.rs::probe_tells_the_user` for the user id in the probe's
  answer. No run by hand covers it.
- I15: `tests/rollout.rs::unwritten_known_hosts`. It asserts the row once, not
  for each machine, and not the cause after `cannot write <file>:`. No run by
  hand covers it.
- I16: `tests/rollout.rs::half_packed_files_fail_the_machine` for a `tar` that
  fails while it packs, with what `tar` printed;
  `tests/rollout.rs::unreceived_files_fail_the_machine` for a machine that
  cannot unpack, with what ssh printed. No run by hand covers it.
- I17: `tests/rollout.rs::bad_template_before_the_question`. No run by hand
  covers it.
- I18: `tests/rollout.rs::old_key_is_replaced_or_said`. It does not cover a
  port other than 22. No run by hand covers it.
- I19: `tests/rollout.rs::files_must_be_a_directory`. No run by hand covers
  it.
- I20: `tests/rollout.rs::unusable_scratch_is_refused`, with a path that
  holds a space, a single quote, a double quote or a backslash. It does not
  cover another whitespace or the line beneath the outcome. No run by hand
  covers it.
- I21: no automated test and no run by hand covers it.
- I22: `tests/rollout.rs::scratch_is_its_own`, with a stand-in `nix` that tells
  the directory of the file it is given and its mode. The test makes the
  directory as the same user, not as another user, and asserts that the
  empty `TMPDIR` of the test holds only that directory afterwards, and that
  it is empty and keeps its mode. No run by hand covers it.
- I23: `tests/rollout.rs::copy_reads_keys_as_ssh`, with a directory in the
  place of the file. It asserts the exit status, `cannot read <file>: ` and
  that ssh was not called, not the cause and not the lines beneath the
  outcome. It also covers that a file that is not UTF-8 reaches `nix copy`
  whole. `tests/rollout.rs::copy_needs_the_keys_of_the_run` covers the check
  in the copy: a file of keys that the run wrote itself and cannot read
  fails the machine with a row that starts with `cannot read `
  and names the file `0.seen`, `nxf init` does not run on it, and the exit
  status is 1. No run by hand covers either.
- I24: `tests/rollout.rs::scratch_names_run_out`, with the 100 directories
  made by a `sh` as the same user, not as another user. No run by hand covers
  it.
- I25: `tests/rollout.rs::missing_temporary_directory`. It asserts the outcome
  in two parts, `cannot create <TMPDIR>/nxf-rollout.` and
  `: No such file or directory (os error 2)` with the line beneath it, not the
  `<pid>.0` between them. No run by hand covers it.
- I26: `tests/rollout.rs::json`, with a played installation. It asserts the
  whole object. `tests/rollout.rs::reads_the_installation` covers the remote
  command and that the key written to `--known-hosts` is the one the object
  holds.
- I27: `tests/rollout.rs::json_does_not_ask`. It asserts the states, the
  errors, the outcome on standard error and that only the probe ran.
- I28: `tests/rollout.rs::json_refusal`. It does not assert `details`.
- I29: `tests/rollout.rs::refused_installation`.
- `tests/rollout.rs::probe` covers how the probe's answer is read, and
  `tests/rollout.rs::copy_trusts_as_the_link` that `nix copy` gets one file
  with the keys of the link and the mode 600, in options that hold no quote.
  `tests/rollout.rs::host_name_with_a_space` covers that no option names a
  file by the name of a machine.
- `tests/rollout.rs::long_names_widen_the_outcome` covers that the outcome
  lists the revision of every installed machine apart from its name, however
  long the name is, with `node-1` beside `node-number-two`: the names share
  one column, and the `known_hosts` line follows the revision.
  `tests/rollout.rs::unwritten_key_names_its_file` covers that a machine whose
  file with the key the installation made cannot be written has a `✗` row
  that starts with `cannot write <TMPDIR>/nxf-rollout.` and holds `/0.known: `,
  that it is named under `Not installed`, and that the exit status is 1. It
  does not assert the cause.

What a machine holds after an installation needs real machines with kexec,
disko, `nixos-install` and a restart.

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
0226ab0 of the branch of pull request #9 (squashed into commit e3d896f, whose
`src/` differs from it), against one Incus Ubuntu 24.04 VM with SSH as root, and with
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

I1 and I5 to I7 were run by hand again with nxf 0.13.1, built from commit
306eb61 of the branch of pull request #9 (its `src/` is that of commit
e3d896f on `main`), against one Incus Ubuntu 24.04 VM with SSH as root, with `--files`,
`--known-hosts` and `--yes`:

- The plan row was `Ubuntu 24.04.5 LTS; it will be erased`. The rows of the
  phases were `Started from memory`, `Founded on the machine`, `Installed`
  and `Started, and its host key is the one it was given`. The outcome was
  `1 machine installed` (I5).
- The installed machine had its own files, the installation record and nxf
  0.13.1 (I6).
- The Ubuntu key in the `--known-hosts` file was replaced by the reported
  one, and the line of another machine was kept (I7).
- During the run the directory of the run was `nxf-rollout.<pid>.0` with the
  mode 700, and the file that `nix copy` got had the mode 600 and held the
  accepted key and the line of the user's `known_hosts`. Afterwards no
  directory of the run was left.
- A second run only probed the machine: its row was `installed by nxf` and
  the outcome was `1 machine installed by nxf` (I1).

The run is not repeatable in CI.

I1 and I5 to I7 were run by hand again with nxf 0.14.1, built from commit
336a4bb of the branch of pull request #12 (its `src/` is that of the head of
that branch), against one Incus Ubuntu 24.04 VM with SSH as root, with
`--files`, `--known-hosts` and `--yes`, a scratch `HOME` and `TMPDIR`, and a
template with a disko layout and the nxf module, pinned to that commit:

- The plan row was `Ubuntu 24.04.5 LTS; it will be erased`. The rows of the
  phases were `Started from memory`, `Founded on the machine`, `Installed`
  and `Started, and its host key is the one it was given`. The outcome was
  `1 machine installed`, with the revision `b28716fd9774` of the machine
  beside its name (I5).
- The installed machine had the file of its own in `/etc/nixos`, the
  installation record and nxf 0.14.1, and the commit of the flake that its
  `nxf status` named was the revision the run reported (I6).
- The Ubuntu key in the `--known-hosts` file was replaced by the key the
  machine answers with, and the line of another machine was kept. The
  `known_hosts` file of the scratch `HOME` was not changed, and the scratch
  `TMPDIR` was empty afterwards (I7).
- A second run only probed the machine: its row was `installed by nxf` and
  the outcome was `1 machine installed by nxf` (I1).
- `status`, `version`, `version --system`, `doctor`, `hold --for 1h` and
  `release` with `--config` each gave one row for the machine and an outcome
  that named the one machine.

The run is not repeatable in CI.

See the [operating documentation](../../README.md#from-another-computer).
