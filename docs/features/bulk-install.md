# Bulk Install

Installing the machines of a hosts file from another computer.

## Brief

Whoever brings up several machines wants each one installed the same way,
from one template, without carrying a USB installer from machine to machine,
and wants to run the same command again after fixing a machine that failed.

`nxf rollout --config <file> --template <flake>#<name>` asks every machine
over ssh what it is, all at once, and shows a plan:

- A machine installed by nxf, or one that runs nxf, is left alone.
- A NixOS installer is installed.
- Another system, such as Ubuntu, first starts a NixOS installer from memory
  (kexec, from `--kexec-url` or the nixos-images release), and is then
  installed.
- A machine that does not answer is a failed row.

Installing erases disks, so rollout asks before it installs anything. `--yes`
confirms without a question, and without a terminal nothing is erased unless
`--yes` is given. The machines are then installed one at a time. For each one
rollout copies this nxf and the template, stored in the Nix store with the
uncommitted changes of its checkout, to the machine. It runs
`nxf init --hardware` there, with `<files>/<name>/` laid over the template
when `--files <files>` is given, and then runs `nxf install`. It restarts the
machine and waits up to `NXF_ROLLOUT_WAIT` seconds, 600 by default, until the
new system answers with the host key the installation made. A failed machine
does not stop the others.

The outcome names the revision and the `known_hosts` line of every installed
machine. `--known-hosts <file>` writes the lines into that file and replaces
the key of the system that was there before.

Boundaries:

- The flake never exists on the computer that runs rollout; each machine
  founds and commits its own.
- It does not upgrade machines that are already managed; that is
  [Bulk Upgrade](bulk-upgrade.md).
- The SSH user must be root, and the machine needs a way to the internet for
  the installer image and the inputs.
- The first connection to a machine nobody knows accepts its host key; a key
  that differs from one in `~/.ssh/known_hosts` is refused. After the
  installation only the key it made is accepted.
- It does not roll back a machine whose installation failed halfway. Its
  disks may already be erased, and a later run installs it again when it
  still answers as an installer.

## Acceptance scenarios

| ID | Given | When | Then |
| --- | --- | --- | --- |
| I1: Repeatable | Every machine is installed by nxf | `rollout` runs twice | Each machine is only probed; every row is `installed by nxf`; the outcome is `N machines installed by nxf` |
| I2: Asks before erasing | A managed machine, an Ubuntu machine and a NixOS installer, no terminal and no `--yes` | `rollout` runs | The plan says which machines would be erased; nothing is copied or run beyond the probe; the command fails and names `--yes` |
| I3: A machine does not answer | One machine is unreachable | `rollout --yes` runs | Its row is `unreachable` with the ssh error; it is never taken for a machine to install; the command fails |
| I4: Install from an installer | A machine booted into a NixOS installer | `rollout --yes` runs | The machine flake is founded on the machine with its hardware configuration, installed and restarted; the new system answers with the reported host key; the outcome names its revision and `known_hosts` line |
| I5: Install over another system | A machine runs Ubuntu | `rollout --yes` runs | The installer starts from memory for the machine's architecture, then I4 follows |
| I6: Machine files | `--files <dir>` with `<dir>/<name>/` for some machines | `rollout --yes` runs | Those machines get their files over the template; the others get the template alone |
| I7: Known hosts are written | `--known-hosts <file>` holds the key of the system that was there before | A machine is installed | The old key of that address is replaced by the new one; a non-default port is written as `[address]:port` |
| I8: One machine fails | Two machines to install and the first one's installation fails | `rollout --yes` runs | The second is still installed; the command fails and names the first under `Not installed` |
| I9: Template with uncommitted changes | The template's checkout has an uncommitted change | `rollout` copies the template | The machines get the changed content |

## Existing verification

[Rollout fixtures](../../tests/rollout.rs) cover I1 (`repeatable`), I2
(`asks_before_erasing`), I3 (`unreachable`), the probe's reading of what a
machine is (`probe`), the `known_hosts` line of I7 (`known_hosts`) and I9
(`template_source_is_stored`). They run through a mock ssh that only answers
the probe.

I4 to I8 need real machines: kexec, disko, `nixos-install` and a restart.
There is no automated test for them, and no run on a virtual machine is
recorded here.

See the [operating documentation](../../README.md#from-another-computer).
