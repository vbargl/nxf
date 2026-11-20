//! Tests for src/hosts.rs: the hosts file.

mod common;

use std::path::Path;

use common::assert_contains;
use nxf::hosts::{self, Format, Host};

fn host(name: &str, address: &str, user: &str, port: Option<u16>) -> Host {
    Host {
        name: name.to_string(),
        address: address.to_string(),
        user: user.to_string(),
        port,
    }
}

const TOML: &str = r#"
title = "a set of machines"

[k3s]
version = "1.31"

[hosts.node-2.ssh]
address = "10.0.0.12"
user = "admin"
port = 2222
keyPath = "~/.ssh/other"

[hosts.node-1]
note = "kept by another tool"

[hosts.node-1.ssh]
address = "node-1.example.org"

[hosts.node-1.k3s]
role = "server"
"#;

/// nxf reads the ssh table of each host; the user defaults to root, hosts
/// come in name order, and every other key or section is ignored.
#[test]
fn toml() {
    assert_eq!(
        hosts::parse(TOML, Format::Toml).unwrap(),
        [
            host("node-1", "node-1.example.org", "root", None),
            host("node-2", "10.0.0.12", "admin", Some(2222)),
        ]
    );
}

/// JSON and YAML describe the same hosts as TOML.
#[test]
fn json_and_yaml() {
    let expected = hosts::parse(TOML, Format::Toml).unwrap();
    let json = r#"{
        "k3s": {"version": "1.31"},
        "hosts": {
            "node-2": {"ssh": {"address": "10.0.0.12", "user": "admin", "port": 2222}},
            "node-1": {"ssh": {"address": "node-1.example.org"}, "k3s": {"role": "server"}}
        }
    }"#;
    assert_eq!(hosts::parse(json, Format::Json).unwrap(), expected);
    let yaml = "
k3s:
  version: '1.31'
hosts:
  node-2:
    ssh: {address: 10.0.0.12, user: admin, port: 2222}
  node-1:
    ssh:
      address: node-1.example.org
    k3s:
      role: server
";
    assert_eq!(hosts::parse(yaml, Format::Yaml).unwrap(), expected);
}

/// The format follows the file name.
#[test]
fn formats() {
    for (name, format) in [
        ("hosts.toml", Format::Toml),
        ("hosts.json", Format::Json),
        ("hosts.yaml", Format::Yaml),
        ("hosts.yml", Format::Yaml),
    ] {
        assert_eq!(Format::of(Path::new(name)), Ok(format));
    }
    assert_contains(
        &Format::of(Path::new("hosts.hcl")).unwrap_err(),
        "no known format",
    );
    assert!(Format::of(Path::new("hosts")).is_err());
}

/// A file that names no usable host is an error that says what is missing.
#[test]
fn invalid() {
    let error = |text: &str| hosts::parse(text, Format::Toml).unwrap_err();
    assert_contains(&error("title = 'x'"), "no hosts table");
    assert_contains(&error("[hosts]"), "empty");
    assert_contains(&error("[hosts.a]\nrole = 'x'"), "host a has no ssh.address");
    assert_contains(
        &error("[hosts.a.ssh]\naddress = ''"),
        "host a has no ssh.address",
    );
    assert_contains(
        &error("[hosts.a.ssh]\naddress = 'x'\nport = 70000"),
        "invalid ssh.port",
    );
    assert_contains(
        &error("[hosts.a.ssh]\naddress = 'x'\nuser = 5"),
        "invalid ssh.user",
    );
    assert!(!error("[hosts.a").is_empty());
    assert!(hosts::load(Path::new("/nonexistent/hosts.toml"))
        .unwrap_err()
        .starts_with("Cannot read /nonexistent/hosts.toml"));
}
