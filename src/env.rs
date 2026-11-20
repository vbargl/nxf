//! The environment of the process. Every variable nxf reads is named here, and
//! the other modules read the environment only through these functions.

use std::path::PathBuf;
use std::time::Duration;

/// Setting: the checkout of the machine flake; `/etc/nixos`.
pub const FLAKE: &str = "NXF_FLAKE";
/// Setting: the per-user state; `$XDG_STATE_HOME/nxf`, then `~/.local/state/nxf`.
pub const STATE: &str = "NXF_STATE";
/// Setting: the seconds each hook has to end; 60.
pub const HOOK_TIMEOUT: &str = "NXF_HOOK_TIMEOUT";
/// Setting: the seconds ssh has to connect to a machine; 10.
pub const SSH_CONNECT_TIMEOUT: &str = "NXF_SSH_CONNECT_TIMEOUT";
/// Setting: the seconds a machine has to answer a command with `--config`; 120.
pub const SSH_TIMEOUT: &str = "NXF_SSH_TIMEOUT";
/// Setting: the seconds a machine has for `rollout --upgrade`; 10800.
pub const UPGRADE_TIMEOUT: &str = "NXF_UPGRADE_TIMEOUT";
/// Setting: the seconds rollout waits for the installer, and then for the installed system; 600.
pub const ROLLOUT_WAIT: &str = "NXF_ROLLOUT_WAIT";

/// Stand-in: the running system; `/run/current-system`.
pub const SYSTEM: &str = "NXF_SYSTEM";
/// Stand-in: the booted system; `/run/booted-system`.
pub const BOOTED: &str = "NXF_BOOTED";
/// Stand-in: the updater's state; `/var/lib/nxf`.
pub const UPDATER_STATE: &str = "NXF_UPDATER_STATE";
/// Stand-in: the `systemctl` program; `systemctl`.
pub const SYSTEMCTL: &str = "NXF_SYSTEMCTL";
/// Stand-in: the directory of the health checks; `/etc/nxf/health.d`.
pub const HEALTH_DIR: &str = "NXF_HEALTH_DIR";
/// Stand-in: the directory of the two hooks; `/etc/nxf/hooks`.
pub const HOOKS_DIR: &str = "NXF_HOOKS_DIR";
/// Stand-in: the `ssh` program; `ssh`.
pub const SSH: &str = "NXF_SSH";
/// Stand-in: where the new system is mounted; `/mnt`. Install needs no root when it is set.
pub const INSTALL_ROOT: &str = "NXF_INSTALL_ROOT";
/// Stand-in: whether the system runs from memory, `yes` or not; read from `/proc/mounts`.
pub const INSTALLER: &str = "NXF_INSTALLER";
/// Stand-in: the script of the disk layout; built from the configuration.
pub const DISKO: &str = "NXF_DISKO";
/// Stand-in: the package rollout copies to the machines; the package nxf runs from.
pub const PACKAGE: &str = "NXF_PACKAGE";

/// Set for the failure hook: the saved results. nxf does not read it.
pub const RESULTS: &str = "NXF_RESULTS";

/// Of the system: `NO_COLOR` asks for output without colour, even when empty.
pub const NO_COLOR: &str = "NO_COLOR";
/// Of the system: `0` asks for output without colour.
pub const CLICOLOR: &str = "CLICOLOR";
/// Of the system: anything but empty or `0` asks for colour on every output.
pub const CLICOLOR_FORCE: &str = "CLICOLOR_FORCE";
/// Of the system: `dumb` is a terminal that cannot redraw.
pub const TERM: &str = "TERM";
/// Of the system: the configuration of Nix.
const NIX_CONFIG: &str = "NIX_CONFIG";
/// Of the system: the home directory.
pub const HOME: &str = "HOME";
/// Of the system: the directory of the state of a user.
pub const XDG_STATE_HOME: &str = "XDG_STATE_HOME";
/// Of the system: the name of the user.
pub const USER: &str = "USER";
/// Of the system: where the programs are.
pub const PATH: &str = "PATH";

/// A path variable; none when it is not set or empty.
pub fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// A path variable, or the default.
pub fn path(name: &str, default: &str) -> PathBuf {
    var(name).unwrap_or_else(|| PathBuf::from(default))
}

/// A text variable as it is set, also when empty.
pub fn text(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Whether a variable is set, also when empty.
pub fn is_set(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// A time in whole seconds; the default when the variable is not a number.
pub fn seconds(name: &str, default: u64) -> Duration {
    Duration::from_secs(
        text(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default),
    )
}

/// Lets Nix use flakes, whether or not the machine enables them for people.
pub fn enable_flakes() {
    let extra = "extra-experimental-features = nix-command flakes";
    let config = match text(NIX_CONFIG) {
        Some(config) if !config.is_empty() => format!("{config}\n{extra}"),
        _ => extra.to_string(),
    };
    // Called once, first in the program, before any thread runs.
    std::env::set_var(NIX_CONFIG, config);
}
