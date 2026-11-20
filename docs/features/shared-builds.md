# Shared Builds

Machine-wide tracking of NixOS build results across users.

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

Without a usable shared directory, publication tries per-user state. If both
locations are the same inaccessible directory, no record can be saved.
Metadata failures warn without failing a successful build, update or switch.

Boundaries:

- nxf does not create the shared directory. The module provisions it, and
  without it records stay in per-user state.
- Source roots, result links, read markers and Nix's fetcher cache keep their
  per-user locations.
- A record is a plain symlink and no GC root. It is ignored once its system
  is garbage-collected.
- A build with uncommitted changes is not recorded.
- Only a publication imports old records, and only those of the caller's own
  state. Status imports nothing.
- A recorded output does not activate a system that is not running: `switch`
  builds again.
- Root, the user `nxf` and the members of the group `nxf` can replace each
  other's records. They are trusted alike.
- It does not solve access to private evaluation sources.

## Acceptance scenarios

These Given/When/Then scenarios describe observable behavior and serve as
templates for acceptance tests. Use throwaway Git flakes, mock nh and isolated
state directories; test user permissions and garbage collection only in a VM.

| ID | Given | When | Then |
| --- | --- | --- | --- |
| S1: Manual build visible to the updater | A clean checkout, a provisioned shared directory and no input changes | A group member builds and that output is running; the updater then runs as `nxf` | Both users' JSON status agrees on `built`, `matches = true` and `revision`; the updater calls no nh build or switch |
| S2: Replace another user's record | A shared record owned by another authorized user | Alice, Bob, `nxf` and root publish the same tree in turn, including root with umask `022` | Each publication succeeds; another group member can replace root's record; readers see a complete old or new symlink |
| S3: Standalone use | No shared `systems` directory has been provisioned | A user builds successfully | The record is saved under that user's state; the shared directory is not created implicitly |
| S4: Shared publication denied | Separate per-user state is writable, but the shared directory is not writable/searchable by this process, for example a non-member or a session without its new group | The user builds or switches | The task succeeds, warns about shared access and active group membership, and saves a per-user record; doctor warns and reports the fallback destination |
| S5: All record writes fail | A build succeeds but neither shared nor per-user records can be written | A build, switch or input update completes | Warnings identify the metadata failure; activation still proceeds when requested; an updated lockfile stays committed and the update is not reported as failed or restored |
| S6: Import existing records | The caller has legacy records, including a conflicting key, dangling links and invalid targets | The caller next publishes a shared build record | Valid records from that caller's state are copied without replacing existing shared entries; invalid records are skipped; the originals remain; no other home is scanned |
| S7: Checkout changes during a build | A clean checkout before building | A tracked file changes, or a different tree is committed, before the build returns | The successful output is not recorded under either tree; nxf warns that status cannot identify the build |
| S8: Update builds before committing | Updating an input changes `flake.lock` | nxf builds the dirty lockfile and then commits it | The resulting clean tree is recorded only if its content still corresponds to what was built; edits during the build prevent publication |
| S9: State overrides and read-only status | Explicit `NXF_STATE` and `NXF_UPDATER_STATE` locations, plus legacy records | A user builds, then runs `status --json` | Shared records use the provisioned updater location; result links, source roots and fallback records use `NXF_STATE`; JSON status neither imports records nor writes a read marker |
| S10: A different system is running | A record exists for the clean checkout, but its output is not running | The user requests `switch` | nxf still calls nh to build and activates that exact result; the record is not used as an activation shortcut |

## Existing verification

Every test below runs in `just check`. The tests in `tests/` use a throwaway
flake, the mock nh and isolated state directories. The
[NixOS VM test](../../nix/shared-records-check.nix) is the flake check
`shared-records`.

| ID | Covered by | Not covered |
| --- | --- | --- |
| S1 | `tests/auto_update.rs::manual_build_skips_service_build`, with a conflicting local record that must not override the shared one | Two Unix users: the test runs as one user, and a second `NXF_STATE` stands in for the user `nxf` |
| S2 | `nix/shared-records-check.nix`: alice, bob, `nxf` and root with umask `022` publish the same tree in turn, and alice replaces root's record | A reader during a replacement. That it sees a complete symlink follows from replacement by rename |
| S3 | `tests/tasks.rs::standalone_records` | |
| S4 | `tests/tasks.rs::shared_fallback` for `switch`; `tests/doctor.rs::record_destination` for the doctor, also when no separate fallback exists; `nix/shared-records-check.nix` for a user outside the group | A session without its new group |
| S5 | `tests/tasks.rs::record_failures_are_nonfatal`, for `build`, `switch` and `update` | |
| S6 | `tests/tasks.rs::imports_legacy_records` | That no other home is scanned: the test has one state directory |
| S7 | `tests/tasks.rs::changed_during_build`, for a tracked edit and for a new commit: no record is written | The warning is not asserted |
| S8 | `tests/tasks.rs::update_build_identity`; `tests/tasks.rs::dirty_update_identity` for a checkout that had uncommitted changes before the update | |
| S9 | `tests/tasks.rs::publishes_shared` for the shared records, the result link and the source roots; `tests/auto_update.rs::shared_state` for fallback records in `NXF_STATE`; `tests/tasks.rs::imports_legacy_records` for status not importing; `tests/status.rs::json` for the read marker | |
| S10 | `tests/tasks.rs::switch_builds_recorded_system` | |

Beyond the scenarios, [doctor fixtures](../../tests/doctor.rs) cover linked
Git worktrees (`linked_worktree`), and the VM test covers the repair of the
directory's mode by tmpfiles and per-user source roots that survive garbage
collection.

See the [operating documentation](../../README.md#status) for state paths and
migration details.
