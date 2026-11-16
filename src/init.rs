//! Founding a machine flake: nxf does the mechanics, a template supplies the
//! content. The files of the template have no meaning to nxf.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{self, Check, Level};
use crate::{flake, git, nixos, output};

/// The module file every founded flake gets. `nix/init-module.nix` generates
/// it from the options of the NixOS module, and a flake check compares them.
pub const MODULE: &str = include_str!("../nix/nxf.nix");
pub const MODULE_FILE: &str = "nxf.nix";

pub struct Options {
    pub dir: Option<PathBuf>,
    pub template: String,
    pub hardware: bool,
    /// A directory whose content is laid over the unpacked template.
    pub files: Option<PathBuf>,
}

/// A template in a local directory is named relative to where nxf was called.
fn absolute(template: &str) -> String {
    let (flake, name) = match template.split_once('#') {
        Some((flake, name)) => (flake, Some(name)),
        None => (template, None),
    };
    let local = flake == "." || flake.starts_with("./") || flake.starts_with("../");
    let flake = match std::path::absolute(flake) {
        Ok(path) if local => path.to_string_lossy().into_owned(),
        _ => flake.to_string(),
    };
    match name {
        Some(name) => format!("{flake}#{name}"),
        None => flake,
    }
}

fn hardware(dir: &Path) -> Result<(), String> {
    let output = Command::new("nixos-generate-config")
        // The flake describes the file systems; the disks may not be partitioned yet.
        .args(["--show-hardware-config", "--no-filesystems"])
        .output()
        .map_err(|err| format!("cannot run nixos-generate-config: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "nixos-generate-config failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    std::fs::write(dir.join("hardware.nix"), output.stdout)
        .map_err(|err| format!("cannot write hardware.nix: {err}"))
}

/// Whether the configuration imports the nxf module. Keeping the `nxf` input
/// in the flake is up to its owner; this only says whether it is there.
fn module(dir: &Path, host: &str) -> Check {
    match flake::uses_module(dir, host) {
        Ok(true) => doctor::check(
            Level::Ok,
            "module",
            format!("nixosConfigurations.{host} imports the nxf module"),
        ),
        Ok(false) => doctor::check(
            Level::Warn,
            "module",
            format!(
                "nixosConfigurations.{host} does not import the nxf module, so {MODULE_FILE} has no effect; \
                 add `inputs.nxf.url = \"github:vbargl/nxf\";` to flake.nix and \
                 `nxf.nixosModules.default` and `./{MODULE_FILE}` to its modules"
            ),
        ),
        Err(_) => doctor::check(
            Level::Warn,
            "module",
            format!("cannot evaluate nixosConfigurations.{host}, so the nxf module was not looked for"),
        ),
    }
}

pub fn run(options: &Options) -> Result<(), String> {
    let dir = match &options.dir {
        Some(dir) => std::path::absolute(dir).map_err(|err| format!("{}: {err}", dir.display()))?,
        None => flake::location().dir,
    };
    let shown = dir.display();
    let empty = match std::fs::read_dir(&dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => return Err(format!("cannot read {shown}: {err}")),
    };
    if !empty {
        return Err(format!(
            "{shown} is not empty; nxf init only founds a new machine flake."
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|err| format!("cannot create {shown}: {err}"))?;

    output::start("Founding the machine flake");
    output::fact("Directory", &dir.to_string_lossy());
    let template = absolute(&options.template);
    flake::unpack(&dir, &template)
        .map_err(|err| format!("cannot unpack the template {template}:\n{err}"))?;
    output::fact("Template", &template);
    if options.hardware {
        hardware(&dir)?;
        output::fact("Hardware", "hardware.nix from nixos-generate-config");
    }
    if let Some(files) = &options.files {
        if !files.is_dir() {
            return Err(format!("{} is not a directory", files.display()));
        }
        // The machine's own values: laid over the template, never read by nxf.
        let copied = Command::new("cp")
            .args(["--recursive", "--no-target-directory"])
            .arg(files)
            .arg(&dir)
            .status()
            .map_err(|err| format!("cannot run cp: {err}"))?;
        if !copied.success() {
            return Err(format!("cannot copy {} into the flake", files.display()));
        }
        output::fact("Files", &files.to_string_lossy());
    }
    let module_file = dir.join(MODULE_FILE);
    if module_file.exists() {
        output::fact("Module", &format!("{MODULE_FILE} of the template is kept"));
    } else {
        std::fs::write(&module_file, MODULE)
            .map_err(|err| format!("cannot write {}: {err}", module_file.display()))?;
        output::fact("Module", &format!("{MODULE_FILE} with the options of nxf"));
    }

    git::init(&dir)?;
    if git::identity(&dir).is_none() {
        git::set_identity(&dir, "nxf", &format!("nxf@{}", nixos::hostname()))?;
    }
    // Nix reads only tracked files of a Git flake.
    git::commit_all(&dir, "Found the machine flake")?;
    flake::write_lock(&dir).map_err(|err| {
        format!("cannot lock the inputs; {shown} is kept without a flake.lock, remove it before another try:\n{err}")
    })?;
    if git::clean_tree(&dir).is_none() {
        git::commit_all(&dir, "flake.lock: lock the inputs")?;
    }
    output::fact("Commit", &git::commit_line(&dir, "HEAD"));

    output::section("Checks", "");
    let location = flake::Location {
        dir: dir.clone(),
        source: "nxf init",
    };
    let mut checks = vec![doctor::tools()];
    checks.extend(doctor::checkout(&location));
    if checks.iter().all(|check| check.level != Level::Fail) {
        if let Ok(configuration) = flake::configuration(&dir) {
            checks.push(module(&dir, &configuration));
        }
    }
    doctor::print(&checks, 2);
    // The flake is founded whatever the checks say; they say what is left to do.
    output::success("Machine flake founded");
    match doctor::verdict(&checks) {
        Ok(passed) => output::effect(&format!("{passed}.")),
        Err(failed) => output::attention(&format!("{failed}; fix this before the first build.")),
    }
    Ok(())
}
