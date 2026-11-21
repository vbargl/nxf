//! Tests for src/state.rs. Tests that need their own environment are in
//! tests/environment.rs.

mod common;

use std::path::Path;

use common::{fake_systems, kept_sources};
use nxf::state;

/// The records, the results and the mark of an installation have one name
/// each, in the updater's state.
#[test]
fn names() {
    assert_eq!(state::SYSTEMS, "systems");
    assert_eq!(state::INSTALLED, "installed.json");
    assert_eq!(state::UPDATER, "/var/lib/nxf");
    assert_eq!(
        state::update_file(Path::new("/state")),
        Path::new("/state/update.json")
    );
}

/// A directory can be used when this process can search it and create entries
/// in it; a file, a missing path and a directory it may only read cannot.
#[test]
fn writable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = common::temp_dir("state");
    assert!(state::writable_directory(&dir));
    assert!(!state::writable_directory(&dir.join("missing")));
    std::fs::write(dir.join("file"), "").unwrap();
    assert!(!state::writable_directory(&dir.join("file")));
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    // The superuser may write anywhere.
    let superuser = unsafe { libc::geteuid() } == 0;
    assert_eq!(state::writable_directory(&locked), superuser);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

/// The sources of the build before stay kept until those of the new build
/// are: when the new ones cannot be kept, the old ones are still there, and
/// when they can, they replace the old ones.
#[test]
fn sources_stay_until_replaced() {
    let mut fx = common::fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    // Two inputs with the same content are one source, so they are made to differ.
    fx.bump(&["a"]);
    fx.run(&["update"]).ok();
    assert_eq!(kept_sources(&fx), ["1", "2"]);

    // A nix that cannot make the roots of the sources, and is nix otherwise.
    let nix = common::sh("sh", &["-c", "command -v nix"]);
    common::write_script(
        &fx.dir.join("bin/nix"),
        &format!(
            "#!/bin/sh\ncase \"$*\" in *\"/sources/input\"*) [ -z \"$NXF_TEST_ROOTS_FAIL\" ] || {{ echo 'error: no space left on device' >&2; exit 1; }} ;; esac\nexec '{nix}' \"$@\"\n"
        ),
    );
    fx.set("NXF_TEST_ROOTS_FAIL", "1");
    fx.bump(&["a", "b"]);
    let failed = fx.run(&["update"]);
    failed.ok();
    common::assert_contains(
        &failed.stderr,
        "the sources of the inputs could not be kept in the Nix store: error: no space left on device",
    );
    assert_eq!(kept_sources(&fx), ["1", "2"]);

    fx.unset("NXF_TEST_ROOTS_FAIL");
    fx.bump(&["a"]);
    fx.run(&["update"]).ok();
    assert_eq!(kept_sources(&fx), ["2", "4"]);

    // Both inputs with the same content are one source: the root of the other goes.
    fx.bump(&["b"]);
    fx.bump(&["b"]);
    fx.run(&["update"]).ok();
    assert_eq!(kept_sources(&fx), ["4"]);
}

/// The sources of a tree that is kept are not kept again, by a build or by
/// the switch of an upgrade, and a new tree keeps its own.
#[test]
fn sources_kept_once() {
    let fx = common::fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let nix = common::spy(&fx, "nix");
    let archives = || {
        common::calls(&nix)
            .iter()
            .filter(|call| call.starts_with("flake archive"))
            .count()
    };
    fx.run(&["build"]).ok();
    fx.run(&["build"]).ok();
    assert_eq!(archives(), 1);
    assert_eq!(kept_sources(&fx), ["1"]);

    // An upgrade keeps the sources of the tree it commits once, not twice.
    fx.bump(&["a", "b"]);
    fx.run(&["upgrade"]).ok();
    assert_eq!(archives(), 2);
    assert_eq!(kept_sources(&fx), ["2"]);
    let marks: Vec<_> = std::fs::read_dir(fx.state.join("nxf/kept"))
        .unwrap()
        .map(|mark| mark.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(marks, [fx.tree()]);
}

/// A mark whose roots are gone does not make the build skip: the sources are
/// kept again, and so are those of a tree whose mark was replaced.
#[test]
fn stale_mark_keeps_again() {
    let fx = common::fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let nix = common::spy(&fx, "nix");
    let archives = || {
        common::calls(&nix)
            .iter()
            .filter(|call| call.starts_with("flake archive"))
            .count()
    };
    fx.run(&["build"]).ok();
    let mark = fx.state.join("nxf/kept").join(fx.tree());
    assert!(mark.exists());
    let roots = fx.state.join("nxf/sources");
    std::fs::remove_dir_all(&roots).unwrap();
    assert!(mark.exists());
    fx.run(&["build"]).ok();
    assert_eq!(archives(), 2);
    assert_eq!(kept_sources(&fx), ["1"]);

    // A root that leads elsewhere is no root of the marked sources.
    for root in std::fs::read_dir(&roots).unwrap() {
        let root = root.unwrap().path();
        std::fs::remove_file(&root).unwrap();
        std::os::unix::fs::symlink(&fake_systems().one, &root).unwrap();
    }
    fx.run(&["build"]).ok();
    assert_eq!(archives(), 3);
    assert_eq!(kept_sources(&fx), ["1"]);

    // The mark of another tree does not stand for this one.
    std::fs::rename(&mark, mark.with_file_name("0000")).unwrap();
    fx.run(&["build"]).ok();
    assert_eq!(archives(), 4);
    assert!(mark.exists());
    assert!(!mark.with_file_name("0000").exists());
}
