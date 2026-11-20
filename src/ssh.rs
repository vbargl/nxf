//! Reaching a machine over ssh: the program, its options, how it checks the
//! host key of the machine, and how a word is quoted for its shell.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::env;
use crate::hosts::Host;

/// How ssh checks the host key of a machine.
#[derive(Clone)]
pub enum Trust {
    /// As ssh does by itself, with the known_hosts files of the user and the system.
    Keys,
    /// Before and during the installation: a key the user knows must match,
    /// and a key nobody knows yet is accepted into a file of this run only.
    First { seen: PathBuf },
    /// After the installation: only the key the installation made.
    Made { known: PathBuf },
}

/// A machine, and how ssh trusts its host key.
#[derive(Clone)]
pub struct Link {
    pub host: Host,
    pub trust: Trust,
}

/// The known_hosts file of the user, which every link trusts before the installation.
pub fn user_known_hosts() -> Option<PathBuf> {
    Some(env::var(env::HOME)?.join(".ssh/known_hosts"))
}

/// The keys of a known_hosts file as ssh reads them, as bytes; none when the
/// file is not there. One that is there and cannot be read is an error.
pub fn keys_of(file: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(file) {
        Ok(keys) => Ok(Some(keys)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("cannot read {}: {err}", file.display())),
    }
}

/// The name a known_hosts file gives a machine: its address, and its port
/// when that is not the usual one.
pub fn known_name(host: &Host) -> String {
    match host.port {
        Some(port) if port != 22 => format!("[{}]:{port}", host.address),
        _ => host.address.clone(),
    }
}

impl Link {
    /// The known_hosts files whose keys the link trusts.
    pub fn trusted(&self) -> Vec<PathBuf> {
        match &self.trust {
            Trust::Keys => Vec::new(),
            Trust::First { seen } => std::iter::once(seen.clone())
                .chain(user_known_hosts())
                .collect(),
            Trust::Made { known } => vec![known.clone()],
        }
    }

    pub fn options(&self) -> Vec<String> {
        self.options_with(&self.trusted())
    }

    /// The options of ssh, with the given known_hosts files for those of the link.
    pub fn options_with(&self, known_hosts: &[PathBuf]) -> Vec<String> {
        let known_hosts: Vec<String> = known_hosts
            .iter()
            .map(|file| file.display().to_string())
            .collect();
        let known_hosts = format!("UserKnownHostsFile={}", known_hosts.join(" "));
        let connect = env::seconds(env::SSH_CONNECT_TIMEOUT, 10).as_secs();
        let mut options = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            format!("ConnectTimeout={connect}"),
        ];
        match &self.trust {
            Trust::Keys => {}
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

    /// The command that runs `remote` on the machine.
    pub fn command(&self, remote: &str) -> Command {
        let mut ssh = Command::new(env::path(env::SSH, "ssh"));
        ssh.args(self.options())
            .arg(self.host.destination())
            .arg(remote);
        ssh
    }

    /// Runs a remote command with the given standard input and returns
    /// whether it succeeded, and what it printed.
    pub fn capture(&self, remote: &str, input: &str) -> (bool, String, String) {
        let mut ssh = self.command(remote);
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
}

/// Quotes a word for the remote login shell, which is not always POSIX: a
/// plain word stays bare, single quotes hold anything without a single quote,
/// and double quotes hold a single quote when nothing in the word is special
/// inside them. These three forms mean the same to sh, fish and Nushell.
pub fn quoted(word: &str) -> String {
    let bare = !word.is_empty()
        && word
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_./:@=+,".contains(ch));
    if bare {
        word.to_string()
    } else if !word.contains('\'') {
        format!("'{word}'")
    } else if !word.contains(['"', '$', '`', '\\', '!']) {
        format!("\"{word}\"")
    } else {
        // Only a POSIX shell reads this form.
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}
