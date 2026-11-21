//! Tests for src/cli.rs: command routing.

mod common;

use common::{assert_contains, fixture, outcomes, pair};

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
    // The names stand in the column of the labels.
    assert_contains(&result.stdout, "\n  switch    Activate the configuration");
    assert_contains(&result.stdout, "\n  upgrade   Update each input");
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

/// The commands are those that clap knows, in the order they are declared;
/// each is recognised as the first word, and the text of an unknown task
/// lists them.
#[test]
fn command_list() {
    let commands = nxf::cli::commands();
    for known in ["status", "rollout"] {
        assert!(commands.iter().any(|command| command == known), "{known}");
    }
    let fx = fixture();
    for command in &commands {
        let help = fx.run(&[command, "--help"]);
        help.ok();
        assert_contains(&help.stdout, &format!("Usage: nxf {command}"));
    }
    let unknown = fx.run(&["nosuch"]);
    assert_eq!(unknown.code, 1);
    assert_contains(
        &unknown.stderr,
        &format!("  Commands  {}\n", commands.join(", ")),
    );
}

/// What is refused under --json is also one JSON object on standard output:
/// the cause and the lines beneath it. The text stays on standard error, and
/// the exit status is that of the refusal.
#[test]
fn json_refusals() {
    let fx = fixture();
    let refused = |args: &[&str], code: i32| {
        let result = fx.run(args);
        assert_eq!(result.code, code, "{args:?}");
        assert_contains(&result.stderr, "✗ ");
        serde_json::from_str::<serde_json::Value>(&result.stdout).unwrap()
    };
    let unknown = refused(&["--json", "build", "nosuch"], 1);
    assert_eq!(unknown["error"], "Unknown task or input: nosuch");
    let beneath: Vec<&str> = unknown["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect();
    assert!(beneath[0].starts_with("Tasks"), "{beneath:?}");
    assert!(beneath[1].starts_with("Inputs"), "{beneath:?}");
    assert!(beneath[2].starts_with("Commands"), "{beneath:?}");

    let parser = refused(&["--json", "build", "--frob"], 2);
    assert_eq!(parser["error"], "unexpected argument '--frob' found");

    let input = refused(&["status", "--json", "nosuch"], 1);
    assert!(input["error"]
        .as_str()
        .unwrap()
        .starts_with("Unknown input: nosuch."));
    assert_eq!(input["details"], serde_json::json!([]));
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

/// What a command makes is writable by the group when the group may write the
/// checkout, and the command works on that checkout: the update does, and so
/// does `init` of the directory nxf would use. A hold, which only writes the
/// state of the updater, `release`, and `init` of another directory do not.
#[test]
fn group_sharing() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    let mut fx = fixture();
    std::fs::set_permissions(&fx.checkout, std::fs::Permissions::from_mode(0o2775)).unwrap();
    let run = |fx: &common::Fixture, args: &[&str]| {
        let mut command = fx.command(args);
        // The mask of the caller decides what nxf does not.
        unsafe {
            command.pre_exec(|| {
                libc::umask(0o022);
                Ok(())
            })
        };
        assert!(command.output().unwrap().status.success());
    };
    let group_writes =
        |file: &std::path::Path| std::fs::metadata(file).unwrap().permissions().mode() & 0o020 != 0;
    run(&fx, &["hold", "--reason", "quiet"]);
    assert!(!group_writes(&fx.updater.join("hold.json")));
    run(&fx, &["auto-update"]);
    assert!(group_writes(&fx.updater.join("update.json")));

    // A template that needs nothing from the network.
    let template = fx.dir.join("template");
    std::fs::create_dir_all(template.join("machine")).unwrap();
    std::fs::write(
        template.join("flake.nix"),
        "{ outputs = _: { templates.machine.path = ./machine; }; }\n",
    )
    .unwrap();
    std::fs::write(
        template.join("machine/flake.nix"),
        "{ outputs = _: { nixosConfigurations.default = { options.services = { }; }; }; }\n",
    )
    .unwrap();
    let reference = format!("{}#machine", template.display());
    let base = fx.dir.clone();
    let empty = |name: &str| {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o2775)).unwrap();
        dir
    };
    // The directory nxf would use, named or not.
    let own = empty("own");
    fx.set("NXF_FLAKE", &own);
    run(&fx, &["init", "--template", &reference]);
    assert!(group_writes(&own.join("nxf.nix")));
    let named = empty("named");
    fx.set("NXF_FLAKE", &named);
    run(
        &fx,
        &["init", named.to_str().unwrap(), "--template", &reference],
    );
    assert!(group_writes(&named.join("nxf.nix")));
    // Another directory than the one nxf would use.
    let other = empty("other");
    run(
        &fx,
        &["init", other.to_str().unwrap(), "--template", &reference],
    );
    assert!(!group_writes(&other.join("nxf.nix")));

    // `release` on machines does not share either: what it starts has the mask of the caller.
    let log = fx.dir.join("mask.log");
    common::write_script(
        &fx.dir.join("bin/ssh"),
        &format!("#!/bin/sh\numask >> '{}'\n", log.display()),
    );
    let hosts = fx.dir.join("hosts.toml");
    std::fs::write(&hosts, "[hosts.node.ssh]\naddress = \"node.test\"\n").unwrap();
    fx.set("NXF_FLAKE", fx.checkout.clone());
    run(&fx, &["release", "--config", hosts.to_str().unwrap()]);
    assert_eq!(std::fs::read_to_string(log).unwrap(), "0022\n");
}
