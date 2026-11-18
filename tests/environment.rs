//! Tests that change the process environment. They hold one lock, and start
//! no nxf process, so that no other test sees a half-changed environment.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use common::{fake_systems, temp_dir};
use nxf::flake::{self, Location};
use nxf::nixos::{self, Remembered};
use nxf::output::{styled, Style, RESET};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

const CHANGED: [&str; 9] = [
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "TERM",
    "NXF_FLAKE",
    "NXF_STATE",
    "NXF_SYSTEM",
    "NXF_UPDATER_STATE",
    "XDG_STATE_HOME",
];

/// Holds the lock and starts from an environment without the variables above.
fn environment(vars: &[(&str, &str)]) -> MutexGuard<'static, ()> {
    let guard = ENVIRONMENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for name in CHANGED {
        std::env::remove_var(name);
    }
    for (name, value) in vars {
        std::env::set_var(name, value);
    }
    guard
}

/// A forced-color terminal distinguishes active, final and supporting text.
#[test]
fn color() {
    let _guard = environment(&[("CLICOLOR_FORCE", "1")]);
    for style in [
        Style::CyanBold,
        Style::GreenBold,
        Style::RedBold,
        Style::Dimmed,
        Style::Bold,
    ] {
        assert_eq!(
            styled("text", style),
            format!("{}text{RESET}", style.code())
        );
    }
    assert_ne!(Style::CyanBold.code(), Style::GreenBold.code());
}

/// NO_COLOR wins even when empty and forced color was requested.
#[test]
fn no_color() {
    {
        let _guard = environment(&[("NO_COLOR", ""), ("CLICOLOR_FORCE", "1")]);
        assert_eq!(styled("plain", Style::GreenBold), "plain");
    }
    let _guard = environment(&[("CLICOLOR", "0"), ("CLICOLOR_FORCE", "1")]);
    assert_eq!(styled("plain", Style::RedBold), "plain");
}

/// Redirected and dumb-terminal output stays free of ANSI sequences.
#[test]
fn redirected() {
    for vars in [
        [("CLICOLOR_FORCE", "0"), ("TERM", "dumb")],
        [("CLICOLOR_FORCE", "0"), ("TERM", "xterm")],
        [("CLICOLOR_FORCE", ""), ("TERM", "xterm")],
    ] {
        let _guard = environment(&vars);
        // The test harness captures stdout, so it is never a terminal here.
        assert_eq!(styled("plain", Style::CyanBold), "plain");
    }
}

/// The location names where the checkout came from.
#[test]
fn location() {
    {
        let _guard = environment(&[("NXF_FLAKE", "/x/y")]);
        assert_eq!(
            flake::location(),
            Location {
                dir: PathBuf::from("/x/y"),
                source: "NXF_FLAKE"
            }
        );
    }
    let _guard = environment(&[]);
    assert_eq!(
        flake::location(),
        Location {
            dir: PathBuf::from("/etc/nixos"),
            source: "default"
        }
    );
}

/// The checkout is NXF_FLAKE, or /etc/nixos like nh, and must hold a flake.
#[test]
fn checkout() {
    let dir = temp_dir("checkout");
    {
        let _guard = environment(&[("NXF_FLAKE", dir.to_str().unwrap())]);
        assert!(flake::checkout().unwrap_err().contains("has no flake.nix"));
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        assert_eq!(flake::checkout().unwrap(), dir);
    }
    let _guard = environment(&[]);
    if Path::new("/etc/nixos/flake.nix").exists() {
        assert_eq!(flake::checkout().unwrap(), Path::new("/etc/nixos"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// The running system can be stood in for.
#[test]
fn running() {
    let one = &fake_systems().one;
    let _guard = environment(&[("NXF_SYSTEM", one.to_str().unwrap())]);
    assert_eq!(&nixos::running(), one);
}

/// The state directory follows NXF_STATE, then XDG_STATE_HOME.
#[test]
fn state_dir() {
    {
        let _guard = environment(&[("XDG_STATE_HOME", "/x/state")]);
        assert_eq!(nixos::state_dir(), Path::new("/x/state/nxf"));
    }
    let _guard = environment(&[
        ("XDG_STATE_HOME", "/x/state"),
        ("NXF_STATE", "/var/lib/nxf"),
    ]);
    assert_eq!(nixos::state_dir(), Path::new("/var/lib/nxf"));
}

/// A checkout the group may write makes what nxf creates writable by the
/// group; one only its owner may write leaves the umask alone.
#[test]
fn shared() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = environment(&[]);
    let private = temp_dir("private");
    let group = temp_dir("group");
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&group, std::fs::Permissions::from_mode(0o2775)).unwrap();
    let before = unsafe { libc::umask(0o022) };
    assert!(!nixos::share_with_group(&private));
    assert_eq!(unsafe { libc::umask(0o022) }, 0o022);
    assert!(!nixos::share_with_group(&private.join("missing")));
    assert!(nixos::share_with_group(&group));
    assert_eq!(unsafe { libc::umask(before) }, 0o002);
}

/// Remembered systems come back by key; dangling records are skipped.
#[test]
fn remember() {
    let systems = fake_systems();
    let state = temp_dir("state");
    let _guard = environment(&[
        ("XDG_STATE_HOME", state.to_str().unwrap()),
        ("NXF_UPDATER_STATE", state.join("none").to_str().unwrap()),
    ]);
    nixos::remember("one", &systems.one).unwrap();
    nixos::remember("one", &systems.one).unwrap();
    std::os::unix::fs::symlink(state.join("missing"), state.join("nxf/systems/gone")).unwrap();
    assert_eq!(
        nixos::remembered(),
        [Remembered {
            key: "one".to_string(),
            system: systems.one.clone()
        }]
    );
    std::fs::remove_dir_all(state).unwrap();
}

/// The automatic updater's builds are remembered under their trees.
#[test]
fn remembered_updater() {
    let systems = fake_systems();
    let state = temp_dir("state");
    let updater = state.join("updater");
    std::fs::create_dir_all(updater.join("systems")).unwrap();
    std::os::unix::fs::symlink(&systems.two, updater.join("systems/tree")).unwrap();
    let _guard = environment(&[
        ("XDG_STATE_HOME", state.to_str().unwrap()),
        ("NXF_UPDATER_STATE", updater.to_str().unwrap()),
    ]);
    assert_eq!(
        nixos::remembered(),
        [Remembered {
            key: "tree".to_string(),
            system: systems.two.clone()
        }]
    );
    std::fs::remove_dir_all(state).unwrap();
}
