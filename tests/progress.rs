//! Tests for Nix internal-json progress, based on Nix's activity types.

mod common;

use common::names;
use nxf::progress::{advance, diagnostic, event, initial, snapshot, State};
use serde_json::{json, Value};

fn after(state: &State, events: &[Value]) -> State {
    let mut state = state.clone();
    for event in events {
        advance(&mut state, Some(event));
    }
    state
}

/// What a failed tool's line says.
fn said(line: &str) -> String {
    diagnostic(line, event(line).as_ref()).into_owned()
}

/// Build counts come from aggregate progress; stopping an activity is not success.
#[test]
fn counts() {
    let start = initial(&[]);
    assert!(snapshot(&start, false).is_empty());
    let building = after(
        &start,
        &[
            json!({"action": "start", "type": 104, "id": 10}),
            json!({"action": "result", "type": 105, "id": 10, "fields": [3, 8, 1, 0]}),
            json!({"action": "start", "type": 105, "id": 11, "fields": ["/nix/store/00000000000000000000000000000000-example-package.drv", "", 1, 1]}),
        ],
    );
    assert_eq!(
        snapshot(&building, false),
        ["3 of 8 derivations built", "Building example-package"]
    );
    let stopped = after(&building, &[json!({"action": "stop", "id": 11})]);
    assert_eq!(snapshot(&stopped, false), ["3 of 8 derivations built"]);
    let finished = after(&stopped, &[json!({"action": "stop", "id": 10})]);
    assert_eq!(snapshot(&finished, false), ["3 derivations built"]);
}

/// Counters from distinct workers aggregate without confusing downloads with builds.
#[test]
fn workers() {
    let state = after(
        &initial(&[]),
        &[
            json!({"action": "start", "type": 104, "id": 1}),
            json!({"action": "result", "type": 105, "id": 1, "fields": [1, 3, 1, 0]}),
            json!({"action": "start", "type": 103, "id": 2}),
            json!({"action": "result", "type": 105, "id": 2, "fields": [50, 100, 1, 0]}),
            json!({"action": "start", "type": 104, "id": 3}),
            json!({"action": "result", "type": 105, "id": 3, "fields": [2, 5, 1, 0]}),
        ],
    );
    assert_eq!(snapshot(&state, false), ["3 of 8 derivations built"]);
}

/// Builder output belongs to its activity, including interleaved parallel builds.
#[test]
fn builder_lines() {
    let first = after(
        &initial(&names(&["personal"])),
        &[
            json!({"action": "msg", "msg": "computing input 'personal'"}),
            json!({"action": "start", "type": 105, "id": 1, "fields": ["snx-rs.drv"]}),
            json!({"action": "result", "type": 101, "id": 1, "fields": ["old\r\u{1b}[31mcompiling\u{1b}[0m\t\u{7}"]}),
        ],
    );
    assert_eq!(snapshot(&first, true), ["Building snx-rs • compiling"]);
    assert_eq!(snapshot(&first, false), ["Building snx-rs"]);
    let second = after(
        &first,
        &[json!({"action": "start", "type": 105, "id": 2, "fields": ["bramble.drv"]})],
    );
    assert_eq!(snapshot(&second, true), ["Building bramble and 1 more"]);
    let first_log = after(
        &second,
        &[json!({"action": "result", "type": 101, "id": 1, "fields": ["linking snx-rs"]})],
    );
    assert_eq!(
        snapshot(&first_log, true),
        ["Building snx-rs and 1 more • linking snx-rs"]
    );
    let second_log = after(
        &first_log,
        &[
            json!({"action": "result", "type": 107, "id": 2, "fields": ["first line\npacking bramble"]}),
        ],
    );
    assert_eq!(
        snapshot(&second_log, true),
        ["Building bramble and 1 more • packing bramble"]
    );
    for ignored in [
        json!({"action": "result", "type": 101, "id": 99, "fields": ["unrelated log"]}),
        json!({"action": "result", "type": 101, "id": 2, "fields": [42]}),
        json!({"action": "result", "type": 101, "id": 2, "fields": [""]}),
    ] {
        assert_eq!(after(&second_log, &[ignored]), second_log);
    }
    let stopped = after(&second_log, &[json!({"action": "stop", "id": 2})]);
    assert_eq!(
        snapshot(&stopped, true),
        ["Building snx-rs • linking snx-rs"]
    );
    let idle = after(&stopped, &[json!({"action": "stop", "id": 1})]);
    assert_eq!(snapshot(&idle, true), ["Checking input personal"]);
}

/// Unknown, malformed and raw terminal output cannot invent progress or break it.
#[test]
fn ignored() {
    let state = initial(&[]);
    for line in [
        "remote: Counting objects: 50%",
        "@nix not-json",
        "@nix []",
        "@nix {}",
    ] {
        let mut next = state.clone();
        assert!(!advance(&mut next, event(line).as_ref()), "{line}");
        assert_eq!(next, state, "{line}");
    }
    for odd in [
        json!({"action": "start", "id": 1, "type": 999}),
        json!({"action": "start", "id": {}, "type": 104}),
        json!({"action": "start", "id": 1, "type": 105, "fields": "oops"}),
    ] {
        assert_eq!(after(&state, &[odd]), state);
    }
}

/// Failure diagnostics decode JSON errors and build logs, hiding protocol records.
#[test]
fn diagnostics() {
    assert_eq!(
        said(r#"@nix {"action":"msg","level":0,"msg":"error: build failed"}"#),
        "error: build failed"
    );
    assert_eq!(
        said(r#"@nix {"action":"result","type":101,"id":1,"fields":["missing header"]}"#),
        "missing header"
    );
    assert_eq!(said(r#"@nix {"action":"start","type":105,"id":1}"#), "");
    assert_eq!(said("@nix malformed"), "");
    assert_eq!(
        said(r#"@nix {"action":"msg","level":6,"msg":"debug internals"}"#),
        ""
    );
    assert_eq!(said("plain tool output"), "plain tool output");
}

/// What shows at once is reported as a change, and what waits for the
/// throttle, the counts and the text of a log line, is not.
#[test]
fn changes() {
    let mut state = initial(&[]);
    let mut step = |event: Value| advance(&mut state, Some(&event));
    assert!(!step(json!({"action": "start", "type": 104, "id": 1})));
    assert!(!step(
        json!({"action": "result", "type": 105, "id": 1, "fields": [1, 4]})
    ));
    assert!(step(
        json!({"action": "start", "type": 105, "id": 2, "fields": ["a.drv"]})
    ));
    assert!(!step(
        json!({"action": "start", "type": 105, "id": 2, "fields": ["a.drv"]})
    ));
    assert!(step(
        json!({"action": "result", "type": 101, "id": 2, "fields": ["one"]})
    ));
    assert!(!step(
        json!({"action": "result", "type": 101, "id": 2, "fields": ["two"]})
    ));
    assert!(!step(
        json!({"action": "result", "type": 101, "id": 9, "fields": ["unknown"]})
    ));
    assert!(step(
        json!({"action": "start", "type": 105, "id": 3, "fields": ["b.drv"]})
    ));
    assert!(step(
        json!({"action": "result", "type": 101, "id": 2, "fields": ["three"]})
    ));
    assert!(step(json!({"action": "stop", "id": 2})));
    assert!(!step(json!({"action": "stop", "id": 2})));
    assert!(step(json!({"action": "stop", "id": 3})));
    assert!(!step(json!({"action": "stop", "id": 1})));
    assert!(step(json!({"action": "msg", "msg": "computing input 'x'"})));
    assert!(!step(
        json!({"action": "msg", "msg": "computing input 'x'"})
    ));
}

/// A long stream of the same events ends with the same counts and builds as a
/// short one: the state is updated in place, and nothing is lost on the way.
#[test]
fn long_stream() {
    let real = [
        json!({"action": "start", "type": 104, "id": 1}),
        json!({"action": "result", "type": 105, "id": 1, "fields": [0, 3]}),
        json!({"action": "start", "type": 105, "id": 2, "fields": ["/nix/store/00000000000000000000000000000000-first.drv"]}),
        json!({"action": "start", "type": 105, "id": 3, "fields": ["/nix/store/00000000000000000000000000000000-second.drv"]}),
        json!({"action": "stop", "id": 2}),
        json!({"action": "result", "type": 105, "id": 1, "fields": [1, 3]}),
    ];
    let noise = |count: usize| -> Vec<Value> {
        (0..count)
            .flat_map(|n| {
                [
                    json!({"action": "result", "type": 101, "id": 3, "fields": [format!("line {n}")]}),
                    json!({"action": "result", "type": 101, "id": 7, "fields": ["unknown build"]}),
                    json!({"action": "msg", "level": 3, "msg": "copying path"}),
                ]
            })
            .collect()
    };
    let play = |count: usize| {
        let mut state = initial(&[]);
        for event in &real {
            advance(&mut state, Some(event));
            // Nothing is built between the first events: the log lines have no build yet.
            for noise in noise(count) {
                advance(&mut state, Some(&noise));
            }
        }
        state
    };
    let (short, long) = (play(1), play(20_000));
    assert_eq!(
        snapshot(&short, false),
        ["1 of 3 derivations built", "Building second"]
    );
    assert_eq!(snapshot(&long, false), snapshot(&short, false));
    assert_eq!(long.builds, short.builds);
    assert_eq!(long.active, short.active);
}

/// Root input identity is available before fetching; nested and pinned nodes stay quiet.
#[test]
fn inputs() {
    let message = |name: &str| json!({"action": "msg", "level": 6, "msg": format!("computing input '{name}'")});
    let personal = after(
        &initial(&names(&["personal", "work"])),
        &[message("personal")],
    );
    assert_eq!(snapshot(&personal, false), ["Checking input personal"]);
    let nested = after(&personal, &[message("personal/nixpkgs")]);
    assert_eq!(snapshot(&nested, false), ["Checking input personal"]);
    let pinned = after(&nested, &[message("unselected")]);
    assert_eq!(snapshot(&pinned, false), ["Checking input personal"]);
    let next = after(&pinned, &[message("work")]);
    assert_eq!(snapshot(&next, false), ["Checking input work"]);
}
