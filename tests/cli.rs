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

/// The example in the help shows what the help claims for it: the words
/// after `update` are inputs to update, and none of them is a task that
/// would run as well.
#[test]
fn help_example_names_only_inputs() {
    let result = fixture().run(&["--help"]);
    result.ok();
    let (_, example) = result.stdout.split_once("nxf update ").unwrap();
    let inputs: Vec<&str> = example.lines().next().unwrap().split_whitespace().collect();
    assert!(!inputs.is_empty());
    for task in nxf::tasks::defined() {
        assert!(!inputs.contains(&task.name), "{} is a task", task.name);
    }
}

/// What the command line refuses has the shape of every failure, for tasks
/// and for commands alike: the cause as the outcome and what helps beneath
/// it, with the exit status of a wrong call. Help that is asked for is none.
#[test]
fn refusals_have_the_shape_of_failures() {
    let fx = fixture();
    let task = fx.run(&["build", "--frob"]);
    assert_eq!(task.code, 2);
    assert!(
        task.stderr
            .starts_with("\n✗ unexpected argument '--frob' found\n  tip: "),
        "{}",
        task.stderr
    );
    assert!(task.stderr.contains("\n  Usage: nxf "), "{}", task.stderr);

    let command = fx.run(&["rollout"]);
    assert_eq!(command.code, 2);
    assert!(
        command.stderr.starts_with(
            "\n✗ the following required arguments were not provided:\n  --config <FILE>\n"
        ),
        "{}",
        command.stderr
    );
    assert!(
        command.stderr.contains("\n  Usage: nxf rollout "),
        "{}",
        command.stderr
    );

    let help = fx.run(&["status", "--help"]);
    help.ok();
    assert!(help.stdout.contains("Usage: nxf status"), "{}", help.stdout);
    assert_eq!(help.stderr, "");
}
