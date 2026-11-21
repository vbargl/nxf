//! Tests for src/results.rs: the JSON shapes nxf writes and reads.

use nxf::results::{
    machines_json, outcomes_json, Installation, Kind, Machine, Outcome, Refusal, Saved, State,
};
use serde_json::{json, Value};

/// Each word of an outcome is the one the README documents, and reads back.
#[test]
fn words() {
    let words = [
        (Kind::Done, "done"),
        (Kind::Unchanged, "unchanged"),
        (Kind::Failed, "failed"),
        (Kind::Skipped, "skipped"),
        (Kind::Flagged, "flagged"),
        (Kind::Reported, "reported"),
    ];
    for (kind, word) in words {
        assert_eq!(kind.as_str(), word);
        assert_eq!(Kind::parse(word), Some(kind));
    }
    assert_eq!(Kind::parse("succeeded"), None);
}

/// An outcome has the fields `task`, `inputs`, `outcome` and `details`, and
/// the list of outcomes is a JSON array.
#[test]
fn outcome_shape() {
    let outcome = Outcome::new(
        "update",
        vec!["a".to_string()],
        Kind::Failed,
        vec!["first".to_string(), "second".to_string()],
    );
    let expected = json!({
        "task": "update", "inputs": ["a"], "outcome": "failed", "details": ["first", "second"]
    });
    assert_eq!(outcome.to_json(), expected);
    assert_eq!(Outcome::from_json(&expected), Some(outcome.clone()));
    let list: Value = serde_json::from_str(&outcomes_json(&[outcome])).unwrap();
    assert_eq!(list, json!([expected]));
}

/// A result of another shape is none: the single `detail` text that an older
/// nxf saved, an unknown word, a missing field.
#[test]
fn outcome_of_another_shape() {
    for value in [
        json!({"task": "update", "inputs": [], "outcome": "done", "detail": ""}),
        json!({"task": "update", "inputs": [], "outcome": "lost", "details": []}),
        json!({"task": "update", "inputs": [], "outcome": "done", "details": "text"}),
        json!({"task": "update", "outcome": "done", "details": []}),
    ] {
        assert_eq!(Outcome::from_json(&value), None, "{value}");
    }
}

/// The saved results are the time and the results; one result of another
/// shape makes the whole of them none.
#[test]
fn saved_shape() {
    let saved = Saved {
        time: 7,
        results: vec![Outcome::new("build", Vec::new(), Kind::Done, Vec::new())],
    };
    let expected = json!({
        "time": 7,
        "results": [{"task": "build", "inputs": [], "outcome": "done", "details": []}]
    });
    assert_eq!(saved.to_json(), expected);
    assert_eq!(Saved::from_json(&expected), Some(saved));
    let older = json!({"time": 7, "results": [
        {"task": "build", "inputs": [], "outcome": "done", "details": []},
        {"task": "switch", "inputs": [], "outcome": "done", "detail": ""},
    ]});
    assert_eq!(Saved::from_json(&older), None);
}

/// A refusal is its cause and the lines beneath it, with the cause first and
/// the indentation gone. Only that shape reads back.
#[test]
fn refusal_shape() {
    let refusal = Refusal::from_text("nh exited with status 3.\nthe build said no\n\n  and more");
    assert_eq!(refusal.error, "the build said no");
    assert_eq!(refusal.details, ["and more", "nh exited with status 3."]);
    let text = refusal.to_json();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value,
        json!({"error": "the build said no", "details": ["and more", "nh exited with status 3."]})
    );
    assert_eq!(Refusal::from_json(&text), Some(refusal));
    assert_eq!(Refusal::from_json("[]"), None);
    assert_eq!(Refusal::from_json("Welcome to the machine"), None);
    assert_eq!(Refusal::from_json(r#"{"error": "x"}"#), None);
}

/// The machines of a rollout have a state each, and the result counts them.
/// `error` and `details` appear for a machine that went wrong, `revision` and
/// `hostKey` for one that was installed.
#[test]
fn machines_shape() {
    let mut installed = Machine::new("node-1", State::Installed);
    installed.revision = Some("0123".to_string());
    installed.host_key = Some("ssh-ed25519 AAAA".to_string());
    let machines = [
        installed,
        Machine::new("node-2", State::Managed),
        Machine::went_wrong(
            "node-3",
            State::Failed,
            "The installation failed\nlast line",
        ),
        Machine::went_wrong("node-4", State::NotStarted, "it will be erased"),
        Machine::new("node-5", State::Upgraded),
    ];
    let value: Value = serde_json::from_str(&machines_json(false, &machines)).unwrap();
    assert_eq!(
        value["counts"],
        json!({"managed": 1, "installed": 1, "upgraded": 1, "failed": 1, "not started": 1})
    );
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["machines"],
        json!([
            {"name": "node-1", "state": "installed", "revision": "0123", "hostKey": "ssh-ed25519 AAAA"},
            {"name": "node-2", "state": "managed"},
            {"name": "node-3", "state": "failed", "error": "The installation failed", "details": ["last line"]},
            {"name": "node-4", "state": "not started", "error": "it will be erased", "details": []},
            {"name": "node-5", "state": "upgraded"},
        ])
    );
}

/// The installation is the host key, the system and the revision; one
/// without a host key is none.
#[test]
fn installation_shape() {
    let installation = Installation {
        host_key: "ssh-ed25519 AAAA".to_string(),
        system: Some("/nix/store/abc-nixos-system".to_string()),
        revision: None,
    };
    let text = installation.to_json();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value,
        json!({"hostKey": "ssh-ed25519 AAAA", "system": "/nix/store/abc-nixos-system", "revision": null})
    );
    assert_eq!(Installation::from_json(&text), Some(installation));
    assert_eq!(Installation::from_json(r#"{"hostKey": ""}"#), None);
    assert_eq!(Installation::from_json("Host key  ssh-ed25519 AAAA"), None);
}
