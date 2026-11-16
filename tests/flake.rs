//! Tests for src/flake.rs.

mod common;

use common::{fixture, hostname};
use nxf::flake::{self, InputRow, InputUpdate};
use serde_json::json;

fn names(inputs: &[&str]) -> Vec<String> {
    inputs.iter().map(|name| name.to_string()).collect()
}

/// Input rows carry revisions, sources and differences from another lock.
#[test]
fn input_rows() {
    let lock = json!({
        "root": "root",
        "version": 7,
        "nodes": {
            "root": {"inputs": {"gh": "gh", "path": "path", "same": "same", "follows": ["gh"]}},
            "gh": {"locked": {"rev": "0123456789abcdef"}, "original": {"type": "github", "owner": "o", "repo": "r"}},
            "path": {"locked": {"narHash": "sha256-abcdefghijkl"}, "original": {"type": "path", "path": "/src"}},
            "same": {"locked": {"rev": "ffffffffffffffff"}, "original": {"type": "git", "url": "https://x/y"}},
        }
    });
    let row = |name: &str, rev: &str, url: &str, change: Option<&str>| InputRow {
        name: name.to_string(),
        rev: rev.to_string(),
        url: url.to_string(),
        change: change.map(str::to_string),
    };
    let mut other = lock.clone();
    other["nodes"]["gh"]["locked"]["rev"] = json!("aaaaaaaaaaaaaaaa");
    assert_eq!(
        flake::input_rows(&lock, &[], Some(&other)),
        [
            row(
                "gh",
                "0123456789ab",
                "https://github.com/o/r",
                Some("→ aaaaaaaaaaaa in the checkout")
            ),
            row("path", "sha256-abcde", "/src", None),
            row("same", "ffffffffffff", "https://x/y", None),
            row("follows", "follows gh", "", None),
        ]
    );
    assert_eq!(
        flake::input_rows(&lock, &names(&["same"]), None),
        [row("same", "ffffffffffff", "https://x/y", None)]
    );
    assert!(flake::input_rows(&lock, &names(&["missing"]), None).is_empty());
    let mut removed = other.clone();
    removed["nodes"]["root"]["inputs"]
        .as_object_mut()
        .unwrap()
        .shift_remove("gh");
    assert_eq!(
        flake::input_rows(&lock, &names(&["gh"]), Some(&removed)),
        [row(
            "gh",
            "0123456789ab",
            "https://github.com/o/r",
            Some("not in the checkout")
        )]
    );
}

/// Update summaries compare full pins and include added and removed inputs.
#[test]
fn input_updates() {
    let before = json!({
        "root": "root",
        "nodes": {
            "root": {"inputs": {"a": "a", "removed": "removed", "follows": ["a"]}},
            "a": {"locked": {"rev": "0123456789ab1111"}},
            "removed": {"locked": {"rev": "eeeeeeeeeeeeeeee"}},
        }
    });
    let mut after = before.clone();
    after["nodes"]["a"]["locked"]["rev"] = json!("0123456789ab2222");
    after["nodes"]["root"]["inputs"]
        .as_object_mut()
        .unwrap()
        .shift_remove("removed");
    after["nodes"]["root"]["inputs"]["added"] = json!("added");
    after["nodes"]["added"] = json!({"locked": {"rev": "aaaaaaaaaaaaaaaa"}});
    let update = |name: &str, before: &str, after: &str, changed: bool| InputUpdate {
        name: name.to_string(),
        before: before.to_string(),
        after: after.to_string(),
        changed,
    };
    assert_eq!(
        flake::input_updates(&before, &after, &[]),
        [
            update("a", "0123456789ab", "0123456789ab", true),
            update("removed", "eeeeeeeeeeee", "removed", true),
            update("follows", "follows a", "follows a", false),
            update("added", "new", "aaaaaaaaaaaa", true),
        ]
    );
    let follows = flake::input_updates(&before, &after, &names(&["follows"]));
    assert_eq!(follows.len(), 1);
    assert!(!follows[0].changed);
}

/// The lockfile is read with its root inputs.
#[test]
fn lock() {
    let fx = fixture();
    let lock = flake::lock(&fx.checkout).unwrap();
    assert_eq!(flake::root_inputs(&lock), ["a", "b"]);
}

/// Update changes only the named inputs and does not commit the lockfile.
#[test]
fn update() {
    let fx = fixture();
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    fx.bump(&["a", "b"]);
    flake::update(&fx.checkout, &names(&["b"])).unwrap();
    assert_eq!(fx.locked("a"), a);
    assert_ne!(fx.locked("b"), b);
    assert_eq!(fx.porcelain(), "M flake.lock");
}

/// Configurations are the attribute names of nixosConfigurations.
#[test]
fn configurations() {
    let fx = fixture();
    assert_eq!(flake::configurations(&fx.checkout).unwrap(), [hostname()]);
}

/// The configuration is the one named default, then the only one.
#[test]
fn choose() {
    assert_eq!(flake::choose(&names(&["default"])).unwrap(), "default");
    assert_eq!(flake::choose(&names(&["node-7"])).unwrap(), "node-7");
    assert_eq!(
        flake::choose(&names(&["a", "default", "b"])).unwrap(),
        "default"
    );
    assert!(flake::choose(&names(&["a", "b"]))
        .unwrap_err()
        .contains("none is named default"));
    assert!(flake::choose(&[])
        .unwrap_err()
        .contains("no nixosConfigurations"));
}
