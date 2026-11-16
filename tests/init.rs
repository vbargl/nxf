//! Tests for src/init.rs: founding a machine flake from a template, end to end.

mod common;

use std::path::{Path, PathBuf};

use common::{assert_contains, assert_lacks, fake_systems, fixture, git, hostname, Fixture};
use nxf::init::MODULE;

/// A flake with two templates for the given host: `machine`, whose
/// configuration has the options of the nxf module, and `bare`, whose has not.
/// Both follow the fixture's local input `a`.
fn templates(fx: &Fixture, host: &str) -> PathBuf {
    let dir = fx.dir.join("templates");
    for (name, options) in [
        ("machine", "options.services.nxf = { };"),
        ("bare", "options.services = { };"),
    ] {
        let template = dir.join(name);
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(
            template.join("flake.nix"),
            format!(
                "{{\n  inputs.a = {{ url = \"git+file://{}/a\"; flake = false; }};\n  outputs = _: {{ nixosConfigurations.\"{host}\" = {{ {options} }}; }};\n}}\n",
                fx.dir.display()
            ),
        )
        .unwrap();
        std::fs::write(template.join("configuration.nix"), "{ }\n").unwrap();
        std::fs::write(template.join(".gitignore"), "/secrets\n").unwrap();
    }
    std::fs::write(
        dir.join("flake.nix"),
        "{\n  outputs = _: {\n    templates.machine = { path = ./machine; description = \"machine\"; };\n    templates.bare = { path = ./bare; description = \"bare\"; };\n  };\n}\n",
    )
    .unwrap();
    dir
}

fn template(fx: &Fixture, host: &str, name: &str) -> String {
    format!("{}#{name}", templates(fx, host).display())
}

fn status_of(output: &str, check: &str) -> Option<&'static str> {
    output.lines().find_map(|line| {
        let mut columns = line.split_whitespace();
        let status = match columns.next()? {
            "✓" => "ok",
            "!" => "warn",
            "✗" => "fail",
            _ => return None,
        };
        (columns.next()? == check).then_some(status)
    })
}

fn files(dir: &Path) -> Vec<String> {
    git(dir, &["ls-files"])
        .lines()
        .map(str::to_string)
        .collect()
}

/// Init unpacks the template, adds nxf.nix, and commits everything in a
/// repository without a remote; the result passes the checks and can update.
#[test]
fn founds_a_flake() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let target = fx.dir.join("etc/nixos");
    let result = fx.run(&[
        "init",
        target.to_str().unwrap(),
        "--template",
        &template(&fx, &hostname(), "machine"),
    ]);
    result.ok();
    assert_eq!(
        files(&target),
        [
            ".gitignore",
            "configuration.nix",
            "flake.lock",
            "flake.nix",
            "nxf.nix"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(target.join("nxf.nix")).unwrap(),
        MODULE
    );
    assert_eq!(git(&target, &["remote"]), "");
    assert_eq!(git(&target, &["status", "--porcelain"]), "");
    assert_eq!(git(&target, &["branch", "--show-current"]), "main");
    assert_eq!(
        git(&target, &["log", "--format=%s", "--reverse"]),
        "Found the machine flake\nflake.lock: lock the inputs"
    );
    for check in [
        "tools", "flake", "git", "lockfile", "writable", "identity", "changes", "host", "module",
    ] {
        assert_eq!(
            status_of(&result.stdout, check),
            Some("ok"),
            "{}",
            result.stdout
        );
    }
    assert!(
        result
            .stdout
            .ends_with("\n✓ Machine flake founded\n  All checks passed.\n"),
        "{}",
        result.stdout
    );

    // The founded flake is one nxf works with.
    let founded = fx.with("NXF_FLAKE", &target);
    founded.run(&["doctor"]).ok();
    founded.bump(&["a"]);
    founded.run(&["update"]).ok();
    assert_eq!(
        git(&target, &["log", "-1", "--format=%s"]),
        "flake.lock: update a"
    );
}

/// The module file enables automatic updates and shows every option of the
/// module commented out with its default.
#[test]
fn module_file() {
    assert_contains(
        MODULE,
        "  services.nxf.autoUpdate = {\n    enable = true;\n",
    );
    for option in [
        "# services.nxf.flake = \"/etc/nixos\";",
        "# user = \"root\";",
        "# schedule = \"*-*-* 00/6:00:00\";",
        "# randomizedDelay = \"1h\";",
        "# fixedDelay = true;",
        "# healthChecks = { };",
        "# healthTimeout = 300;",
        "# healthInterval = 10;",
        "# gitIdentity = ",
        "# gitCredentialsFile = null;",
        "# nixConfigFile = null;",
        "# timeout = \"3h\";",
    ] {
        assert_contains(MODULE, option);
    }
    let active: Vec<&str> = MODULE
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(
        active,
        [
            "{ config, ... }:",
            "{",
            "services.nxf.autoUpdate = {",
            "enable = true;",
            "};",
            "}"
        ]
    );
}

/// A template that does not import the nxf module is founded all the same;
/// init only says that the module is missing and what to add.
#[test]
fn template_without_module() {
    let fx = fixture();
    let target = fx.dir.join("machine");
    let result = fx.run(&[
        "init",
        target.to_str().unwrap(),
        "--template",
        &template(&fx, &hostname(), "bare"),
    ]);
    result.ok();
    assert_eq!(status_of(&result.stdout, "module"), Some("warn"));
    assert_contains(&result.stdout, "does not import the nxf module");
    assert_contains(&result.stdout, "nxf.nixosModules.default");
    assert!(target.join("nxf.nix").exists());
    assert_eq!(git(&target, &["status", "--porcelain"]), "");
}

/// A flake for another machine is founded anywhere, as on an operator's
/// computer, and checked for that host.
#[test]
fn for_another_host() {
    let fx = fixture();
    let target = fx.dir.join("staging/node-7");
    let node = template(&fx, "node-7", "machine");
    let result = fx.run(&[
        "init",
        target.to_str().unwrap(),
        "--template",
        &node,
        "--host",
        "node-7",
    ]);
    result.ok();
    assert_eq!(status_of(&result.stdout, "host"), Some("ok"));
    assert_eq!(status_of(&result.stdout, "module"), Some("ok"));
    assert_contains(&result.stdout, "nixosConfigurations.node-7 exists");

    // Without --host the same template is founded, and the missing host is reported.
    let other = fx.dir.join("staging/unnamed");
    let unnamed = fx.run(&["init", other.to_str().unwrap(), "--template", &node]);
    unnamed.ok();
    assert_eq!(status_of(&unnamed.stdout, "host"), Some("fail"));
    assert_contains(
        &unnamed.stdout,
        "✓ Machine flake founded\n  ! 1 check failed; fix this before the first build.",
    );
    assert_eq!(git(&other, &["status", "--porcelain"]), "");
}

/// Without a directory the flake is founded where nxf looks for it, and a
/// template in a local directory is named relative to the caller.
#[test]
fn default_directory_and_relative_template() {
    let fx = fixture();
    let target = fx.dir.join("default");
    let root = templates(&fx, &hostname());
    let fx = fx.with("NXF_FLAKE", &target);
    let output = fx
        .command(&["init", "--template", "./templates#machine"])
        .current_dir(root.parent().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target.join("flake.lock").exists());
    fx.run(&["doctor"]).ok();
}

/// Init never writes into a directory that already holds something.
#[test]
fn refuses_existing_content() {
    let fx = fixture();
    let machine = template(&fx, &hostname(), "machine");
    let result = fx.run(&[
        "init",
        fx.checkout.to_str().unwrap(),
        "--template",
        &machine,
    ]);
    assert_ne!(result.code, 0);
    assert_contains(&result.stderr, "is not empty");
    assert!(!fx.checkout.join("nxf.nix").exists());
    assert_eq!(fx.porcelain(), "");
    // An empty directory is fine.
    let empty = fx.dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    fx.run(&["init", empty.to_str().unwrap(), "--template", &machine])
        .ok();
}

/// A template may bring its own nxf.nix, which init keeps.
#[test]
fn keeps_the_templates_module_file() {
    let fx = fixture();
    let root = templates(&fx, &hostname());
    std::fs::write(
        root.join("machine/nxf.nix"),
        "{ services.nxf.autoUpdate.enable = false; }\n",
    )
    .unwrap();
    let target = fx.dir.join("own");
    let result = fx.run(&[
        "init",
        target.to_str().unwrap(),
        "--template",
        &format!("{}#machine", root.display()),
    ]);
    result.ok();
    assert_contains(&result.stdout, "nxf.nix of the template is kept");
    assert_contains(
        &std::fs::read_to_string(target.join("nxf.nix")).unwrap(),
        "enable = false",
    );
}

/// A template is required, and an unknown one founds nothing usable.
#[test]
fn invalid_requests() {
    let fx = fixture();
    let target = fx.dir.join("nothing");
    assert_ne!(fx.run(&["init", target.to_str().unwrap()]).code, 0);
    assert!(!target.exists());
    let missing = format!("{}#missing", templates(&fx, &hostname()).display());
    let result = fx.run(&["init", target.to_str().unwrap(), "--template", &missing]);
    assert_ne!(result.code, 0);
    assert_contains(&result.stderr, "cannot unpack the template");
    assert_lacks(&result.stdout, "Machine flake founded");
    assert!(!target.join(".git").exists());
}

/// A repository without a Git identity gets one of its own, so that the
/// first commit and later lockfile commits work for root on a new machine.
#[test]
fn identity_for_a_new_machine() {
    let mut fx = fixture();
    for name in [
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
    ] {
        fx.unset(name);
    }
    // Git must not derive an identity from the account either.
    fx.set("GIT_CONFIG_GLOBAL", fx.dir.join("gitconfig"));
    std::fs::write(fx.dir.join("gitconfig"), "[user]\n\tuseConfigOnly = true\n").unwrap();
    let target = fx.dir.join("fresh");
    let machine = template(&fx, "node-7", "machine");
    fx.run(&[
        "init",
        target.to_str().unwrap(),
        "--template",
        &machine,
        "--host",
        "node-7",
    ])
    .ok();
    assert_eq!(
        git(&target, &["log", "-1", "--format=%an <%ae>"]),
        "nxf <nxf@node-7>"
    );
}
