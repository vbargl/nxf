//! Installing the machine a flake describes, on that machine, from a NixOS
//! installer: partition the disks, install the system, and keep the flake as
//! the machine's /etc/nixos.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

use crate::output::{self, Mark};
use crate::{env, flake, git, nixos, process, state, time};

pub struct Options {
    pub dir: PathBuf,
    /// Skip partitioning: the target is already mounted.
    pub mounted: bool,
    pub reboot: bool,
}

/// Where the new system is mounted. NXF_INSTALL_ROOT stands in for it in tests.
fn target() -> PathBuf {
    env::path(env::INSTALL_ROOT, "/mnt")
}

/// Whether the root of the mounts that /proc/mounts lists is in memory. The
/// last `/` line is the mount in effect; one before it lies beneath it.
pub fn root_in_memory(mounts: &str) -> bool {
    let root = mounts.lines().rev().find_map(|line| {
        let mut fields = line.split_whitespace();
        let (_, point, kind) = (fields.next(), fields.next(), fields.next());
        (point == Some("/")).then_some(kind)
    });
    matches!(root.flatten(), Some("tmpfs" | "rootfs" | "ramfs"))
}

/// Whether this system runs from memory, as a NixOS installer does. A system
/// that runs from a disk would be erased under itself. NXF_INSTALLER (yes or
/// no) stands in for the answer in tests.
fn installer() -> bool {
    if let Some(answer) = env::text(env::INSTALLER) {
        return answer == "yes";
    }
    std::fs::read_to_string("/proc/mounts").is_ok_and(|mounts| root_in_memory(&mounts))
}

/// Whether nxf manages the running system: it installed it, or the system
/// has nxf. Such a machine is not an installer, also when it runs from
/// memory. NXF_UPDATER_STATE and NXF_SYSTEM stand in for the two places.
fn managed() -> bool {
    use std::os::unix::fs::PermissionsExt;
    state::updater_dir().join(state::INSTALLED).exists()
        || std::fs::metadata(nixos::running().join("sw/bin/nxf"))
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// The script of the configuration's disko layout that erases, formats and
/// mounts the disks. NXF_DISKO stands in for it in tests.
fn disko(dir: &Path, configuration: &str) -> Result<PathBuf, String> {
    if let Some(script) = env::var(env::DISKO) {
        return Ok(script);
    }
    let built = process::run(
        Command::new("nix")
            .args(["build", "--no-link", "--print-out-paths"])
            .arg(format!(
                "{}#nixosConfigurations.\"{configuration}\".config.system.build.diskoScript",
                dir.display()
            )),
    )
    .map_err(|err| {
        format!("The configuration has no disk layout to apply\n{err}\nImport the disko module and describe the disks, or mount the target yourself and add --mounted.")
    })?;
    Ok(PathBuf::from(
        String::from_utf8_lossy(&built.stdout).trim().to_string(),
    ))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    process::run(
        Command::new("cp")
            .args(["--archive", "--no-target-directory"])
            .arg(from)
            .arg(to),
    )
    .map(drop)
    .map_err(|err| format!("cannot copy {} to {}\n{err}", from.display(), to.display()))
}

/// Gives the machine its SSH host key now, so that the key can be told to
/// whoever connects after the first boot. The private key stays on the machine.
fn host_key(root: &Path) -> Result<String, String> {
    let key = root.join("etc/ssh/ssh_host_ed25519_key");
    let public = key.with_extension("pub");
    if !public.exists() {
        std::fs::create_dir_all(root.join("etc/ssh")).map_err(|err| err.to_string())?;
        process::run(
            Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-C", "", "-f"])
                .arg(&key),
        )?;
    }
    let line = std::fs::read_to_string(&public)
        .map_err(|err| format!("cannot read {}: {err}", public.display()))?;
    let mut fields = line.split_whitespace();
    match (fields.next(), fields.next()) {
        (Some(kind), Some(key)) => Ok(format!("{kind} {key}")),
        _ => Err(format!("{} holds no key", public.display())),
    }
}

/// How many links lead from the profile of the system to its store path.
const PROFILE_LINKS: usize = 4;

/// The store path of the installed system, as the new machine will see it.
/// The profile links are followed by hand: they point into the new system's
/// own /nix/store, which is not this one.
fn installed_system(root: &Path) -> Option<PathBuf> {
    let profiles = root.join("nix/var/nix/profiles");
    let mut link = profiles.join("system");
    for _ in 0..PROFILE_LINKS {
        let to = std::fs::read_link(&link).ok()?;
        if to.is_absolute() {
            // Under a stand-in root, the link may name the path with the root.
            return Some(Path::new("/").join(to.strip_prefix(root).unwrap_or(&to)));
        }
        link = profiles.join(to);
    }
    None
}

pub fn run(options: &Options) -> Result<(), String> {
    let dir = std::path::absolute(&options.dir)
        .map_err(|err| format!("{}: {err}", options.dir.display()))?;
    let root = target();
    let stands_in = env::var(env::INSTALL_ROOT).is_some();
    if !dir.join("flake.nix").exists() {
        return Err(format!(
            "{} has no flake.nix\nFound a machine flake first: nxf init <dir> --template <flake>#<name>",
            dir.display()
        ));
    }
    let tree = git::clean_tree(&dir).ok_or(format!(
        "{} has uncommitted changes or is not a Git repository\nCommit them: the machine gets exactly what is installed.",
        dir.display()
    ))?;
    if !dir.join("flake.lock").exists() {
        return Err(format!("{} has no flake.lock", dir.display()));
    }
    if !stands_in && !nixos::is_root() {
        return Err("Installing needs root".to_string());
    }
    if managed() {
        return Err(
            "nxf manages this system, and installing erases disks\nStart the machine from a NixOS installer to install it again."
                .to_string(),
        );
    }
    if !installer() {
        return Err(
            "This system runs from a disk, and installing erases disks\nStart the machine from a NixOS installer, or let `nxf rollout` do it."
                .to_string(),
        );
    }
    let configuration = flake::configuration(&dir)?;

    output::line("");
    output::context(&format!("{} · {}", nixos::hostname(), dir.display()));
    let total = if options.mounted { 2 } else { 3 };
    output::phases(0, total);

    if !options.mounted {
        output::start("Partitioning the disks");
        let script = disko(&dir, &configuration)?;
        output::stream(&mut Command::new(&script), false, &[])?;
        output::row(Mark::Done, "Erased, formatted and mounted");
        output::sub("Layout", &format!("nixosConfigurations.{configuration}"));
    }
    if !root.is_dir() {
        return Err(format!("{} is not mounted", root.display()));
    }

    output::start("Installing the system");
    let home = root.join("etc/nixos");
    if home.exists() && std::fs::read_dir(&home).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(format!(
            "{} already holds something; the target is not empty",
            home.display()
        ));
    }
    copy(&dir, &home)?;
    let mut install = Command::new("nixos-install");
    install
        .arg("--root")
        .arg(&root)
        .arg("--flake")
        .arg(format!("{}#{configuration}", home.display()))
        .args(["--no-root-passwd", "--no-channel-copy"]);
    output::stream(&mut install, false, &[])?;
    let system = installed_system(&root);
    // The state of the updater, as the new machine will have it.
    let updater = root.join(state::UPDATER.trim_start_matches('/'));
    output::row(Mark::Done, "Installed");
    output::sub("Flake", "/etc/nixos");
    if let Some(inside) = &system {
        output::sub("System", &inside.to_string_lossy());
        let records = updater.join(state::SYSTEMS);
        std::fs::create_dir_all(&records).map_err(|err| err.to_string())?;
        let record = records.join(&tree);
        let _ = std::fs::remove_file(&record);
        std::os::unix::fs::symlink(inside, &record).map_err(|err| err.to_string())?;
    }

    output::start("Preparing the first start");
    let key = host_key(&root)?;
    let marker = updater.join(state::INSTALLED);
    std::fs::create_dir_all(marker.parent().unwrap_or(&root)).map_err(|err| err.to_string())?;
    std::fs::write(
        &marker,
        format!(
            "{:#}\n",
            json!({"time": time::now(), "tree": tree, "configuration": configuration})
        ),
    )
    .map_err(|err| format!("cannot write {}: {err}", marker.display()))?;
    output::row(Mark::Done, "Host key made, installation recorded");

    output::success("NixOS installed");
    output::fact("Host key", &key);
    if options.reboot {
        output::effect("Restarting into the new system.");
        let _ = Command::new(nixos::systemctl()).arg("reboot").status();
    } else {
        output::effect("Restart the machine to start it.");
    }
    Ok(())
}
