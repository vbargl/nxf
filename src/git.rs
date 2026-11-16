//! Git queries on a repository.

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn git(dir: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run git: {err}"))
}

fn succeeds(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_ok_and(|output| output.status.success())
}

fn text(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = git(dir, args)?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The committed tree of HEAD, or none with uncommitted changes.
pub fn clean_tree(dir: &Path) -> Option<String> {
    if !succeeds(dir, &["diff", "--quiet", "HEAD", "--"]) {
        return None;
    }
    text(dir, &["rev-parse", "HEAD^{tree}"])
        .ok()
        .map(|tree| tree.trim().to_string())
}

/// The newest commit with one of the given trees.
pub fn commit_of(dir: &Path, trees: &[String]) -> Option<String> {
    let log = text(dir, &["log", "--all", "--date-order", "--format=%H %T"]).ok()?;
    log.lines().find_map(|line| {
        let (commit, tree) = line.split_once(' ')?;
        trees
            .iter()
            .any(|known| known == tree)
            .then(|| commit.to_string())
    })
}

/// Whether a file differs from HEAD, staged or not.
pub fn changed(dir: &Path, file: &str) -> bool {
    !succeeds(dir, &["diff", "--quiet", "HEAD", "--", file])
}

/// Puts a file back as it is in HEAD, staged and in the work tree.
pub fn restore(dir: &Path, file: &str) -> Result<(), String> {
    text(
        dir,
        &[
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--",
            file,
        ],
    )
    .map(drop)
}

pub fn commit_line(dir: &Path, commit: &str) -> String {
    text(dir, &["log", "-1", "--format=%h %s (%cr)", commit])
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|_| commit.to_string())
}

pub fn show_file(dir: &Path, commit: &str, file: &str) -> Result<String, String> {
    text(dir, &["show", &format!("{commit}:{file}")])
}

/// Whether the directory is inside a Git work tree with at least one commit.
pub fn is_repo(dir: &Path) -> bool {
    succeeds(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
}

/// The identity Git commits with in the repository, or none if Git would
/// refuse to commit.
pub fn identity(dir: &Path) -> Option<String> {
    let ident = text(dir, &["var", "GIT_COMMITTER_IDENT"]).ok()?;
    // "Name <email> 1700000000 +0100": drop the time and zone.
    let mut parts = ident.trim().rsplitn(3, ' ');
    let (_zone, _time) = (parts.next()?, parts.next()?);
    parts.next().map(str::to_string)
}

/// The tracked files that differ from HEAD, staged or not.
pub fn changed_files(dir: &Path) -> Vec<String> {
    text(dir, &["diff", "--name-only", "HEAD", "--"])
        .map(|files| files.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Commits one file, leaving everything else as it is.
pub fn commit_file(dir: &Path, file: &str, message: &str) -> Result<(), String> {
    let output = git(
        dir,
        &["commit", "--quiet", "--message", message, "--", file],
    )?;
    if output.status.success() {
        return Ok(());
    }
    let said = [
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ]
    .join("\n");
    Err(format!("git commit failed: {}", said.trim()))
}

/// Makes the directory a repository without a remote.
pub fn init(dir: &Path) -> Result<(), String> {
    text(dir, &["init", "--quiet", "--initial-branch", "main"]).map(drop)
}

/// Sets the identity of commits made in this repository.
pub fn set_identity(dir: &Path, name: &str, email: &str) -> Result<(), String> {
    text(dir, &["config", "user.name", name])?;
    text(dir, &["config", "user.email", email]).map(drop)
}

/// Commits every file that is not ignored.
pub fn commit_all(dir: &Path, message: &str) -> Result<(), String> {
    text(dir, &["add", "--all"])?;
    let output = git(dir, &["commit", "--quiet", "--message", message])?;
    if output.status.success() {
        return Ok(());
    }
    let said = [
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ]
    .join("\n");
    Err(format!("git commit failed: {}", said.trim()))
}
