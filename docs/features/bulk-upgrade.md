# Bulk Upgrade

Upgrading the machines of a hosts file from another computer, one wave at a
time.

## Brief

`nxf rollout --config <file>` installs the machines that nxf does not manage
yet ([Bulk Install](bulk-install.md)). Once they are managed, whoever looks after several machines wants to
upgrade them now, without waiting for each timer or logging in to each one,
and wants a bad update to stop at the first machine that shows it.

`nxf rollout --config <file> --upgrade` runs the task `nxf upgrade` on each
machine over ssh, as `nxf --json upgrade`, in the name order of the hosts
file. The first machine upgrades alone, as a canary. When it succeeds, the
others follow in waves of `--batch` machines (default 1). A wave runs to its
end. A failure, or a machine that does not answer, starts no later wave, and
those machines are listed as not started. Named inputs are passed on to every
machine. Each machine has `NXF_UPGRADE_TIMEOUT` seconds, 3 hours by default.

Boundaries:

- It is the manual task on each machine. Holds, input strategies, the check
  and failure hooks and the health checks do not apply, and the results are
  not saved as the automatic update.
- Each machine updates its own flake and commits its own lockfile. The
  machines get the same input revisions only when upstream did not move in
  between; nxf does not pin revisions across machines.
- It installs nothing, erases nothing and does not ask. The install options
  (`--template`, `--files`, `--yes`, `--kexec-url`, `--known-hosts`) are
  refused together with `--upgrade`, and inputs and `--batch` are refused
  without it.
- It does not reboot machines and does not wait for them.

## Acceptance scenarios

| ID | Given | When | Then |
| --- | --- | --- | --- |
| U1: All machines upgrade | Three managed machines and a new revision of an input | `rollout --upgrade` runs | Each machine runs `nxf --json upgrade`, the first alone and then one at a time; each row says whether inputs changed; the outcome is `3 machines upgraded` |
| U2: The canary fails | The first machine's switch fails | `rollout --upgrade` runs | Only the first machine was asked; the others are `not started`; the command fails and names them |
| U3: A machine does not answer | The second machine is unreachable | `rollout --upgrade` runs | The first upgrades, the second is a failed `unreachable` row, the third is not started |
| U4: A wave runs to its end | `--batch 2` and the second machine fails | `rollout --upgrade --batch 2` runs | The second and third machines run in one wave; the command fails on 1 of 3 and lists nobody as not started |
| U5: Inputs are passed on | Inputs named after `--upgrade` | `rollout --upgrade b` runs | Every machine runs `nxf --json upgrade b` |
| U6: Mixed options are refused | `--upgrade` with an install option, `--batch 0`, or `--batch` or inputs without `--upgrade` | The command is given | It fails before any ssh connection |
| U7: An unknown input | An input that the first machine's flake does not have | `rollout --upgrade <input>` runs | The first machine refuses it, its row shows the cause and not the help beneath it, and no other machine is asked |

## Existing verification

[Fleet fixtures](../../tests/fleet.rs) cover U1 (`upgrade`), U2
(`upgrade_stops`), U3 (`upgrade_unreachable`), U4 and U5 (`upgrade_batch`),
U6 (`upgrade_refused`), U7 (`upgrade_unknown_input`) and the order of the
waves (`waves`). They run through a mock ssh, with the real nxf on both sides
and the mock nh. In the fixtures the machines share one checkout, so only the
first one finds the new revision.

U1 to U5 and U7 were also run by hand against three Incus NixOS VMs, each
with its own machine flake, a local Git input changed per machine and SSH as
root; a broken input made the build fail through an assertion of the
configuration. That run is not repeatable in CI.

See the [operating documentation](../../README.md#upgrading-machines).
