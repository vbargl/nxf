//! Tests for src/results.rs: the JSON shapes nxf writes and reads.

use nxf::results::{outcomes_json, Kind, Outcome, Refusal, Saved};
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
