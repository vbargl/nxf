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
