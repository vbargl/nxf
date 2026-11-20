//! Tests for src/nixos.rs. Tests that need their own environment are in
//! tests/environment.rs.

mod common;

use common::{fake_systems, hostname};
use nxf::nixos;

/// Changes summarise the dix diff between two systems as labelled facts.
#[test]
fn changes() {
    let built = fake_systems();
    let facts = nixos::changes(&built.one, &built.two).unwrap();
    assert_eq!(facts[0], ("Packages", "1 added, 1 removed".to_string()));
    assert_eq!(facts[1].0, "Size");
    assert!(facts[1].1.contains(" → "), "{}", facts[1].1);
    assert_eq!(facts.len(), 2);
    let same = nixos::changes(&built.one, &built.one).unwrap();
    assert_eq!(same[0], ("Packages", "no changes".to_string()));
}

/// Size is the closure size of a system.
#[test]
fn size() {
    let built = fake_systems();
    assert!(nixos::size(&built.two).unwrap() > nixos::size(&built.one).unwrap());
}

/// The host name is the machine's, which nh selects the configuration by.
#[test]
fn host_name() {
    assert_eq!(nixos::hostname(), hostname());
    assert!(!nixos::hostname().is_empty());
}

/// The versions of the input sources kept as roots in the state, sorted.
fn kept_sources(fx: &common::Fixture) -> Vec<String> {
    let mut versions: Vec<String> = std::fs::read_dir(fx.state.join("nxf/sources"))
        .map(|entries| {
            entries
                .map(|entry| {
                    let source = std::fs::canonicalize(entry.unwrap().path()).unwrap();
                    std::fs::read_to_string(source.join("v")).unwrap()
                })
                .collect()
        })
        .unwrap_or_default();
    versions.sort();
    versions
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
}
