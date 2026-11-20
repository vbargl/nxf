//! Tests for src/output.rs: live progress and terminal compatibility, through
//! the mock nh. Style tests are in tests/environment.rs.

mod common;

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use common::{assert_contains, assert_lacks, events, fake_systems, fixture, Fixture};
use nxf::output::{self, CLEARED};
use nxf::text::strip_ansi;
use serde_json::{json, Value};
use unicode_width::UnicodeWidthStr;

/// A fixture whose mock nh prints the given events.
fn building(records: &[Value]) -> Fixture {
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.env
        .insert("NXF_TEST_NH_STDOUT".to_string(), events(records));
    fx
}

/// The same in a terminal: styling and live progress are possible.
fn in_terminal(records: &[Value]) -> Fixture {
    let mut fx = building(records);
    fx.unset("NO_COLOR");
    fx.set("TERM", "xterm");
    fx
}

/// The final screen of a terminal session, without trailing blank rows.
fn screen(output: &str, width: u16) -> Vec<String> {
    let mut parser = vt100::Parser::new(200, width, 0);
    parser.process(output.as_bytes());
    let mut rows: Vec<String> = parser
        .screen()
        .rows(0, width)
        .map(|row| row.trim_end().to_string())
        .collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

/// Asserts that the target is printed while nh is still running.
fn assert_live(fx: &Fixture, terminal: bool, target: &str) {
    let release = fx.dir.join("release");
    let mut command = if terminal {
        fx.terminal_command(80, &["build"])
    } else {
        fx.command(&["build"])
    };
    let mut child = command
        .env("NXF_TEST_NH_WAIT", &release)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let (sender, receiver) = mpsc::channel();
    let wanted = target.to_string();
    let reader = std::thread::spawn(move || {
        let mut seen = String::new();
        let mut line = Vec::new();
        // Live rows end with a newline, so reading by line does not hold them back.
        while stdout
            .read_until(b'\n', &mut line)
            .is_ok_and(|read| read > 0)
        {
            seen.push_str(&String::from_utf8_lossy(&line));
            line.clear();
            if strip_ansi(&seen).contains(&wanted) {
                let _ = sender.send(());
            }
        }
        let mut rest = String::new();
        let _ = stdout.read_to_string(&mut rest);
        seen + &rest
    });
    // Less than the 30 s the mock nh is held for: what is seen was printed
    // while nh ran. A passing run does not wait, it goes on at the first row.
    let visible = receiver.recv_timeout(Duration::from_secs(20)).is_ok();
    std::fs::write(&release, "").unwrap();
    let status = child.wait().unwrap();
    let seen = reader.join().unwrap();
    assert!(status.success(), "{seen}");
    assert!(
        visible,
        "Progress was buffered until the child exited:\n{seen}"
    );
}

/// Progress must be visible before a running child is allowed to finish.
#[test]
fn live_progress() {
    let fx = building(&[
        json!({"action": "start", "type": 105, "id": 1, "fields": ["/nix/store/00000000000000000000000000000000-first-package.drv", "", 1, 1]}),
        json!({"action": "stop", "id": 1}),
        json!({"action": "start", "type": 105, "id": 2, "fields": ["/nix/store/00000000000000000000000000000000-second-package.drv", "", 1, 1]}),
    ]);
    assert_live(&fx, false, "Building second-package");
}

/// The first compiler line must appear while its builder is still running.
#[test]
fn live_builder_line() {
    let fx = in_terminal(&[
        json!({"action": "start", "type": 105, "id": 1, "fields": ["snx-rs.drv"]}),
        json!({"action": "result", "type": 101, "id": 1, "fields": ["compiling snx-rs"]}),
    ]);
    assert_live(&fx, true, "Building snx-rs • compiling snx-rs");
}

/// A real terminal reuses two rows, clears them, and leaves later output below.
#[test]
fn updating_lines() {
    let fx = in_terminal(&[
        json!({"action": "start", "type": 104, "id": 10}),
        json!({"action": "result", "type": 105, "id": 10, "fields": [0, 8, 1, 0]}),
        json!({"action": "start", "type": 105, "id": 1, "fields": ["snx-rs.drv"]}),
        json!({"action": "result", "type": 101, "id": 1, "fields": ["compiling snx-rs"]}),
        json!({"action": "result", "type": 105, "id": 10, "fields": [1, 8, 1, 0]}),
        json!({"action": "stop", "id": 1}),
        json!({"action": "start", "type": 105, "id": 2, "fields": ["bramble.drv"]}),
        json!({"action": "result", "type": 101, "id": 2, "fields": ["packing bramble"]}),
        json!({"action": "stop", "id": 2}),
        json!({"action": "result", "type": 105, "id": 10, "fields": [2, 8, 0, 0]}),
        json!({"action": "stop", "id": 10}),
    ]);
    let result = fx.terminal(80, &["build"]);
    result.ok();
    assert_contains(&result.stdout, "\u{1b}[2A");
    assert_contains(&result.stdout, "\r\u{1b}[2K");
    let plain = strip_ansi(&result.stdout);
    assert_contains(&plain, "Building snx-rs • compiling snx-rs");
    assert_contains(&plain, "Building bramble • packing bramble");
    assert_eq!(result.stdout.matches(CLEARED).count(), 1);
    // Nothing of the live rows is left; the permanent output follows the heading.
    let rows = screen(&result.stdout, 80);
    assert!(!rows.iter().any(|row| row.contains("• ")), "{rows:#?}");
    assert!(
        !rows.iter().any(|row| row.contains("derivations")),
        "{rows:#?}"
    );
    let heading = rows
        .iter()
        .position(|row| row == "Building configuration")
        .expect("the phase heading stays");
    assert_eq!(rows[heading + 1], "  ✓ Built", "{rows:#?}");
    assert!(
        rows[heading + 2].starts_with("    System    /nix/store/"),
        "{rows:#?}"
    );
    assert_contains(rows.last().unwrap(), "The running system was not switched.");
}

/// Every child releases its temporary rows before another child or a summary prints.
#[test]
fn consecutive_streams() {
    let fx = in_terminal(&[
        json!({"action": "start", "type": 105, "id": 1, "fields": ["bramble.drv"]}),
        json!({"action": "result", "type": 101, "id": 1, "fields": ["compiler output"]}),
    ]);
    let result = fx.terminal(80, &["switch"]);
    result.ok();
    assert_eq!(result.stdout.matches(CLEARED).count(), 2);
    let rows = screen(&result.stdout, 80);
    assert!(
        !rows.iter().any(|row| row.contains("compiler output")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().any(|row| row == "✓ Configuration activated"),
        "{rows:#?}"
    );
}

/// Cell-width clipping handles wide Unicode without wrapping or leaking controls.
#[test]
fn terminal_width() {
    let fx = in_terminal(&[
        json!({"action": "start", "type": 105, "id": 1, "fields": ["bramble.drv"]}),
        json!({"action": "result", "type": 101, "id": 1, "fields": ["\u{1b}[31m编译🦀👩‍💻é very long compiler output that cannot fit on the terminal\u{1b}[0m"]}),
    ]);
    let result = fx.terminal(32, &["build"]);
    result.ok();
    let plain = strip_ansi(&result.stdout);
    let live: Vec<&str> = plain
        .split(['\n', '\r'])
        .filter(|line| line.contains("Building bramble"))
        .collect();
    assert!(!live.is_empty(), "{plain}");
    assert!(live.iter().any(|line| line.contains('…')), "{live:#?}");
    assert!(live.iter().all(|line| line.width() < 32), "{live:#?}");
    let tiny = fx.terminal(2, &["build"]);
    tiny.ok();
    assert_contains(&tiny.stdout, "…");
}

/// Even forced color must not enable cursor movement in plain-output modes.
#[test]
fn plain_progress() {
    for (name, value) in [("NO_COLOR", ""), ("CLICOLOR", "0"), ("TERM", "dumb")] {
        let mut fx = in_terminal(&[
            json!({"action": "start", "type": 105, "id": 1, "fields": ["bramble.drv"]}),
            json!({"action": "result", "type": 101, "id": 1, "fields": ["hidden compiler output"]}),
        ]);
        fx.set("CLICOLOR_FORCE", "1");
        fx.set(name, value);
        let result = fx.terminal(80, &["build"]);
        result.ok();
        assert_lacks(&result.stdout, "\u{1b}[2A");
        assert_lacks(&result.stdout, "hidden compiler output");
        assert_contains(&result.stdout, "Building bramble");
    }
}

/// Streaming diagnostics must never turn a failing subprocess into success.
#[test]
fn stream_failure() {
    let mut failing = Command::new("sh");
    failing.args(["-c", "echo diagnostic >&2; exit 3"]);
    assert_eq!(
        output::stream(&mut failing, true, &[]).unwrap_err(),
        "Command exited with status 3.\ndiagnostic"
    );
    let mut missing = Command::new("/nonexistent/tool");
    assert_contains(
        &output::stream(&mut missing, true, &[]).unwrap_err(),
        "cannot run /nonexistent/tool",
    );
}

/// Only the last eight diagnostic lines of a failing tool are kept.
#[test]
fn stream_failure_tail() {
    let mut failing = Command::new("sh");
    failing.args(["-c", "seq 1 20; exit 1"]);
    let error = output::stream(&mut failing, true, &[]).unwrap_err();
    assert_eq!(
        error,
        format!(
            "Command exited with status 1.\n{}",
            (13..=20)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    );
}
