//! Tests for src/cli.rs: command routing.

mod common;

use common::{assert_contains, fixture, outcomes, pair, write_script};

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

/// Commands refuse an extra word instead of running it as a task: each fails
/// with the cause of its refusal, and nh is not called.
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
        (
            &["init", "machine", "more", "--template", "x#y"],
            "unexpected argument 'more'",
        ),
        (
            &["install", "machine", "more"],
            "unexpected argument 'more'",
        ),
        (
            &[
                "rollout",
                "--config",
                "hosts.toml",
                "--template",
                "x#y",
                "more",
            ],
            "✗ Inputs and --batch belong to the upgrade",
        ),
        (&["auto-update", "more"], "unexpected argument 'more'"),
    ] {
        let result = fx.run(args);
        assert_ne!(result.code, 0, "nxf {} succeeded", args.join(" "));
        assert_contains(&result.stderr, cause);
    }
    assert!(fx.nh_calls().is_empty());
}

/// `auto-update --state` saves the results in the directory it names, not
/// in the updater's own.
#[test]
fn auto_update_state() {
    let fx = fixture();
    // A held run is decided at once, without Nix.
    fx.run(&["hold", "--reason", "quiet"]).ok();
    let state = fx.dir.join("elsewhere");
    fx.run(&["auto-update", "--state", state.to_str().unwrap()])
        .ok();
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(state.join("update.json")).unwrap()).unwrap();
    assert_eq!(outcomes(&report), [pair("update", "skipped")]);
    assert!(!fx.updater.join("update.json").exists());
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
