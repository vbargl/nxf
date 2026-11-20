//! Tests for src/process.rs: child processes, their failures and the atomic
//! write of a file.

mod common;

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use common::{assert_contains, assert_lacks, temp_dir, write_script};
use nxf::process;

fn sh(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", script]);
    command
}

/// A program that is asked for input gets none.
#[test]
fn no_input() {
    for run in [process::capture, process::run] {
        let output = run(&mut sh("readlink /proc/self/fd/0")).unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "/dev/null\n");
    }
}

/// A failure names the program and its status first, and then what it said:
/// its output and its errors, the last eight lines.
#[test]
fn failure() {
    let error = process::run(&mut sh("echo out; echo error >&2; exit 3")).unwrap_err();
    assert_eq!(error, "sh exited with status 3.\nout\nerror");
    let error = process::run(&mut sh("seq 1 20; exit 1")).unwrap_err();
    assert_eq!(
        error,
        format!("sh exited with status 1.\n{}", process::tail(&seq(13, 20)))
    );
    assert_eq!(
        process::run(&mut sh("exit 4")).unwrap_err(),
        "sh exited with status 4."
    );
    assert_eq!(
        process::run(&mut sh("kill -9 $$")).unwrap_err(),
        "sh was stopped by a signal."
    );
}

fn seq(first: u32, last: u32) -> String {
    (first..=last)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The program of a failure has no directory, however it was run.
#[test]
fn failure_name() {
    let dir = temp_dir("process");
    write_script(&dir.join("tool"), "#!/bin/sh\nexit 2\n");
    let error = process::run(&mut Command::new(dir.join("tool"))).unwrap_err();
    assert_eq!(error, "tool exited with status 2.");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A program that cannot be run says which one.
#[test]
fn missing() {
    for error in [
        process::run(&mut Command::new("/nonexistent/tool")).unwrap_err(),
        process::capture(&mut Command::new("/nonexistent/tool")).unwrap_err(),
    ] {
        assert_contains(&error, "cannot run /nonexistent/tool: ");
    }
}

/// Capturing is not judging: the status of the program is the caller's.
#[test]
fn captured_status() {
    let output = process::capture(&mut sh("echo said; exit 5")).unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "said\n");
}

/// The first line of a failure is told from what the program said.
#[test]
fn failure_line() {
    assert!(process::is_failure_line("nh exited with status 3."));
    assert!(process::is_failure_line("nh was stopped by a signal."));
    assert!(!process::is_failure_line("error: build failed"));
}

/// The tail keeps the last eight lines that are not empty.
#[test]
fn tail() {
    assert_eq!(process::tail(&format!("{}\n\n", seq(1, 12))), seq(5, 12));
    assert_eq!(process::tail("\n \n"), "");
}

/// A program that ends in time has its output, each stream on its own or
/// both in the order they were written.
#[test]
fn within() {
    let in_time = || Instant::now() + Duration::from_secs(30);
    let script = "echo one; echo two >&2; echo three";
    let done = process::run_within(&mut sh(script), in_time(), false).unwrap();
    assert!(done.status.unwrap().success());
    assert_eq!(
        (done.stdout.as_str(), done.stderr.as_str()),
        ("one\nthree\n", "two\n")
    );
    let done = process::run_within(&mut sh(script), in_time(), true).unwrap();
    assert_eq!(
        (done.stdout.as_str(), done.stderr.as_str()),
        ("one\ntwo\nthree\n", "")
    );
}

/// A program that does not end in time is killed at the deadline, also when
/// a child of it holds its output open.
#[test]
fn deadline() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(300);
    let done = process::run_within(&mut sh("sleep 30 & sleep 30"), deadline, false).unwrap();
    assert!(done.status.is_none());
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// A program that ended is not waited for longer than a moment, when a child
/// of it holds its output open; what it said is kept.
#[test]
fn lingering_child() {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(30);
    let done = process::run_within(&mut sh("sleep 30 & echo said"), deadline, false).unwrap();
    assert!(done.status.unwrap().success());
    assert_eq!(done.stdout, "said\n");
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// A script passes, or says why it did not: its last line, else its status;
/// one that the deadline cut short says nothing, whatever it printed.
#[test]
fn script() {
    let dir = temp_dir("process");
    let at = |seconds| Instant::now() + Duration::from_secs(seconds);
    let make = |name: &str, body: &str| {
        let path = dir.join(name);
        write_script(&path, &format!("#!/bin/sh\n{body}\n"));
        path
    };
    assert_eq!(process::script(&make("ok", "exit 0"), at(30), &[]), Ok(()));
    assert_eq!(
        process::script(
            &make("says", "echo first; echo 'the last words' >&2; exit 1"),
            at(30),
            &[]
        ),
        Err(Some("the last words".to_string()))
    );
    assert_eq!(
        process::script(&make("silent", "exit 7"), at(30), &[]),
        Err(Some("exit status 7".to_string()))
    );
    assert_eq!(
        process::script(&make("killed", "kill -9 $$"), at(30), &[]),
        Err(Some("killed by a signal".to_string()))
    );
    assert_eq!(
        process::script(&make("slow", "echo timed out; exec sleep 30"), at(0), &[]),
        Err(None)
    );
    let seen = dir.join("seen");
    let body = format!("printf %s \"$VARIABLE\" > '{}'", seen.display());
    let variables: [(&str, &Path); 1] = [("VARIABLE", Path::new("/value"))];
    assert_eq!(
        process::script(&make("vars", &body), at(30), &variables),
        Ok(())
    );
    assert_eq!(std::fs::read_to_string(seen).unwrap(), "/value");
    assert!(process::script(&dir.join("absent"), at(30), &[])
        .unwrap_err()
        .unwrap()
        .starts_with("cannot run "));
    std::fs::remove_dir_all(dir).unwrap();
}

/// A file is written whole, in a directory that is made when it is not
/// there, and no temporary file is left, whether the write worked or not.
#[test]
fn atomic() {
    let dir = temp_dir("process");
    let file = dir.join("deeper/file.json");
    process::write_atomic(&file, "one").unwrap();
    process::write_atomic(&file, "two").unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
    let names = |dir: &Path| -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(names(&dir.join("deeper")), ["file.json"]);

    // A directory where the file belongs: the replacement fails.
    let blocked = dir.join("deeper/blocked");
    std::fs::create_dir(&blocked).unwrap();
    assert!(process::write_atomic(&blocked, "text").is_err());
    let left = names(&dir.join("deeper"));
    assert_lacks(&left.join(" "), ".tmp");
    std::fs::remove_dir_all(dir).unwrap();
}
