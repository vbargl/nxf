# Automatic Update

Updating a machine unattended, from the timer, and returning to the previous
system when the new one is not healthy.

## Brief

Whoever looks after a machine wants its inputs kept current without logging
in, wants a bad update to undo itself, and wants to read afterwards what
happened. Whoever runs several machines also wants a say before an update
starts and a signal after one failed.

`nxf-auto-update.timer` starts `nxf-auto-update.service`, which runs
`nxf auto-update` as the user `nxf`. One run does this, in this order:

1. It removes a hold that has ended. It skips the run when a hold is active,
   when files other than `flake.lock` have uncommitted changes, or when the
   check hook fails.
2. It reads the strategy of each input from the configuration in the checkout
   and runs the task `update`. An input that is `off` is left alone. An input
   that is `flag` or `report` and cannot be fetched or does not build is
   restored and recorded as `flagged` or `reported`, and the run goes on.
3. When the committed checkout is not what runs, it runs the task `switch`,
   unless that revision is marked as bad.
4. After a switch it runs the health checks of the new configuration, in name
   order, and repeats the round until all pass or `health.timeout` runs out.
   When checks still fail, it marks the revision as bad in
   `/var/lib/nxf/bad/<tree>` and activates the previous system again. It
   activates the previous system also when the mark cannot be written, and
   the result `health` then names the failed mark too.
5. It saves the results to `/var/lib/nxf/update.json`. When a task failed or
   an input was reported, it runs the failure hook with the saved file in
   `NXF_RESULTS`. A failure hook that fails adds the result `hook`.
6. It exits with status 1 when a result is `failed`, and with 0 otherwise.

A run that cannot go on is saved too: the results of the tasks that ran,
followed by `update` `failed` with the cause. The failure hook runs, and the
exit status is 1. A run that fails before any task ran, for example because
the strategies of the configuration cannot be read, has that one result.
The nxf module refuses a strategy other than `off`, `flag` and `report` when
the configuration is evaluated, and the cause is then
`Cannot read the update strategies of the configuration`. When the failure
hook fails too, the result `hook` `failed` is saved, and the command shows
`The failure hook failed too:` with what the hook said. When the results
cannot be saved, the command fails with the cause and the failed save
beneath it. The failure hook does not run when the first save fails.

`nxf status` prints the saved results once to each user, as
`Last automatic update`.

Boundaries:

- Tasks run by hand use no hold, no strategy, no hook and no health check,
  and their results are not saved.
- The run does not restart the machine. A new kernel waits for the next
  restart, and `nxf status` says `Restart needed`.
- nxf ships no health checks and no hooks. The configuration supplies them,
  and without health checks nothing is verified after a switch.
- A health check fails when any of its commands does. The module runs each
  script with `set -e`, also the lines that other modules add to the same
  check.
- The options of the module, the configurations it refuses and the unit of its
  durations are in the [README](../../README.md#install).
- The check hook can only skip the run, and the failure hook can only add
  its own failure.
- A skipped run, a flagged input and a reported input do not fail the run.
- A bad revision stays in the repository. The mark belongs to the tree of
  the checkout, and nxf does not remove it.
- Only the latest results are kept: each run replaces `update.json`.
- `nxf status` is the only way the results reach people. nxf sends no
  notification.
- nxf holds no lock between its processes. Nothing keeps a task run by hand
  and the automatic update apart when both run on one machine.
- A run that is ended after its switch and before its health checks ended,
  by its timeout, a stop or a loss of power, leaves that system unchecked.
  The next run finds the checkout running, so it does not switch and does
  not check. No bad mark is written, and the previous system is not
  activated again.

## Acceptance scenarios

In every row the action is one run of `nxf auto-update`, unless the When says
otherwise. "The results" are those in `update.json`.

| ID | Given | When | Then |
| --- | --- | --- | --- |
| A1: Update and switch | A new revision of an input, no hold and a clean checkout | The run starts | The lockfile is committed; the results are `update` `done` and `switch` `done`; the output has the rows `✓ update` and `✓ switch`; the exit status is 0 |
| A2: Nothing to do | No new revision, and the committed checkout is what runs | The run starts | nh is not called; the one result is `update` `unchanged`; the exit status is 0 |
| A3: The checkout is ahead of the system | No new revision, and the committed checkout is not what runs | The run starts | The results are `update` `unchanged` and `switch` `done`; the exit status is 0 |
| A4: Held | A hold set with `nxf hold --reason <text>` | The run starts | nh is not called and the lockfile is unchanged; the one result is `update` `skipped` with `held: <text> (until released)`; the exit status is 0 |
| A5: A hold has ended | A hold whose end has passed | The run starts | The run goes on as without a hold; `hold.json` is removed |
| A6: Work in progress | A file other than `flake.lock` has uncommitted changes | The run starts | nh is not called; the one result is `update` `skipped` with `the checkout has uncommitted changes`; the exit status is 0 |
| A7: A lockfile left behind | Only `flake.lock` has uncommitted changes | The run starts | The lockfile is restored and the run goes on; the checkout ends without uncommitted changes |
| A8: The check hook refuses | A check hook that fails, and a failure hook | The run starts | nh is not called and the lockfile is unchanged; the failure hook does not run; the one result is `update` `skipped` with the last line the check hook printed, or `timed out` when it did not end in `hooks.timeout` seconds; the exit status is 0 |
| A9: An input is off | The strategy `off` for one input, and new revisions of it and of another input | The run starts | The input that is off keeps its revision; the other is updated; no result names the input that is off |
| A10: A flagged input fails | An input with the strategy `flag` that cannot be fetched or does not build, and a new revision of another input | The run starts | The other input is updated and switched to; the results hold `update` `flagged` with the input in `inputs` and what its update said in `detail`; the failure hook does not run; the exit status is 0; `nxf status` shows the row `· update <input>  flagged, …` |
| A11: A reported input fails | The same with the strategy `report`, which is the default | The run starts | The results hold `update` `reported` with the input; the failure hook runs and finds the saved results in the file `$NXF_RESULTS`; the exit status is 0 |
| A12: The switch fails | The committed checkout is not what runs, and the activation fails | The run starts | The results end with `switch` `failed`; no health check runs and no system is activated a second time; the failure hook runs; the exit status is 1 |
| A13: Healthy | Health checks that pass, at once or in a later round | The run switches | The result `health` is `done`; no bad mark is written; the exit status is 0 |
| A14: No health checks | No executable file among the health checks | The run switches | There is no result `health` |
| A15: Unhealthy | A health check that still fails when `health.timeout` runs out | The run switches | nh switches to the previous system; `/var/lib/nxf/bad/<tree>` holds `<check>: ` and the last line the check printed, or `timed out`; the result `health` is `failed` with that text and `Returned to <system>.`; the exit status is 1; `nxf status` says `The checkout failed its health check` and shows the row `✗ health`; `nxf status --json` has the text in `bad` |
| A16: A bad revision is not tried again | The revision of the checkout is marked as bad | The run starts | nh is not called; the results hold `switch` `skipped` with `this revision failed its health check (<reason>); waiting for a newer one`; the exit status is 0 |
| A17: A newer revision after a bad one | A bad mark, and a new revision of an input | The run starts | The new revision is committed, switched to and checked; `nxf status --json` has `bad` null |
| A18: The return fails | An unhealthy system, and the activation of the previous system fails | The run switches | The result `health` is `failed` with `Could not return to <system>:` and what nh said; the exit status is 1 |
| A19: The failure hook fails | A run with a failed task or a reported input, and a failure hook that fails | The run ends | The results gain `hook` `failed` with the last line the hook printed, and are saved again; the exit status is 1; `nxf status` shows the row `✗ hook` |
| A20: The summary is read once | Saved results that this user has not seen | `nxf status` runs | It prints `Last automatic update` with one row for each result that is not `unchanged`; the next `nxf status` of this user does not print it, and that of another user does |
| A21: JSON does not mark as read | Saved results that this user has not seen | `nxf status --json` runs | `lastUpdate` holds the time and the results; the next `nxf status` still prints the summary |
| A22: The service ended before it saved | `nxf-auto-update.service` ended with a result other than `success`, later than the saved results | `nxf status` runs | The summary has the row `✗ service` with the result of the unit, and `journalctl -u nxf-auto-update.service` |
| A23: A task by hand | A hold and a health check that fails | `nxf switch` runs by hand | nh builds and switches; no health check runs; the exit status is 0 |
| A24: The bad mark cannot be written | A health check that still fails when `health.timeout` runs out, and a `/var/lib/nxf/bad` that cannot be written | The run switches | nh switches to the previous system; the result `health` is `failed` with `<check>: ` and the last line the check printed, then `cannot write` with the path of the mark, then `Returned to <system>.`; the exit status is 1 |
| A25: The run fails before a task | The configuration in the checkout does not take `services.nxf.autoUpdate` from the nxf module and names a strategy other than `off`, `flag` and `report`, and a failure hook | The run starts | The one result is `update` `failed` with `unknown update strategy '<name>'` in `detail`; the failure hook runs and finds the saved results in the file `$NXF_RESULTS`; the outcome starts with `unknown update strategy '<name>'`; the exit status is 1; `nxf status` shows the row `✗ update` with the cause |
| A26: An unknown strategy with the module | The configuration in the checkout imports the nxf module and names a strategy other than `off`, `flag` and `report` | The run starts | nh is not called; the one result is `update` `failed` with `Cannot read the update strategies of the configuration` and what Nix said in `detail`; the outcome is `Cannot read the update strategies of the configuration`; the exit status is 1 |
| A27: The run cannot go on after a task | A new revision of an input, an input to report that cannot be fetched, and a checkout that loses its `flake.nix` while the update builds | The run starts | The results are `update` `done`, `update` `reported` with the input, and `update` `failed` with `<checkout> has no flake.nix` in `detail`; the outcome starts with `<checkout> has no flake.nix`; the exit status is 1 |
| A28: The results cannot be saved | A run that fails before any task ran, and a state directory that cannot be written | The run starts | The outcome is the cause of the run, with `cannot save <state>/update.json:` and the reason beneath it; the exit status is 1 |
| A29: The failure hook fails after a run that cannot go on | A run that fails before any task ran, and a failure hook that fails | The run starts | The results are `update` `failed` and `hook` `failed` with the last line the hook printed; beneath the outcome the line `The failure hook failed too:` holds that line; the exit status is 1 |
| A30: A run was ended before its health checks ended | An earlier run switched to the committed checkout and was ended before its health checks ended; no new revision; a health check that fails | The run starts | nh is not called; the one result is `update` `unchanged`; `/var/lib/nxf/bad/<tree>` does not exist; the exit status is 0 |

## Existing verification

Every test below uses the mock nh and stand-ins for the running system, the
health checks and the hooks. No test activates a system or starts the timer
or the service; `nix/module-check.nix`, the flake check `module`, evaluates
the units, checks that a health check script holds `set -e`, and checks the
refusals of the module.

| ID | Covered by | Not covered |
| --- | --- | --- |
| A1 | `tests/auto_update.rs::updates_and_switches` | |
| A2 | `tests/auto_update.rs::unchanged_and_running` | |
| A3 | `tests/auto_update.rs::switches_to_committed_checkout` | |
| A4 | `tests/hold.rs::until_released` for nh, the lockfile, the outcome `skipped` and `held: <text>`; `tests/cli.rs::auto_update_state` for the one result `update` `skipped` | `(until released)` in `detail` |
| A5 | `tests/hold.rs::expiry` | |
| A6 | `tests/auto_update.rs::uncommitted_changes` | |
| A7 | `tests/auto_update.rs::uncommitted_changes` | That the change left behind is restored and not committed: the test asserts a checkout without uncommitted changes and `update` `done` |
| A8 | `tests/hooks.rs::refusing_check`; `tests/hooks.rs::hanging_check` for a hook that does not end | For a hook that does not end: the lockfile, the task and the outcome of the result, and that it is the one result. The test asserts `timed out` in `detail`, that nh is not called and the exit status |
| A9 | `tests/auto_update.rs::off_is_left_alone`; `tests/auto_update.rs::all_off` for every input | The `inputs` of the results: the test asserts that the results are `update` `done` and `switch` `done` |
| A10 | `tests/auto_update.rs::flagged_failure_goes_on` for an input that cannot be fetched; `tests/hooks.rs::flagged_runs_none` for the failure hook, with an input that does not build | |
| A11 | `tests/auto_update.rs::reported_failure_goes_on` for an input that cannot be fetched; `tests/auto_update.rs::failed_build_is_restored` for one that does not build; `tests/hooks.rs::failure_sees_results` for the failure hook | |
| A12 | `tests/health.rs::failed_switch`; `tests/hooks.rs::failed_task_runs_failure` for the failure hook | |
| A13 | `tests/health.rs::healthy`; `tests/health.rs::passes_later` for a later round | For a later round: the bad mark |
| A14 | `tests/health.rs::no_checks` | |
| A15 | `tests/health.rs::unhealthy_returns_and_marks`; `tests/health.rs::hanging_check` for `timed out`; `tests/health.rs::own_words_are_not_the_deadline` for a check that prints `timed out` itself; `tests/health.rs::times_of_the_new_configuration` for the times | `The checkout` before `failed its health check`: the test asserts `failed its health check`. For `timed out`: the content of the mark and `Returned to <system>.` in the result. The two tests assert `<check>: timed out` in the result and the exit status |
| A16 | `tests/health.rs::unhealthy_returns_and_marks` | `this revision failed its health check (<reason>)` in `detail`: the test asserts only `waiting for a newer one` |
| A17 | `tests/health.rs::unhealthy_returns_and_marks` | |
| A18 | | No automated test covers it |
| A19 | `tests/hooks.rs::failure_of_its_own`, after a reported input | A failure hook that fails after a failed task |
| A20 | `tests/status.rs::unread_update`; `tests/status.rs::update_read_per_user`; `tests/status.rs::latest_update`; `tests/status.rs::update_details` | |
| A21 | `tests/status.rs::json`; `tests/status.rs::json_monitoring` for `lastUpdate` | |
| A22 | `tests/status.rs::update_unit_result` | |
| A23 | `tests/hold.rs::manual_tasks_run` for the hold; `tests/health.rs::manual_switch` for the health check; `tests/hooks.rs::manual_update` for `nxf update` beside a check hook that fails | The hold and the failing check in one run. That no check runs is asserted by its effect: nh is called to build and to switch once, and the exit status is 0 |
| A24 | `tests/health.rs::unmarked_still_returns` | The path of the mark after `cannot write` |
| A25 | `tests/auto_update.rs::undecided_run_is_saved`; `tests/auto_update.rs::unknown_strategy` asserts that nh is not called | |
| A26 | | No automated test covers it: the configuration of the fixtures does not import the module |
| A27 | `tests/auto_update.rs::undecided_run_keeps_what_ran` | `<checkout>` before `has no flake.nix`, in the outcome and in `detail`, and that the outcome starts with it: the test asserts `has no flake.nix` in stderr and in `detail` |
| A28 | `tests/auto_update.rs::unsaved_run_keeps_its_cause`, also for a save that fails after the failure hook failed | |
| A29 | `tests/auto_update.rs::failing_hook_is_said` | |
| A30 | `tests/auto_update.rs::unchanged_and_running` for a run that finds the checkout running | A health check that fails: the fixture of the test has no health checks. The missing bad mark |

See the [operating documentation](../../README.md#automatic-updates).
