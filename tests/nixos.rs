//! Tests for src/nixos.rs. Tests that need their own environment are in
//! tests/environment.rs.

mod common;

use common::{assert_contains, fake_systems};
use nxf::nixos;

/// Changes summarise the dix diff between two systems as labelled facts.
#[test]
fn changes() {
    let built = fake_systems();
    let facts = nixos::changes(&built.one, &built.two).unwrap();
    assert_eq!(facts[0], ("Packages", "1 added, 1 removed".to_string()));
    assert_eq!(facts[1].0, "Size");
    assert_contains(&facts[1].1, " → ");
    assert_eq!(facts.len(), 2);
    let same = nixos::changes(&built.one, &built.one).unwrap();
    assert_eq!(same[0], ("Packages", "no changes".to_string()));
}

/// The closure of a system with more content is larger.
#[test]
fn size() {
    let built = fake_systems();
    assert!(nixos::size(&built.two).unwrap() > nixos::size(&built.one).unwrap());
}
