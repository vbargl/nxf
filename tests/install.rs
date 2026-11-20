//! Tests for src/install.rs: installing from an installer, with stand-ins for
//! the disks, the mount point and nixos-install. Nothing touches the machine.

mod common;

use std::path::PathBuf;

use common::{append, assert_contains, assert_lacks, fixture, git, write_script, Fixture};
use serde_json::Value;

/// A fixture in a stand-in installer: the target is a directory, the disk
/// script creates it, and nixos-install records its call and links a system.
fn installer() -> (Fixture, PathBuf) {
    let mut fx = fixture();
    let root = fx.dir.join("mnt");
    let disko = fx.dir.join("disko");
    write_script(
        &disko,
        &format!(
            "#!/bin/sh\necho 'wiping /dev/vda'\necho disko >> '{0}/steps'\nmkdir -p '{1}'\n",
            fx.dir.display(),
            root.display()
        ),
    );
    write_script(
        &fx.dir.join("bin/nixos-install"),
        &format!(
            r#"#!/usr/bin/env bash
echo "nixos-install $*" >> '{dir}/steps'
[[ -z ${{NXF_TEST_INSTALL_FAILS:-}} ]] || {{ echo 'error: builder failed' >&2; exit 1; }}
mkdir -p '{root}/nix/store/abc-nixos-system' '{root}/nix/var/nix/profiles'
ln -sfn '{root}/nix/store/abc-nixos-system' '{root}/nix/var/nix/profiles/system'
"#,
            dir = fx.dir.display(),
            root = root.display()
        ),
    );
    fx.set("NXF_INSTALL_ROOT", &root);
    fx.set("NXF_DISKO", &disko);
    fx.set("NXF_INSTALLER", "yes");
    (fx, root)
}

fn steps(fx: &Fixture) -> Vec<String> {
    std::fs::read_to_string(fx.dir.join("steps"))
        .map(|steps| steps.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Install partitions, installs from the copied flake, makes the host key and
/// records the installation; the flake becomes the machine's /etc/nixos.
#[test]
fn partitions_and_records() {
    let (fx, root) = installer();
    let result = fx.run(&["install", fx.checkout.to_str().unwrap()]);
    result.ok();
    let home = root.join("etc/nixos");
    assert_eq!(
        steps(&fx),
        [
            "disko".to_string(),
            format!(
                "nixos-install --root {} --flake {}#{} --no-root-passwd --no-channel-copy",
                root.display(),
                home.display(),
                common::hostname()
            )
        ]
    );
    // The machine's flake is the same repository, with its history and no remote.
    assert_eq!(git(&home, &["rev-parse", "HEAD"]), fx.head());
    assert_eq!(git(&home, &["status", "--porcelain"]), "");
    assert_eq!(git(&home, &["remote"]), "");

    let marker: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("var/lib/nxf/installed.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(marker["tree"], fx.tree());
    assert_eq!(marker["configuration"], common::hostname());
    // The record names the system as the new machine will see it.
    assert_eq!(
        std::fs::read_link(root.join("var/lib/nxf/systems").join(fx.tree())).unwrap(),
        PathBuf::from("/nix/store/abc-nixos-system")
    );

    let public = std::fs::read_to_string(root.join("etc/ssh/ssh_host_ed25519_key.pub")).unwrap();
    let key: Vec<&str> = public.split_whitespace().take(2).collect();
    assert_eq!(key[0], "ssh-ed25519");
    assert!(root.join("etc/ssh/ssh_host_ed25519_key").exists());
    let out = &result.stdout;
    assert_contains(out, "[1/3] Partitioning the disks\n");
    assert_contains(out, "    wiping /dev/vda\n");
    assert_contains(out, "[2/3] Installing the system\n  ✓ Installed\n    Flake     /etc/nixos\n    System    /nix/store/abc-nixos-system\n");
    assert!(
        out.ends_with(&format!(
            "\n✓ NixOS installed\n  Host key  {}\n  Restart the machine to start it.\n",
            key.join(" ")
        )),
        "{out}"
    );
}

/// An already mounted target is not partitioned, and an existing host key is kept.
#[test]
fn mounted() {
    let (fx, root) = installer();
    std::fs::create_dir_all(root.join("etc/ssh")).unwrap();
    std::fs::write(
        root.join("etc/ssh/ssh_host_ed25519_key.pub"),
        "ssh-ed25519 AAAAexisting root@old\n",
    )
    .unwrap();
    let result = fx.run(&["install", fx.checkout.to_str().unwrap(), "--mounted"]);
    result.ok();
    assert_eq!(steps(&fx).len(), 1);
    assert_lacks(&result.stdout, "Partitioning");
    assert_contains(&result.stdout, "[1/2] Installing the system");
    assert_contains(&result.stdout, "  Host key  ssh-ed25519 AAAAexisting\n");
}

/// Install refuses before it touches a disk: outside an installer, with
/// uncommitted changes, without a flake, and into a target that holds something.
#[test]
fn refuses() {
    let (mut fx, root) = installer();
    let dir = fx.checkout.to_str().unwrap().to_string();

    fx.set("NXF_INSTALLER", "no");
    let disk = fx.run(&["install", &dir]);
    assert_ne!(disk.code, 0);
    assert_contains(
        &disk.stderr,
        "✗ This system runs from a disk, and installing erases disks",
    );
    fx.set("NXF_INSTALLER", "yes");

    let missing = fx.run(&["install", fx.dir.to_str().unwrap()]);
    assert_ne!(missing.code, 0);
    assert_contains(&missing.stderr, "has no flake.nix");

    append(&fx.checkout.join("flake.nix"), "# edited\n");
    let dirty = fx.run(&["install", &dir]);
    assert_ne!(dirty.code, 0);
    assert_contains(&dirty.stderr, "has uncommitted changes");
    git(&fx.checkout, &["checkout", "--quiet", "flake.nix"]);
    assert!(steps(&fx).is_empty());

    std::fs::create_dir_all(root.join("etc/nixos")).unwrap();
    std::fs::write(root.join("etc/nixos/configuration.nix"), "{ }").unwrap();
    let occupied = fx.run(&["install", &dir, "--mounted"]);
    assert_ne!(occupied.code, 0);
    assert_contains(&occupied.stderr, "already holds something");
    assert!(steps(&fx).is_empty());
}

/// Install refuses a target that --mounted says is there and is not, and a
/// flake without a lockfile; neither reaches the disks or nixos-install.
#[test]
fn refuses_unmounted_and_unlocked() {
    let (fx, root) = installer();
    let dir = fx.checkout.to_str().unwrap().to_string();
    let unmounted = fx.run(&["install", &dir, "--mounted"]);
    assert_ne!(unmounted.code, 0);
    assert_contains(
        &unmounted.stderr,
        &format!("✗ {} is not mounted", root.display()),
    );

    git(&fx.checkout, &["rm", "--quiet", "flake.lock"]);
    common::commit(&fx.checkout, "no lockfile");
    let unlocked = fx.run(&["install", &dir]);
    assert_ne!(unlocked.code, 0);
    assert_contains(&unlocked.stderr, &format!("✗ {dir} has no flake.lock"));
    assert!(steps(&fx).is_empty());
}

/// The profile of the new system is followed through relative links to the
/// store path at their end, which is recorded as the machine will see it.
#[test]
fn relative_profile_links() {
    let (fx, root) = installer();
    write_script(
        &fx.dir.join("bin/nixos-install"),
        &format!(
            "#!/bin/sh\nmkdir -p '{0}'\nln -s system-1-link '{0}/system'\nln -s /nix/store/abc-nixos-system '{0}/system-1-link'\n",
            root.join("nix/var/nix/profiles").display()
        ),
    );
    let result = fx.run(&["install", fx.checkout.to_str().unwrap()]);
    result.ok();
    assert_contains(
        &result.stdout,
        "    System    /nix/store/abc-nixos-system\n",
    );
    assert_eq!(
        std::fs::read_link(root.join("var/lib/nxf/systems").join(fx.tree())).unwrap(),
        PathBuf::from("/nix/store/abc-nixos-system")
    );
}

/// With --reboot the restart is asked for once the system is installed,
/// and the advice to restart by hand is left out.
#[test]
fn reboot() {
    let (fx, _root) = installer();
    // The systemctl in front on PATH is the fixture's; here it notes its call.
    write_script(
        &fx.systemctl,
        &format!(
            "#!/bin/sh\necho \"systemctl $*\" >> '{}/steps'\n",
            fx.dir.display()
        ),
    );
    let result = fx.run(&["install", fx.checkout.to_str().unwrap(), "--reboot"]);
    result.ok();
    assert_eq!(
        steps(&fx).last().map(String::as_str),
        Some("systemctl reboot")
    );
    assert!(
        result
            .stdout
            .ends_with("\n  Restarting into the new system.\n"),
        "{}",
        result.stdout
    );
}

/// A failed nixos-install fails the command with what it said, and nothing
/// is recorded as installed.
#[test]
fn failure_records_nothing() {
    let (fx, root) = installer();
    let fx = fx.with("NXF_TEST_INSTALL_FAILS", "1");
    let result = fx.run(&["install", fx.checkout.to_str().unwrap()]);
    assert_ne!(result.code, 0);
    assert_contains(
        &result.stderr,
        "✗ nixos-install exited with status 1.\n  error: builder failed",
    );
    assert!(!root.join("var/lib/nxf/installed.json").exists());
    assert_lacks(&result.stdout, "NixOS installed");
}

/// Install refuses on a system that nxf manages, also when it runs from
/// memory as an installer does: one that nxf installed, or one whose system
/// has nxf. Nothing is partitioned or installed.
#[test]
fn refuses_a_managed_system() {
    let (mut fx, _) = installer();
    let dir = fx.checkout.to_str().unwrap().to_string();
    let refused = |fx: &Fixture| {
        let result = fx.run(&["install", &dir]);
        assert_ne!(result.code, 0);
        assert_contains(
            &result.stderr,
            "✗ nxf manages this system, and installing erases disks",
        );
        assert!(steps(fx).is_empty());
    };

    let record = fx.updater.join("installed.json");
    std::fs::create_dir_all(&fx.updater).unwrap();
    std::fs::write(&record, "{}\n").unwrap();
    refused(&fx);
    std::fs::remove_file(&record).unwrap();

    let system = fx.dir.join("system");
    std::fs::create_dir_all(system.join("sw/bin")).unwrap();
    write_script(&system.join("sw/bin/nxf"), "#!/bin/sh\n");
    fx.set("NXF_SYSTEM", &system);
    refused(&fx);
}

/// With `--reboot` the machine is restarted through the stand-in for systemd,
/// not through whichever `systemctl` the path holds.
#[test]
fn reboots_through_the_stand_in() {
    let (mut fx, _) = installer();
    let noting = |log: &str| format!("#!/bin/sh\necho \"$*\" >> '{}/{log}'\n", fx.dir.display());
    // The one of the path stays in front and only notes a call: a call that
    // went by the path must fail this test, not restart the machine it runs on.
    write_script(&fx.systemctl, &noting("path.log"));
    let stand_in = fx.dir.join("stand-in-systemctl");
    write_script(&stand_in, &noting("stand-in.log"));
    fx.set("NXF_SYSTEMCTL", &stand_in);
    let result = fx.run(&["install", fx.checkout.to_str().unwrap(), "--reboot"]);
    result.ok();
    assert_contains(&result.stdout, "\n  Restarting into the new system.\n");
    assert_eq!(
        std::fs::read_to_string(fx.dir.join("stand-in.log")).ok(),
        Some("reboot\n".to_string())
    );
    assert!(!fx.dir.join("path.log").exists());
}

/// The root is in memory when the last `/` line of the mounts says so: that
/// mount is in effect, and a `rootfs` listed before a disk lies beneath it.
#[test]
fn root_is_the_last_mount() {
    use nxf::install::root_in_memory;
    const DISK: &str = "/dev/sda2 / ext4 rw,relatime 0 0\n";
    const MEMORY: &str = "rootfs / rootfs rw 0 0\n";
    const ELSEWHERE: &str = "tmpfs /run tmpfs rw,nosuid 0 0\n";
    assert!(!root_in_memory(&format!("{MEMORY}{ELSEWHERE}{DISK}")));
    assert!(root_in_memory(&format!("{DISK}{ELSEWHERE}{MEMORY}")));
    assert!(root_in_memory("tmpfs / tmpfs rw 0 0\n"));
    assert!(!root_in_memory(ELSEWHERE));
}
