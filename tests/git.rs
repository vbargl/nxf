//! Tests for src/git.rs.

mod common;

use common::{append, commit, fixture, git as run_git, temp_dir};
use nxf::git;
use nxf::state::Tree;

/// A fixture for calls of `nxf::git` in this process. A variable of the caller
/// that names another repository or index would send those calls there.
fn own_repository() -> common::Fixture {
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
        assert!(std::env::var_os(name).is_none(), "{name} is set");
    }
    fixture()
}

/// The committed tree is HEAD's tree, and none with uncommitted changes.
#[test]
fn clean_tree() {
    let fx = own_repository();
    assert_eq!(git::clean_tree(&fx.checkout), Some(Tree::new(fx.tree())));
    append(&fx.checkout.join("flake.nix"), "# dirty\n");
    assert_eq!(git::clean_tree(&fx.checkout), None);
}

/// A tree maps back to its commit, and an unknown tree to none.
#[test]
fn commit_of() {
    let fx = own_repository();
    let first = Tree::new(fx.tree());
    assert_eq!(
        git::commit_of(&fx.checkout, std::slice::from_ref(&first)),
        Some(fx.head())
    );
    assert_eq!(
        git::commit_of(&fx.checkout, &[Tree::new("0".repeat(40))]),
        None
    );
    append(&fx.checkout.join("flake.nix"), "# more\n");
    commit(&fx.checkout, "more");
    // The newest commit with any of the trees wins.
    assert_eq!(
        git::commit_of(&fx.checkout, &[first, Tree::new(fx.tree())]),
        Some(fx.head())
    );
}

/// The commit line shows the short hash and subject.
#[test]
fn commit_line() {
    let fx = own_repository();
    // The repository's own length comes before the one of the user's configuration.
    append(&fx.checkout.join(".git/config"), "[core]\n\tabbrev = 12\n");
    let line = git::commit_line(&fx.checkout, "HEAD");
    let (short, rest) = line.split_once(' ').unwrap();
    assert_eq!(short, &fx.head()[..12], "{line}");
    assert!(rest.starts_with("init ("), "{line}");
}

/// A file is read as it was at a commit.
#[test]
fn show_file() {
    let fx = own_repository();
    let head = fx.head();
    std::fs::write(fx.checkout.join("flake.lock"), "changed\n").unwrap();
    let lock: serde_json::Value =
        serde_json::from_str(&git::show_file(&fx.checkout, &head, "flake.lock").unwrap()).unwrap();
    assert_eq!(lock["root"], "root");
}

/// A repository needs a commit to count.
#[test]
fn is_repo() {
    let fx = own_repository();
    assert!(git::is_repo(&fx.checkout));
    let empty = temp_dir("empty-repo");
    let without_git = git::is_repo(&empty);
    run_git(&empty, &["init", "--quiet"]);
    let without_commit = git::is_repo(&empty);
    // Removed before the assertions, so that a failure leaves nothing behind.
    std::fs::remove_dir_all(empty).unwrap();
    assert!(!without_git);
    assert!(!without_commit);
}

/// A changed file is noticed and restored to HEAD, staged or not.
#[test]
fn changed_restore() {
    let fx = own_repository();
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

/// One file is committed, as the identity that `identity` gives, while other
/// changes stay as they are.
#[test]
fn commit_file() {
    let fx = own_repository();
    append(&fx.checkout.join("flake.nix"), "# kept\n");
    append(&fx.checkout.join("flake.lock"), "\n");
    // This process reads the configuration of the user: the repository's own
    // settings come before it, so that no hook and no signing key of the user
    // is asked. Only the Git variables of the caller come before these.
    append(
        &fx.checkout.join(".git/config"),
        "[user]\n\tname = test\n\temail = test@example.invalid\n[commit]\n\tgpgSign = false\n[core]\n\thooksPath = /dev/null\n",
    );
    let identity = git::identity(&fx.checkout);
    git::commit_file(&fx.checkout, "flake.lock", "lock only").unwrap();
    assert_eq!(
        identity,
        Some(run_git(&fx.checkout, &["log", "-1", "--format=%cn <%ce>"]))
    );
    assert_eq!(
        run_git(&fx.checkout, &["log", "-1", "--format=%s"]),
        "lock only"
    );
    assert_eq!(fx.porcelain(), "M flake.nix");
}
