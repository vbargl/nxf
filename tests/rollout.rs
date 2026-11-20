//! Tests for src/rollout.rs: what each machine is found to be, and what is
//! planned for it. The installation itself needs a real machine; it is
//! checked by hand on a virtual machine, not here.

mod common;

use common::{
    assert_contains, assert_lacks, commit, fixture, git, temp_dir, write_script, Fixture,
};
use nxf::hosts::Host;
use nxf::rollout::{known_hosts_line, parse_probe, template, Found, PROBE};

/// The probe's answer says what a machine is.
#[test]
fn probe() {
    let parsed = |text: &str| parse_probe(text).map(|probe| (probe.found, probe.arch));
    assert_eq!(
        parsed("nxf-probe managed=yes installer=no arch=x86_64\nnxf-os NixOS 26.05\n"),
        Some((Found::Managed, "x86_64".to_string()))
    );
    assert_eq!(
        parsed("motd\nnxf-probe managed=no installer=yes arch=aarch64\nnxf-os NixOS\n"),
        Some((Found::Installer, "aarch64".to_string()))
    );
    assert_eq!(
        parsed("nxf-probe managed=no installer=no arch=x86_64\nnxf-os Ubuntu 24.04.5 LTS\n"),
        Some((
            Found::Other("Ubuntu 24.04.5 LTS".to_string()),
            "x86_64".to_string()
        ))
    );
    assert_eq!(parsed("Welcome!\n"), None);
    assert_eq!(parsed(""), None);
}

/// The known_hosts line names a port only when it is not the default.
#[test]
fn known_hosts() {
    let host = |port| Host {
        name: "node".to_string(),
        address: "10.0.0.5".to_string(),
        user: "root".to_string(),
        port,
    };
    assert_eq!(
        known_hosts_line(&host(None), "ssh-ed25519 AAAA"),
        "10.0.0.5 ssh-ed25519 AAAA"
    );
    assert_eq!(
        known_hosts_line(&host(Some(22)), "ssh-ed25519 AAAA"),
        "10.0.0.5 ssh-ed25519 AAAA"
    );
    assert_eq!(
        known_hosts_line(&host(Some(2222)), "ssh-ed25519 AAAA"),
        "[10.0.0.5]:2222 ssh-ed25519 AAAA"
    );
}

/// The source of the template is in the Nix store when it is named, with the
/// uncommitted changes of its checkout, because it is copied to the machines
/// from there. Nix names the store path of a flake before it writes it.
#[test]
fn template_source_is_stored() {
    let dir = temp_dir("template");
    let unique = format!("{:?} {:?}", dir, std::time::SystemTime::now());
    std::fs::write(
        dir.join("flake.nix"),
        "{ outputs = _: { templates.machine = { path = ./machine; description = \"\"; }; }; }\n",
    )
    .unwrap();
    std::fs::create_dir(dir.join("machine")).unwrap();
    std::fs::write(dir.join("machine/value"), &unique).unwrap();
    git(&dir, &["init", "--quiet", "--initial-branch", "main"]);
    commit(&dir, "init");
    let changed = format!("{unique} changed");
    std::fs::write(dir.join("machine/value"), &changed).unwrap();

    let (source, name) = template(&format!("{}#machine", dir.display())).unwrap();
    assert_eq!(name, "machine");
    assert!(source.starts_with("/nix/store"), "{}", source.display());
    assert_eq!(
        std::fs::read_to_string(source.join("machine/value")).ok(),
        Some(changed)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Machines behind a mock ssh that answers the probe from a file.
fn machines(answers: &[(&str, &str)]) -> (Fixture, String) {
    let mut fx = fixture();
    let dir = fx.dir.join("machines");
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = String::new();
    for (name, answer) in answers {
        std::fs::write(dir.join(format!("{name}.probe")), answer).unwrap();
        file.push_str(&format!("[hosts.{name}.ssh]\naddress = \"{name}\"\n\n"));
    }
    let ssh = dir.join("ssh");
    write_script(
        &ssh,
        &format!(
            r#"#!/usr/bin/env bash
while [[ $1 == -* ]]; do shift 2; done
destination=$1; shift
echo "$destination $*" >> '{dir}/ssh.log'
answer='{dir}'/${{destination#*@}}.probe
[[ -s $answer ]] || {{ echo "ssh: connect to host ${{destination#*@}} port 22: No route to host" >&2; exit 255; }}
cat "$answer"
"#,
            dir = dir.display()
        ),
    );
    fx.set("NXF_SSH", &ssh);
    let hosts = dir.join("hosts.toml");
    std::fs::write(&hosts, file).unwrap();
    (fx, hosts.display().to_string())
}

const MANAGED: &str = "nxf-probe managed=yes installer=no arch=x86_64\nnxf-os NixOS 26.05\n";
const UBUNTU: &str = "nxf-probe managed=no installer=no arch=x86_64\nnxf-os Ubuntu 24.04.5 LTS\n";
const INSTALLER: &str = "nxf-probe managed=no installer=yes arch=x86_64\nnxf-os NixOS 25.11\n";

/// Machines that nxf installed are left alone, so running rollout again changes nothing.
#[test]
fn repeatable() {
    let (fx, hosts) = machines(&[("node-1", MANAGED), ("node-2", MANAGED)]);
    for _ in 0..2 {
        let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y"]);
        result.ok();
        assert_contains(&result.stdout, "\n  ✓ node-1    installed by nxf\n");
        assert!(
            result
                .stdout
                .ends_with("\n\n✓ 2 machines installed by nxf\n"),
            "{}",
            result.stdout
        );
    }
    // Each machine was only asked what it is.
    let log = std::fs::read_to_string(fx.dir.join("machines/ssh.log")).unwrap();
    assert_eq!(log.lines().count(), 4);
    assert!(log.lines().all(|line| line.ends_with(" sh")));
}

/// Without a confirmation nothing is erased: the plan is shown and the command fails.
#[test]
fn asks_before_erasing() {
    let (fx, hosts) = machines(&[
        ("node-1", MANAGED),
        ("node-2", UBUNTU),
        ("node-3", INSTALLER),
    ]);
    let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y"]);
    assert_eq!(result.code, 1);
    assert_contains(&result.stdout, "\n  ✓ node-1    installed by nxf\n");
    assert_contains(
        &result.stdout,
        "\n  ! node-2    Ubuntu 24.04.5 LTS; it will be erased\n",
    );
    assert_contains(
        &result.stdout,
        "\n  ! node-3    NixOS installer; its disks will be erased\n",
    );
    assert_contains(
        &result.stderr,
        "✗ 2 machines would be erased; nothing was changed\n  Add --yes to erase and install node-2, node-3.",
    );
    let log = std::fs::read_to_string(fx.dir.join("machines/ssh.log")).unwrap();
    assert_eq!(log.lines().count(), 3);
}

/// A machine that does not answer fails the command, and is never taken for
/// one to install.
#[test]
fn unreachable() {
    let (fx, hosts) = machines(&[("node-1", MANAGED), ("node-2", "")]);
    let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y", "--yes"]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stdout,
        "\n  ✗ node-2    unreachable: ssh: connect to host node-2 port 22: No route to host\n",
    );
    assert_contains(&result.stderr, "✗ 1 of 2 machines gave no answer");
    assert_lacks(&result.stdout, "will be erased");
}

/// What the probe finds on a stand-in machine: its root file system is of the
/// given kind, the named files exist, and `on_path` says whether the shell
/// finds nxf. The probe runs as it is sent, with its paths under a directory.
fn probed(kind: &str, files: &[&str], on_path: bool) -> Found {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let root = temp_dir("probe");
    let place = |file: &str| root.join(file.trim_start_matches('/'));
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for tool in ["awk", "uname"] {
        let found = common::sh("sh", &["-c", &format!("command -v {tool}")]);
        std::os::unix::fs::symlink(found, tools.join(tool)).unwrap();
    }
    write_script(&tools.join("id"), "#!/bin/sh\necho 0\n");
    if on_path {
        write_script(&tools.join("nxf"), "#!/bin/sh\n");
    }
    let mut script = PROBE.to_string();
    for file in [
        "/proc/mounts",
        "/etc/NIXOS",
        "/etc/os-release",
        "/var/lib/nxf/installed.json",
        "/run/current-system/sw/bin/nxf",
    ] {
        assert_contains(&script, file);
        script = script.replace(file, place(file).to_str().unwrap());
        std::fs::create_dir_all(place(file).parent().unwrap()).unwrap();
    }
    std::fs::write(place("/proc/mounts"), format!("none / {kind} rw 0 0\n")).unwrap();
    std::fs::write(place("/etc/os-release"), "PRETTY_NAME=\"Other 1\"\n").unwrap();
    for file in files {
        write_script(&place(file), "");
    }
    let mut shell = Command::new(common::sh("sh", &["-c", "command -v sh"]))
        .env("PATH", &tools)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    shell
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let answer = shell.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&root);
    parse_probe(&String::from_utf8_lossy(&answer.stdout))
        .unwrap()
        .found
}

/// A machine that nxf installed, or whose system has nxf, is managed whatever
/// its root file system is: one that runs from memory is not taken for an
/// installer to erase. An installer stays one, also with nxf copied to it.
#[test]
fn managed_from_memory() {
    const MARKER: &str = "/var/lib/nxf/installed.json";
    const SYSTEM: &str = "/run/current-system/sw/bin/nxf";
    assert_eq!(probed("tmpfs", &["/etc/NIXOS"], false), Found::Installer);
    assert_eq!(probed("tmpfs", &["/etc/NIXOS"], true), Found::Installer);
    for sign in [MARKER, SYSTEM] {
        assert_eq!(
            probed("tmpfs", &["/etc/NIXOS", sign], false),
            Found::Managed
        );
        assert_eq!(probed("ext4", &[sign], false), Found::Managed);
    }
    assert_eq!(probed("ext4", &["/etc/NIXOS"], true), Found::Managed);
    assert_eq!(
        probed("ext4", &[], false),
        Found::Other("Other 1".to_string())
    );
}
