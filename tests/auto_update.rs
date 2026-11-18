//! Tests for src/auto_update.rs: the unattended update, end to end.

mod common;

use common::{append, assert_contains, fake_systems, fixture, git, Fixture};
use serde_json::{json, Value};

fn saved(fx: &Fixture) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fx.updater.join("update.json")).unwrap()).unwrap()
}

fn outcomes(report: &Value) -> Vec<(String, String)> {
    report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| {
            (
                result["task"].as_str().unwrap().to_string(),
                result["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pair(task: &str, outcome: &str) -> (String, String) {
    (task.to_string(), outcome.to_string())
}

/// A new input revision is built, committed and switched to.
#[test]
fn updates_and_switches() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_TEST_BUILD", system);
    let old = fx.locked("a");
    fx.bump(&["a"]);
    let result = fx.run(&["auto-update"]);
    result.ok();
    assert_ne!(fx.locked("a"), old);
    assert_eq!(fx.porcelain(), "");
    assert_eq!(fx.nh_actions(), ["build", "build", "switch"]);
    let report = saved(&fx);
    assert!(report["time"].as_u64().unwrap() > 0);
    assert_eq!(
        outcomes(&report),
        [pair("update", "done"), pair("switch", "done")]
    );
    assert_contains(&result.stdout, "  ✓ update\n  ✓ switch\n");
}

/// The identity of the module's default, a name and an empty e-mail, commits
/// the lockfile.
#[test]
fn commits_without_email() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("GIT_AUTHOR_NAME", "nxf")
        .with("GIT_COMMITTER_NAME", "nxf")
        .with("GIT_AUTHOR_EMAIL", "")
        .with("GIT_COMMITTER_EMAIL", "");
    fx.bump(&["a"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        git(&fx.checkout, &["log", "-1", "--format=%an <%ae> %cn <%ce>"]),
        "nxf <> nxf <>"
    );
}

/// Nothing new and a running system that matches: nothing is built or activated.
#[test]
fn unchanged_and_running() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system);
    fx.record_build(system);
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(outcomes(&saved(&fx)), [pair("update", "unchanged")]);
}

/// A committed checkout that is not what runs is switched to without an update.
#[test]
fn switches_to_committed_checkout() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
    assert_eq!(
        outcomes(&saved(&fx)),
        [pair("update", "unchanged"), pair("switch", "done")]
    );
}

/// An update that does not build never stays in the repository and is not
/// switched to; the run fails and says which input to retry.
#[test]
fn failed_build_is_restored() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system)
        .with("NXF_TEST_NH_FAIL_ACTION", "build");
    fx.record_build(system);
    let (old, head) = (fx.locked("a"), fx.head());
    fx.bump(&["a"]);
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    assert_eq!(fx.locked("a"), old);
    assert_eq!(fx.head(), head);
    assert_eq!(fx.porcelain(), "");
    assert_eq!(fx.nh_actions(), ["build"]);
    let report = saved(&fx);
    assert_eq!(outcomes(&report), [pair("update", "failed")]);
    assert_contains(
        report["results"][0]["detail"].as_str().unwrap(),
        "a: Command exited with status 3.",
    );
    assert_contains(
        &fx.run(&["status"]).stdout,
        "  ✗ update    a: Command exited with status 3.",
    );
}

/// The run does nothing while other files have uncommitted changes, but a
/// lockfile left by a killed run is restored.
#[test]
fn uncommitted_changes() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.bump(&["a"]);
    append(&fx.checkout.join("flake.nix"), "# work in progress\n");
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(
        saved(&fx)["results"],
        json!([{"task": "update", "inputs": [], "outcome": "skipped", "detail": "the checkout has uncommitted changes"}])
    );
    common::git(&fx.checkout, &["checkout", "--quiet", "flake.nix"]);
    append(&fx.checkout.join("flake.lock"), "\n");
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.porcelain(), "");
    assert_eq!(outcomes(&saved(&fx))[0], pair("update", "done"));
}

/// Builds are remembered in NXF_STATE, where every user's status finds them.
#[test]
fn shared_state() {
    let system = &fake_systems().one;
    let mut fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system);
    let updater = fx.updater.clone();
    fx.set("NXF_STATE", &updater);
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        &std::fs::canonicalize(updater.join("systems").join(fx.tree())).unwrap(),
        system
    );
    fx.unset("NXF_STATE");
    assert_contains(
        &fx.run(&["status"]).stdout,
        "Running system matches the checkout (",
    );
}
