//! Tests for src/hooks.rs: the checks before an automatic update and the
//! hooks after a failed one, end to end.

mod common;

use common::{assert_contains, fake_systems, fixture, write_script, Fixture};
use serde_json::{json, Value};

/// A fixture with a new revision of input a to update to, and directories
/// for the checks and the hooks.
fn updating() -> Fixture {
    let mut fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_HOOK_TIMEOUT", "2");
    for (variable, name) in [("NXF_PRE_DIR", "pre.d"), ("NXF_FAILURE_DIR", "failure.d")] {
        let dir = fx.dir.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        fx.set(variable, dir);
    }
    fx.bump(&["a"]);
    fx
}

/// The same, with a system that runs the checkout and an update that does
/// not build, so that the update is the one task that fails.
fn failing() -> Fixture {
    let system = &fake_systems().one;
    let fx = updating()
        .with("NXF_SYSTEM", system)
        .with("NXF_TEST_NH_FAIL_ACTION", "build");
    fx.record_build(system);
    fx
}

fn script(fx: &Fixture, dir: &str, name: &str, script: &str) {
    write_script(
        &fx.dir.join(dir).join(name),
        &format!("#!/bin/sh\n{script}\n"),
    );
}

fn results(fx: &Fixture) -> Value {
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(fx.updater.join("update.json")).unwrap())
            .unwrap();
    report["results"].clone()
}

/// Checks that pass let the update run, and no hook runs after a success.
#[test]
fn passing_checks() {
    let fx = updating();
    let old = fx.locked("a");
    script(&fx, "pre.d", "10-open", "exit 0");
    script(
        &fx,
        "failure.d",
        "10-mark",
        "touch \"$(dirname \"$0\")/ran\"",
    );
    fx.run(&["auto-update"]).ok();
    assert_ne!(fx.locked("a"), old);
    assert_eq!(fx.nh_actions(), ["build", "build", "switch"]);
    assert!(!fx.dir.join("failure.d/ran").exists());
}

/// A check that fails skips the run with what it said; nothing is fetched,
/// built or activated, the checks after it do not run, and the run does not
/// count as failed.
#[test]
fn refusing_check() {
    let fx = updating();
    let old = fx.locked("a");
    script(&fx, "pre.d", "10-open", "exit 0");
    script(
        &fx,
        "pre.d",
        "20-rollout",
        "echo checking; echo 'rollout is paused' >&2; exit 1",
    );
    script(&fx, "pre.d", "30-later", "touch \"$(dirname \"$0\")/ran\"");
    script(
        &fx,
        "failure.d",
        "10-mark",
        "touch \"$(dirname \"$0\")/ran\"",
    );
    let result = fx.run(&["auto-update"]);
    result.ok();
    assert_eq!(fx.locked("a"), old);
    assert!(fx.nh_calls().is_empty());
    assert!(!fx.dir.join("pre.d/ran").exists());
    assert!(!fx.dir.join("failure.d/ran").exists());
    assert_eq!(
        results(&fx),
        json!([{"task": "update", "inputs": [], "outcome": "skipped", "detail": "20-rollout: rollout is paused"}])
    );
    assert_contains(
        &fx.run(&["status"]).stdout,
        "update    skipped, 20-rollout: rollout is paused",
    );
}

/// A check that does not end is stopped and skips the run.
#[test]
fn hanging_check() {
    let fx = updating().with("NXF_HOOK_TIMEOUT", "1");
    script(&fx, "pre.d", "10-hangs", "sleep 30");
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(results(&fx)[0]["detail"], "10-hangs: timed out");
}

/// A hand-run update asks no check.
#[test]
fn manual_update() {
    let fx = updating();
    let old = fx.locked("a");
    script(&fx, "pre.d", "10-closed", "exit 1");
    fx.run(&["update"]).ok();
    assert_ne!(fx.locked("a"), old);
}

/// After a failed run every hook runs, in name order, and finds the saved
/// results in NXF_RESULTS.
#[test]
fn failure_hooks() {
    let fx = failing();
    let copy = fx.dir.join("copy.json");
    script(
        &fx,
        "failure.d",
        "10-copy",
        &format!("cp \"$NXF_RESULTS\" {}", copy.display()),
    );
    script(
        &fx,
        "failure.d",
        "20-order",
        &format!("test -e {}", copy.display()),
    );
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    let seen: Value = serde_json::from_str(&std::fs::read_to_string(&copy).unwrap()).unwrap();
    assert_eq!(seen["results"][0]["task"], "update");
    assert_eq!(seen["results"][0]["outcome"], "failed");
    // Hooks that pass add nothing to the results.
    assert_eq!(results(&fx).as_array().unwrap().len(), 1);
}

/// A hook that fails is a result of its own; the others still run.
#[test]
fn failing_hook() {
    let fx = failing();
    script(
        &fx,
        "failure.d",
        "10-broken",
        "echo 'cannot pause' >&2; exit 4",
    );
    script(
        &fx,
        "failure.d",
        "20-mark",
        "touch \"$(dirname \"$0\")/ran\"",
    );
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    assert!(fx.dir.join("failure.d/ran").exists());
    let results = results(&fx);
    assert_eq!(results[1]["task"], "hooks");
    assert_eq!(results[1]["outcome"], "failed");
    assert_eq!(results[1]["detail"], "10-broken: cannot pause");
    assert_contains(
        &fx.run(&["status"]).stdout,
        "  ✗ hooks     10-broken: cannot pause",
    );
}
