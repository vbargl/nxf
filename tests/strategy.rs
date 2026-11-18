//! Tests for src/strategy.rs: the strategies of the automatic update.

use nxf::strategy::{Strategies, Strategy};
use serde_json::json;

/// Every input is reported unless the configuration says otherwise.
#[test]
fn defaults() {
    for settings in [
        json!(null),
        json!({}),
        json!({"strategy": null, "inputs": {}}),
    ] {
        let strategies = Strategies::from_settings(&settings).unwrap();
        assert_eq!(strategies, Strategies::default());
        assert_eq!(strategies.of("nixpkgs"), Strategy::Report);
    }
}

/// An input has its own strategy, or the one for all inputs.
#[test]
fn of_an_input() {
    let strategies = Strategies::from_settings(&json!({
        "strategy": "flag",
        "inputs": {"work": "off", "nixpkgs": "report"},
    }))
    .unwrap();
    assert_eq!(strategies.of("work"), Strategy::Off);
    assert_eq!(strategies.of("nixpkgs"), Strategy::Report);
    assert_eq!(strategies.of("other"), Strategy::Flag);
}

/// A word that is no strategy, or no word, is an error that names it.
#[test]
fn invalid() {
    let error = Strategies::from_settings(&json!({"strategy": "never"})).unwrap_err();
    assert_eq!(
        error,
        "unknown update strategy 'never'; it is off, flag or report"
    );
    // The strategy before 0.11.0 is gone, not another name of `report`.
    let error = Strategies::from_settings(&json!({"inputs": {"work": "panic"}})).unwrap_err();
    assert_eq!(
        error,
        "unknown update strategy 'panic'; it is off, flag or report"
    );
    let error = Strategies::from_settings(&json!({"inputs": {"work": true}})).unwrap_err();
    assert_eq!(error, "the update strategy of input 'work' is not a word");
}
