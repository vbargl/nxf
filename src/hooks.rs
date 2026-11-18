//! The configuration's two hooks around an automatic update: one that is
//! asked before it starts, and one that runs after it failed. Each is one
//! executable, and nxf knows nothing about what it does.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::health;

pub const DEFAULT_DIR: &str = "/etc/nxf/hooks";

/// The hook of that name, when the configuration has one. NXF_HOOKS_DIR
/// stands in for the directory in tests.
fn hook(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("NXF_HOOKS_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from)
        .join(name);
    std::fs::metadata(&path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .then_some(path)
}

fn run(hook: &Path, variables: &[(&str, &Path)]) -> Option<String> {
    let deadline = Instant::now() + health::seconds("NXF_HOOK_TIMEOUT", 60);
    health::passes(hook, deadline, variables).err()
}

/// What the check hook said when it did not let the update start.
pub fn refused() -> Option<String> {
    run(&hook("check")?, &[])
}

/// Runs the failure hook with the saved results in NXF_RESULTS, and returns
/// what it said when it failed itself.
pub fn failure(results: &Path) -> Option<String> {
    run(&hook("failure")?, &[("NXF_RESULTS", results)])
}
