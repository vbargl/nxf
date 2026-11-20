//! Tests for src/doctor.rs, end to end through `nxf doctor`.

mod common;

use common::{
    append, assert_contains, assert_lacks, configurations, fake_systems, fixture, hostname,
    status_of,
};

/// A healthy checkout passes; an unrecorded system only warns.
#[test]
fn healthy() {
    let fx = fixture();
    let result = fx.run(&["doctor"]);
    result.ok();
    for check in [
        "tools", "flake", "git", "lockfile", "writable", "identity", "changes", "config", "state",
    ] {
        assert_eq!(
            status_of(&result.stdout, check),
            Some("ok"),
            "{}",
            result.stdout
        );
    }
    assert_eq!(status_of(&result.stdout, "running"), Some("warn"));
    // One outcome ends the report.
    assert!(
        result.stdout.ends_with("\n\n✓ Checks passed, 1 warning\n"),
        "{}",
        result.stdout
    );
}

/// A system built from the checkout is named.
#[test]
fn running() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_SYSTEM", system);
    fx.record_build(system);
    let result = fx.run(&["doctor"]);
    assert_eq!(status_of(&result.stdout, "running"), Some("ok"));
    assert_contains(&result.stdout, "built from flake commit");
}

/// A missing flake fails, and the checks that need it are skipped.
#[test]
fn missing_flake() {
    let fx = fixture();
    let dir = fx.dir.clone();
    let fx = fx.with("NXF_FLAKE", dir);
    let result = fx.run(&["doctor"]);
    assert_ne!(result.code, 0);
    assert_eq!(status_of(&result.stdout, "flake"), Some("fail"));
    assert_eq!(status_of(&result.stdout, "git"), None);
    assert_contains(&result.stdout, "skipped");
    assert_contains(&result.stderr, "✗ 1 check failed");
    assert_lacks(&result.stdout, "passed");
}

/// A flake outside Git fails, since nxf reads its history.
#[test]
fn not_a_repo() {
    let fx = fixture();
    let plain = fx.dir.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::copy(fx.checkout.join("flake.nix"), plain.join("flake.nix")).unwrap();
    let fx = fx.with("NXF_FLAKE", plain);
    let result = fx.run(&["doctor"]);
    assert_ne!(result.code, 0);
    assert_eq!(status_of(&result.stdout, "git"), Some("fail"));
}

/// Uncommitted changes only warn.
#[test]
fn uncommitted() {
    let fx = fixture();
    append(&fx.checkout.join("flake.nix"), "# dirty\n");
    let result = fx.run(&["doctor"]);
    result.ok();
    assert_eq!(status_of(&result.stdout, "changes"), Some("warn"));
}

/// The configuration is the one named default, or the only one; several
/// without that name fail.
#[test]
fn configuration() {
    let fx = fixture();
    let host = hostname();
    let only = fx.run(&["doctor"]);
    assert_eq!(status_of(&only.stdout, "config"), Some("ok"));
    assert_contains(&only.stdout, "the only configuration");

    configurations(&fx, &[(&host, ""), ("other", "")], "two configurations");
    let ambiguous = fx.run(&["doctor"]);
    assert_ne!(ambiguous.code, 0);
    assert_eq!(status_of(&ambiguous.stdout, "config"), Some("fail"));
    assert_contains(&ambiguous.stdout, "none is named default");

    configurations(
        &fx,
        &[(&host, ""), ("default", "")],
        "a default configuration",
    );
    let named = fx.run(&["doctor"]);
    named.ok();
    assert_contains(&named.stdout, "config    nixosConfigurations.default\n");
}

/// A read-only checkout fails, as update could not commit.
#[test]
fn read_only() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture();
    std::fs::set_permissions(&fx.checkout, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = fx.run(&["doctor"]);
    assert_ne!(result.code, 0);
    assert_eq!(status_of(&result.stdout, "writable"), Some("fail"));
}

/// A linked worktree's .git file must not make a writable checkout fail.
#[test]
fn linked_worktree() {
    let fx = fixture();
    let linked = fx.dir.join("linked");
    common::git(
        &fx.checkout,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    assert!(linked.join(".git").is_file());
    let fx = fx.with("NXF_FLAKE", &linked);
    let result = fx.run(&["doctor"]);
    result.ok();
    assert_eq!(status_of(&result.stdout, "writable"), Some("ok"));
}

/// Doctor reports the actual record destination without creating or importing records.
#[test]
fn record_destination() {
    use std::os::unix::fs::PermissionsExt;
    let mut fx = fixture();
    let local = fx.run(&["doctor"]);
    assert_contains(
        &local.stdout,
        &format!("build records go to {}", fx.systems().display()),
    );
    let shared = fx.updater.join("systems");
    assert!(!fx.updater.exists());
    std::fs::create_dir_all(&shared).unwrap();
    let result = fx.run(&["doctor"]);
    assert_eq!(status_of(&result.stdout, "records"), Some("ok"));
    assert_contains(
        &result.stdout,
        &format!("build records go to {}", shared.display()),
    );
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = fx.run(&["doctor"]);
    result.ok();
    assert_eq!(status_of(&result.stdout, "records"), Some("warn"));
    assert_contains(&result.stdout, "active nxf group membership");
    assert_contains(&result.stdout, &fx.systems().display().to_string());
    assert_eq!(std::fs::read_dir(&shared).unwrap().count(), 0);
    fx.set("NXF_STATE", fx.updater.clone());
    let result = fx.run(&["doctor"]);
    result.ok();
    assert_eq!(status_of(&result.stdout, "records"), Some("warn"));
    assert_contains(&result.stdout, "build records cannot be saved");
    assert_lacks(&result.stdout, "fall back to");
}
