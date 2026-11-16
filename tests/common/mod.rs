//! Shared test fixtures: a throwaway machine flake with two local inputs
//! behind NXF_FLAKE, a mock nh, fake built systems, and helpers to run nxf
//! against them. Nothing here touches the machine.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub const NXF: &str = env!("CARGO_BIN_EXE_nxf");

const GIT_ENV: [(&str, &str); 6] = [
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_AUTHOR_NAME", "test"),
    ("GIT_AUTHOR_EMAIL", "test@example.invalid"),
    ("GIT_COMMITTER_NAME", "test"),
    ("GIT_COMMITTER_EMAIL", "test@example.invalid"),
];

/// Variables of the caller's environment that would change what nxf does.
const INHERITED: [&str; 13] = [
    "NXF_BOOTED",
    "NXF_HEALTH_DIR",
    "NXF_HEALTH_TIMEOUT",
    "NXF_HEALTH_INTERVAL",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "NXF_FLAKE",
    "NXF_STATE",
    "NXF_SYSTEM",
    "NXF_UPDATER_STATE",
    "NXF_SYSTEMCTL",
    "XDG_STATE_HOME",
];

pub struct Ran {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    fn from(output: Output) -> Self {
        Ran {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    pub fn ok(&self) -> &Self {
        assert_eq!(
            self.code, 0,
            "stdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }
}

pub fn sh(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .envs(GIT_ENV)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("cannot run {program}: {err}"));
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut all = vec!["-C", dir.to_str().unwrap()];
    all.extend(args);
    sh("git", &all)
}

pub fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "--all"]);
    git(dir, &["commit", "--quiet", "--message", message]);
}

pub fn temp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nxf-{label}.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap()
        .trim()
        .to_string()
}

pub fn write_script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn append(path: &Path, text: &str) {
    let mut content = std::fs::read_to_string(path).unwrap();
    content.push_str(text);
    std::fs::write(path, content).unwrap();
}

/// A machine flake with two local non-flake inputs, a and b, as NXF_FLAKE,
/// plus a mock nh that records its calls and links its out-link to
/// $NXF_TEST_BUILD.
pub struct Fixture {
    pub dir: PathBuf,
    pub checkout: PathBuf,
    pub log: PathBuf,
    pub state: PathBuf,
    pub updater: PathBuf,
    pub systemctl: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&self.dir)
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn fixture() -> Fixture {
    let dir = temp_dir("test");
    for input in ["a", "b"] {
        let repo = dir.join(input);
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("v"), "1").unwrap();
        git(&repo, &["init", "--quiet", "--initial-branch", "main"]);
        commit(&repo, "1");
    }
    let checkout = dir.join("flake");
    std::fs::create_dir_all(&checkout).unwrap();
    let root = dir.display();
    std::fs::write(
        checkout.join("flake.nix"),
        format!(
            "{{\n  inputs.a = {{ url = \"git+file://{root}/a\"; flake = false; }};\n  inputs.b = {{ url = \"git+file://{root}/b\"; flake = false; }};\n  outputs = _: {{ nixosConfigurations.\"{}\" = {{ }}; }};\n}}\n",
            hostname()
        ),
    )
    .unwrap();
    git(&checkout, &["init", "--quiet", "--initial-branch", "main"]);
    git(&checkout, &["add", "flake.nix"]);
    sh(
        "nix",
        &[
            "flake",
            "lock",
            &format!("git+file://{}", checkout.display()),
        ],
    );
    commit(&checkout, "init");

    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("nh.log");
    write_script(
        &bin.join("nh"),
        &format!(
            r#"#!/usr/bin/env bash
echo "$* @ $(git -C '{checkout}' rev-parse --short HEAD)" >> '{log}'
[[ -z ${{NXF_TEST_NH_STDOUT:-}} ]] || printf '%s\n' "$NXF_TEST_NH_STDOUT"
[[ -z ${{NXF_TEST_NH_STDERR:-}} ]] || printf '%s\n' "$NXF_TEST_NH_STDERR" >&2
if [[ -n ${{NXF_TEST_NH_WAIT:-}} ]]; then
    for _ in $(seq 300); do [[ -e $NXF_TEST_NH_WAIT ]] && break; sleep 0.01; done
fi
[[ ${{NXF_TEST_NH_STATUS:-0}} == 0 ]] || exit "$NXF_TEST_NH_STATUS"
[[ $2 != ${{NXF_TEST_NH_FAIL_ACTION:-}} ]] || exit 3
ln -sfn "${{NXF_TEST_BUILD:-{root}/system}}" "${{@: -1}}"
"#,
            checkout = checkout.display(),
            log = log.display(),
        ),
    );
    let systemctl = bin.join("systemctl");
    write_script(&systemctl, "#!/bin/sh\nexit 0\n");

    let state = dir.join("state");
    let updater = dir.join("updater");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut env: BTreeMap<String, String> = GIT_ENV
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    for (name, value) in [
        ("NXF_FLAKE", checkout.display().to_string()),
        ("XDG_STATE_HOME", state.display().to_string()),
        ("NXF_SYSTEM", dir.join("no-system").display().to_string()),
        ("NXF_UPDATER_STATE", updater.display().to_string()),
        ("NXF_SYSTEMCTL", systemctl.display().to_string()),
        (
            "NXF_BOOTED",
            dir.join("no-booted-system").display().to_string(),
        ),
        (
            "NXF_HEALTH_DIR",
            dir.join("no-health-checks").display().to_string(),
        ),
        ("NO_COLOR", "1".to_string()),
        ("PATH", path),
    ] {
        env.insert(name.to_string(), value);
    }
    Fixture {
        dir,
        checkout,
        log,
        state,
        updater,
        systemctl,
        env,
    }
}

impl Fixture {
    /// Adds environment for nxf, such as NXF_SYSTEM.
    pub fn with(mut self, name: &str, value: impl AsRef<Path>) -> Self {
        self.set(name, value);
        self
    }

    pub fn set(&mut self, name: &str, value: impl AsRef<Path>) {
        self.env
            .insert(name.to_string(), value.as_ref().display().to_string());
    }

    pub fn unset(&mut self, name: &str) {
        self.env.remove(name);
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(NXF);
        command.args(args).stdin(Stdio::null());
        for name in INHERITED {
            command.env_remove(name);
        }
        command.envs(&self.env);
        command
    }

    pub fn run(&self, args: &[&str]) -> Ran {
        Ran::from(self.command(args).output().unwrap())
    }

    /// Runs nxf in a pseudo-terminal of the given width; stderr joins stdout.
    pub fn terminal(&self, width: usize, args: &[&str]) -> Ran {
        let mut script = Command::new("script");
        script
            .args(["--quiet", "--return", "--command"])
            .arg(format!(
                "stty cols {width} rows 24; exec '{NXF}' {}",
                args.join(" ")
            ))
            .arg("/dev/null")
            .stdin(Stdio::null());
        for name in INHERITED {
            script.env_remove(name);
        }
        script.envs(&self.env);
        Ran::from(script.output().unwrap())
    }

    pub fn locked(&self, input: &str) -> String {
        let lock: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(self.checkout.join("flake.lock")).unwrap(),
        )
        .unwrap();
        lock["nodes"][input]["locked"]["rev"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub fn short(&self, input: &str) -> String {
        self.locked(input)[..12].to_string()
    }

    /// Bumps the given inputs to a new commit.
    pub fn bump(&self, inputs: &[&str]) {
        for input in inputs {
            let repo = self.dir.join(input);
            let version: u32 = std::fs::read_to_string(repo.join("v"))
                .unwrap()
                .parse()
                .unwrap();
            std::fs::write(repo.join("v"), (version + 1).to_string()).unwrap();
            commit(&repo, "bump");
        }
    }

    pub fn head(&self) -> String {
        git(&self.checkout, &["rev-parse", "HEAD"])
    }

    pub fn short_head(&self) -> String {
        git(&self.checkout, &["rev-parse", "--short", "HEAD"])
    }

    pub fn tree(&self) -> String {
        git(&self.checkout, &["rev-parse", "HEAD^{tree}"])
    }

    pub fn porcelain(&self) -> String {
        git(&self.checkout, &["status", "--porcelain"])
    }

    pub fn nh_calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .map(|log| log.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The nh action of every call, such as `build`.
    pub fn nh_actions(&self) -> Vec<String> {
        self.nh_calls()
            .iter()
            .map(|call| call.split(' ').nth(1).unwrap_or("").to_string())
            .collect()
    }

    pub fn systems(&self) -> PathBuf {
        self.state.join("nxf/systems")
    }

    /// Records that the checkout's current tree built the given system.
    pub fn record_build(&self, system: &Path) {
        let systems = self.systems();
        std::fs::create_dir_all(&systems).unwrap();
        let link = systems.join(self.tree());
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(system, link).unwrap();
    }

    pub fn save_update(&self, report: serde_json::Value) {
        std::fs::create_dir_all(&self.updater).unwrap();
        std::fs::write(self.updater.join("update.json"), report.to_string()).unwrap();
    }
}

pub struct Systems {
    pub one: PathBuf,
    pub two: PathBuf,
}

/// Two distinct store paths standing in for built systems.
pub fn fake_systems() -> &'static Systems {
    static SYSTEMS: OnceLock<Systems> = OnceLock::new();
    SYSTEMS.get_or_init(|| {
        let dir = temp_dir("systems");
        let add = |name: &str, content: String| {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("file"), content).unwrap();
            PathBuf::from(sh("nix-store", &["--add", path.to_str().unwrap()]))
        };
        let numbers: Vec<String> = (1..=2000).map(|number| number.to_string()).collect();
        let systems = Systems {
            one: add("one", "one".to_string()),
            two: add("two", numbers.join(" ")),
        };
        let _ = std::fs::remove_dir_all(&dir);
        systems
    })
}

/// Internal-json lines as Nix prints them.
pub fn events(records: &[serde_json::Value]) -> String {
    records
        .iter()
        .map(|record| format!("@nix {record}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[track_caller]
pub fn assert_contains(text: &str, part: &str) {
    assert!(text.contains(part), "expected <{part}> in:\n{text}");
}

#[track_caller]
pub fn assert_lacks(text: &str, part: &str) {
    assert!(!text.contains(part), "unexpected <{part}> in:\n{text}");
}
