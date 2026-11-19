# Shared build records

## Brief

A manual build and the automatic updater should agree about which system a
checkout produced, even when they run as different users. nxf records the
checkout's Git tree ID (its tracked content, including `flake.lock`) as a
symlink to the built system in the shared Nix store.

The NixOS module provisions `/var/lib/nxf/systems` for root, the dedicated
`nxf` user and members of group `nxf`. They publish records there atomically.
Status prefers shared records, so the updater can skip a redundant build and
switch when the committed checkout is already running. Configuration
selection and update strategies still require evaluation.

Without a usable shared directory, publication falls back to per-user state.
Metadata failures warn without failing a successful build, update or switch.
Source roots, result links, read markers and Nix's fetcher cache keep their
existing locations. This feature does not reuse a recorded output to activate
a system that is not running, or solve access to private evaluation sources.

## Acceptance scenarios

These Given/When/Then scenarios describe observable behavior and serve as
templates for acceptance tests. Use throwaway Git flakes, mock nh and isolated
state directories; test user permissions and garbage collection only in a VM.

| ID | Given | When | Then |
| --- | --- | --- | --- |
| S1: Manual build visible to the updater | A clean checkout, a provisioned shared directory and no input changes | A group member builds and that output is running; the updater then runs as `nxf` | Both users' JSON status agrees on `built`, `matches = true` and `revision`; the updater calls no nh build or switch |
| S2: Replace another user's record | A shared record owned by another authorized user | Alice, Bob, `nxf` and root publish the same tree in turn, including root with umask `022` | Each publication succeeds; another group member can replace root's record; readers see a complete old or new symlink |
| S3: Standalone use | No shared `systems` directory has been provisioned | A user builds successfully | The record is saved under that user's state; the shared directory is not created implicitly |
| S4: Shared publication denied | The shared directory exists but is not writable/searchable by this process, for example a non-member or a session without its new group | The user builds or switches | The task succeeds, warns about shared access and active group membership, and saves a per-user record; doctor warns and reports the fallback destination |
| S5: All record writes fail | A build succeeds but neither shared nor per-user records can be written | A build, switch or input update completes | Warnings identify the metadata failure; activation still proceeds when requested; an updated lockfile stays committed and the update is not reported as failed or restored |
| S6: Import existing records | The caller has legacy records, including a conflicting key, dangling links and invalid targets | The caller next publishes a shared build record | Valid records from that caller's state are copied without replacing existing shared entries; invalid records are skipped; the originals remain; no other home is scanned |
| S7: Checkout changes during a build | A clean checkout before building | A tracked file changes, or a different tree is committed, before the build returns | The successful output is not recorded under either tree; nxf warns that status cannot identify the build |
| S8: Update builds before committing | Updating an input changes `flake.lock` | nxf builds the dirty lockfile and then commits it | The resulting clean tree is recorded only if its content still corresponds to what was built; edits during the build prevent publication |
| S9: State overrides and read-only status | Explicit `NXF_STATE` and `NXF_UPDATER_STATE` locations, plus legacy records | A user builds, then runs `status --json` | Shared records use the provisioned updater location; result links, source roots and fallback records use `NXF_STATE`; JSON status neither imports records nor writes a read marker |
| S10: A different system is running | A record exists for the clean checkout, but its output is not running | The user requests `switch` | nxf still calls nh to build and activates that exact result; the record is not used as an activation shortcut |

## Existing verification

- [Task fixtures](../../tests/tasks.rs) cover publication, fallback, migration,
  build identity and activation behavior.
- [Updater fixtures](../../tests/auto_update.rs) cover S1, including a
  conflicting local record that must not override the shared record.
- [Doctor fixtures](../../tests/doctor.rs) cover destination and access warnings;
  [status fixtures](../../tests/status.rs) cover JSON status and read markers.
- The [NixOS VM test](../../nix/records-test.nix) exercises replacement across
  users, non-member fallback, tmpfiles permission repair and per-user source
  roots surviving garbage collection. Atomic reader visibility follows from
  symlink replacement by rename; it is not a separate concurrent-reader test.

See the [operating documentation](../../README.md#status) for state paths and
migration details.
