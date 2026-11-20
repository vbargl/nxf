//! Tests for src/hold.rs: holding automatic updates back, end to end.

mod common;

use common::{assert_contains, assert_lacks, fake_systems, fixture, saved};
use nxf::hold;
use serde_json::Value;

/// Durations are a number with a unit.
#[test]
fn durations() {
    assert_eq!(hold::duration("90m"), Ok(5400));
    assert_eq!(hold::duration("12h"), Ok(43200));
    assert_eq!(hold::duration("3d"), Ok(259200));
    assert_eq!(hold::duration("2w"), Ok(1209600));
    for invalid in ["", "3", "d", "0d", "-1d", "3x", "1.5h"] {
        assert!(hold::duration(invalid).is_err(), "{invalid}");
    }
}

/// A hold stops the automatic update, is shown by status, and ends on release.
#[test]
fn until_released() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let before = fx.locked("a");
    fx.bump(&["a"]);
    let held = fx.run(&["hold", "--reason", "waiting for the vendor fix"]);
    held.ok();
    assert_contains(&held.stdout, "waiting for the vendor fix (until released)");

    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(fx.locked("a"), before);
    let result = &saved(&fx)["results"][0];
    assert_eq!(result["outcome"], "skipped");
    assert_contains(
        result["detail"].as_str().unwrap(),
        "held: waiting for the vendor fix",
    );

    let status = fx.run(&["status"]);
    assert_contains(
        &status.stdout,
        "! Automatic updates are held: waiting for the vendor fix (until released)",
    );
    let json: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    assert_eq!(json["hold"]["reason"], "waiting for the vendor fix");
    assert_eq!(json["hold"]["until"], Value::Null);

    assert_contains(&fx.run(&["release"]).stdout, "Automatic updates continue.");
    assert_contains(&fx.run(&["release"]).stdout, "No hold is set.");
    assert_lacks(&fx.run(&["status"]).stdout, "held");
    fx.run(&["auto-update"]).ok();
    assert_ne!(fx.locked("a"), before);
}

/// A hold with an end names it, and stops holding once the end has passed.
#[test]
fn expiry() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.run(&["hold", "--reason", "release week", "--for", "3d"])
        .ok();
    let json: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    let (since, until) = (
        json["hold"]["since"].as_u64().unwrap(),
        json["hold"]["until"].as_u64().unwrap(),
    );
    assert_eq!(until - since, 259200);
    assert_contains(&fx.run(&["status"]).stdout, "release week (until 20");

    // The same hold, ended a minute ago.
    let ended =
        serde_json::json!({"reason": "release week", "since": 1, "until": hold::now() - 60});
    std::fs::write(fx.updater.join("hold.json"), ended.to_string()).unwrap();
    assert_lacks(&fx.run(&["status"]).stdout, "held");
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
    assert!(!fx.updater.join("hold.json").exists());
}

/// A reason is required, and an end must be valid and in the future; each
/// refusal names its cause.
#[test]
fn invalid_requests() {
    let fx = fixture();
    for (args, cause) in [
        (&["hold"][..], "  --reason <REASON>"),
        (
            &["hold", "--reason", "r", "--for", "soon"],
            "✗ Invalid duration 'soon'",
        ),
        (
            &["hold", "--reason", "r", "--until", "not a date"],
            "✗ Invalid time 'not a date'",
        ),
        (
            &["hold", "--reason", "r", "--until", "2001-01-01"],
            "✗ The hold would end in the past.",
        ),
        (
            &[
                "hold",
                "--reason",
                "r",
                "--until",
                "2999-01-01",
                "--for",
                "1d",
            ],
            "'--until <UNTIL>' cannot be used with '--for <DURATION>'",
        ),
        (&["release", "now"], "unexpected argument 'now'"),
    ] {
        let result = fx.run(args);
        assert_ne!(result.code, 0, "nxf {} succeeded", args.join(" "));
        assert_contains(&result.stderr, cause);
    }
    assert!(!fx.updater.join("hold.json").exists());
    fx.run(&["hold", "--reason", "r", "--until", "2999-01-01 12:00"])
        .ok();
    assert_contains(&fx.run(&["status"]).stdout, "r (until 2999-01-01 12:00)");
}

/// Tasks run by hand are not held.
#[test]
fn manual_tasks_run() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.run(&["hold", "--reason", "frozen"]).ok();
    fx.run(&["switch"]).ok();
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
}
