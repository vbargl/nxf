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

/// nxf enables flakes for the tools it runs, after the Nix settings that its
/// caller gave it.
#[test]
fn nix_config() {
    let mut fx = fixture().with("NIX_CONFIG", "warn-dirty = false");
    let seen = fx.dir.join("nix-config");
    // Status asks the stand-in for systemctl, which notes the setting it got.
    write_script(
        &fx.systemctl,
        &format!(
            "#!/bin/sh\nprintf '%s' \"$NIX_CONFIG\" > '{}'\n",
            seen.display()
        ),
    );
    let flakes = "extra-experimental-features = nix-command flakes";
    fx.run(&["status"]).ok();
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap(),
        format!("warn-dirty = false\n{flakes}")
    );
    fx.unset("NIX_CONFIG");
    fx.run(&["status"]).ok();
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), flakes);
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
