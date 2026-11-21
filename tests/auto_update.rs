//! Tests for src/auto_update.rs: the unattended update, end to end.

mod common;

use common::{
    append, assert_contains, assert_detail, details, fake_systems, fixture, git, lose, outcomes,
    pair, saved, strategies, Fixture,
};
use serde_json::{json, Value};

fn failure_hook(fx: &mut Fixture, script: &str) {
    let hooks = fx.dir.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    common::write_script(&hooks.join("failure"), script);
    fx.set("NXF_HOOKS_DIR", &hooks);
}

/// A new input revision is built, committed and switched to.
#[test]
fn updates_and_switches() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_TEST_BUILD", system);
    let old = fx.locked("a");
    fx.bump(&["a"]);
    let result = fx.run(&["auto-update"]);
    result.ok();
    assert_ne!(fx.locked("a"), old);
    assert_eq!(fx.porcelain(), "");
    assert_eq!(fx.nh_actions(), ["build", "build", "switch"]);
    let report = saved(&fx);
    assert!(report["time"].as_u64().unwrap() > 0);
    assert_eq!(
        outcomes(&report),
        [pair("update", "done"), pair("switch", "done")]
    );
    assert_contains(&result.stdout, "  ✓ update\n  ✓ switch\n");
}

/// The identity of the module's default, a name and an empty e-mail, commits
/// the lockfile.
#[test]
fn commits_without_email() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("GIT_AUTHOR_NAME", "nxf")
        .with("GIT_COMMITTER_NAME", "nxf")
        .with("GIT_AUTHOR_EMAIL", "")
        .with("GIT_COMMITTER_EMAIL", "");
    fx.bump(&["a"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        git(&fx.checkout, &["log", "-1", "--format=%an <%ae> %cn <%ce>"]),
        "nxf <> nxf <>"
    );
}

/// Nothing new and a running system that matches: nothing is built or activated.
#[test]
fn unchanged_and_running() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system);
    fx.record_build(system);
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(outcomes(&saved(&fx)), [pair("update", "unchanged")]);
}

/// A committed checkout that is not what runs is switched to without an update.
#[test]
fn switches_to_committed_checkout() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.nh_actions(), ["build", "switch"]);
    assert_eq!(
        outcomes(&saved(&fx)),
        [pair("update", "unchanged"), pair("switch", "done")]
    );
}

/// An update that does not build never stays in the repository and is not
/// switched to. It is reported, and the run does not fail.
#[test]
fn failed_build_is_restored() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system)
        .with("NXF_TEST_NH_FAIL_ACTION", "build");
    fx.record_build(system);
    let (old, head) = (fx.locked("a"), fx.head());
    fx.bump(&["a"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.locked("a"), old);
    assert_eq!(fx.head(), head);
    assert_eq!(fx.porcelain(), "");
    assert_eq!(fx.nh_actions(), ["build"]);
    let report = saved(&fx);
    assert_eq!(
        outcomes(&report),
        [pair("update", "unchanged"), pair("update", "reported")]
    );
    assert_eq!(report["results"][1]["inputs"], json!(["a"]));
    assert_detail(&report["results"][1], "nh exited with status 3.");
    assert_contains(
        &fx.run(&["status"]).stdout,
        "\n  · update a  reported, nh exited with status 3.",
    );
}

/// The run does nothing while other files have uncommitted changes, but a
/// lockfile left by a killed run is restored.
#[test]
fn uncommitted_changes() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.bump(&["a"]);
    append(&fx.checkout.join("flake.nix"), "# work in progress\n");
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
    assert_eq!(
        saved(&fx)["results"],
        json!([{"task": "update", "inputs": [], "outcome": "skipped", "details": ["the checkout has uncommitted changes"]}])
    );
    common::git(&fx.checkout, &["checkout", "--quiet", "flake.nix"]);
    append(&fx.checkout.join("flake.lock"), "\n");
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.porcelain(), "");
    assert_eq!(outcomes(&saved(&fx))[0], pair("update", "done"));
}

/// Builds are remembered in NXF_STATE, where every user's status finds them.
#[test]
fn shared_state() {
    let system = &fake_systems().one;
    let mut fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system);
    let updater = fx.updater.clone();
    fx.set("NXF_STATE", &updater);
    fx.run(&["auto-update"]).ok();
    assert_eq!(
        &std::fs::canonicalize(updater.join("systems").join(fx.tree())).unwrap(),
        system
    );
    fx.unset("NXF_STATE");
    assert_contains(
        &fx.run(&["status"]).stdout,
        "Running system matches the checkout (",
    );
}

/// A manual build is visible to the service, which skips nh for the running tree.
#[test]
fn manual_build_skips_service_build() {
    let systems = fake_systems();
    let mut fx = fixture()
        .with("NXF_TEST_BUILD", &systems.one)
        .with("NXF_SYSTEM", &systems.one);
    std::fs::create_dir_all(fx.updater.join("systems")).unwrap();
    // A conflicting old local record must not override the machine-wide record.
    fx.record_build(&systems.two);
    fx.run(&["build"]).ok();
    let manual: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    std::fs::remove_file(&fx.log).unwrap();
    fx.set("NXF_STATE", fx.updater.clone());
    let service: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    for key in ["built", "matches", "revision"] {
        assert_eq!(manual[key], service[key]);
    }
    assert_eq!(service["matches"], true);
    fx.run(&["auto-update"]).ok();
    assert!(fx.nh_calls().is_empty());
}

/// An input that is off is left alone; the others are updated.
#[test]
fn off_is_left_alone() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    strategies(&fx, "{ inputs.a = \"off\"; }");
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    fx.bump(&["a", "b"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!(fx.locked("a"), a);
    assert_ne!(fx.locked("b"), b);
    assert_eq!(
        outcomes(&saved(&fx)),
        [pair("update", "done"), pair("switch", "done")]
    );
    // By hand it is updated like any other input.
    fx.run(&["update", "a"]).ok();
    assert_ne!(fx.locked("a"), a);
}

/// With every input off, nothing is updated and the checkout is switched to.
#[test]
fn all_off() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    strategies(&fx, "{ strategy = \"off\"; }");
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    fx.bump(&["a", "b"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!((fx.locked("a"), fx.locked("b")), (a, b));
    assert_eq!(
        outcomes(&saved(&fx)),
        [pair("update", "unchanged"), pair("switch", "done")]
    );
}

/// A flagged input whose update fails is recorded; the run goes on with the
/// other inputs, switches, and does not fail.
#[test]
fn flagged_failure_goes_on() {
    let fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_SYSTEM", &fake_systems().two);
    strategies(&fx, "{ inputs.a = \"flag\"; }");
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    lose(&fx, "a");
    fx.bump(&["b"]);
    let result = fx.run(&["auto-update"]);
    result.ok();
    assert_eq!(fx.locked("a"), a);
    assert_ne!(fx.locked("b"), b);
    assert_eq!(fx.porcelain(), "");
    let report = saved(&fx);
    assert_eq!(
        outcomes(&report),
        [
            pair("update", "done"),
            pair("update", "flagged"),
            pair("switch", "done")
        ]
    );
    assert_eq!(report["results"][1]["inputs"], json!(["a"]));
    assert_detail(&report["results"][1], "does not exist");
    assert_contains(&result.stdout, "\n  · update a  flagged, ");
    assert_contains(&fx.run(&["status"]).stdout, "\n  · update a  flagged, ");
}

/// An input without a strategy of its own is reported when its update fails:
/// the run goes on with the other inputs, switches, and does not fail.
#[test]
fn reported_failure_goes_on() {
    let fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_SYSTEM", &fake_systems().two);
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    lose(&fx, "a");
    fx.bump(&["b"]);
    let result = fx.run(&["auto-update"]);
    result.ok();
    assert_eq!(fx.locked("a"), a);
    assert_ne!(fx.locked("b"), b);
    assert_eq!(fx.porcelain(), "");
    let report = saved(&fx);
    assert_eq!(
        outcomes(&report),
        [
            pair("update", "done"),
            pair("update", "reported"),
            pair("switch", "done")
        ]
    );
    assert_eq!(report["results"][1]["inputs"], json!(["a"]));
    assert_contains(&result.stdout, "\n  · update a  reported, ");
    // Every input is tried, also after one that failed.
    let fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_SYSTEM", &fake_systems().two);
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    lose(&fx, "a");
    lose(&fx, "b");
    fx.run(&["auto-update"]).ok();
    assert_eq!((fx.locked("a"), fx.locked("b")), (a, b));
    assert_eq!(
        outcomes(&saved(&fx)),
        [
            pair("update", "unchanged"),
            pair("update", "reported"),
            pair("update", "reported"),
            pair("switch", "done")
        ]
    );
}

/// The strategies are read from the committed configuration; a word that is
/// none of them stops the run before anything is updated.
#[test]
fn unknown_strategy() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    strategies(&fx, "{ inputs.a = \"sometimes\"; }");
    fx.bump(&["a"]);
    let result = fx.run(&["auto-update"]);
    assert_ne!(result.code, 0);
    assert_contains(&result.stderr, "unknown update strategy 'sometimes'");
    assert!(fx.nh_calls().is_empty());
}

/// After a build of a committed flake, the locked inputs are in the Nix
/// store and kept there, so the next build needs no access to their origin.
#[test]
fn keeps_the_sources() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.bump(&["a"]);
    fx.run(&["auto-update"]).ok();
    assert_eq!(common::kept_sources(&fx), ["1", "2"]);
}

/// A run that fails before any task ran, such as a checkout whose strategies
/// cannot be read, is a failed update like any other: it is saved for
/// status, the failure hook runs with it, and the command fails with the cause.
#[test]
fn undecided_run_is_saved() {
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let seen = fx.dir.join("seen.json");
    failure_hook(
        &mut fx,
        &format!("#!/bin/sh\ncp \"$NXF_RESULTS\" '{}'\n", seen.display()),
    );
    strategies(&fx, "{ inputs.a = \"sometimes\"; }");
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    assert_contains(&result.stderr, "✗ unknown update strategy 'sometimes'");
    let report = saved(&fx);
    assert_eq!(outcomes(&report), [pair("update", "failed")]);
    assert_detail(&report["results"][0], "unknown update strategy 'sometimes'");
    let seen: Value = serde_json::from_str(&std::fs::read_to_string(&seen).unwrap()).unwrap();
    assert_eq!(seen["results"], report["results"]);
    assert_contains(&fx.run(&["status"]).stdout, "  ✗ update    unknown update");
}

/// When the failure hook fails after a run that failed before any task, the
/// command says so beneath the cause, and the hook is a saved result too.
#[test]
fn failing_hook_is_said() {
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    failure_hook(&mut fx, "#!/bin/sh\necho 'the pager is down' >&2\nexit 1\n");
    strategies(&fx, "{ inputs.a = \"sometimes\"; }");
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    assert_contains(&result.stderr, "✗ unknown update strategy 'sometimes'");
    assert_contains(
        &result.stderr,
        "\n  The failure hook failed too: the pager is down\n",
    );
    let report = saved(&fx);
    assert_eq!(
        outcomes(&report),
        [pair("update", "failed"), pair("hook", "failed")]
    );
    assert_eq!(details(&report["results"][1]), ["the pager is down"]);
}

/// A run that cannot go on after tasks ran keeps what ran: the results of
/// the tasks and the record of an input stay saved, followed by the failure.
#[test]
fn undecided_run_keeps_what_ran() {
    let fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_SYSTEM", &fake_systems().two);
    lose(&fx, "a");
    fx.bump(&["b"]);
    // An nh that builds, and leaves a checkout that cannot be compared afterwards.
    let nh = fx.dir.join("bin/nh");
    std::fs::rename(&nh, fx.dir.join("bin/nh-building")).unwrap();
    common::write_script(
        &nh,
        &format!(
            "#!/bin/sh\n'{0}/bin/nh-building' \"$@\" && rm '{0}/flake/flake.nix'\n",
            fx.dir.display()
        ),
    );
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    assert_contains(&result.stderr, "has no flake.nix");
    let report = saved(&fx);
    assert_eq!(
        outcomes(&report),
        [
            pair("update", "done"),
            pair("update", "reported"),
            pair("update", "failed")
        ]
    );
    assert_eq!(report["results"][1]["inputs"], json!(["a"]));
    assert_detail(&report["results"][2], "has no flake.nix");
}

/// A failed run that cannot be saved either still fails with its cause, and
/// says beneath it what could not be saved: at first, and after the hook.
#[test]
fn unsaved_run_keeps_its_cause() {
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    strategies(&fx, "{ inputs.a = \"sometimes\"; }");
    // A file is where the state directory would be made.
    let file = fx.dir.join("file");
    std::fs::write(&file, "").unwrap();
    let state = file.join("state");
    let result = fx.run(&["auto-update", "--state", state.to_str().unwrap()]);
    assert_eq!(result.code, 1);
    const CAUSE: &str = "✗ unknown update strategy 'sometimes'; it is off, flag or report\n";
    assert_contains(
        &result.stderr,
        &format!(
            "{CAUSE}  cannot save {}: ",
            state.join("update.json").display()
        ),
    );

    // A failure hook that leaves a directory where the results are replaced next.
    failure_hook(
        &mut fx,
        "#!/bin/sh\nrm \"$NXF_RESULTS\"\nmkdir \"$NXF_RESULTS\"\necho 'the pager is down'\nexit 1\n",
    );
    let result = fx.run(&["auto-update"]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stderr,
        &format!(
            "{CAUSE}  The failure hook failed too: the pager is down\n  cannot save {}: ",
            fx.updater.join("update.json").display()
        ),
    );
}
