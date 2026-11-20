//! Tests for src/cli.rs: command routing.

mod common;

use common::{assert_contains, fixture};

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
    assert_contains(&result.stdout, "Usage:");
    assert_contains(&result.stdout, "Tasks:");
    assert!(result
        .stdout
        .lines()
        .any(|line| line.starts_with("  switch") && line.contains(" Activate")));
    assert_contains(&result.stdout, "auto-update");
}

/// Commands reject extra arguments instead of treating them as tasks, and
/// say which word they do not take.
#[test]
fn commands_reject_tasks() {
    let fx = fixture();
    for (args, cause) in [
        (&["version", "update"][..], "unexpected argument 'update'"),
        (
            &["status", "--detailed", "update"],
            "✗ Unknown input: update. Inputs are a, b.",
        ),
        (&["doctor", "update"], "unexpected argument 'update'"),
    ] {
        let result = fx.run(args);
        assert_ne!(result.code, 0, "nxf {} succeeded", args.join(" "));
        assert_contains(&result.stderr, cause);
    }
    assert!(fx.nh_calls().is_empty());
}
