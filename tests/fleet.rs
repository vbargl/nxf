//! Tests for src/fleet.rs: commands against the machines of a hosts file,
//! end to end through a mock ssh.

mod common;

use std::path::PathBuf;

use common::{assert_contains, assert_lacks, fake_systems, fixture, write_script, Fixture, NXF};
use serde_json::{json, Value};

/// Three machines behind a mock ssh. Each has its own updater state; the mock
/// runs the remote command here, with that machine's environment. A machine
/// with a `down` file does not answer.
struct Machines {
    fx: Fixture,
    dir: PathBuf,
    file: String,
}

impl Machines {
    fn machine(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.test"))
    }

    fn updater(&self, name: &str) -> PathBuf {
        self.machine(name).join("updater")
    }

    fn run(&self, args: &[&str]) -> common::Ran {
        self.fx.run(&[args, &["--config", &self.file]].concat())
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("ssh.log"))
            .map(|log| log.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

fn machines() -> Machines {
    let mut fx = fixture();
    let dir = fx.dir.join("machines");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::os::unix::fs::symlink(NXF, dir.join("bin/nxf")).unwrap();
    for name in ["node-1", "node-2", "node-3"] {
        std::fs::create_dir_all(dir.join(format!("{name}.test/updater"))).unwrap();
    }
    let ssh = dir.join("ssh");
    write_script(
        &ssh,
        &format!(
            r#"#!/usr/bin/env bash
options=()
while [[ $1 == -* ]]; do options+=("$1" "$2"); shift 2; done
destination=$1; shift
echo "${{options[*]}} $destination $*" >> '{dir}/ssh.log'
machine='{dir}'/${{destination#*@}}
if [[ -e $machine/down ]]; then
    echo "ssh: connect to host ${{destination#*@}} port 22: Connection timed out" >&2
    exit 255
fi
export NXF_UPDATER_STATE=$machine/updater XDG_STATE_HOME=$machine/state
[[ ! -e $machine/env ]] || source "$machine/env"
PATH='{dir}/bin':$PATH exec bash -c "$*"
"#,
            dir = dir.display()
        ),
    );
    fx.set("NXF_SSH", &ssh);
    let file = dir.join("hosts.toml");
    std::fs::write(
        &file,
        "[k3s]\nversion = \"1.31\"\n\n[hosts.node-2.ssh]\naddress = \"node-2.test\"\nuser = \"admin\"\nport = 2222\n\n[hosts.node-1.ssh]\naddress = \"node-1.test\"\n\n[hosts.node-3.ssh]\naddress = \"node-3.test\"\n",
    )
    .unwrap();
    Machines {
        fx,
        file: file.display().to_string(),
        dir,
    }
}

/// Status shows one row for each machine, in name order, and one outcome.
#[test]
fn status() {
    let set = machines();
    let system = &fake_systems().one;
    // node-1 runs its checkout and updated; node-2 is held; node-3 never updated.
    let records = set.machine("node-1").join("state/nxf/systems");
    std::fs::create_dir_all(&records).unwrap();
    std::os::unix::fs::symlink(system, records.join(set.fx.tree())).unwrap();
    std::fs::write(
        set.machine("node-1").join("env"),
        format!("export NXF_SYSTEM={}\n", system.display()),
    )
    .unwrap();
    std::fs::write(
        set.updater("node-1").join("update.json"),
        json!({"time": nxf::hold::now() - 7200, "results": []}).to_string(),
    )
    .unwrap();
    std::fs::write(
        set.updater("node-2").join("hold.json"),
        json!({"reason": "release week", "since": 1, "until": null}).to_string(),
    )
    .unwrap();
    let result = set.run(&["status"]);
    result.ok();
    let short = &set.fx.head()[..12];
    assert_contains(&result.stdout, &format!("{} · 3 machines\n\n", set.file));
    assert_contains(
        &result.stdout,
        &format!("\n  ✓ node-1    {short}  updated 2 hours ago\n"),
    );
    assert_contains(
        &result.stdout,
        "\n  ! node-2    unknown       no automatic update yet  held: release week, checkout is not what runs\n",
    );
    assert_contains(&result.stdout, "\n  ! node-3    unknown ");
    assert!(
        result
            .stdout
            .ends_with("\n\n! 2 of 3 machines need attention\n"),
        "{}",
        result.stdout
    );
    // ssh is told who, where and never to ask.
    let calls = set.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.contains("-o BatchMode=yes")));
    assert!(calls
        .iter()
        .any(|call| call.contains("-p 2222 admin@node-2.test nxf status --json")));
    assert!(calls
        .iter()
        .any(|call| call.ends_with(" root@node-1.test nxf status --json")));
}

/// A machine that does not answer is a failed row, and the command fails.
#[test]
fn unreachable() {
    let set = machines();
    std::fs::write(set.machine("node-2").join("down"), "").unwrap();
    let result = set.run(&["status"]);
    assert_eq!(result.code, 1);
    assert_contains(
        &result.stdout,
        "\n  ✗ node-2    unreachable: ssh: connect to host node-2.test port 22: Connection timed out\n",
    );
    assert_contains(&result.stdout, "\n  ! node-1    ");
    assert_contains(
        &result.stderr,
        "✗ 1 of 3 machines gave no status\n  2 machines need attention.",
    );
}

/// The JSON status has one entry for each machine: its status, or an error.
#[test]
fn status_json() {
    let set = machines();
    std::fs::write(set.machine("node-3").join("down"), "").unwrap();
    let result = set.run(&["status", "--json"]);
    assert_eq!(result.code, 1);
    let all: Value = serde_json::from_str(&result.stdout).unwrap();
    assert_eq!(all.as_object().unwrap().len(), 3);
    assert_eq!(all["node-1"]["matches"], false);
    assert_eq!(all["node-1"]["rebootRequired"], false);
    assert_eq!(all["node-2"]["inputs"], json!(["a", "b"]));
    assert_contains(all["node-3"]["error"].as_str().unwrap(), "unreachable");
    assert_eq!(result.stderr, "");
}

/// One command holds automatic updates on every machine, with one end for
/// all, and one releases them.
#[test]
fn hold_and_release() {
    let set = machines();
    let result = set.run(&["hold", "--reason", "it's a bad update", "--for", "3d"]);
    result.ok();
    assert!(
        result
            .stdout
            .ends_with("\n\n✓ Automatic updates are held on 3 machines\n"),
        "{}",
        result.stdout
    );
    assert_contains(
        &result.stdout,
        "\n  ✓ node-1    Automatic updates are held: it's a bad update (until 20",
    );
    let holds: Vec<Value> = ["node-1", "node-2", "node-3"]
        .iter()
        .map(|name| {
            serde_json::from_str(
                &std::fs::read_to_string(set.updater(name).join("hold.json")).unwrap(),
            )
            .unwrap()
        })
        .collect();
    assert!(holds
        .iter()
        .all(|hold| hold["reason"] == "it's a bad update"));
    let until = holds[0]["until"].as_u64().unwrap();
    assert!(holds.iter().all(|hold| hold["until"] == until));
    assert!(until > nxf::hold::now() + 259_000);
    // The machine itself is not held: the command ran elsewhere.
    assert!(!set.fx.updater.join("hold.json").exists());

    let status = set.run(&["status"]);
    assert_contains(&status.stdout, "held: it's a bad update (until 20");

    let released = set.run(&["release"]);
    released.ok();
    assert_contains(
        &released.stdout,
        "\n  ✓ node-2    Automatic updates continue.\n",
    );
    assert!(released
        .stdout
        .ends_with("\n✓ Automatic updates continue on 3 machines\n"));
    assert!(!set.updater("node-1").join("hold.json").exists());
}

/// A machine that could not be held is named, and the command fails.
#[test]
fn hold_partly_fails() {
    let set = machines();
    std::fs::write(set.machine("node-1").join("down"), "").unwrap();
    let result = set.run(&["hold", "--reason", "stop"]);
    assert_eq!(result.code, 1);
    assert_contains(&result.stdout, "\n  ✗ node-1    unreachable: ");
    assert_contains(
        &result.stdout,
        "\n  ✓ node-2    Automatic updates are held: stop",
    );
    assert_contains(&result.stderr, "✗ Not held on 1 of 3 machines");
    assert!(set.updater("node-2").join("hold.json").exists());
}

/// Version and doctor report what each machine said last.
#[test]
fn version_and_doctor() {
    let set = machines();
    let version = set.run(&["version"]);
    version.ok();
    assert_contains(
        &version.stdout,
        &format!("\n  ✓ node-3    nxf {}\n", env!("CARGO_PKG_VERSION")),
    );
    let doctor = set.run(&["doctor"]);
    doctor.ok();
    assert_contains(
        &doctor.stdout,
        "\n  ✓ node-1    ✓ Checks passed, 1 warning\n",
    );
    assert!(doctor.stdout.ends_with("\n✓ Checks passed on 3 machines\n"));
}

/// Requests that make no sense for a set of machines, and unusable files, are refused.
#[test]
fn invalid_requests() {
    let set = machines();
    for args in [&["status", "--detailed"][..], &["status", "a"]] {
        let result = set.run(args);
        assert_ne!(result.code, 0);
        assert_contains(
            &result.stderr,
            "With --config, status shows one row for each machine",
        );
    }
    assert!(set.calls().is_empty());
    let missing = set
        .fx
        .run(&["status", "--config", "/nonexistent/hosts.toml"]);
    assert_ne!(missing.code, 0);
    assert_contains(&missing.stderr, "✗ Cannot read /nonexistent/hosts.toml");
    // Tasks never take a hosts file.
    let task = set.run(&["switch"]);
    assert_ne!(task.code, 0);
    assert_lacks(&task.stdout, "Building");
    assert!(set.calls().is_empty());
}

/// Words are quoted in forms that sh, fish and Nushell read alike.
#[test]
fn quoting() {
    use nxf::fleet::quoted;
    assert_eq!(quoted("status"), "status");
    assert_eq!(quoted("@1790000000"), "@1790000000");
    assert_eq!(quoted("release week"), "'release week'");
    assert_eq!(quoted(""), "''");
    assert_eq!(quoted("it's bad"), "\"it's bad\"");
    assert_eq!(quoted("say \"no\" & $HOME"), "'say \"no\" & $HOME'");
    assert_eq!(quoted("it's $HOME"), "'it'\\''s $HOME'");
}
