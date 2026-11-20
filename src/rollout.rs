//! Bringing every machine of a hosts file to the same state: installed by
//! nxf. A machine that already is stays untouched, so the command can be
//! repeated. Any other machine is started into a NixOS installer and
//! installed there with `nxf init` and `nxf install`.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::fleet::quoted;
use crate::hosts::Host;
use crate::output::{self, Mark};
use crate::text::plain;

/// The installer image nxf starts on a machine that runs another system.
pub const KEXEC_URL: &str = "https://github.com/nix-community/nixos-images/releases/download/nixos-25.11/nixos-kexec-installer-noninteractive-{arch}-linux.tar.gz";

pub struct Options {
    pub file: PathBuf,
    pub template: String,
    /// A directory with one directory of files for each machine, by name.
    pub files: Option<PathBuf>,
    pub yes: bool,
    pub kexec_url: Option<String>,
    /// A known_hosts file that gets the host keys of the installed machines.
    pub known_hosts: Option<PathBuf>,
}

/// What a machine is before rollout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// Installed by nxf, or running nxf: nothing to do.
    Managed,
    /// A NixOS installer, ready to install from.
    Installer,
    /// Another system, with its name. Its disks would be erased.
    Other(String),
    Unreachable(String),
}

/// Says in plain sh what the machine is; sent on standard input, so that no
/// login shell reads it. The installation record or nxf in the system mean
/// managed wherever the root is; nxf on the path alone may be an installer's copy.
pub const PROBE: &str = r#"
root=$(awk '$2 == "/" { kind = $3 } END { print kind }' /proc/mounts)
installer=no
case $root in tmpfs|rootfs|ramfs) [ -e /etc/NIXOS ] && installer=yes ;; esac
managed=no
if [ -e /var/lib/nxf/installed.json ] || [ -x /run/current-system/sw/bin/nxf ]; then
    managed=yes
elif [ $installer = no ] && command -v nxf >/dev/null 2>&1; then
    managed=yes
fi
os=$(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown system}")
echo "nxf-probe managed=$managed installer=$installer uid=$(id -u 2>/dev/null || echo unknown) arch=$(uname -m)"
echo "nxf-os $os"
"#;

pub struct Probe {
    pub found: Found,
    pub arch: String,
    /// The user id the probe ran as, when it told one. Installing needs 0.
    pub user: Option<String>,
}

/// Reads the answer of the probe.
pub fn parse_probe(text: &str) -> Option<Probe> {
    let facts = text
        .lines()
        .find_map(|line| line.strip_prefix("nxf-probe "))?;
    let fact = |name: &str| {
        facts
            .split_whitespace()
            .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
    };
    let os = text
        .lines()
        .find_map(|line| line.strip_prefix("nxf-os "))
        .unwrap_or("unknown system");
    let found = match (fact("managed")?, fact("installer")?) {
        ("yes", _) => Found::Managed,
        (_, "yes") => Found::Installer,
        _ => Found::Other(os.to_string()),
    };
    Some(Probe {
        found,
        arch: fact("arch")?.to_string(),
        user: fact("uid").map(str::to_string),
    })
}

fn seconds(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// How ssh checks the machine's host key.
#[derive(Clone)]
enum Trust {
    /// Before and during the installation: a key the user knows must match,
    /// and a key nobody knows yet is accepted into a file of this run only.
    First { seen: PathBuf },
    /// After the installation: only the key the installation made.
    Made { known: PathBuf },
}

#[derive(Clone)]
struct Link {
    host: Host,
    trust: Trust,
}

impl Link {
    /// The known_hosts files whose keys the link trusts.
    fn trusted(&self) -> Vec<PathBuf> {
        match &self.trust {
            Trust::First { seen } => std::iter::once(seen.clone())
                .chain(std::env::var("HOME").map(|home| format!("{home}/.ssh/known_hosts").into()))
                .collect(),
            Trust::Made { known } => vec![known.clone()],
        }
    }

    fn options(&self) -> Vec<String> {
        self.options_with(&self.trusted())
    }

    /// The options of ssh, with the given known_hosts files for those of the link.
    fn options_with(&self, known_hosts: &[PathBuf]) -> Vec<String> {
        let known_hosts: Vec<String> = known_hosts
            .iter()
            .map(|file| file.display().to_string())
            .collect();
        let known_hosts = format!("UserKnownHostsFile={}", known_hosts.join(" "));
        let mut options = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            format!("ConnectTimeout={}", seconds("NXF_SSH_CONNECT_TIMEOUT", 10)),
        ];
        match &self.trust {
            Trust::First { .. } => options.extend([
                "-o".to_string(),
                "StrictHostKeyChecking=accept-new".to_string(),
                "-o".to_string(),
                known_hosts,
            ]),
            Trust::Made { .. } => options.extend([
                "-o".to_string(),
                "StrictHostKeyChecking=yes".to_string(),
                "-o".to_string(),
                known_hosts,
                "-o".to_string(),
                "GlobalKnownHostsFile=/dev/null".to_string(),
            ]),
        }
        if let Some(port) = self.host.port {
            options.extend(["-p".to_string(), port.to_string()]);
        }
        options
    }

    fn ssh(&self, remote: &str) -> Command {
        let program = std::env::var_os("NXF_SSH")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "ssh".into());
        let mut ssh = Command::new(program);
        ssh.args(self.options())
            .arg(self.host.destination())
            .arg(remote);
        ssh
    }

    /// Runs a remote command with the given standard input and returns
    /// whether it succeeded, and what it printed.
    fn capture(&self, remote: &str, input: &str) -> (bool, String, String) {
        let mut ssh = self.ssh(remote);
        ssh.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let Ok(mut child) = ssh.spawn() else {
            return (false, String::new(), "cannot run ssh".to_string());
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        match child.wait_with_output() {
            Ok(output) => (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ),
            Err(err) => (false, String::new(), err.to_string()),
        }
    }

    /// Runs a remote command. On a terminal its latest line is shown until
    /// the next one replaces it; what it printed is kept for the caller and,
    /// when it fails, its last lines are the error.
    fn stream(&self, remote: &str, what: &str) -> Result<String, String> {
        let (reader, writer) = os_pipe::pipe().map_err(|err| err.to_string())?;
        let errors = writer.try_clone().map_err(|err| err.to_string())?;
        let mut ssh = self.ssh(remote);
        let mut child = ssh
            .stdin(Stdio::null())
            .stdout(writer)
            .stderr(errors)
            .spawn()
            .map_err(|err| format!("cannot run ssh: {err}"))?;
        drop(ssh);
        let mut said = String::new();
        let mut pending = false;
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let line = plain(&line);
            if !line.trim().is_empty() {
                output::settle(pending);
                pending = output::pending(line.trim());
            }
            said.push_str(&line);
            said.push('\n');
        }
        output::settle(pending);
        let status = child.wait().map_err(|err| err.to_string())?;
        if status.success() {
            return Ok(said);
        }
        let lines: Vec<&str> = said
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        Err(format!(
            "{what} failed\n{}",
            lines[lines.len().saturating_sub(8)..].join("\n")
        ))
    }

    fn probe(&self) -> Probe {
        let (ok, stdout, stderr) = self.capture("sh", PROBE);
        match parse_probe(&stdout) {
            Some(probe) if ok => probe,
            _ => {
                let said = plain(&stderr);
                let why = said
                    .lines()
                    .map(str::trim)
                    .rfind(|line| !line.is_empty())
                    .unwrap_or("no answer");
                Probe {
                    found: Found::Unreachable(why.to_string()),
                    arch: String::new(),
                    user: None,
                }
            }
        }
    }

    /// Waits until the machine is what is expected.
    fn wait_for(&self, expected: impl Fn(&Found) -> bool, what: &str) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(seconds("NXF_ROLLOUT_WAIT", 600));
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let probe = self.probe();
            if expected(&probe.found) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "{what} did not come up in time\nLast seen: {:?}",
                    probe.found
                ));
            }
        }
    }
}

/// The package this nxf runs from, which is copied to the machines.
/// NXF_PACKAGE stands in for it when nxf does not run from the store.
fn package() -> Result<PathBuf, String> {
    if let Some(package) = std::env::var_os("NXF_PACKAGE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(package));
    }
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|err| format!("cannot find the nxf program: {err}"))?;
    let mut parts = exe.components();
    let store: PathBuf = parts.by_ref().take(4).collect();
    if store.starts_with("/nix/store") && store.components().count() == 4 {
        Ok(store)
    } else {
        Err(format!(
            "{} is not in the Nix store, so it cannot be copied to the machines\nRun nxf from its package.",
            exe.display()
        ))
    }
}

/// The source of the template as a store path, and the name of the template.
/// The machines get the same source, without a way to the repository.
/// `nix flake prefetch` writes the source to the store; `nix flake metadata`
/// may only name the path it would have there.
pub fn template(reference: &str) -> Result<(PathBuf, String), String> {
    let (flake, name) = reference
        .split_once('#')
        .ok_or("The template must be given as <flake>#<name>")?;
    let output = Command::new("nix")
        .args(["flake", "prefetch", "--json", flake])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run nix: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "Cannot read the template {flake}\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let fetched: Value = serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())?;
    let path = fetched["storePath"]
        .as_str()
        .ok_or("nix flake prefetch gave no store path")?;
    Ok((PathBuf::from(path), name.to_string()))
}

fn copy(link: &Link, paths: &[&Path], stem: &Path) -> Result<(), String> {
    if !matches!(link.trust, Trust::First { .. }) {
        return Err("the machine is already installed".to_string());
    }
    // One file with the keys the link trusts: not every Nix splits NIX_SSHOPTS
    // as a shell does, so no option may need a quote.
    let trusted = stem.with_extension("trusted");
    // Read as ssh reads them, as bytes; only a file that is not there is left out.
    let mut keys = Vec::new();
    for file in link.trusted() {
        match std::fs::read(&file) {
            Ok(known) => {
                keys.extend(known);
                keys.push(b'\n');
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("cannot read {}: {err}", file.display())),
        }
    }
    // For the user alone: it holds a copy of the keys the user knows.
    let mut merged = std::fs::OpenOptions::new();
    merged.write(true).create(true).truncate(true).mode(0o600);
    merged
        .open(&trusted)
        .and_then(|mut file| file.write_all(&keys))
        .map_err(|err| format!("cannot write {}: {err}", trusted.display()))?;
    let mut nix = Command::new("nix");
    nix.args(["copy", "--no-check-sigs", "--to"])
        .arg(format!("ssh-ng://{}", link.host.destination()))
        .args(paths)
        .env("NIX_SSHOPTS", link.options_with(&[trusted]).join(" "));
    output::stream(&mut nix, true, &[])
        .map_err(|err| format!("Copying nxf to the machine failed\n{err}"))
}

/// Sends the files of a machine as a tar stream into a directory there. tar
/// writes into the connection, and the upload fails unless both end well.
fn upload(link: &Link, files: &Path, to: &str) -> Result<(), String> {
    let mut tar = Command::new("tar")
        .arg("--create")
        .arg("--directory")
        .arg(files)
        .arg(".")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run tar: {err}"))?;
    let archive = tar.stdout.take().ok_or("cannot read from tar")?;
    let mut ssh = link.ssh(&format!(
        "rm -rf {to} && mkdir -p {to} && tar --extract --directory {to}"
    ));
    let sent = ssh
        .stdin(archive)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    // The command holds the read end of the archive until it is dropped.
    drop(ssh);
    let packed = std::thread::spawn(move || tar.wait_with_output());
    let sent = sent.and_then(|child| child.wait_with_output());
    let packed = packed.join().map_err(|_| "cannot wait for tar")?;
    let (packed, sent) = (
        packed.map_err(|err| format!("cannot run tar: {err}"))?,
        sent.map_err(|err| format!("cannot run ssh: {err}"))?,
    );
    if !packed.status.success() || !sent.status.success() {
        let said = format!(
            "{}\n{}",
            String::from_utf8_lossy(&packed.stderr).trim(),
            String::from_utf8_lossy(&sent.stderr).trim()
        );
        return Err(format!(
            "Sending the machine's files failed\n{}",
            said.trim()
        ));
    }
    Ok(())
}

struct Installed {
    key: String,
    revision: Option<String>,
}

/// Installs one machine and returns its host key.
fn install(
    host: &Host,
    probe: &Probe,
    options: &Options,
    package: &Path,
    source: &Path,
    name: &str,
    stem: &Path,
) -> Result<Installed, String> {
    let seen = stem.with_extension("seen");
    let link = Link {
        host: host.clone(),
        trust: Trust::First { seen },
    };
    let files = options
        .files
        .as_ref()
        .map(|files| files.join(&host.name))
        .filter(|files| files.is_dir());
    output::line("");
    output::context(&format!("{} · {}", host.name, host.destination()));
    output::phases(0, 4);

    output::start("Starting the installer");
    if probe.found == Found::Installer {
        output::row(Mark::Quiet, "Already in a NixOS installer");
    } else {
        let url = options
            .kexec_url
            .clone()
            .unwrap_or_else(|| KEXEC_URL.to_string())
            .replace("{arch}", &probe.arch);
        let script = format!(
            "set -e\nmkdir -p /root/kexec && cd /root/kexec\nif command -v curl >/dev/null 2>&1; then curl -fsSL {image}; else wget -qO- {image}; fi | tar -xzf -\nTMPDIR=/root/kexec setsid --wait /root/kexec/kexec/run\n",
            image = quoted(&url)
        );
        let (ok, stdout, stderr) = link.capture("sh", &script);
        if !ok {
            let said = plain(&format!("{stdout}\n{stderr}"));
            let lines: Vec<&str> = said
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            return Err(format!(
                "Starting the installer failed\n{}",
                lines[lines.len().saturating_sub(8)..].join("\n")
            ));
        }
        link.wait_for(|found| *found == Found::Installer, "The installer")?;
        output::row(Mark::Done, "Started from memory");
        output::sub("Image", &url);
    }

    output::start("Founding the machine flake");
    copy(&link, &[package, source], stem)?;
    // What the operator and the file system name is one word each to the machine's shell.
    let nxf = quoted(&format!("{}/bin/nxf", package.display()));
    let template = quoted(&format!("path:{}#{name}", source.display()));
    let mut init = format!(
        "rm -rf /tmp/nxf-machine && {nxf} init /tmp/nxf-machine --template {template} --hardware"
    );
    if let Some(files) = &files {
        upload(&link, files, "/tmp/nxf-files")?;
        init.push_str(" --files /tmp/nxf-files");
    }
    link.stream(&init, "Founding the machine flake")?;
    output::row(Mark::Done, "Founded on the machine");

    output::start("Installing");
    let said = link.stream(
        &format!("{nxf} install /tmp/nxf-machine"),
        "The installation",
    )?;
    let key = said
        .lines()
        .find_map(|line| line.trim().strip_prefix("Host key"))
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .ok_or("The installation did not report the host key")?;
    output::row(Mark::Done, "Installed");

    output::start("Starting the system");
    let _ = link.capture("systemctl reboot", "");
    let known = stem.with_extension("known");
    std::fs::write(&known, format!("{}\n", known_hosts_line(host, &key)))
        .map_err(|err| format!("cannot write {}: {err}", known.display()))?;
    let started = Link {
        host: host.clone(),
        trust: Trust::Made { known },
    };
    started.wait_for(|found| *found == Found::Managed, "The installed system")?;
    let (_, status, _) = started.capture("nxf status --json", "");
    let revision = serde_json::from_str::<Value>(&status)
        .ok()
        .and_then(|status| status["revision"].as_str().map(str::to_string));
    output::row(
        Mark::Done,
        "Started, and its host key is the one it was given",
    );
    Ok(Installed { key, revision })
}

/// The line that makes ssh know the machine.
pub fn known_hosts_line(host: &Host, key: &str) -> String {
    match host.port {
        Some(port) if port != 22 => format!("[{}]:{port} {key}", host.address),
        _ => format!("{} {key}", host.address),
    }
}

fn remember(file: &Path, host: &Host, key: &str) -> Result<(), String> {
    let name = match host.port {
        Some(port) if port != 22 => format!("[{}]:{port}", host.address),
        _ => host.address.clone(),
    };
    // Drop the key of the system that was there before: it must not stay trusted.
    if file.exists() {
        let removed = Command::new("ssh-keygen")
            .args(["-R", &name, "-f"])
            .arg(file)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("cannot run ssh-keygen: {err}"))?;
        if !removed.status.success() {
            let said = plain(&String::from_utf8_lossy(&removed.stderr));
            let why = said.lines().map(str::trim).rfind(|line| !line.is_empty());
            return Err(format!(
                "cannot remove the old key of {name} from {}: {}",
                file.display(),
                why.unwrap_or("ssh-keygen failed")
            ));
        }
    }
    let mut known = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|err| format!("cannot write {}: {err}", file.display()))?;
    writeln!(known, "{}", known_hosts_line(host, key))
        .map_err(|err| format!("cannot write {}: {err}", file.display()))
}

fn machines(count: usize) -> String {
    if count == 1 {
        "1 machine".to_string()
    } else {
        format!("{count} machines")
    }
}

/// The outcome when machines were left out: how many of all, and which of
/// them were not installed. The silent ones have their rows in the plan.
fn not_rolled_out(failed: &[&str], silent: usize, all: usize) {
    output::failure(&format!(
        "{} of {} not rolled out",
        failed.len() + silent,
        machines(all)
    ));
    if !failed.is_empty() {
        output::effect_err(&format!("Not installed: {}.", failed.join(", ")));
    }
}

pub fn run(options: &Options, hosts: &[Host]) -> Result<bool, String> {
    // Before any machine is asked: without its files a machine would be
    // erased and installed from the template alone.
    if let Some(files) = options.files.as_ref().filter(|files| !files.is_dir()) {
        return Err(format!(
            "{} is not a directory\n--files takes a directory that holds one directory of files for each machine, by name.",
            files.display()
        ));
    }
    let scratch = std::env::temp_dir().join(format!("nxf-rollout.{}", std::process::id()));
    // ssh and Nix split their options at spaces, and files in here are named in them.
    let splits = |ch: char| ch.is_whitespace() || "'\"\\".contains(ch);
    if scratch.to_string_lossy().contains(splits) {
        return Err(format!(
            "The temporary directory {} has a space, a quote or a backslash in its path\nThe options of ssh cannot name a file there. Set TMPDIR to a directory without one.",
            std::env::temp_dir().display()
        ));
    }
    // Made here, for this user alone: one that is there may be another user's,
    // with links that lead the writes of this run elsewhere.
    let made = std::fs::DirBuilder::new().mode(0o700).create(&scratch);
    made.map_err(|err| match err.kind() {
        std::io::ErrorKind::AlreadyExists => format!(
            "{} is there already\nrollout keeps the keys of a run in a directory it makes itself. Remove that one, or set TMPDIR to another directory.",
            scratch.display()
        ),
        _ => format!(
            "cannot create {}: {err}\nrollout keeps the keys of a run there. TMPDIR names the directory it is made in.",
            scratch.display()
        ),
    })?;
    let result = rollout(options, hosts, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn rollout(options: &Options, hosts: &[Host], scratch: &Path) -> Result<bool, String> {
    // The scratch files of a machine extend a stem named by its place in the list:
    // ssh splits the options that name them, and a host's name may hold a space.
    let stem = |index: usize| scratch.join(index.to_string());
    // Every machine is asked what it is, all at once.
    let probes: Vec<Probe> = std::thread::scope(|scope| {
        let asked: Vec<_> = hosts
            .iter()
            .enumerate()
            .map(|(index, host)| {
                let link = Link {
                    host: host.clone(),
                    trust: Trust::First {
                        seen: stem(index).with_extension("seen"),
                    },
                };
                scope.spawn(move || link.probe())
            })
            .collect();
        asked
            .into_iter()
            .map(|asked| {
                asked.join().unwrap_or(Probe {
                    found: Found::Unreachable("no answer".to_string()),
                    arch: String::new(),
                    user: None,
                })
            })
            .collect()
    });

    output::context(&format!(
        "{} · {}",
        options.file.display(),
        machines(hosts.len())
    ));
    output::line("");
    let width = hosts
        .iter()
        .map(|host| host.name.chars().count() + 2)
        .max()
        .unwrap_or(0)
        .max(output::LABEL);
    let mut to_install = Vec::new();
    let mut failed = Vec::new();
    let mut silent = 0;
    for (index, (host, probe)) in hosts.iter().zip(&probes).enumerate() {
        let name = &host.name;
        let (what, erased) = match &probe.found {
            Found::Managed => {
                output::row(Mark::Done, &format!("{name:<width$}installed by nxf"));
                continue;
            }
            Found::Unreachable(why) => {
                silent += 1;
                output::row(Mark::Failed, &format!("{name:<width$}unreachable: {why}"));
                continue;
            }
            Found::Installer => ("NixOS installer", "its disks will be erased"),
            Found::Other(os) => (os.as_str(), "it will be erased"),
        };
        // Refused here, before anything is erased: only root can install.
        if let Some(user) = probe.user.as_deref().filter(|user| *user != "0") {
            failed.push(host.name.as_str());
            output::row(
                Mark::Failed,
                &format!("{name:<width$}{what}; not installed: the SSH user must be root, and its user id is {user}"),
            );
            continue;
        }
        to_install.push((index, host, probe));
        output::row(Mark::Attention, &format!("{name:<width$}{what}; {erased}"));
    }
    if to_install.is_empty() {
        if !failed.is_empty() {
            not_rolled_out(&failed, silent, hosts.len());
            return Ok(false);
        }
        if silent > 0 {
            output::failure(&format!(
                "{silent} of {} gave no answer",
                machines(hosts.len())
            ));
            return Ok(false);
        }
        output::success(&format!("{} installed by nxf", machines(hosts.len())));
        return Ok(true);
    }

    let names: Vec<&str> = to_install
        .iter()
        .map(|(_, host, _)| host.name.as_str())
        .collect();
    if !options.yes && !std::io::stdin().is_terminal() {
        output::failure(&format!(
            "{} would be erased; nothing was changed",
            machines(to_install.len())
        ));
        output::effect_err(&format!(
            "Add --yes to erase and install {}.",
            names.join(", ")
        ));
        return Ok(false);
    }
    // Before the question: nobody confirms an installation that cannot start.
    let package = package()?;
    let (source, name) = template(&options.template)?;
    if !options.yes {
        output::line("");
        output::effect(&format!(
            "Erase the disks of {} and install NixOS? Type yes:",
            names.join(", ")
        ));
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|err| err.to_string())?;
        if answer.trim() != "yes" {
            output::failure("Nothing was changed");
            return Ok(false);
        }
    }

    let mut installed = Vec::new();
    let mut unwritten = Vec::new();
    for (index, host, probe) in &to_install {
        match install(
            host,
            probe,
            options,
            &package,
            &source,
            &name,
            &stem(*index),
        ) {
            Ok(done) => {
                // The machine is installed whatever becomes of the file.
                let written = options
                    .known_hosts
                    .as_ref()
                    .map_or(Ok(()), |file| remember(file, host, &done.key));
                if let Err(error) = written {
                    output::row(
                        Mark::Failed,
                        &format!("Its host key was not written: {error}"),
                    );
                    unwritten.push(host.name.as_str());
                }
                installed.push((*host, done));
            }
            Err(error) => {
                let mut lines = error.lines();
                output::row(Mark::Failed, lines.next().unwrap_or("Failed"));
                for line in lines {
                    output::detail(line);
                }
                failed.push(host.name.as_str());
            }
        }
    }

    if !failed.is_empty() || silent > 0 {
        not_rolled_out(&failed, silent, hosts.len());
    } else if !unwritten.is_empty() {
        output::failure(&format!(
            "{} installed, but not every host key was written",
            machines(installed.len())
        ));
    } else {
        output::success(&format!("{} installed", machines(installed.len())));
    }
    if let (Some(file), false) = (&options.known_hosts, unwritten.is_empty()) {
        output::effect_err(&format!(
            "Not written to {}: {}.",
            file.display(),
            unwritten.join(", ")
        ));
    }
    for (host, done) in &installed {
        let revision = done
            .revision
            .as_deref()
            .map(|revision| revision.chars().take(12).collect::<String>())
            .unwrap_or_default();
        output::fact(&format!("{:<w$}", host.name, w = width - 2), &revision);
        output::note(&known_hosts_line(host, &done.key));
    }
    if !unwritten.is_empty() {
        output::note("Add the muted lines of those machines to known_hosts yourself.");
    } else if !installed.is_empty() && options.known_hosts.is_none() {
        output::note("Add the muted lines to known_hosts, or give --known-hosts <file>.");
    }
    Ok(failed.is_empty() && silent == 0 && unwritten.is_empty())
}
