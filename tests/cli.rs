//! Tests for src/cli.rs: command routing.

mod common;

use common::fixture;

/// Version prints the tool version.
#[test]
fn version() {
    let result = fixture().run(&["version"]);
    result.ok();
    assert_eq!(
        result.stdout.trim(),
        format!("nxf {}", env!("CARGO_PKG_VERSION"))
    );
}

/// Bare nxf prints help and lists the tasks with their descriptions.
#[test]
fn help() {
    let result = fixture().run(&[]);
    result.ok();
    assert!(result.stdout.contains("Usage:"));
    assert!(result.stdout.contains("Tasks:"));
    assert!(result
        .stdout
        .lines()
        .any(|line| line.starts_with("  switch") && line.contains(" Activate")));
    assert!(result.stdout.contains("auto-update"));
}

/// Commands reject extra arguments instead of treating them as tasks.
#[test]
fn commands_reject_tasks() {
    let fx = fixture();
    for args in [
        &["version", "update"][..],
        &["status", "--detailed", "update"],
        &["doctor", "update"],
    ] {
        assert_ne!(fx.run(args).code, 0, "nxf {} succeeded", args.join(" "));
    }
}
