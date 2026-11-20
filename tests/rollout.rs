//! Tests for src/rollout.rs: what each machine is found to be, and what is
//! planned for it. The steps of an installation are played behind a mock ssh;
//! a real installation is checked by hand on a virtual machine.

mod common;

use common::{
    assert_contains, assert_lacks, commit, fixture, git, temp_dir, write_script, Fixture,
};
use nxf::hosts::Host;
use nxf::rollout::{known_hosts_line, parse_probe, template, Found, Probe, PROBE};

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
fn probed(kind: &str, files: &[&str], on_path: bool) -> Probe {
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
    write_script(&tools.join("id"), "#!/bin/sh\necho 1000\n");
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
    parse_probe(&String::from_utf8_lossy(&answer.stdout)).unwrap()
}

/// A machine that nxf installed, or whose system has nxf, is managed whatever
/// its root file system is: one that runs from memory is not taken for an
/// installer to erase. An installer stays one, also with nxf copied to it.
#[test]
fn managed_from_memory() {
    const MARKER: &str = "/var/lib/nxf/installed.json";
    const SYSTEM: &str = "/run/current-system/sw/bin/nxf";
    let found = |kind: &str, files: &[&str], on_path: bool| probed(kind, files, on_path).found;
    assert_eq!(found("tmpfs", &["/etc/NIXOS"], false), Found::Installer);
    assert_eq!(found("tmpfs", &["/etc/NIXOS"], true), Found::Installer);
    for sign in [MARKER, SYSTEM] {
        assert_eq!(found("tmpfs", &["/etc/NIXOS", sign], false), Found::Managed);
        assert_eq!(found("ext4", &[sign], false), Found::Managed);
    }
    assert_eq!(found("ext4", &["/etc/NIXOS"], true), Found::Managed);
    assert_eq!(
        found("ext4", &[], false),
        Found::Other("Other 1".to_string())
    );
}

const ROOT_INSTALLER: &str =
    "nxf-probe managed=no installer=yes uid=0 arch=x86_64\nnxf-os NixOS 25.11\n";

/// Installers behind a mock ssh that plays an installation: it keeps the sent
/// files, reports a host key for `nxf install`, and answers as managed from
/// then on. A mock nix names the template's source; NXF_PACKAGE stands in for nxf.
fn installers(names: &[&str]) -> (Fixture, String) {
    let answers: Vec<(&str, &str)> = names.iter().map(|name| (*name, ROOT_INSTALLER)).collect();
    let (mut fx, hosts) = machines(&answers);
    let dir = fx.dir.join("machines");
    write_script(
        &dir.join("ssh"),
        &format!(
            r#"#!/usr/bin/env bash
while [[ $1 == -* ]]; do shift 2; done
destination=$1; shift
name=${{destination#*@}}
echo "$name $*" >> '{dir}/ssh.log'
case "$*" in
    sh) cat > /dev/null; cat '{dir}'/"$name.probe" ;;
    *'tar --extract'*) cat > '{dir}'/"$name.tar" ;;
    *'bin/nxf install '*)
        echo "  Host key  ssh-ed25519 AAAA$name"
        printf 'nxf-probe managed=yes installer=no uid=0 arch=x86_64\nnxf-os NixOS\n' > '{dir}'/"$name.probe" ;;
    'nxf status --json') echo '{{"revision": "0123456789abcdef"}}' ;;
esac
"#,
            dir = dir.display()
        ),
    );
    write_script(
        &fx.dir.join("bin/nix"),
        &format!(
            r#"#!/usr/bin/env bash
echo "$* | ${{NIX_SSHOPTS:-}}" >> '{dir}/nix.log'
case "$1 $2" in
    'flake prefetch') echo '{{"storePath": "/nix/store/template-source"}}' ;;
    'copy '*) [[ -z ${{NXF_TEST_COPY_FAILS:-}} ]] || {{ echo 'error: cannot connect' >&2; exit 1; }} ;;
    *) exit 1 ;;
esac
"#,
            dir = dir.display()
        ),
    );
    fx.set("NXF_PACKAGE", "/nix/store/nxf-package");
    // The keys the user knows are read for `nix copy`: not those of whoever runs the tests.
    fx.set("HOME", fx.dir.join("home"));
    (fx, hosts)
}

fn ssh_log(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.dir.join("machines/ssh.log")).unwrap_or_default()
}

/// The files of a machine are packed straight into the connection, and an
/// archive that tar could not finish is not sent on as complete: the machine
/// fails with what tar said, and nothing is founded from half of its files.
#[test]
fn unpacked_files_fail_the_machine() {
    let (fx, hosts) = installers(&["node-1"]);
    let files = fx.dir.join("files");
    std::fs::create_dir_all(files.join("node-1")).unwrap();
    std::fs::write(files.join("node-1/identity.toml"), "name = \"node-1\"\n").unwrap();
    let rollout = |fx: &Fixture| {
        fx.run(&[
            "rollout",
            "--config",
            &hosts,
            "--template",
            "x#y",
            "--yes",
            "--files",
            files.to_str().unwrap(),
        ])
    };
    let tar = fx.dir.join("bin/tar");
    write_script(
        &tar,
        "#!/bin/sh\nprintf 'half an archive'\necho 'tar: ./secret: Cannot open: Permission denied' >&2\nexit 2\n",
    );
    let result = rollout(&fx);
    assert_eq!(result.code, 1);
    assert_contains(&result.stdout, "\n  ✗ Sending the machine's files failed\n");
    assert_contains(
        &result.stdout,
        "tar: ./secret: Cannot open: Permission denied",
    );
    assert_contains(&result.stderr, "Not installed: node-1.");
    assert_lacks(&ssh_log(&fx), "bin/nxf init");

    // With a tar that works, the machine gets its files and is installed.
    std::fs::remove_file(&tar).unwrap();
    std::fs::write(fx.dir.join("machines/node-1.probe"), ROOT_INSTALLER).unwrap();
    let result = rollout(&fx);
    result.ok();
    let archive = fx.dir.join("machines/node-1.tar");
    let sent = common::sh("tar", &["--list", "--file", archive.to_str().unwrap()]);
    assert_contains(&sent, "./identity.toml");
    assert_contains(&ssh_log(&fx), "--files /tmp/nxf-files");
    assert_contains(&result.stdout, "\n✓ 1 machine installed\n");
}

/// The upload fails as well when the machine could not unpack what it was
/// sent: the machine fails with what ssh said, and nothing is founded there.
#[test]
fn unreceived_files_fail_the_machine() {
    let (mut fx, hosts) = installers(&["node-1"]);
    let files = fx.dir.join("files");
    std::fs::create_dir_all(files.join("node-1")).unwrap();
    std::fs::write(files.join("node-1/identity.toml"), "name = \"node-1\"\n").unwrap();
    // An ssh whose machine takes the whole archive and cannot unpack it.
    let ssh = fx.dir.join("machines/ssh-full");
    write_script(
        &ssh,
        &format!(
            r#"#!/usr/bin/env bash
if [[ ${{*: -1}} == *'tar --extract'* ]]; then
    cat > /dev/null
    echo 'tar: ./identity.toml: Cannot write: No space left on device' >&2
    exit 2
fi
exec '{}/machines/ssh' "$@"
"#,
            fx.dir.display()
        ),
    );
    fx.set("NXF_SSH", &ssh);
    let result = fx.run(&[
        "rollout",
        "--config",
        &hosts,
        "--template",
        "x#y",
        "--yes",
        "--files",
        files.to_str().unwrap(),
    ]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stdout,
        "\n  ✗ Sending the machine's files failed\n    tar: ./identity.toml: Cannot write: No space left on device\n",
    );
    assert_contains(&result.stderr, "Not installed: node-1.");
    assert_lacks(&ssh_log(&fx), "bin/nxf init");
}

/// A `--files` that is no directory is refused before any machine is asked,
/// instead of installing from the template alone. A machine without a
/// directory of its own in it is still installed, from the template alone.
#[test]
fn files_must_be_a_directory() {
    let (fx, hosts) = installers(&["node-1"]);
    let rollout = |files: &std::path::Path| {
        fx.run(&[
            "rollout",
            "--config",
            &hosts,
            "--template",
            "x#y",
            "--yes",
            "--files",
            files.to_str().unwrap(),
        ])
    };
    let file = fx.dir.join("a-file");
    std::fs::write(&file, "").unwrap();
    for wrong in [fx.dir.join("missing"), file] {
        let result = rollout(&wrong);
        assert_eq!(result.code, 1);
        assert_contains(
            &result.stderr,
            &format!("✗ {} is not a directory\n", wrong.display()),
        );
        assert_eq!(ssh_log(&fx), "");
    }

    let files = fx.dir.join("files");
    std::fs::create_dir_all(files.join("node-2")).unwrap();
    rollout(&files).ok();
    let log = ssh_log(&fx);
    assert_contains(&log, "node-1 /nix/store/nxf-package/bin/nxf install ");
    assert_lacks(&log, "--files");
}

/// A known_hosts file that cannot be written does not end the run: the
/// machine stays installed, the cause is shown, the next machine is still
/// installed, both host keys are printed, and the command fails.
#[test]
fn unwritten_known_hosts() {
    let (fx, hosts) = installers(&["node-1", "node-2"]);
    let file = fx.dir.join("missing/known_hosts");
    let result = fx.run(&[
        "rollout",
        "--config",
        &hosts,
        "--template",
        "x#y",
        "--yes",
        "--known-hosts",
        file.to_str().unwrap(),
    ]);
    assert_eq!(result.code, 1);
    let log = ssh_log(&fx);
    assert_contains(&log, "node-1 /nix/store/nxf-package/bin/nxf install ");
    assert_contains(&log, "node-2 /nix/store/nxf-package/bin/nxf install ");
    assert_contains(
        &result.stdout,
        &format!(
            "\n  ✗ Its host key was not written: cannot write {}: ",
            file.display()
        ),
    );
    assert_contains(
        &result.stderr,
        &format!(
            "✗ 2 machines installed, but not every host key was written\n  Not written to {}: node-1, node-2.",
            file.display()
        ),
    );
    for name in ["node-1", "node-2"] {
        assert_contains(
            &result.stdout,
            &format!("\n  {name:<8}  0123456789ab\n  {name} ssh-ed25519 AAAA{name}\n"),
        );
    }
}

/// The key of the system that was there before is replaced by the new one.
/// When it cannot be removed, that is said and the command fails, instead of
/// the old key staying trusted beside the new one.
#[test]
fn old_key_is_replaced_or_said() {
    let (fx, hosts) = installers(&["node-1"]);
    let file = fx.dir.join("known_hosts");
    let before = "node-1 ssh-ed25519 AAAAbefore\nnode-9 ssh-ed25519 AAAAother\n";
    std::fs::write(&file, before).unwrap();
    let rollout = |fx: &Fixture| {
        fx.run(&[
            "rollout",
            "--config",
            &hosts,
            "--template",
            "x#y",
            "--yes",
            "--known-hosts",
            file.to_str().unwrap(),
        ])
    };
    let keygen = fx.dir.join("bin/ssh-keygen");
    write_script(
        &keygen,
        "#!/bin/sh\necho 'link known_hosts to known_hosts.old: Operation not permitted' >&2\nexit 255\n",
    );
    let result = rollout(&fx);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stdout,
        &format!(
            "\n  ✗ Its host key was not written: cannot remove the old key of node-1 from {}: link known_hosts to known_hosts.old: Operation not permitted\n",
            file.display()
        ),
    );
    assert_contains(&result.stdout, "\n  node-1 ssh-ed25519 AAAAnode-1\n");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);

    std::fs::remove_file(&keygen).unwrap();
    std::fs::write(fx.dir.join("machines/node-1.probe"), ROOT_INSTALLER).unwrap();
    rollout(&fx).ok();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "node-9 ssh-ed25519 AAAAother\nnode-1 ssh-ed25519 AAAAnode-1\n"
    );
}

/// The probe tells the user id, and a machine that would be installed by a
/// user other than root is refused in the plan, before anything is erased:
/// its row names the cause, it is not installed, and the command fails. The
/// other machines are still installed, and a managed machine is not affected.
#[test]
fn installs_only_as_root() {
    let user = |answer: &str| parse_probe(answer).and_then(|probe| probe.user);
    assert_eq!(user(ROOT_INSTALLER).as_deref(), Some("0"));
    assert_eq!(user(INSTALLER), None);

    let (fx, hosts) = installers(&["node-1", "node-2", "node-3", "node-4"]);
    let answer = |name: &str, answer: &str| {
        std::fs::write(fx.dir.join(format!("machines/{name}.probe")), answer).unwrap();
    };
    answer(
        "node-1",
        "nxf-probe managed=no installer=no uid=1000 arch=x86_64\nnxf-os Ubuntu 24.04.5 LTS\n",
    );
    answer(
        "node-2",
        "nxf-probe managed=no installer=yes uid=1000 arch=x86_64\nnxf-os NixOS 25.11\n",
    );
    answer(
        "node-3",
        "nxf-probe managed=yes installer=no uid=1000 arch=x86_64\nnxf-os NixOS 26.05\n",
    );
    let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y", "--yes"]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stdout,
        "\n  ✗ node-1    Ubuntu 24.04.5 LTS; not installed: the SSH user must be root, and its user id is 1000\n",
    );
    assert_contains(
        &result.stdout,
        "\n  ✗ node-2    NixOS installer; not installed: the SSH user must be root, and its user id is 1000\n",
    );
    assert_contains(&result.stdout, "\n  ✓ node-3    installed by nxf\n");
    assert_contains(
        &result.stdout,
        "\n  ! node-4    NixOS installer; its disks will be erased\n",
    );
    assert_contains(
        &result.stderr,
        "✗ 2 of 4 machines not rolled out\n  Not installed: node-1, node-2.",
    );
    // Only the machine of root went beyond the probe.
    let log = ssh_log(&fx);
    for line in log.lines().filter(|line| !line.ends_with(" sh")) {
        assert!(line.starts_with("node-4 "), "{log}");
    }
    assert_contains(&log, "node-4 /nix/store/nxf-package/bin/nxf install ");

    // With nothing left to install, the refused machines still fail the command.
    answer("node-4", MANAGED);
    let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y"]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stderr,
        "✗ 2 of 4 machines not rolled out\n  Not installed: node-1, node-2.",
    );
}

/// The probe as it is sent tells the user id of whoever it runs as.
#[test]
fn probe_tells_the_user() {
    assert_eq!(probed("ext4", &[], false).user.as_deref(), Some("1000"));
}

/// A template that cannot be used is found before the question, not after
/// the answer: nobody confirms an installation that then cannot start.
#[test]
fn bad_template_before_the_question() {
    let (fx, hosts) = installers(&["node-1"]);
    let result = fx.terminal(
        100,
        &["rollout", "--config", &hosts, "--template", "no-name"],
    );
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stdout,
        "✗ The template must be given as <flake>#<name>",
    );
    assert_lacks(&result.stdout, "Type yes");
    // Nothing went beyond the probe.
    assert_eq!(ssh_log(&fx).lines().count(), 1);
}

/// `nix copy` connects with the trust of the link: one file holds the keys
/// this run accepted and the keys the user knows, so that a key that differs
/// from a known one is refused there too. No option needs a shell to split it.
#[test]
fn copy_trusts_as_the_link() {
    let (mut fx, hosts) = installers(&["node-1"]);
    let dir = fx.dir.join("machines");
    let known = fx.dir.join("home/.ssh/known_hosts");
    std::fs::create_dir_all(known.parent().unwrap()).unwrap();
    std::fs::write(&known, "node-9 ssh-ed25519 AAAAuser").unwrap();
    // An ssh that accepts the key of the machine into the first file it is given.
    let ssh = dir.join("ssh-accepting");
    write_script(
        &ssh,
        &format!(
            r#"#!/usr/bin/env bash
for option in "$@"; do
    [[ $option == UserKnownHostsFile=* ]] || continue
    files=${{option#*=}}
    echo 'node-1 ssh-ed25519 AAAAseen' > "${{files%% *}}"
done
exec '{dir}/ssh' "$@"
"#,
            dir = dir.display()
        ),
    );
    fx.set("NXF_SSH", &ssh);
    // A nix that splits NIX_SSHOPTS at spaces alone, and keeps the file it names.
    write_script(
        &fx.dir.join("bin/nix"),
        &format!(
            r#"#!/usr/bin/env bash
case "$1 $2" in
    'flake prefetch') echo '{{"storePath": "/nix/store/template-source"}}' ;;
    'copy '*)
        echo "$* | $NIX_SSHOPTS" >> '{dir}/nix.log'
        for option in $NIX_SSHOPTS; do
            [[ $option != UserKnownHostsFile=* ]] || cp "${{option#*=}}" '{dir}/copy.known_hosts'
        done
        echo 'error: cannot connect' >&2; exit 1 ;;
esac
"#,
            dir = dir.display()
        ),
    );
    let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y", "--yes"]);
    assert_eq!(result.code, 1);
    assert_contains(&result.stdout, "\n  ✗ Copying nxf to the machine failed\n");
    let log = std::fs::read_to_string(dir.join("nix.log")).unwrap();
    let (command, options) = log.trim_end().split_once(" | ").unwrap();
    assert_eq!(
        command,
        "copy --no-check-sigs --to ssh-ng://root@node-1 /nix/store/nxf-package /nix/store/template-source"
    );
    assert_lacks(options, "'");
    assert_lacks(options, "\"");
    // Split at spaces, every option is whole: a flag and its one value.
    let words: Vec<&str> = options.split(' ').collect();
    let names: Vec<&str> = words
        .chunks(2)
        .map(|option| {
            assert_eq!(option[0], "-o", "{options}");
            option[1].split_once('=').unwrap().0
        })
        .collect();
    assert_eq!(
        names,
        [
            "BatchMode",
            "ConnectTimeout",
            "StrictHostKeyChecking",
            "UserKnownHostsFile"
        ]
    );
    assert_contains(options, " -o StrictHostKeyChecking=accept-new ");
    let trusted = std::fs::read_to_string(dir.join("copy.known_hosts")).unwrap();
    assert_contains(&trusted, "node-1 ssh-ed25519 AAAAseen\n");
    assert_contains(&trusted, "node-9 ssh-ed25519 AAAAuser\n");
}

/// A temporary directory whose path ssh options cannot carry is refused
/// before any machine is asked: its files are named in those options.
#[test]
fn unusable_scratch_is_refused() {
    let (mut fx, hosts) = installers(&["node-1"]);
    let scratch = fx.dir.join("with space");
    std::fs::create_dir_all(&scratch).unwrap();
    fx.set("TMPDIR", &scratch);
    let result = fx.run(&["rollout", "--config", &hosts, "--template", "x#y", "--yes"]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stderr,
        &format!(
            "✗ The temporary directory {} has a space or a quote in its path\n",
            scratch.display()
        ),
    );
    assert_eq!(ssh_log(&fx), "");
}

/// What the operator names reaches the shell of the machine as one word each,
/// whatever it holds: a template name with a space, an image address with a quote.
#[test]
fn names_reach_the_machine_whole() {
    let (mut fx, hosts) = installers(&["node-1"]);
    let dir = fx.dir.join("machines");
    std::fs::write(dir.join("installer.probe"), ROOT_INSTALLER).unwrap();
    std::fs::write(
        dir.join("node-1.probe"),
        "nxf-probe managed=no installer=no uid=0 arch=x86_64\nnxf-os Ubuntu 24.04.5 LTS\n",
    )
    .unwrap();
    // An ssh whose machine keeps the script that starts the installer, and is one from then on.
    let ssh = dir.join("ssh-starting");
    write_script(
        &ssh,
        &format!(
            r#"#!/usr/bin/env bash
if [[ ${{*: -1}} == sh ]]; then
    script=$(cat)
    [[ $script == *kexec* ]] || exec '{dir}/ssh' "$@" <<< "$script"
    echo "$script" > '{dir}/kexec.sh'
    exec cp '{dir}/installer.probe' '{dir}/node-1.probe'
fi
exec '{dir}/ssh' "$@"
"#,
            dir = dir.display()
        ),
    );
    fx.set("NXF_SSH", &ssh);
    let result = fx.run(&[
        "rollout",
        "--config",
        &hosts,
        "--template",
        "x#my machine",
        "--yes",
        "--kexec-url",
        "http://mirror/it's/{arch}.tar.gz",
    ]);
    result.ok();
    assert_contains(
        &ssh_log(&fx),
        "node-1 rm -rf /tmp/nxf-machine && /nix/store/nxf-package/bin/nxf init /tmp/nxf-machine --template 'path:/nix/store/template-source#my machine' --hardware\n",
    );
    let script = std::fs::read_to_string(dir.join("kexec.sh")).unwrap();
    assert_contains(
        &script,
        "curl -fsSL \"http://mirror/it's/x86_64.tar.gz\"; else",
    );
}
