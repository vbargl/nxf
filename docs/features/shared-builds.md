# Shared Builds

Machine-wide tracking of NixOS build results across users.

## Brief

Whoever builds on a machine by hand, beside the automatic updater that runs
there as the user `nxf`, wants both to agree about which system a checkout
produced. nxf records the checkout's Git tree ID (its tracked content,
including `flake.lock`) as a symlink to the built system in the shared Nix
store.

The NixOS module provisions `/var/lib/nxf/systems` for root, the dedicated
`nxf` user and members of group `nxf`. They publish records there atomically.
Status prefers shared records, so the updater can skip a redundant build and
switch when the committed checkout is already running. Configuration
selection and update strategies still require evaluation.

Without a usable shared directory, publication tries per-user state. If both
locations are the same inaccessible directory, no record can be saved.
Metadata failures warn without failing a successful build, update or switch.

Boundaries:

- No task creates the shared directory for another user: the module
  provisions it, and `nxf install` creates it on the system it installs. A
  process whose own state is the updater's state, as the service's is,
  creates `systems` there as its own. Without the shared directory records
  stay in per-user state.
- Source roots, result links, read markers and Nix's fetcher cache keep their
  per-user locations.
- The sources of a tree are kept once. The file `kept/<tree>` in the per-user
  state lists them, and the keeping of another tree replaces it. A build of
  the same tree skips the copy while every listed source is the target of a
  root and is in the store. A file whose roots are gone does not skip it.
- A record is a plain symlink and no GC root. It is ignored once its system
  is garbage-collected.
- A build with uncommitted changes is not recorded.
- Only a publication imports old records, and only those of the caller's own
  state. Status imports nothing.
- A recorded output does not activate a system that is not running: `switch`
  builds again.
- Root, the user `nxf` and the members of the group `nxf` can replace each
  other's records. They are trusted alike.
- A record is replaced by rename, so a reader sees the complete old symlink
  or the complete new one.
- It does not solve access to private evaluation sources.

## Acceptance scenarios

These Given/When/Then scenarios describe observable behaviour and serve as
templates for acceptance tests. Use throwaway Git flakes, mock nh and isolated
state directories; test user permissions and garbage collection only in a VM.

| ID | Given | When | Then |
| --- | --- | --- | --- |
| S1: Manual build visible to the updater | A clean checkout, a provisioned shared directory, no input changes, and a running system that a group member built from the checkout | The updater runs as `nxf` | `nxf status --json` of both users agrees on `built`, `matches = true` and `revision`; the updater calls no nh build or switch |
| S2: Replace another user's record | A shared record of the tree that another authorized user wrote: a group member, the user `nxf`, or root with umask `022` | An authorized user builds the same tree | The build succeeds; the record belongs to the user who built and links to the built system |
| S3: Standalone use | No shared `systems` directory has been provisioned | A user builds successfully | The record is saved in `systems` under that user's state; the updater location is not created, unless it is that state |
| S4: Shared publication denied | Separate per-user state is writable, but the shared directory is not writable/searchable by this process, for example a non-member or a session without its new group | The user builds or switches | The task succeeds, warns about shared access and active group membership, and saves a per-user record; doctor warns and reports the fallback destination |
| S5: All record writes fail | A build succeeds but neither shared nor per-user records can be written | A build, switch or input update completes | Warnings identify the metadata failure; activation still proceeds when requested; an updated lockfile stays committed and the update is not reported as failed or restored |
| S6: Import existing records | The caller has legacy records, including a conflicting key, dangling links and invalid targets | The caller next publishes a shared build record | Valid records from that caller's state are copied without replacing existing shared entries; invalid records are skipped; the originals remain; no other home is scanned |
| S7: Checkout changes during a build | A clean checkout before building | A tracked file changes, or a different tree is committed, before the build returns | The successful output is not recorded under either tree; nxf warns that status cannot identify the build |
| S8: Update builds before committing | Updating an input changes `flake.lock` | nxf builds the dirty lockfile and then commits it | The resulting clean tree is recorded only if its content still corresponds to what was built; edits during the build prevent publication |
| S9: State overrides and read-only status | Explicit `NXF_STATE` and `NXF_UPDATER_STATE` locations, the second with a provisioned `systems`, legacy records, an unread update summary, and a build by this user | The user runs `nxf status --json` | The record of the build is in `systems` of the updater location; the result link and the source roots are in `NXF_STATE`; no legacy record is imported; the next `nxf status` still prints the summary |
| S10: A different system is running | A record exists for the clean checkout, but its output is not running | The user requests `switch` | nxf still calls nh to build and activates that exact result; the record is not used as an activation shortcut |

## Existing verification

Every test below runs in `just check`. The tests in `tests/` use a throwaway
flake, the mock nh and isolated state directories. The
[NixOS VM test](../../nix/shared-records-check.nix) is the flake check
`shared-records`.

| ID | Covered by | Not covered |
| --- | --- | --- |
| S1 | `tests/auto_update.rs::manual_build_skips_service_build`, with a conflicting local record that must not override the shared one | Two Unix users: the test runs as one user, and a second `NXF_STATE` stands in for the user `nxf` |
| S2 | `nix/shared-records-check.nix`: alice, bob, `nxf` and root with umask `022` build the same tree in turn, and alice replaces root's record | |
| S3 | `tests/tasks.rs::standalone_records` for a state that is separate from the updater location: the record is in `systems` of that state and the updater location does not exist afterwards; `tests/auto_update.rs::shared_state` for a state that is the updater location: the record is in `systems` there | For `shared_state`: that `systems` was missing before the run |
| S4 | `tests/tasks.rs::shared_fallback` for `switch`; `nix/shared-records-check.nix` for a build by a user outside the group; `tests/doctor.rs::record_destination` for the doctor, also when no separate fallback exists | A session without its new group. For the build in the VM test: the warning about group membership |
| S5 | `tests/tasks.rs::record_failures_are_nonfatal`, for `build`, `switch` and `update` | For `switch`, that nh is asked to switch: the test asserts the exit status 0 and the two warnings |
| S6 | `tests/tasks.rs::imports_legacy_records` | That no other home is scanned: the test has one state directory |
| S7 | `tests/tasks.rs::changed_during_build`, for a tracked edit and for a new commit: no record is written | The warning |
| S8 | `tests/tasks.rs::update_build_identity`; `tests/tasks.rs::dirty_update_identity` for a checkout that had uncommitted changes before the update | |
| S9 | `tests/tasks.rs::publishes_shared` for the shared record, the result link and the source roots, with a separate `NXF_STATE`; `tests/tasks.rs::imports_legacy_records` for status not importing; `tests/status.rs::json` for the summary that stays unread | No test has all of the Given in one run: `publishes_shared` has no legacy records and no summary, `imports_legacy_records` has no explicit `NXF_STATE` and no summary, and `json` has no legacy records and no explicit `NXF_STATE` |
| S10 | `tests/tasks.rs::switch_builds_recorded_system` | |

Beyond the scenarios, `tests/doctor.rs::linked_worktree` covers linked Git
worktrees, and the VM test covers the repair of the directory's mode by
tmpfiles and per-user source roots that survive garbage collection.
`tests/state.rs::sources_stay_until_replaced` covers that the source roots
of a build stay until those of the next build replace them.
`tests/state.rs::sources_kept_once` covers that a build, and the switch of an
upgrade, keep the sources of a tree once, and `tests/state.rs::stale_mark_keeps_again`
that a file whose roots are gone or lead elsewhere does not skip the copy.

See the [operating documentation](../../README.md#status) for state paths and
migration details.
