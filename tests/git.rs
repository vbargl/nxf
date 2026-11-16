//! Tests for src/git.rs.

mod common;

use common::{append, commit, fixture, git as run_git, temp_dir};
use nxf::git;

/// The committed tree is HEAD's tree, and none with uncommitted changes.
#[test]
fn clean_tree() {
    let fx = fixture();
    assert_eq!(git::clean_tree(&fx.checkout), Some(fx.tree()));
    append(&fx.checkout.join("flake.nix"), "# dirty\n");
    assert_eq!(git::clean_tree(&fx.checkout), None);
}

/// A tree maps back to its commit, and an unknown tree to none.
#[test]
fn commit_of() {
    let fx = fixture();
    let first = fx.tree();
    assert_eq!(
        git::commit_of(&fx.checkout, std::slice::from_ref(&first)),
        Some(fx.head())
    );
    assert_eq!(git::commit_of(&fx.checkout, &["0".repeat(40)]), None);
    append(&fx.checkout.join("flake.nix"), "# more\n");
    commit(&fx.checkout, "more");
    // The newest commit with any of the trees wins.
    assert_eq!(
        git::commit_of(&fx.checkout, &[first, fx.tree()]),
        Some(fx.head())
    );
}

/// The commit line shows the short hash and subject.
#[test]
fn commit_line() {
    let fx = fixture();
    let line = git::commit_line(&fx.checkout, "HEAD");
    assert!(
        line.starts_with(&format!("{} init (", fx.short_head())),
        "{line}"
    );
}

/// A file is read as it was at a commit.
#[test]
fn show_file() {
    let fx = fixture();
    let head = fx.head();
    std::fs::write(fx.checkout.join("flake.lock"), "changed\n").unwrap();
    let lock: serde_json::Value =
        serde_json::from_str(&git::show_file(&fx.checkout, &head, "flake.lock").unwrap()).unwrap();
    assert_eq!(lock["root"], "root");
}

/// A repository needs a commit to count.
#[test]
fn is_repo() {
    let fx = fixture();
    assert!(git::is_repo(&fx.checkout));
    let empty = temp_dir("empty-repo");
    assert!(!git::is_repo(&empty));
    run_git(&empty, &["init", "--quiet"]);
    assert!(!git::is_repo(&empty));
    std::fs::remove_dir_all(empty).unwrap();
}

/// A changed file is noticed and restored to HEAD, staged or not.
#[test]
fn changed_restore() {
    let fx = fixture();
    let file = fx.checkout.join("flake.lock");
    let original = std::fs::read_to_string(&file).unwrap();
    assert!(!git::changed(&fx.checkout, "flake.lock"));
    std::fs::write(&file, "changed\n").unwrap();
    assert!(git::changed(&fx.checkout, "flake.lock"));
    assert_eq!(git::changed_files(&fx.checkout), ["flake.lock"]);
    run_git(&fx.checkout, &["add", "flake.lock"]);
    assert!(git::changed(&fx.checkout, "flake.lock"));
    git::restore(&fx.checkout, "flake.lock").unwrap();
    assert!(!git::changed(&fx.checkout, "flake.lock"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    assert_eq!(fx.porcelain(), "");
}

/// One file is committed while other changes stay as they are.
#[test]
fn commit_file() {
    let fx = fixture();
    append(&fx.checkout.join("flake.nix"), "# kept\n");
    append(&fx.checkout.join("flake.lock"), "\n");
    // The identity comes from the environment, as in the fixture.
    for (name, value) in [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_AUTHOR_NAME", "test"),
        ("GIT_AUTHOR_EMAIL", "test@example.invalid"),
        ("GIT_COMMITTER_NAME", "test"),
        ("GIT_COMMITTER_EMAIL", "test@example.invalid"),
    ] {
        std::env::set_var(name, value);
    }
    assert_eq!(
        git::identity(&fx.checkout).as_deref(),
        Some("test <test@example.invalid>")
    );
    git::commit_file(&fx.checkout, "flake.lock", "lock only").unwrap();
    assert_eq!(
        run_git(&fx.checkout, &["log", "-1", "--format=%s"]),
        "lock only"
    );
    assert_eq!(fx.porcelain(), "M flake.nix");
}
