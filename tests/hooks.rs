//! Tests for src/hooks.rs: the check hook before an automatic update and the
//! failure hook after a failed one, end to end.

mod common;

use std::path::PathBuf;

use common::{assert_contains, fake_systems, fixture, saved, strategies, write_script, Fixture};
use serde_json::{json, Value};

const CHECK: &str = "check";
const FAILURE: &str = "failure";

/// A fixture with a new revision of input a to update to, and a directory
/// for the hooks.
fn updating() -> Fixture {
    let mut fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_HOOK_TIMEOUT", "2");
    let hooks = fx.dir.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    fx.set("NXF_HOOKS_DIR", hooks);
    fx.bump(&["a"]);
    fx
}

/// The same, with a system that runs the checkout and an update that does
/// not build, so that the input is the one thing that goes wrong.
fn failing() -> Fixture {
    let system = &fake_systems().one;
    let fx = updating()
        .with("NXF_SYSTEM", system)
        .with("NXF_TEST_NH_FAIL_ACTION", "build");
    fx.record_build(system);
    fx
}

fn hook(fx: &Fixture, name: &str, script: &str) {
    write_script(
        &fx.dir.join("hooks").join(name),
        &format!("#!/bin/sh\n{script}\n"),
    );
}

/// A failure hook that leaves a file behind when it runs.
fn marking(fx: &Fixture) -> PathBuf {
    let mark = fx.dir.join("failure-hook-ran");
    hook(fx, FAILURE, &format!("touch {}", mark.display()));
    mark
}

fn results(fx: &Fixture) -> Value {
    saved(fx)["results"].clone()
}

/// Without hooks the update runs as before.
#[test]
fn absent() {
    let fx = updating();
    let old = fx.locked("a");
    fx.run(&["auto-update"]).ok();
    assert_ne!(fx.locked("a"), old);
}

/// A check hook that passes lets the update run, and the failure hook does
/// not run after a success.
#[test]
fn passing_check() {
    let fx = updating();
    let old = fx.locked("a");
    hook(&fx, CHECK, "exit 0");
    let mark = marking(&fx);
    fx.run(&["auto-update"]).ok();
    assert_ne!(fx.locked("a"), old);
    assert_eq!(fx.nh_actions(), ["build", "build", "switch"]);
    assert!(!mark.exists());
}

/// A check hook that fails skips the run with what it said; nothing is
/// fetched, built or activated, and the run does not count as failed.
#[test]
fn refusing_check() {
    let fx = updating();
    let old = fx.locked("a");
    hook(
        &fx,
        CHECK,
        "echo checking; echo 'rollout is paused' >&2; exit 1",
    );
    let mark = marking(&fx);
    let result = fx.run(&["auto-update"]);
    result.ok();
    assert_eq!(fx.locked("a"), old);
    assert!(fx.nh_calls().is_empty());
    assert!(!mark.exists());
    assert_eq!(
        results(&fx),
        json!([{"task": "update", "inputs": [], "outcome": "skipped", "details": ["rollout is paused"]}])
    );
    assert_contains(
        &fx.run(&["status"]).stdout,
        "update    skipped, rollout is paused",
    );
}

/// A check hook that does not end is stopped and skips the run.
#[test]
fn hanging_check() {
    let fx = updating().with("NXF_HOOK_TIMEOUT", "1");
    hook(&fx, CHECK, "sleep 30");
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(common::details(&results(&fx)[0]), ["timed out"]);
}

/// A hand-run update asks no hook.
#[test]
fn manual_update() {
    let fx = updating();
    let old = fx.locked("a");
    hook(&fx, CHECK, "exit 1");
    fx.run(&["update"]).ok();
    assert_ne!(fx.locked("a"), old);
}

/// After a run that could not update an input to report, the failure hook
/// runs and finds the saved results in NXF_RESULTS; when it passes it adds
/// nothing to them, and the run does not fail.
#[test]
fn failure_sees_results() {
    let fx = failing();
    let copy = fx.dir.join("copy.json");
    hook(
        &fx,
        FAILURE,
        &format!("cp \"$NXF_RESULTS\" {}", copy.display()),
    );
    fx.run(&["auto-update"]).ok();
    let seen: Value = serde_json::from_str(&std::fs::read_to_string(&copy).unwrap()).unwrap();
    assert_eq!(seen["results"][1]["task"], "update");
    assert_eq!(seen["results"][1]["inputs"], json!(["a"]));
    assert_eq!(seen["results"][1]["outcome"], "reported");
    assert_eq!(results(&fx).as_array().unwrap().len(), 2);
}

/// A flagged input that could not be updated is a record only: no hook runs.
#[test]
fn flagged_runs_none() {
    let system = &fake_systems().one;
    let fx = updating()
        .with("NXF_SYSTEM", system)
        .with("NXF_TEST_NH_FAIL_ACTION", "build");
    strategies(&fx, "{ inputs.a = \"flag\"; }");
    fx.record_build(system);
    let mark = marking(&fx);
    fx.run(&["auto-update"]).ok();
    assert_eq!(results(&fx)[1]["outcome"], "flagged");
    assert!(!mark.exists());
}

/// A task that fails runs the failure hook, and the run fails.
#[test]
fn failed_task_runs_failure() {
    let fx = updating().with("NXF_TEST_NH_FAIL_ACTION", "switch");
    let mark = marking(&fx);
    assert_eq!(fx.run(&["auto-update"]).code, 1);
    let results = results(&fx);
    assert_eq!(results[1]["task"], "switch");
    assert_eq!(results[1]["outcome"], "failed");
    assert!(mark.exists());
}

/// A failure hook that fails itself is a result of its own.
#[test]
fn failure_of_its_own() {
    let fx = failing();
    hook(&fx, FAILURE, "echo 'cannot pause' >&2; exit 4");
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    let results = results(&fx);
    assert_eq!(results[2]["task"], "hook");
    assert_eq!(results[2]["outcome"], "failed");
    assert_eq!(common::details(&results[2]), ["cannot pause"]);
    assert_contains(&fx.run(&["status"]).stdout, "  ✗ hook      cannot pause");
}
