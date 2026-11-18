//! The configuration's say in an automatic update: checks that must pass
//! before one starts, and hooks that run after one failed. Both are
//! executables in a directory, like the health checks, and nxf knows nothing
//! about what they do.

use std::path::Path;
use std::time::Instant;

use crate::health;

pub const PRE_DIR: &str = "/etc/nxf/pre.d";
pub const FAILURE_DIR: &str = "/etc/nxf/failure.d";

fn name(script: &Path) -> String {
    script
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn deadline() -> Instant {
    Instant::now() + health::seconds("NXF_HOOK_TIMEOUT", 60)
}

/// The first check that does not let the update start, with what it said.
/// The checks run in name order, and the ones after it are not run.
pub fn refused() -> Option<String> {
    health::scripts("NXF_PRE_DIR", PRE_DIR)
        .iter()
        .find_map(|check| {
            let why = health::passes(check, deadline(), &[]).err()?;
            Some(format!("{}: {why}", name(check)))
        })
}

/// Runs every failure hook, each with the saved results in NXF_RESULTS, and
/// returns the hooks that failed, with what they said.
pub fn failure(results: &Path) -> Option<String> {
    let failed: Vec<String> = health::scripts("NXF_FAILURE_DIR", FAILURE_DIR)
        .iter()
        .filter_map(|hook| {
            let why = health::passes(hook, deadline(), &[("NXF_RESULTS", results)]).err()?;
            Some(format!("{}: {why}", name(hook)))
        })
        .collect();
    (!failed.is_empty()).then(|| failed.join("; "))
}
