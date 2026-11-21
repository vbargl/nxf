//! Tests for src/output.rs: live progress and terminal compatibility, through
//! the mock nh. Style tests are in tests/environment.rs.

mod common;

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use common::{assert_contains, assert_lacks, events, fake_systems, fixture, Fixture};
use nxf::output::{self, Tail, CLEARED};
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
        "sh exited with status 3.\ndiagnostic"
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
            "sh exited with status 1.\n{}",
            (13..=20)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    );
}

/// A failing tool's tail holds the last eight lines it said, however many it
/// said, so that the memory it takes does not grow with the output.
#[test]
fn tail_bound() {
    let mut tail = Tail::default();
    for number in 1..=10_000 {
        tail.push(&format!("line {number}"), None);
    }
    assert_eq!(
        tail.text(),
        (9_993..=10_000)
            .map(|number| format!("line {number}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The tail keeps the last eight lines that are not blank, however the tool
/// broke its lines: with carriage returns, escape sequences or JSON events.
#[test]
fn tail_lines() {
    let mut tail = Tail::default();
    for number in 1..=4 {
        tail.push(&format!("old {number}"), None);
    }
    tail.push("\u{1b}[31mred\u{1b}[0m", None);
    tail.push("", None);
    tail.push("   ", None);
    let log = r#"@nix {"action":"result","type":101,"id":1,"fields":["first\rsecond\n\nthird"]}"#;
    tail.push(log, nxf::progress::event(log).as_ref());
    let progress = r#"@nix {"action":"start","type":105,"id":1}"#;
    tail.push(progress, nxf::progress::event(progress).as_ref());
    tail.push("last", None);
    assert_eq!(
        tail.text(),
        "old 2\nold 3\nold 4\nred\nfirst\nsecond\nthird\nlast"
    );
}

/// The peak resident size of this process so far, in kB.
fn peak() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|kilobytes| kilobytes.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

/// What a failing tool said before its last lines does not stay in memory: a
/// tool with 40 MB of output grows the process by a few.
#[test]
fn stream_memory() {
    let before = peak();
    let mut loud = Command::new("sh");
    loud.args([
        "-c",
        "yes 'compiling a file with a warning that repeats itself in a long log' | head -n 500000; echo last >&2; exit 1",
    ]);
    let error = output::stream(&mut loud, true, &[]).unwrap_err();
    assert_contains(&error, "sh exited with status 1.\ncompiling");
    assert!(error.ends_with("last"), "{error}");
    let growth = peak().saturating_sub(before);
    assert!(growth < 16 * 1024, "the peak grew by {growth} kB");
}

/// A tool that never ends a line, or ends it with carriage returns only,
/// does not make the process hold its output: the peak grows by a few MB.
#[test]
fn stream_unbroken_memory() {
    for (script, last) in [
        ("head -c 100000000 /dev/zero | tr '\\0' x", "xxxx"),
        (
            "yes 'a line of some length' | head -n 2000000 | tr '\\n' '\\r'",
            "a line of some length",
        ),
    ] {
        let before = peak();
        let mut loud = Command::new("sh");
        loud.args(["-c", &format!("{script}; echo; echo end; exit 1")]);
        let error = output::stream(&mut loud, true, &[]).unwrap_err();
        assert!(error.starts_with("sh exited with status 1.\n"), "{error}");
        assert!(error.ends_with("end"), "{error}");
        assert_contains(&error, last);
        let growth = peak().saturating_sub(before);
        assert!(growth < 16 * 1024, "{script}: the peak grew by {growth} kB");
    }
}

/// An event of the tool that is longer than a piece is dropped from the tail,
/// whole: no fragment of its JSON is shown beside the lines around it.
#[test]
fn long_event() {
    let mut tool = Command::new("sh");
    tool.args([
        "-c",
        "echo before >&2; printf '@nix {\"action\":\"msg\",\"level\":0,\"msg\":\"%s\"}\\n' \"$(head -c 200000 /dev/zero | tr '\\0' x)\" >&2; echo after >&2; exit 1",
    ]);
    let error = output::stream(&mut tool, true, &[]).unwrap_err();
    assert_eq!(error, "sh exited with status 1.\nbefore\nafter");
}

/// A label column is as wide as the labels, and two more than the longest
/// name when that is wider.
#[test]
fn widened() {
    assert_eq!(output::widened([]), output::LABEL);
    assert_eq!(output::widened(["a", "node-1"]), output::LABEL);
    assert_eq!(output::widened(["a", "a-name-of-ten"]), 15);
    // Characters, not bytes.
    assert_eq!(output::widened(["ééééééééééé"]), 13);
}

/// A revision is shown by its first twelve characters, and a shorter one whole.
#[test]
fn revision() {
    assert_eq!(output::REVISION, 12);
    assert_eq!(output::revision("0123456789abcdef0123"), "0123456789ab");
    assert_eq!(output::revision("0123456789ab"), "0123456789ab");
    assert_eq!(output::revision("unknown"), "unknown");
    assert_eq!(output::revision("ééééééééééééé"), "éééééééééééé");
}

/// Machines are counted in the singular for one.
#[test]
fn machines() {
    assert_eq!(output::machines(0), "0 machines");
    assert_eq!(output::machines(1), "1 machine");
    assert_eq!(output::machines(3), "3 machines");
}

/// A fact with no value ends with its label, not with the padding after it.
#[test]
fn fact_without_value() {
    output::capture();
    output::fact_in(12, "name", "");
    output::fact("Flake", "value");
    let printed = output::release();
    assert_eq!(printed.stdout, "  name\n  Flake     value\n");
}

/// The descriptor whose width bounds the rows is standard output, or standard
/// error when the text of the run is diverted there. This pins the choice,
/// not the width that a terminal reports.
#[test]
fn rows_width() {
    assert_eq!(output::rows_descriptor(false), 1);
    assert_eq!(output::rows_descriptor(true), 2);
}
