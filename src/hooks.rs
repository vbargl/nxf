//! The configuration's two hooks around an automatic update: one that is
//! asked before it starts, and one that runs after it failed. Each is one
//! executable, and nxf knows nothing about what it does.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::health;

pub const CHECK_HOOK: &str = "/etc/nxf/check-hook";
pub const FAILURE_HOOK: &str = "/etc/nxf/failure-hook";

/// The hook, when the configuration has one. The variable stands in for its
/// path in tests.
fn hook(variable: &str, default: &str) -> Option<PathBuf> {
    let path = std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(default), PathBuf::from);
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
    run(&hook("NXF_CHECK_HOOK", CHECK_HOOK)?, &[])
}

/// Runs the failure hook with the saved results in NXF_RESULTS, and returns
/// what it said when it failed itself.
pub fn failure(results: &Path) -> Option<String> {
    run(
        &hook("NXF_FAILURE_HOOK", FAILURE_HOOK)?,
        &[("NXF_RESULTS", results)],
    )
}
