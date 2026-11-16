//! Tests for src/doctor.rs, end to end through `nxf doctor`.

mod common;

use common::{append, assert_contains, assert_lacks, commit, fake_systems, fixture};

/// The status of a check in the doctor's output.
fn status_of(output: &str, check: &str) -> Option<&'static str> {
    output.lines().find_map(|line| {
        let mut columns = line.split_whitespace();
        let status = match columns.next()? {
            "✓" => "ok",
            "!" => "warn",
            "✗" => "fail",
            _ => return None,
        };
        (columns.next()? == check).then_some(status)
    })
}

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
    let file = fx.checkout.join("flake.nix");
    let single = std::fs::read_to_string(&file).unwrap();
    let host = format!("nixosConfigurations.\"{}\" = {{ }};", common::hostname());
    assert!(single.contains(&host));
    let only = fx.run(&["doctor"]);
    assert_eq!(status_of(&only.stdout, "config"), Some("ok"));
    assert_contains(&only.stdout, "the only configuration");

    let several = single.replace(&host, &format!("{host} nixosConfigurations.other = {{ }};"));
    std::fs::write(&file, &several).unwrap();
    commit(&fx.checkout, "two configurations");
    let ambiguous = fx.run(&["doctor"]);
    assert_ne!(ambiguous.code, 0);
    assert_eq!(status_of(&ambiguous.stdout, "config"), Some("fail"));
    assert_contains(&ambiguous.stdout, "none is named default");

    std::fs::write(
        &file,
        several.replace("nixosConfigurations.other", "nixosConfigurations.default"),
    )
    .unwrap();
    commit(&fx.checkout, "a default configuration");
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
