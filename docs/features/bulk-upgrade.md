# Bulk Upgrade

Upgrading the machines of a hosts file from another computer, one wave at a
time.

## Brief

Whoever looks after several machines that nxf manages wants to upgrade them
now, without waiting for each timer or logging in to each one, and wants a
bad update to stop at the first machine that shows it.

`nxf rollout --config <file> --upgrade` runs the task `nxf upgrade` on each
machine over ssh, as `nxf --json upgrade`, in the order of the machines'
names. The order compares bytes, so `node-10` comes before `node-2`. The
first machine in that order upgrades alone, as a canary. When it succeeds,
the others follow in waves of `--batch` machines (default 1). A wave runs to
its end. A failure, or a machine that does not answer, starts no later wave,
and those machines are listed as not started. Named inputs are passed on to
every machine. Each machine has `NXF_UPGRADE_TIMEOUT` seconds, 3 hours by
default. A machine that takes longer is a failed row,
`timed out after <N> seconds; the upgrade may still run on the machine`:
ending ssh does not end the task there.

Boundaries:

- It is the manual task on each machine. Holds, input strategies, the check
  and failure hooks and the health checks do not apply, and the results are
  not saved as the automatic update.
- Each machine updates its own flake and commits its own lockfile. The
  machines get the same input revisions only when upstream did not move in
  between; nxf does not pin revisions across machines.
- It installs nothing, erases nothing and does not ask; installing is
  [Bulk Install](bulk-install.md). The install options (`--template`,
  `--files`, `--yes`, `--kexec-url`, `--known-hosts`) are refused together
  with `--upgrade`, and inputs and `--batch` are refused without it.
- It does not restart machines and does not wait for them.
- It keeps no record of an earlier run. A second run asks every machine
  again, from the first.
- The SSH user must be able to work with the flake and to switch: root, or a
  member of the group `nxf`. nxf does not check this before the first wave.
- nxf holds no lock between its processes. Nothing keeps the upgrade and an
  automatic update of the timer apart when both run on one machine.

## Acceptance scenarios

| ID | Given | When | Then |
| --- | --- | --- | --- |
| U1: All machines upgrade | Three managed machines and a new revision of an input | `rollout --upgrade` runs | Each machine runs `nxf --json upgrade`, the first alone and then one at a time; each row is `inputs updated, switched` or `no input changed, switched`; the outcome is `3 machines upgraded`; the exit status is 0 |
| U2: The canary fails | The first machine's switch fails | `rollout --upgrade` runs | Only the first machine was asked; its row is `switch failed:` with the cause; the other rows are `not started`; the outcome is `Upgrade failed on 1 of 3 machines` with `Not started: <names>. Run the rollout again once the failure is fixed.`; the exit status is 1 |
| U3: A machine does not answer | The second machine is unreachable | `rollout --upgrade` runs | The first upgrades; the row of the second is `unreachable:` with the last line ssh printed; the row of the third is `not started`; the outcome is `Upgrade failed on 1 of 3 machines`; the exit status is 1 |
| U4: A wave runs to its end | `--batch 2` and the second machine fails | `rollout --upgrade --batch 2` runs | The second and third machines run in one wave, so the third has a `✓` row; the outcome is `Upgrade failed on 1 of 3 machines` without a `Not started` line; the exit status is 1 |
| U5: Inputs are passed on | Inputs named after `--upgrade` | `rollout --upgrade b` runs | Every machine runs `nxf --json upgrade b` |
| U6: Mixed options are refused | `--upgrade` with an install option, `--batch 0`, or `--batch` or inputs without `--upgrade` | The command is given | No ssh connection is made. With an install option or `--batch 0` the error names the argument and the exit status is 2. With `--template` and `--batch` or inputs the outcome is `Inputs and --batch belong to the upgrade` and the exit status is 1; without `--template` the error asks for it and the exit status is 2 |
| U7: An unknown input | An input that the first machine's flake does not have | `rollout --upgrade <input>` runs | The first machine's row is `Unknown task or input: <input>`, without the help beneath it; the other rows are `not started`; the exit status is 1 |
| U8: A second run after a failed wave | An earlier run upgraded the first machine and failed on the second | `rollout --upgrade` runs again | Every machine is asked again, in the same order and from the first; a machine that finds no new revision and switches has the row `no input changed, switched` |
| U9: A machine runs past its limit | The first machine's upgrade does not end within `NXF_UPGRADE_TIMEOUT` seconds | `rollout --upgrade` runs | Only the first machine was asked; its row is `timed out after <N> seconds; the upgrade may still run on the machine`, and no row says `unreachable`; the other rows are `not started`; the outcome is `Upgrade failed on 1 of 3 machines`; the exit status is 1 |

## Existing verification

[Fleet fixtures](../../tests/fleet.rs) run through a mock ssh, with the real
nxf on both sides and the mock nh. In the fixtures the machines share one
checkout, so only the first one finds the new revision.

- U1: `tests/fleet.rs::upgrade`. It asserts the order in which the machines
  are asked, not that no two run at once, and the rows of the first two
  machines, not of the third.
- U2: `tests/fleet.rs::upgrade_stops`.
- U3: `tests/fleet.rs::upgrade_unreachable`. It does not assert the outcome.
- U4: `tests/fleet.rs::upgrade_batch`.
- U5: `tests/fleet.rs::upgrade_batch`.
- U6: `tests/fleet.rs::upgrade_refused` asserts the text of each refusal,
  that the exit status is not 0 and that ssh was not called, not which exit
  status. `tests/cli.rs::refusals_have_the_shape_of_failures` asserts the
  exit status 2 of what clap refuses, for `nxf rollout` without `--config`.
- U7: `tests/fleet.rs::upgrade_unknown_input`. It asserts the row
  `not started` of the second machine, not of the third.
- U8: no automated test covers it. `tests/fleet.rs::upgrade` covers the row
  `no input changed, switched`.
- U9: `tests/fleet.rs::upgrade_timeout`, with a limit of 1 second and the
  row `timed out after 1 second`. It asserts the row `not started` of the
  second machine, not of the third, and not the outcome.
- `tests/fleet.rs::waves` covers the order of the waves, and
  `tests/hosts.rs::toml` that machines come in the order of their names. No
  test has two names whose byte order differs from their numeric order.

U1 to U5 and U7 were also run by hand against three Incus NixOS VMs, each
with its own machine flake, a local Git input changed per machine and SSH as
root; a broken input made the build fail through an assertion of the
configuration. That run is recorded in commit eb6f355, where nxf is 0.13.0.
The record does not name the commit the nxf that ran was built from, and it
keeps none of the output. The run is not repeatable in CI.

The exit statuses and texts of U6 were seen by hand with nxf 0.13.0, built
from commit 1905cce, through a mock ssh that was not called.

See the [operating documentation](../../README.md#upgrading-machines).
