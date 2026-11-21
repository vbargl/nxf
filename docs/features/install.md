# Install

Installing the machine a flake describes, on that machine, from a NixOS
installer.

## Brief

Whoever sets up one machine has started it from a NixOS installer and wants
it installed from a machine flake in one step. The flake is to stay on the
machine as its `/etc/nixos`, and the SSH host key is to be known before the
first connection.

`nxf install <dir>` takes a machine flake as `nxf init` founds it. It does
this, in this order:

1. It refuses unless `<dir>` has a `flake.nix`, is a Git repository without
   uncommitted changes and has a `flake.lock`, nxf runs as root, and the
   system runs from memory. A system runs from memory when the last `/`
   line of `/proc/mounts` names a `tmpfs`, a `rootfs` or a `ramfs`. It also
   refuses on a system that nxf manages, whether or not it runs from memory:
   one that has `/var/lib/nxf/installed.json`, or an executable
   `/run/current-system/sw/bin/nxf`. `--mounted` does not change this.
2. It builds the script of the configuration's
   [disko](https://github.com/nix-community/disko) layout and runs it: the
   disks are erased, formatted and mounted at `/mnt`. With `--mounted` it
   skips this and uses what is mounted at `/mnt`.
3. It refuses when `/mnt` is not a directory. With `--mounted` it refuses a
   target whose `/etc/nixos` is not empty. It copies the flake to
   `/mnt/etc/nixos`, as the same Git repository, and runs `nixos-install`
   with `--root /mnt`, `--flake /mnt/etc/nixos#<configuration>`,
   `--no-root-passwd` and `--no-channel-copy`.
4. It records the installed system under the flake's tree in
   `/mnt/var/lib/nxf/systems`.
5. It makes the ed25519 SSH host key, unless the target has one, and writes
   `/mnt/var/lib/nxf/installed.json` with the time, the tree and the
   configuration.
6. It prints the outcome `NixOS installed` with the host key. With `--reboot`
   it then runs `systemctl reboot`.

With `--json` it prints nothing while it runs. At its end it prints one JSON
object on standard output: `hostKey`, `system` (the store path of the
installed system, or null) and `revision` (the commit of the installed flake,
or null). A refusal or a failure prints the object `{"error": "<cause>",
"details": ["…"]}` instead, and the text of the failure stays on standard
error. `nxf rollout` reads the host key from this object.

Boundaries:

- It runs on the machine itself. From another computer it is `nxf rollout`;
  see [Bulk Install](bulk-install.md).
- It erases the disks that the layout names and asks no question. The checks
  that nxf does not manage the system and that the system runs from memory
  are what keep it from a running system.
- `NXF_INSTALLER` replaces the answer whether the system runs from memory:
  with `yes` install also runs on a system that runs from a disk, unless nxf
  manages it.
- Without `--mounted` the check that `/mnt/etc/nixos` is empty comes after
  the disks were partitioned.
- `--mounted` does not check that something is mounted at `/mnt`: it checks
  only that `/mnt` is a directory. With nothing mounted there, the flake is
  copied and `nixos-install` installs into the directory, which is in the
  installer's memory.
- A failed installation is not rolled back. Disks that were erased stay
  erased, and `installed.json` is not written.
- It sets no root password.
- Inputs are fetched on the machine, so a private input needs its
  credentials in the installer.
- The private host key stays on the machine. Only the public key is printed.
- Without `--reboot` the machine is not restarted.

## Acceptance scenarios

| ID | Given | When | Then |
| --- | --- | --- | --- |
| N1: Install | A committed machine flake with a disk layout, root, and a system that runs from memory | `nxf install <dir>` runs | The phases start with `[1/3] Partitioning the disks` and `[2/3] Installing the system`; `/mnt/etc/nixos` is the same commit without uncommitted changes; `/mnt/var/lib/nxf/installed.json` holds the tree and the configuration; `/mnt/var/lib/nxf/systems/<tree>` links to the installed system; the outcome is `NixOS installed` with `Host key` and `Restart the machine to start it.`; the exit status is 0 |
| N2: A mounted target | The target is mounted at `/mnt` and holds an SSH host key | `nxf install <dir> --mounted` runs | Nothing is partitioned; the first phase is `[1/2] Installing the system`; `Host key` shows the key that was there |
| N3: Outside an installer | The system runs from a disk | `nxf install <dir>` runs | Nothing is partitioned; the outcome is `This system runs from a disk, and installing erases disks`; the exit status is 1 |
| N4: No flake | `<dir>` has no `flake.nix` | `nxf install <dir>` runs | Nothing is partitioned; the outcome is `<dir> has no flake.nix`; the exit status is 1 |
| N5: Uncommitted changes | `<dir>` has uncommitted changes | `nxf install <dir>` runs | Nothing is partitioned; the outcome is `<dir> has uncommitted changes or is not a Git repository`; the exit status is 1 |
| N6: The target holds something | A target whose `/etc/nixos` is not empty | `nxf install <dir> --mounted` runs | `nixos-install` is not called; the outcome is `/mnt/etc/nixos already holds something; the target is not empty`; the exit status is 1 |
| N7: nixos-install fails | `nixos-install` exits with an error | `nxf install <dir>` runs | The outcome is the last line `nixos-install` said, with `nixos-install exited with status <N>.` beneath it; `installed.json` is not written; `NixOS installed` is not printed; the exit status is 1 |
| N8: No lockfile | `<dir>` is committed and has no `flake.lock` | `nxf install <dir>` runs | Nothing is partitioned; the outcome is `<dir> has no flake.lock`; the exit status is 1 |
| N9: Not root | A committed machine flake, and a user other than root | `nxf install <dir>` runs | Nothing is partitioned; the outcome is `Installing needs root`; the exit status is 1 |
| N10: No disk layout | The configuration has no disko layout | `nxf install <dir>` runs | The outcome is `The configuration has no disk layout to apply`, with what Nix said and the advice to import the disko module or to add `--mounted`; the exit status is 1 |
| N11: Nothing is mounted | `/mnt` is not a directory | `nxf install <dir> --mounted` runs | The outcome is `/mnt is not mounted`; the exit status is 1 |
| N12: Restart | The conditions of N1 | `nxf install <dir> --reboot` runs | After the outcome of N1 the last line is `Restarting into the new system.` instead of `Restart the machine to start it.`, and `systemctl reboot` runs |
| N13: A system that nxf manages | The system runs from memory and has `/var/lib/nxf/installed.json`, or an executable `/run/current-system/sw/bin/nxf` | `nxf install <dir>` runs | Nothing is partitioned and `nixos-install` is not called; the outcome is `nxf manages this system, and installing erases disks` with `Start the machine from a NixOS installer to install it again.`; the exit status is 1 |
| N14: A directory with nothing mounted | `/mnt` is a directory and nothing is mounted on it | `nxf install <dir> --mounted` runs | Nothing refuses: `/mnt/etc/nixos` holds the flake, `nixos-install` runs with `--root /mnt`, and the outcome is `NixOS installed` |
| N15: JSON | The conditions of N1 | `nxf install <dir> --json` runs | Standard output is one JSON object with `hostKey`, which is the key of `/mnt/etc/ssh/ssh_host_ed25519_key.pub`, `system`, which is the store path of the installed system, and `revision`, which is the commit of `<dir>`; standard error is empty; the exit status is 0 |
| N16: A refusal under JSON | The conditions of N3 or N7 | `nxf install <dir> --json` runs | Standard output is one JSON object: `error` is the outcome and `details` the lines beneath it, with the exit line of `nixos-install` last for N7; standard error has the text of the failure; the exit status is 1 |

## Existing verification

[Install fixtures](../../tests/install.rs) run with stand-ins: a directory
for `/mnt` (`NXF_INSTALL_ROOT`), a script for the disk layout (`NXF_DISKO`),
a mock `nixos-install`, and `NXF_INSTALLER` for the answer whether the system
runs from memory. `NXF_UPDATER_STATE` stands in for `/var/lib/nxf` and
`NXF_SYSTEM` for `/run/current-system`. No test partitions a disk, runs the
real `nixos-install` or reads the `/proc/mounts` of the machine.

| ID | Covered by | Not covered |
| --- | --- | --- |
| N1 | `tests/install.rs::partitions_and_records` | The disko script of a real configuration, the real `nixos-install`, and the reading of `/proc/mounts` |
| N2 | `tests/install.rs::mounted` | A mounted file system: the stand-in for `/mnt` is a directory |
| N3 | `tests/install.rs::refuses` | Which exit status: the test asserts that it is not 0. The reading of `/proc/mounts`: `NXF_INSTALLER` gives the answer |
| N4 | `tests/install.rs::refuses` | `<dir>` in the outcome: the test asserts only `has no flake.nix`. Which exit status: the test asserts that it is not 0 |
| N5 | `tests/install.rs::refuses` | The rest of the outcome: the test asserts only `has uncommitted changes`. Which exit status: the test asserts that it is not 0 |
| N6 | `tests/install.rs::refuses` | The rest of the outcome: the test asserts only `already holds something`. Which exit status: the test asserts that it is not 0 |
| N7 | `tests/install.rs::failure_records_nothing` | Which exit status: the test asserts that it is not 0 |
| N8 | `tests/install.rs::refuses_unmounted_and_unlocked` | Which exit status: the test asserts that it is not 0 |
| N9 | | No automated test covers it; with `NXF_INSTALL_ROOT` set, install does not require root |
| N10 | | No automated test covers it; `NXF_DISKO` replaces the build of the layout |
| N11 | `tests/install.rs::refuses_unmounted_and_unlocked`, with `NXF_INSTALL_ROOT` in the place of `/mnt` | Which exit status: the test asserts that it is not 0 |
| N12 | `tests/install.rs::reboot`, with a `systemctl` that notes its call; `tests/install.rs::reboots_through_the_stand_in` for the restart through `NXF_SYSTEMCTL` and not through the `systemctl` of `PATH` | A real restart |
| N13 | `tests/install.rs::refuses_a_managed_system` | Which exit status: the test asserts that it is not 0. The line beneath the outcome. The same with `--mounted` |
| N15 | `tests/install.rs::json` | The `system` of an installation whose profile is not found, which is null |
| N16 | `tests/install.rs::json_refusal`, for N3 and N7 | The other refusals |
| N14 | `tests/install.rs::mounted`: its stand-in for `/mnt` is a directory with nothing mounted on it | What `/mnt/etc/nixos` holds, the arguments of `nixos-install` and the outcome: the test asserts the exit status 0, one call of `nixos-install` and the `Host key` line. That the system lands in memory |

`tests/install.rs::root_is_the_last_mount` covers which line of
`/proc/mounts` decides whether the system runs from memory, on text it is
given.

The installation of real machines through `nxf rollout`, which runs
`nxf install` on each, is recorded in [Bulk Install](bulk-install.md).

See the [operating documentation](../../README.md#on-the-machine).
