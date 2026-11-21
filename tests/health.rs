//! Tests for src/health.rs: health checks after an automatic switch, end to end.

mod common;

use common::{
    assert_contains, fake_systems, fixture, outcomes, pair, saved, write_script, Fixture,
};
use serde_json::Value;

/// A fixture that runs one system and builds another, with quick health rounds.
fn switching() -> Fixture {
    let systems = fake_systems();
    let mut fx = fixture()
        .with("NXF_SYSTEM", &systems.one)
        .with("NXF_TEST_BUILD", &systems.two);
    let checks = fx.dir.join("health.d");
    std::fs::create_dir_all(&checks).unwrap();
    fx.set("NXF_HEALTH_DIR", checks);
    settings(&fx, 1);
    fx
}

/// The times the configuration gives its checks, with no pause between rounds.
fn settings(fx: &Fixture, timeout: u64) {
    std::fs::write(
        fx.dir.join("health.conf"),
        format!("timeout={timeout}\ninterval=0\n"),
    )
    .unwrap();
}

fn check(fx: &Fixture, name: &str, script: &str) {
    write_script(
        &fx.dir.join("health.d").join(name),
        &format!("#!/bin/sh\n{script}\n"),
    );
}

/// The task, the outcome and the details of each saved result.
fn results(fx: &Fixture) -> Vec<(String, String, Vec<String>)> {
    saved(fx)["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| {
            let field = |name: &str| result[name].as_str().unwrap().to_string();
            (field("task"), field("outcome"), common::details(result))
        })
        .collect()
}

/// Without checks nothing is verified, as before.
#[test]
fn no_checks() {
    let fx = switching();
    std::fs::write(fx.dir.join("health.d/README"), "not executable").unwrap();
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        outcomes(&saved(&fx)),
        [pair("update", "unchanged"), pair("switch", "done")]
    );
}

/// Passing checks are reported and the new system stays.
#[test]
fn healthy() {
    let fx = switching();
    check(&fx, "10-units", "exit 0");
    check(&fx, "20-node", "exit 0");
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
    assert_eq!(
        outcomes(&saved(&fx)),
        [
            pair("update", "unchanged"),
            pair("switch", "done"),
            pair("health", "done")
        ]
    );
    assert!(!fx.updater.join("bad").exists());
}

/// A check may need several rounds, such as a node that becomes ready later.
#[test]
fn passes_later() {
    let fx = switching();
    let counter = fx.dir.join("rounds");
    check(
        &fx,
        "node-ready",
        &format!(
            "echo round >> '{0}'; [ \"$(wc -l < '{0}')\" -ge 3 ] || {{ echo 'node is not ready'; exit 1; }}",
            counter.display()
        ),
    );
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        std::fs::read_to_string(&counter).unwrap().lines().count(),
        3
    );
    assert_eq!(outcomes(&saved(&fx))[2], pair("health", "done"));
}

/// An unhealthy system is replaced by the previous one, the run fails, and
/// the revision is not tried again until a newer one comes.
#[test]
fn unhealthy_returns_and_marks() {
    let systems = fake_systems();
    let fx = switching();
    check(&fx, "10-units", "exit 0");
    check(
        &fx,
        "20-k3s",
        "echo 'starting'; echo 'k3s is not running' >&2; exit 1",
    );
    let failed = fx.run(&["auto-update"]);
    assert_eq!(failed.code, 1);
    let calls = fx.nh_calls();
    assert_eq!(fx.nh_actions(), ["build", "switch", "switch"]);
    assert_contains(&calls[1], &format!("os switch {} ", systems.two.display()));
    assert_contains(&calls[2], &format!("os switch {} ", systems.one.display()));
    let health = &results(&fx)[2];
    assert_eq!((health.0.as_str(), health.1.as_str()), ("health", "failed"));
    assert_eq!(health.2[0], "20-k3s: k3s is not running");
    assert_eq!(
        health.2[1],
        format!("Returned to {}.", systems.one.display())
    );
    assert_eq!(health.2.len(), 2);
    assert_eq!(
        std::fs::read_to_string(fx.updater.join("bad").join(fx.tree()))
            .unwrap()
            .trim(),
        "20-k3s: k3s is not running"
    );

    let status = fx.run(&["status"]);
    assert_contains(&status.stdout, "failed its health check");
    assert_contains(&status.stdout, "✗ health    20-k3s: k3s is not running");
    let json: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    assert_eq!(json["bad"], "20-k3s: k3s is not running");

    // The same revision is left alone, and the run does not fail again.
    let again = fx.run(&["auto-update"]);
    again.ok();
    assert_eq!(fx.nh_calls().len(), 3);
    let skipped = &results(&fx)[1];
    assert_eq!(
        (skipped.0.as_str(), skipped.1.as_str()),
        ("switch", "skipped")
    );
    assert_contains(&skipped.2[0], "waiting for a newer one");

    // A newer revision is tried, and passes once the check does.
    check(&fx, "20-k3s", "exit 0");
    fx.bump(&["a"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        outcomes(&saved(&fx)),
        [
            pair("update", "done"),
            pair("switch", "done"),
            pair("health", "done")
        ]
    );
    let json: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    assert_eq!(json["bad"], Value::Null);
}

/// A check that hangs is stopped when the time runs out.
#[test]
fn hanging_check() {
    let fx = switching();
    settings(&fx, 1);
    check(&fx, "hangs", "exec sleep 60");
    let started = std::time::Instant::now();
    assert_eq!(fx.run(&["auto-update"]).code, 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_contains(&results(&fx)[2].2[0], "hangs: timed out");
}

/// The times are those of the configuration switched to, like the checks.
/// The environment of the run is that of the system before the switch.
#[test]
fn times_of_the_new_configuration() {
    let fx = switching()
        .with("NXF_HEALTH_TIMEOUT", "300")
        .with("NXF_HEALTH_INTERVAL", "10");
    settings(&fx, 1);
    check(&fx, "fails", "exit 1");
    let started = std::time::Instant::now();
    assert_eq!(fx.run(&["auto-update"]).code, 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_eq!(fx.nh_actions(), ["build", "switch", "switch"]);
}

/// A failed switch is not followed by a health check or a return.
#[test]
fn failed_switch() {
    let fx = switching().with("NXF_TEST_NH_FAIL_ACTION", "switch");
    check(
        &fx,
        "never-run",
        &format!("touch '{}'", fx.dir.join("ran").display()),
    );
    assert_eq!(fx.run(&["auto-update"]).code, 1);
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
    assert!(!fx.dir.join("ran").exists());
    assert_eq!(
        outcomes(&saved(&fx)),
        [pair("update", "unchanged"), pair("switch", "failed")]
    );
}

/// Switching by hand runs no health check.
#[test]
fn manual_switch() {
    let fx = switching();
    check(&fx, "fails", "exit 1");
    fx.run(&["switch"]).ok();
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
}

/// An unhealthy system is replaced by the previous one also when its
/// revision cannot be marked as bad, and the result names all three: the
/// check that failed, the mark that was not written and the return.
#[test]
fn unmarked_still_returns() {
    let systems = fake_systems();
    let fx = switching();
    check(&fx, "20-k3s", "echo 'k3s is not running' >&2; exit 1");
    // A file where the directory of the marks belongs: no mark can be written.
    std::fs::create_dir_all(&fx.updater).unwrap();
    std::fs::write(fx.updater.join("bad"), "").unwrap();
    assert_eq!(fx.run(&["auto-update"]).code, 1);
    let calls = fx.nh_calls();
    assert_eq!(fx.nh_actions(), ["build", "switch", "switch"]);
    assert_contains(&calls[2], &format!("os switch {} ", systems.one.display()));
    let health = &results(&fx)[2];
    assert_eq!((health.0.as_str(), health.1.as_str()), ("health", "failed"));
    assert_eq!(health.2[0], "20-k3s: k3s is not running");
    assert_contains(&health.2[1], "cannot write ");
    assert_eq!(
        health.2[2],
        format!("Returned to {}.", systems.one.display())
    );
}

/// A check that fails with the words "timed out" of its own has failed. It
/// is not taken for a check that the deadline cut short, for which its
/// earlier result would count: the system is unhealthy.
#[test]
fn own_words_are_not_the_deadline() {
    let fx = switching();
    let rounds = fx.dir.join("rounds");
    // Passes in the first round and fails from the second one on.
    check(
        &fx,
        "10-probe",
        &format!(
            "echo round >> '{0}'; [ \"$(wc -l < '{0}')\" -lt 2 ] || {{ echo 'timed out'; exit 1; }}",
            rounds.display()
        ),
    );
    // Fails in the first round, so that there is a second one.
    check(
        &fx,
        "20-node",
        &format!("[ \"$(wc -l < '{}')\" -ge 2 ]", rounds.display()),
    );
    assert_eq!(fx.run(&["auto-update"]).code, 1);
    assert_eq!(fx.nh_actions(), ["build", "switch", "switch"]);
    assert_contains(&results(&fx)[2].2[0], "10-probe: timed out");
}
