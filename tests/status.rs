//! Tests for src/status.rs: the report and the running commit, end to end.

mod common;

use common::{
    append, assert_contains, assert_lacks, commit, fake_systems, fixture, git, write_script,
    Fixture,
};
use serde_json::{json, Value};

fn failed_update(time: u64, detail: &str) -> Value {
    json!({"time": time, "results": [{"task": "update", "inputs": ["a"], "outcome": "failed", "detail": detail}]})
}

fn unit_results(fx: &Fixture, text: &str) {
    let file = fx.dir.join("units.txt");
    std::fs::write(&file, text).unwrap();
    write_script(
        &fx.systemctl,
        &format!("#!/bin/sh\ncat '{}'\n", file.display()),
    );
}

fn marker(fx: &Fixture) -> std::path::PathBuf {
    fx.state.join("nxf/update-read")
}

/// Status prints the latest summary once for this user, across all status modes.
#[test]
fn unread_update() {
    let mut fx = fixture();
    fx.save_update(failed_update(1, "a: updated"));
    let first = fx.run(&["status"]);
    first.ok();
    assert_contains(&first.stdout, "Automatic update summary");
    assert_contains(&first.stdout, "a: updated");
    for flags in [&[][..], &["--checkedout"], &["--detailed"]] {
        if flags.contains(&"--detailed") {
            // --detailed needs a real running store path for the closure size.
            fx.set("NXF_SYSTEM", &fake_systems().one);
        }
        let again = fx.run(&[&["status"], flags].concat());
        again.ok();
        assert_lacks(&again.stdout, "Automatic update summary");
    }
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(fx.updater.join("update.json")).unwrap())
            .unwrap();
    assert_eq!(saved["time"], 1);
}

/// Only the latest result is displayed; a new run becomes unread even if its
/// outcome is unchanged, and reading it does not replay previous failures.
#[test]
fn latest_update() {
    let fx = fixture();
    fx.save_update(failed_update(1, "old failure"));
    fx.save_update(failed_update(2, "a: unchanged"));
    let latest = fx.run(&["status", "--checkedout", "b"]);
    latest.ok();
    assert_contains(&latest.stdout, "a: unchanged");
    assert_lacks(&latest.stdout, "old failure");
    fx.save_update(failed_update(3, "a: unchanged"));
    assert_contains(&fx.run(&["status"]).stdout, "Automatic update summary");
    assert_lacks(&fx.run(&["status"]).stdout, "Automatic update summary");
}

/// Read tracking is per Unix user's state directory, not in shared updater data.
#[test]
fn update_read_per_user() {
    let mut fx = fixture();
    fx.save_update(failed_update(1, "shared update"));
    assert_contains(&fx.run(&["status"]).stdout, "shared update");
    assert_lacks(&fx.run(&["status"]).stdout, "shared update");
    fx.set("XDG_STATE_HOME", fx.dir.join("other-user-state"));
    assert_contains(&fx.run(&["status"]).stdout, "shared update");
    assert_lacks(&fx.run(&["status"]).stdout, "shared update");
}

/// Invalid status requests and other commands cannot acknowledge a summary.
#[test]
fn failed_status_keeps_update() {
    let fx = fixture();
    fx.save_update(failed_update(1, "unread failure"));
    assert_ne!(fx.run(&["status", "missing-input"]).code, 0);
    fx.run(&["version"]).ok();
    assert!(!marker(&fx).exists());
    assert_contains(&fx.run(&["status"]).stdout, "unread failure");
}

/// Every result that is not unchanged is listed with its detail.
#[test]
fn update_details() {
    let fx = fixture();
    fx.save_update(json!({
        "time": 1,
        "results": [
            {"task": "update", "inputs": ["a"], "outcome": "done", "detail": ""},
            {"task": "update", "inputs": ["b"], "outcome": "unchanged", "detail": ""},
            {"task": "build", "inputs": [], "outcome": "failed", "detail": "build error"},
            {"task": "switch", "inputs": [], "outcome": "skipped", "detail": "not attempted"},
        ]
    }));
    let result = fx.run(&["status"]);
    result.ok();
    assert_contains(&result.stdout, "update a: done");
    assert_lacks(&result.stdout, "update b");
    assert_contains(&result.stdout, "build: failed. build error");
    assert_contains(&result.stdout, "switch: skipped. not attempted");
    assert_eq!(git(&fx.checkout, &["rev-list", "--count", "HEAD"]), "1");
}

/// The unit result catches an update killed before it saved. A newer saved
/// result replaces it.
#[test]
fn update_unit_result() {
    let fx = fixture();
    std::fs::create_dir_all(&fx.updater).unwrap();
    unit_results(
        &fx,
        "Id=nxf-auto-update.service\nResult=timeout\nExecMainExitTimestamp=@200\n",
    );
    let before_first_save = fx.run(&["status"]);
    before_first_save.ok();
    assert_contains(&before_first_save.stdout, "Update service: timeout.");
    fx.save_update(failed_update(1, "saved result"));
    let failed = fx.run(&["status"]);
    assert_contains(&failed.stdout, "Update service: timeout.");
    assert_contains(&failed.stdout, "journalctl -u nxf-auto-update.service");
    assert_lacks(&fx.run(&["status"]).stdout, "Automatic update summary");

    fx.save_update(failed_update(300, "newer saved result"));
    let recovered = fx.run(&["status"]);
    assert_contains(&recovered.stdout, "newer saved result");
    assert_lacks(&recovered.stdout, "timeout");
}

/// A malformed report cannot break normal status or get marked as read; repair
/// makes it readable. An unavailable system manager still permits file reporting.
#[test]
fn unread_update_errors() {
    let fx = fixture();
    std::fs::create_dir_all(&fx.updater).unwrap();
    std::fs::write(fx.updater.join("update.json"), "{").unwrap();
    let malformed = fx.run(&["status"]);
    malformed.ok();
    assert_contains(
        &malformed.stderr,
        "Cannot read the automatic update summary",
    );
    assert!(!marker(&fx).exists());
    fx.save_update(failed_update(1, "repaired report"));
    write_script(&fx.systemctl, "#!/bin/sh\nexit 1\n");
    assert_contains(&fx.run(&["status"]).stdout, "repaired report");
    assert_lacks(&fx.run(&["status"]).stdout, "Automatic update summary");
}

/// A system manager that never answers does not hold status up.
#[test]
fn slow_system_manager() {
    let fx = fixture();
    fx.save_update(failed_update(1, "still shown"));
    write_script(&fx.systemctl, "#!/bin/sh\nexec sleep 30\n");
    let started = std::time::Instant::now();
    assert_contains(&fx.run(&["status"]).stdout, "still shown");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

/// Failed acknowledgment leaves the latest result unread for the next request.
#[test]
fn unread_update_marker_error() {
    let mut fx = fixture();
    fx.save_update(failed_update(1, "keep unread"));
    let blocked = fx.dir.join("blocked-state");
    std::fs::write(&blocked, "not a directory").unwrap();
    let state = fx.state.clone();
    fx.set("XDG_STATE_HOME", &blocked);
    for _ in 0..2 {
        let result = fx.run(&["status"]);
        result.ok();
        assert_contains(&result.stdout, "keep unread");
        assert_contains(&result.stderr, "Could not mark the update summary as read");
    }
    // An unreadable marker is treated as unread, and cannot be mistaken for a
    // successful acknowledgment by moving the temporary file into a directory.
    fx.set("XDG_STATE_HOME", &state);
    std::fs::create_dir_all(marker(&fx)).unwrap();
    let invalid = fx.run(&["status"]);
    invalid.ok();
    assert_contains(&invalid.stdout, "keep unread");
    assert_contains(&invalid.stderr, "Could not mark the update summary as read");
}

/// Status matches a recorded running system.
#[test]
fn matches() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    fx.record_build(system);
    for flags in [&[][..], &["--checkedout"], &["--detailed"]] {
        let result = fx.run(&[&["status"], flags].concat());
        result.ok();
        assert_contains(&result.stdout, &format!("Flake: {} init", fx.short_head()));
        assert_contains(&result.stdout, "  a  ");
        let lines: Vec<&str> = result.stdout.lines().take(2).collect();
        assert_eq!(
            lines,
            [
                format!(
                    "✓ Running system matches the checkout. ({})",
                    system.display()
                )
                .as_str(),
                ""
            ]
        );
        assert_eq!(result.stdout.matches(system.to_str().unwrap()).count(), 1);
    }
    let version = fx.run(&["version", "--system"]);
    assert_eq!(version.stdout.trim(), fx.head());
}

/// Status uses the updater's record.
#[test]
fn updater() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    let systems = fx.updater.join("systems");
    std::fs::create_dir_all(&systems).unwrap();
    std::os::unix::fs::symlink(system, systems.join(fx.tree())).unwrap();
    assert_contains(
        &fx.run(&["status"]).stdout,
        "Running system matches the checkout.",
    );
}

/// Status shows input changes and the build diff.
#[test]
fn diff() {
    let built = fake_systems();
    let fx = fixture();
    fx.record_build(&built.one);
    let before = fx.short("a");
    fx.bump(&["a"]);
    fx.run(&["update", "a"]).ok();
    fx.record_build(&built.two);
    let fx = fx.with("NXF_SYSTEM", &built.one);
    let result = fx.run(&["status"]);
    result.ok();
    assert_contains(&result.stdout, &format!("  a  {before}"));
    assert_contains(&result.stdout, &format!("(checkout: {})", fx.short("a")));
    let lines: Vec<&str> = result.stdout.lines().take(2).collect();
    assert_eq!(
        lines,
        [
            format!(
                "! Running system differs from the checkout. ({})",
                built.one.display()
            )
            .as_str(),
            ""
        ]
    );
    assert_contains(&result.stdout, "SIZE:");
    assert_contains(&result.stdout, "    nxf status --detailed");
}

/// Detailed status adds the closure size and the full package list.
#[test]
fn detailed() {
    let built = fake_systems();
    let fx = fixture();
    fx.record_build(&built.one);
    fx.bump(&["a"]);
    fx.run(&["update", "a"]).ok();
    fx.record_build(&built.two);
    let fx = fx.with("NXF_SYSTEM", &built.one);
    let result = fx.run(&["status", "--detailed"]);
    result.ok();
    assert_eq!(
        result.stdout.lines().next().unwrap(),
        format!(
            "! Running system differs from the checkout. ({})",
            built.one.display()
        )
    );
    assert_contains(&result.stdout, "    Closure size: ");
    assert_contains(&result.stdout, "Checkout: ");
    assert_contains(&result.stdout, "[A.] two");
}

/// Status says when the checkout is not built.
#[test]
fn not_built() {
    let system = &fake_systems().one;
    let fx = fixture();
    fx.record_build(system);
    fx.bump(&["a"]);
    // The update builds with the mock nh, whose result is not a system.
    fx.run(&["update", "a"]).ok();
    std::fs::remove_file(fx.systems().join(fx.tree())).unwrap();
    let fx = fx.with("NXF_SYSTEM", system);
    let result = fx.run(&["status"]);
    assert_contains(&result.stdout, "The checkout has not been built yet.");
    assert_contains(&result.stdout, "    nxf build");
}

/// Status reports uncommitted changes.
#[test]
fn dirty() {
    let fx = fixture();
    append(&fx.checkout.join("flake.nix"), "# dirty\n");
    let result = fx.run(&["status", "--checkedout"]);
    assert_contains(&result.stdout, ", with uncommitted changes");
    assert!(result
        .stdout
        .starts_with("! Checkout has uncommitted changes"));
    assert_contains(result.stdout.lines().next().unwrap(), &fx.env["NXF_SYSTEM"]);
}

/// Status of an unrecorded system.
#[test]
fn unknown_system() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    let result = fx.run(&["status"]);
    result.ok();
    assert_contains(&result.stdout, "Flake: unknown");
    assert!(result
        .stdout
        .starts_with("! Cannot compare the running system with the checkout."));
    assert_contains(
        result.stdout.lines().next().unwrap(),
        &format!("({})", system.display()),
    );
    assert_ne!(fx.run(&["version", "--system"]).code, 0);
}

/// Status filters inputs and rejects unknown ones.
#[test]
fn filter() {
    let fx = fixture();
    let result = fx.run(&["status", "--checkedout", "b"]);
    assert_contains(&result.stdout, "  b  ");
    assert_lacks(&result.stdout, "  a  ");
    let unknown = fx.run(&["status", "update"]);
    assert_ne!(unknown.code, 0);
    assert_contains(&unknown.stderr, "Unknown input: update");
}

/// When several commits built the running system, the checkout's is named,
/// and otherwise the newest.
#[test]
fn same_system_several_commits() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    let file = fx.checkout.join("flake.nix");
    fx.record_build(system);
    for number in [1, 2] {
        append(&file, &format!("# comment {number}\n"));
        commit(&fx.checkout, &format!("comment {number}"));
        fx.record_build(system);
    }
    let result = fx.run(&["status"]);
    assert_contains(&result.stdout, "comment 2 (");
    assert_contains(&result.stdout, "Running system matches the checkout.");
    append(&file, "# comment 3\n");
    commit(&fx.checkout, "comment 3");
    let later = fx.run(&["status"]);
    assert_contains(&later.stdout, "comment 2 (");
    assert_contains(&later.stdout, "differs from the checkout");
}

/// The checkout's commit is named even when a newer commit elsewhere built
/// the same system.
#[test]
fn same_system_prefers_checkout() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    fx.record_build(system);
    git(&fx.checkout, &["switch", "--quiet", "--create", "other"]);
    append(&fx.checkout.join("flake.nix"), "# other\n");
    commit(&fx.checkout, "newer elsewhere");
    fx.record_build(system);
    git(&fx.checkout, &["switch", "--quiet", "main"]);
    let result = fx.run(&["status"]);
    assert_contains(&result.stdout, " init (");
    assert_contains(&result.stdout, "Running system matches the checkout.");
}

/// The JSON comparison is for scripts: it tells whether the committed checkout
/// is built and running, and does not mark the update summary as read.
#[test]
fn json() {
    let mut fx = fixture();
    fx.save_update(json!({"time": 1, "results": []}));
    let parse = |text: &str| -> Value { serde_json::from_str(text).unwrap() };
    let unbuilt = fx.run(&["status", "--json"]);
    unbuilt.ok();
    let unbuilt = parse(&unbuilt.stdout);
    assert_eq!(unbuilt["changes"], json!([]));
    assert_eq!(unbuilt["built"], Value::Null);
    assert_eq!(unbuilt["matches"], false);
    assert_eq!(unbuilt["inputs"], json!(["a", "b"]));
    assert_eq!(unbuilt["checkout"], fx.checkout.to_str().unwrap());
    let system = &fake_systems().one;
    fx.record_build(system);
    fx.set("NXF_SYSTEM", system);
    let built = parse(&fx.run(&["status", "--json"]).stdout);
    assert_eq!(built["built"], system.to_str().unwrap());
    assert_eq!(built["running"], system.to_str().unwrap());
    assert_eq!(built["matches"], true);
    append(&fx.checkout.join("flake.nix"), "# edit");
    assert_eq!(
        parse(&fx.run(&["status", "--json"]).stdout)["changes"],
        json!(["flake.nix"])
    );
    assert_contains(&fx.run(&["status"]).stdout, "Automatic update summary");
}
