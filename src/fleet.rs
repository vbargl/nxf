//! Commands against the machines of a hosts file. nxf runs itself on each
//! machine over ssh and reports one row for each; it knows a list of SSH
//! destinations and nothing about what the machines form together.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::hosts::Host;
use crate::output::{self, Mark, Style};
use crate::text::plain;
use crate::{hold, status};

/// What a machine answered. No code means that ssh did not end in time.
pub struct Reply {
    pub host: Host,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Reply {
    fn last(text: &str) -> Option<String> {
        plain(text)
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_string)
    }

    /// Why the machine gave no usable answer, if it did not.
    fn error(&self) -> Option<String> {
        let said = || Self::last(&self.stderr).or_else(|| Self::last(&self.stdout));
        match self.code {
            Some(0) => None,
            // ssh itself exits with 255 when it cannot connect.
            Some(255) => Some(format!(
                "unreachable: {}",
                said().unwrap_or_else(|| "ssh failed".to_string())
            )),
            Some(code) => Some(said().unwrap_or_else(|| format!("nxf exited with status {code}"))),
            None => Some("unreachable: no answer in time".to_string()),
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

fn seconds(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn ask(host: &Host, args: &[String]) -> Reply {
    let program = std::env::var_os("NXF_SSH")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ssh".into());
    let remote = std::iter::once("nxf".to_string())
        .chain(args.iter().map(|arg| quoted(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut ssh = Command::new(program);
    ssh.args(["-o", "BatchMode=yes"]).args([
        "-o",
        &format!("ConnectTimeout={}", seconds("NXF_SSH_CONNECT_TIMEOUT", 10)),
    ]);
    if let Some(port) = host.port {
        ssh.args(["-p", &port.to_string()]);
    }
    ssh.arg(host.destination())
        .arg(remote)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let reply = |code, stdout, stderr| Reply {
        host: host.clone(),
        code,
        stdout,
        stderr,
    };
    let mut child = match ssh.spawn() {
        Ok(child) => child,
        Err(err) => return reply(Some(255), String::new(), format!("cannot run ssh: {err}")),
    };
    // Each pipe is read on its own thread, so that a full pipe cannot stall the machine.
    let collect = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let stdout = collect(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = collect(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let deadline = Instant::now() + Duration::from_secs(seconds("NXF_SSH_TIMEOUT", 120));
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status.code().unwrap_or(255)),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    reply(
        code,
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
    )
}

/// Runs nxf with the arguments on every host at once; replies come back in
/// the order of the hosts.
pub fn run(hosts: &[Host], args: &[String]) -> Vec<Reply> {
    std::thread::scope(|scope| {
        let asked: Vec<_> = hosts
            .iter()
            .map(|host| scope.spawn(move || ask(host, args)))
            .collect();
        asked
            .into_iter()
            .zip(hosts)
            .map(|(asked, host)| {
                asked.join().unwrap_or_else(|_| Reply {
                    host: host.clone(),
                    code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            })
            .collect()
    })
}

fn machines(count: usize) -> String {
    if count == 1 {
        "1 machine".to_string()
    } else {
        format!("{count} machines")
    }
}

fn header(file: &Path, hosts: &[Host]) -> usize {
    output::context(&format!("{} · {}", file.display(), machines(hosts.len())));
    output::line("");
    hosts
        .iter()
        .map(|host| host.name.chars().count() + 2)
        .max()
        .unwrap_or(0)
        .max(output::LABEL)
}

/// What needs attention on a machine, from its `status --json`.
fn attention(status: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    if status["rebootRequired"] == true {
        notes.push("restart needed".to_string());
    }
    if let Some(reason) = status["hold"]["reason"].as_str() {
        notes.push(match status["hold"]["until"].as_u64() {
            Some(until) => format!("held: {reason} (until {})", hold::shown(until)),
            None => format!("held: {reason}"),
        });
    }
    if !status["bad"].is_null() {
        notes.push("failed its health check".to_string());
    }
    let failed = status["lastUpdate"]["results"]
        .as_array()
        .is_some_and(|results| results.iter().any(|result| result["outcome"] == "failed"));
    if failed {
        notes.push("last update failed".to_string());
    }
    if status["changes"]
        .as_array()
        .is_some_and(|changes| !changes.is_empty())
    {
        notes.push("uncommitted changes".to_string());
    } else if status["matches"] == false {
        notes.push("checkout is not what runs".to_string());
    }
    notes
}

/// The state of every machine: revision, last update, and what needs attention.
pub fn status(file: &Path, hosts: &[Host], as_json: bool) -> bool {
    let replies = run(hosts, &["status".to_string(), "--json".to_string()]);
    let answers: Vec<(&Reply, Result<Value, String>)> = replies
        .iter()
        .map(|reply| {
            let answer = match reply.error() {
                Some(error) => Err(error),
                None => serde_json::from_str::<Value>(&reply.stdout)
                    .map_err(|err| format!("unreadable answer: {err}")),
            };
            (reply, answer)
        })
        .collect();
    let silent = answers.iter().filter(|(_, answer)| answer.is_err()).count();
    if as_json {
        let all: serde_json::Map<String, Value> = answers
            .iter()
            .map(|(reply, answer)| {
                let value = match answer {
                    Ok(status) => status.clone(),
                    Err(error) => json!({"error": error}),
                };
                (reply.host.name.clone(), value)
            })
            .collect();
        output::line(&serde_json::to_string_pretty(&all).unwrap_or_default());
        return silent == 0;
    }

    let width = header(file, hosts);
    let mut attentive = 0;
    for (reply, answer) in &answers {
        let name = &reply.host.name;
        match answer {
            Err(error) => output::row(Mark::Failed, &format!("{name:<width$}{error}")),
            Ok(status) => {
                let revision: String = status["revision"]
                    .as_str()
                    .map_or("unknown".to_string(), |revision| {
                        revision.chars().take(12).collect()
                    });
                let updated = match status["lastUpdate"]["time"].as_u64() {
                    Some(time) => format!("updated {}", status::age(time)),
                    None => "no automatic update yet".to_string(),
                };
                let notes = attention(status);
                let mark = if notes.is_empty() {
                    Mark::Done
                } else {
                    attentive += 1;
                    Mark::Attention
                };
                let mut text = format!("{name:<width$}{revision:<12}  {}", output::dim(&updated));
                if !notes.is_empty() {
                    text.push_str(&format!(
                        "  {}",
                        output::styled(&notes.join(", "), Style::Yellow)
                    ));
                }
                output::row(mark, &text);
            }
        }
    }
    let total = hosts.len();
    if silent > 0 {
        output::failure(&format!("{silent} of {} gave no status", machines(total)));
        if attentive > 0 {
            output::effect_err(&format!("{} need attention.", machines(attentive)));
        }
    } else if attentive > 0 {
        output::caution(&format!(
            "{attentive} of {} need attention",
            machines(total)
        ));
    } else {
        output::success(&format!("{} up to date", machines(total)));
    }
    silent == 0
}

/// Runs one command on every machine and reports what each said last.
/// `done` names the outcome when all succeed, such as "Held".
pub fn each(file: &Path, hosts: &[Host], args: &[String], done: &str, failed: &str) -> bool {
    let replies = run(hosts, args);
    let width = header(file, hosts);
    let mut failures = 0;
    for reply in &replies {
        let name = &reply.host.name;
        match reply.error() {
            Some(error) => {
                failures += 1;
                output::row(Mark::Failed, &format!("{name:<width$}{error}"));
            }
            None => {
                let said = Reply::last(&reply.stdout).unwrap_or_default();
                output::row(Mark::Done, &format!("{name:<width$}{said}"));
            }
        }
    }
    let total = hosts.len();
    if failures > 0 {
        output::failure(&format!("{failed} on {failures} of {}", machines(total)));
    } else {
        output::success(&format!("{done} on {}", machines(total)));
    }
    failures == 0
}
