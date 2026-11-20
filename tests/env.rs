//! Tests for src/env.rs: what nxf takes from the environment, end to end.
//! Tests that need the process environment itself are in tests/environment.rs.

mod common;

use common::{fixture, write_script, Fixture};

/// A hosts file with one machine, and a mock ssh that logs how it was called.
/// The log holds the options, the destination and the command, one call a line.
fn machine(fx: &Fixture) -> (String, std::path::PathBuf) {
    let log = fx.dir.join("ssh.log");
    write_script(
        &fx.dir.join("bin/ssh"),
        &format!("#!/bin/sh\necho \"$*\" >> '{}'\n", log.display()),
    );
    let hosts = fx.dir.join("hosts.toml");
    std::fs::write(&hosts, "[hosts.node.ssh]\naddress = \"node.test\"\n").unwrap();
    (hosts.display().to_string(), log)
}

/// nxf enables flakes for the tools it runs, after the Nix settings that its
/// caller gave it.
#[test]
fn nix_config() {
    let mut fx = fixture().with("NIX_CONFIG", "warn-dirty = false");
    let seen = fx.dir.join("nix-config");
    // Status asks the stand-in for systemctl, which notes the setting it got.
    write_script(
        &fx.systemctl,
        &format!(
            "#!/bin/sh\nprintf '%s' \"$NIX_CONFIG\" > '{}'\n",
            seen.display()
        ),
    );
    let flakes = "extra-experimental-features = nix-command flakes";
    fx.run(&["status"]).ok();
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap(),
        format!("warn-dirty = false\n{flakes}")
    );
    fx.unset("NIX_CONFIG");
    fx.run(&["status"]).ok();
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), flakes);
}

/// A time is a whole number of seconds. Anything else is the default.
#[test]
fn seconds() {
    let mut fx = fixture();
    let (hosts, log) = machine(&fx);
    for (value, seconds) in [
        (None, "10"),
        (Some("3"), "3"),
        (Some("soon"), "10"),
        (Some(""), "10"),
    ] {
        match value {
            Some(value) => fx.set("NXF_SSH_CONNECT_TIMEOUT", value),
            None => fx.unset("NXF_SSH_CONNECT_TIMEOUT"),
        }
        let _ = std::fs::remove_file(&log);
        fx.run(&["version", "--config", &hosts]).ok();
        let called = std::fs::read_to_string(&log).unwrap();
        assert!(
            called.contains(&format!("ConnectTimeout={seconds} ")),
            "{value:?}: {called}"
        );
    }
}

/// A variable that is empty is not set: NXF_SSH then names the ssh of the PATH.
#[test]
fn empty() {
    let mut fx = fixture();
    let (hosts, log) = machine(&fx);
    fx.set("NXF_SSH", "");
    fx.run(&["version", "--config", &hosts]).ok();
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains("root@node.test nxf version"));
}
