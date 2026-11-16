//! Tests for src/nixos.rs. Tests that need their own environment are in
//! tests/environment.rs.

mod common;

use common::{fake_systems, hostname};
use nxf::nixos;

/// Changes summarise the dix diff between two systems.
#[test]
fn changes() {
    let built = fake_systems();
    let lines = nixos::changes(&built.one, &built.two).unwrap();
    let counts: Vec<&String> = lines
        .iter()
        .filter(|line| line.ends_with(')') && !line.contains(':'))
        .collect();
    assert_eq!(counts, ["ADDED (1)", "REMOVED (1)"]);
    assert!(lines.iter().any(|line| line.starts_with("PATHS: ")));
    assert!(lines.iter().any(|line| line.starts_with("SIZE: ")));
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
