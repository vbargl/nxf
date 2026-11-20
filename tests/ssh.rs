//! Tests for src/ssh.rs: the options of ssh, how it trusts a host key, and
//! how a remote command is quoted. The calls of ssh are covered end to end
//! by tests/fleet.rs and tests/rollout.rs.

mod common;

use std::path::PathBuf;

use common::{assert_contains, temp_dir};
use nxf::hosts::Host;
use nxf::ssh::{keys_of, known_name, quoted, Link, Trust};

fn host(port: Option<u16>) -> Host {
    Host {
        name: "node".to_string(),
        address: "node.test".to_string(),
        user: "root".to_string(),
        port,
    }
}

/// The options without the time to connect, which the environment may set.
fn options(link: &Link) -> String {
    let all = link.options();
    let mut kept = Vec::new();
    let mut words = all.iter();
    while let Some(word) = words.next() {
        match words.next() {
            Some(option) if option.starts_with("ConnectTimeout=") => {}
            Some(option) => kept.push(format!("{word} {option}")),
            None => kept.push(word.clone()),
        }
    }
    kept.join(" ")
}

/// ssh never asks anything, and a machine on a port is reached on it.
#[test]
fn batch_and_port() {
    let link = |port| Link {
        host: host(port),
        trust: Trust::Keys,
    };
    assert_eq!(options(&link(None)), "-o BatchMode=yes");
    assert_eq!(options(&link(Some(2222))), "-o BatchMode=yes -p 2222");
    assert_eq!(link(None).options()[2], "-o");
    assert_contains(&link(None).options()[3], "ConnectTimeout=");
    assert!(link(None).trusted().is_empty());
}

/// Before the installation a key must match one the user knows, and a new
/// key is only accepted into a file of the run; afterwards only the key the
/// installation made is trusted.
#[test]
fn trust() {
    let first = Link {
        host: host(None),
        trust: Trust::First {
            seen: PathBuf::from("/run/seen"),
        },
    };
    assert_contains(&first.trusted()[0].to_string_lossy(), "/run/seen");
    assert_contains(&options(&first), "-o StrictHostKeyChecking=accept-new");
    assert_contains(&options(&first), "-o UserKnownHostsFile=/run/seen");
    let made = Link {
        host: host(Some(22)),
        trust: Trust::Made {
            known: PathBuf::from("/run/known"),
        },
    };
    assert_eq!(made.trusted(), [PathBuf::from("/run/known")]);
    assert_eq!(
        options(&made),
        "-o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/run/known -o GlobalKnownHostsFile=/dev/null -p 22"
    );
    // The files of a link are the ones the caller names, for Nix to use as well.
    let only = made.options_with(&[PathBuf::from("/a"), PathBuf::from("/b")]);
    assert!(only.contains(&"UserKnownHostsFile=/a /b".to_string()));
}

/// The keys of a file are its bytes; a file that is not there has none, and
/// one that cannot be read is an error.
#[test]
fn keys() {
    let dir = temp_dir("ssh");
    assert_eq!(keys_of(&dir.join("absent")), Ok(None));
    std::fs::write(dir.join("known"), "node ssh-ed25519 AAAA\n").unwrap();
    assert_eq!(
        keys_of(&dir.join("known")),
        Ok(Some(b"node ssh-ed25519 AAAA\n".to_vec()))
    );
    assert_contains(
        &keys_of(&dir).unwrap_err(),
        &format!("cannot read {}: ", dir.display()),
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// A machine on another port than 22 is named with it in a known_hosts file.
#[test]
fn names() {
    assert_eq!(known_name(&host(None)), "node.test");
    assert_eq!(known_name(&host(Some(22))), "node.test");
    assert_eq!(known_name(&host(Some(2222))), "[node.test]:2222");
}

/// Words are quoted in forms that sh, fish and Nushell read alike.
#[test]
fn quoting() {
    assert_eq!(quoted("status"), "status");
    assert_eq!(quoted("@1790000000"), "@1790000000");
    assert_eq!(quoted("release week"), "'release week'");
    assert_eq!(quoted(""), "''");
    assert_eq!(quoted("it's bad"), "\"it's bad\"");
    assert_eq!(quoted("say \"no\" & $HOME"), "'say \"no\" & $HOME'");
    assert_eq!(quoted("it's $HOME"), "'it'\\''s $HOME'");
}
