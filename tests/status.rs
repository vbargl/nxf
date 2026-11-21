//! Tests for src/status.rs: the report and the running commit, end to end.

mod common;

use common::{
    append, assert_contains, assert_lacks, commit, fake_systems, fixture, git, saved, write_script,
    Fixture,
};
use serde_json::{json, Value};

fn failed_update(time: u64, detail: &str) -> Value {
    json!({"time": time, "results": [{"task": "update", "inputs": ["a"], "outcome": "failed", "details": [detail]}]})
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
    assert_contains(&first.stdout, "Last automatic update");
    assert_contains(&first.stdout, "a: updated");
    for flags in [&[][..], &["--checkedout"], &["--detailed"]] {
        if flags.contains(&"--detailed") {
            // --detailed needs a real running store path for the closure size.
            fx.set("NXF_SYSTEM", &fake_systems().one);
        }
        let again = fx.run(&[&["status"], flags].concat());
        again.ok();
        assert_lacks(&again.stdout, "Last automatic update");
    }
    assert_eq!(saved(&fx)["time"], 1);
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
    assert_contains(&fx.run(&["status"]).stdout, "Last automatic update");
    assert_lacks(&fx.run(&["status"]).stdout, "Last automatic update");
}

/// A result with a long name keeps a gap before what it says.
#[test]
fn long_result_name() {
    let fx = fixture();
    fx.save_update(json!({"time": 1, "results": [
        {"task": "update", "inputs": ["unstable"], "outcome": "reported", "details": ["no route", "to host"]},
    ]}));
    assert_contains(
        &fx.run(&["status"]).stdout,
        "\n  · update unstable  reported, no route\n",
    );
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
fn rejected_request_keeps_update() {
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
            {"task": "update", "inputs": ["a"], "outcome": "done", "details": []},
            {"task": "update", "inputs": ["b"], "outcome": "unchanged", "details": []},
            {"task": "build", "inputs": [], "outcome": "failed", "details": ["build error"]},
            {"task": "switch", "inputs": [], "outcome": "skipped", "details": ["not attempted"]},
        ]
    }));
    let result = fx.run(&["status"]);
    result.ok();
    assert_contains(&result.stdout, "\n  ✓ update a\n");
    assert_lacks(&result.stdout, "update b");
    assert_contains(&result.stdout, "\n  ✗ build     build error\n");
    assert_contains(&result.stdout, "\n  · switch    skipped, not attempted\n");
    assert_eq!(git(&fx.checkout, &["rev-list", "--count", "HEAD"]), "1");
}

/// Results that an older nxf saved, with one `detail` text, have another
/// shape: status shows them as no result and does not fail.
#[test]
fn older_update_file() {
    let fx = fixture();
    fx.save_update(json!({"time": 5, "results": [
        {"task": "update", "inputs": ["a"], "outcome": "failed", "detail": "a: no route | to host"},
    ]}));
    let result = fx.run(&["status"]);
    result.ok();
    assert_lacks(&result.stdout, "Last automatic update");
    assert_lacks(&result.stderr, "Cannot read");
    let json: Value = serde_json::from_str(&fx.run(&["status", "--json"]).stdout).unwrap();
    assert_eq!(json["lastUpdate"], Value::Null);
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
    assert_contains(&before_first_save.stdout, "✗ service   timeout");
    fx.save_update(failed_update(1, "saved result"));
    let failed = fx.run(&["status"]);
    assert_contains(&failed.stdout, "✗ service   timeout");
    assert_contains(&failed.stdout, "journalctl -u nxf-auto-update.service");
    assert_lacks(&fx.run(&["status"]).stdout, "Last automatic update");

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
    assert_lacks(&fx.run(&["status"]).stdout, "Last automatic update");
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
        assert_contains(
            &result.stdout,
            &format!("  Flake     {} init", fx.short_head()),
        );
        assert_contains(&result.stdout, "\n  a         ");
        let lines: Vec<&str> = result.stdout.lines().take(2).collect();
        assert_eq!(
            lines,
            [
                format!(
                    "✓ Running system matches the checkout ({})",
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
        "Running system matches the checkout (",
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
    assert_contains(
        &result.stdout,
        &format!(
            "\n  a         {before}  → {} in the checkout",
            fx.short("a")
        ),
    );

    let lines: Vec<&str> = result.stdout.lines().take(2).collect();
    assert_eq!(
        lines,
        [
            format!(
                "! Running system differs from the checkout ({})",
                built.one.display()
            )
            .as_str(),
            ""
        ]
    );
    assert_contains(
        &result.stdout,
        "\nChanges when switched\n  Packages  1 added, 1 removed\n  Size      ",
    );
    assert_contains(&result.stdout, "  Package list: nxf status --detailed");
}

/// A dix that takes the two systems without a command word, as dix 2 does,
/// and prints headings without counts.
fn dix_2(fx: &Fixture) {
    write_script(
        &fx.dir.join("bin/dix"),
        r#"#!/usr/bin/env bash
[[ $1 != --version ]] || { echo "dix 2.2.0"; exit 0; }
paths=()
for word; do [[ $word == --* ]] || paths+=("$word"); done
set -- "${paths[@]}"
if [[ $# != 2 ]]; then
    printf "error: unexpected argument '%s' found\n\nUsage: dix [OPTIONS] <OLD_PATH> <NEW_PATH>\n" "$3" >&2
    exit 2
fi
cat <<END
<<< $1
>>> $2

CHANGED
[U.] bash  5.3p3 -> 5.3p9
[C.] curl  8.22.0, <unchanged> -> <unchanged>, +1.0 KiB

ADDED
[A.] jq  1.8.1

REMOVED
[R.] nano  8.7
[R.] vim  9.2
[R.] w3m  0.5.6

PATHS: 12 -> 10 (+3, -5)
SIZE: 34.7 GiB -> 33.5 GiB
DIFF: -1.14 GiB
END
"#,
    );
}

/// The package changes are read from dix 2 as well as from dix 3: nxf is
/// built with the nixpkgs of the machine flake, which may hold either.
#[test]
fn diff_with_dix_2() {
    let built = fake_systems();
    let fx = fixture().with("NXF_SYSTEM", &built.one);
    fx.record_build(&built.two);
    dix_2(&fx);
    let result = fx.run(&["status"]);
    result.ok();
    assert_contains(
        &result.stdout,
        "\nChanges when switched\n  Packages  2 changed, 1 added, 3 removed\n  Size      34.7 GiB → 33.5 GiB\n",
    );
    let result = fx.run(&["status", "--detailed"]);
    result.ok();
    assert_contains(&result.stdout, "\n    [A.] jq  1.8.1\n");
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
            "! Running system differs from the checkout ({})",
            built.one.display()
        )
    );
    assert_contains(&result.stdout, "  Closure   ");
    assert_contains(&result.stdout, "\nCheckout\n  Flake     ");
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
    assert_contains(
        &result.stdout,
        "  The checkout has not been built yet: nxf build",
    );
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

/// The checkout's tree is read once, and not again to find the commit of the
/// running system.
#[test]
fn reads_tree_once() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system);
    fx.record_build(system);
    let git = common::spy(&fx, "git");
    fx.run(&["status"]).ok();
    let calls = common::calls(&git);
    let count = |part: &str| calls.iter().filter(|call| call.contains(part)).count();
    assert_eq!(count("rev-parse HEAD^{tree}"), 1, "{calls:?}");
    assert_eq!(count("diff --quiet"), 1, "{calls:?}");
}

/// A running system that no build record names cannot be compared: status
/// says so, shows the revision NixOS knows, and `version --system` fails.
#[test]
fn unknown_system() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    let result = fx.run(&["status"]);
    result.ok();
    assert_contains(
        &result.stdout,
        "  Flake     unknown; the system was not built by nxf or the updater\n  Revision  mock-revision\n",
    );
    assert!(result
        .stdout
        .starts_with("! Cannot compare the running system with the checkout ("));
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
    assert_contains(&result.stdout, "\n  b         ");
    assert_lacks(&result.stdout, "\n  a         ");
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
    assert_contains(&result.stdout, "Running system matches the checkout (");
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
    assert_contains(&result.stdout, "Running system matches the checkout (");
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
    assert_contains(&fx.run(&["status"]).stdout, "Last automatic update");
}

/// A system with a kernel directory, standing in for a NixOS system.
fn system_with(
    dir: &std::path::Path,
    name: &str,
    kernel: &str,
    initrd: &str,
) -> std::path::PathBuf {
    let system = dir.join(name);
    std::fs::create_dir_all(&system).unwrap();
    std::fs::write(system.join("kernel"), kernel).unwrap();
    std::fs::write(system.join("initrd"), initrd).unwrap();
    system
}

/// Status says when the running system needs a restart to use its kernel or
/// initrd, and stays quiet when only other things changed.
#[test]
fn restart_needed() {
    let mut fx = fixture();
    let booted = system_with(&fx.dir, "booted", "6.1", "initrd-1");
    let same = system_with(&fx.dir, "same-kernel", "6.1", "initrd-1");
    // Two systems share a kernel when they link to the same store path.
    for part in ["kernel", "initrd"] {
        std::fs::remove_file(same.join(part)).unwrap();
        std::os::unix::fs::symlink(booted.join(part), same.join(part)).unwrap();
    }
    fx.set("NXF_BOOTED", &booted);
    fx.set("NXF_SYSTEM", &same);
    let parse = |text: &str| -> Value { serde_json::from_str(text).unwrap() };
    assert_lacks(&fx.run(&["status"]).stdout, "Restart needed");
    assert_eq!(
        parse(&fx.run(&["status", "--json"]).stdout)["rebootRequired"],
        false
    );

    for (name, kernel, initrd) in [
        ("new-kernel", "6.2", "initrd-1"),
        ("new-initrd", "6.1", "initrd-2"),
    ] {
        fx.set("NXF_SYSTEM", system_with(&fx.dir, name, kernel, initrd));
        let status = fx.run(&["status"]);
        status.ok();
        assert_contains(
            &status.stdout,
            "\n  ! Restart needed: the kernel or initrd changed since the machine booted\n",
        );
        assert_eq!(
            parse(&fx.run(&["status", "--json"]).stdout)["rebootRequired"],
            true
        );
    }

    // Without a booted system to compare with, nothing is claimed.
    fx.set("NXF_BOOTED", fx.dir.join("missing"));
    assert_lacks(&fx.run(&["status"]).stdout, "Restart needed");
}

/// The JSON status names the running revision and the last update for monitoring.
#[test]
fn json_monitoring() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    let parse = |text: &str| -> Value { serde_json::from_str(text).unwrap() };
    let unknown = parse(&fx.run(&["status", "--json"]).stdout);
    assert_eq!(unknown["revision"], Value::Null);
    assert_eq!(unknown["lastUpdate"], Value::Null);
    assert_eq!(unknown["hold"], Value::Null);
    assert_eq!(unknown["bad"], Value::Null);
    fx.record_build(system);
    fx.save_update(failed_update(7, "a: no route"));
    let known = parse(&fx.run(&["status", "--json"]).stdout);
    assert_eq!(known["revision"], fx.head());
    assert_eq!(known["lastUpdate"]["time"], 7);
    assert_eq!(known["lastUpdate"]["results"][0]["outcome"], "failed");
}
