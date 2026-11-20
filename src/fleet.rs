//! Commands against the machines of a hosts file. nxf runs itself on each
//! machine over ssh and reports one row for each; it knows a list of SSH
//! destinations and nothing about what the machines form together. The
//! upgrade is the one of these that changes machines, and it goes in waves.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::env;
use crate::hosts::Host;
use crate::output::{self, Mark, Style};
use crate::text::{last_line, plain};
use crate::{hold, process, status};

/// What a machine answered. No code means that ssh did not end within the limit.
pub struct Reply {
    pub host: Host,
    pub code: Option<i32>,
    pub limit: Duration,
    pub stdout: String,
    pub stderr: String,
}

impl Reply {
    /// Why the machine gave no usable answer, if it did not.
    fn error(&self) -> Option<String> {
        let said = || last_line(&plain(&self.stderr)).or_else(|| last_line(&plain(&self.stdout)));
        match self.code {
            Some(0) => None,
            // ssh itself exits with 255 when it cannot connect.
            Some(255) => Some(format!(
                "unreachable: {}",
                said().unwrap_or_else(|| "ssh failed".to_string())
            )),
            Some(code) => Some(said().unwrap_or_else(|| format!("nxf exited with status {code}"))),
            // The machine may have been reached: it did not finish, which is not unreachable.
            None => Some(match self.limit.as_secs() {
                1 => "timed out after 1 second".to_string(),
                seconds => format!("timed out after {seconds} seconds"),
            }),
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

fn ask(host: &Host, args: &[String], timeout: Duration) -> Reply {
    let program = env::path(env::SSH, "ssh");
    let remote = std::iter::once("nxf".to_string())
        .chain(args.iter().map(|arg| quoted(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut ssh = Command::new(program);
    ssh.args(["-o", "BatchMode=yes"]).args([
        "-o",
        &format!(
            "ConnectTimeout={}",
            env::seconds(env::SSH_CONNECT_TIMEOUT, 10).as_secs()
        ),
    ]);
    if let Some(port) = host.port {
        ssh.args(["-p", &port.to_string()]);
    }
    ssh.arg(host.destination()).arg(remote);
    let reply = |code, stdout, stderr| Reply {
        host: host.clone(),
        code,
        limit: timeout,
        stdout,
        stderr,
    };
    match process::run_within(&mut ssh, Instant::now() + timeout, false) {
        // A signal ends ssh without a code of its own.
        Ok(done) => reply(
            done.status.map(|status| status.code().unwrap_or(255)),
            done.stdout,
            done.stderr,
        ),
        Err(err) => reply(Some(255), String::new(), err),
    }
}

/// Runs nxf with the arguments on every host at once; replies come back in
/// the order of the hosts.
pub fn run(hosts: &[Host], args: &[String]) -> Vec<Reply> {
    let timeout = env::seconds(env::SSH_TIMEOUT, 120);
    run_for(hosts, args, timeout)
}

fn run_for(hosts: &[Host], args: &[String], timeout: Duration) -> Vec<Reply> {
    std::thread::scope(|scope| {
        let asked: Vec<_> = hosts
            .iter()
            .map(|host| scope.spawn(move || ask(host, args, timeout)))
            .collect();
        asked
            .into_iter()
            .zip(hosts)
            .map(|(asked, host)| {
                asked.join().unwrap_or_else(|_| Reply {
                    host: host.clone(),
                    code: None,
                    limit: timeout,
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
                let said = last_line(&plain(&reply.stdout)).unwrap_or_default();
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

/// The waves of an upgrade, as ranges of the hosts: the first machine alone,
/// then `batch` machines at a time.
pub fn waves(count: usize, batch: usize) -> Vec<std::ops::Range<usize>> {
    let batch = batch.max(1);
    let mut waves = Vec::new();
    if count > 0 {
        waves.push(0..1);
    }
    let mut start = 1;
    while start < count {
        let end = (start + batch).min(count);
        waves.push(start..end);
        start = end;
    }
    waves
}

/// What the upgrade did on one machine, from the outcomes its `--json` printed.
fn upgraded(reply: &Reply) -> Result<String, String> {
    if let (None, Some(error)) = (reply.code, reply.error()) {
        // Ending ssh here does not end the task there.
        return Err(format!("{error}; the upgrade may still run on the machine"));
    }
    let outcomes = match reply.code {
        Some(255) | None => None,
        Some(_) => match serde_json::from_str::<Value>(&reply.stdout) {
            Ok(Value::Array(outcomes)) => Some(outcomes),
            _ => None,
        },
    };
    let Some(outcomes) = outcomes else {
        // nxf refused before any task ran: its outcome line is the cause,
        // and the lines beneath only help to resolve it.
        let refused = reply.code.filter(|code| *code != 255).and_then(|_| {
            plain(&reply.stderr)
                .lines()
                .find_map(|line| line.trim().strip_prefix('✗').map(str::trim))
                .filter(|cause| !cause.is_empty())
                .map(str::to_string)
        });
        return Err(refused
            .or_else(|| reply.error())
            .unwrap_or_else(|| "unreadable answer".to_string()));
    };
    if let Some(failed) = outcomes
        .iter()
        .find(|outcome| outcome["outcome"] == "failed")
    {
        let task = failed["task"].as_str().unwrap_or("upgrade");
        // The detail is the task's own failure, whose mark the row repeats.
        let detail = failed["detail"]
            .as_str()
            .map(|detail| detail.trim_start_matches(['✗', ' ']))
            .filter(|detail| !detail.is_empty());
        return Err(match detail {
            Some(detail) => format!("{task} failed: {detail}"),
            None => format!("{task} failed"),
        });
    }
    if reply.code != Some(0) {
        return Err(reply
            .error()
            .unwrap_or_else(|| "the upgrade failed".to_string()));
    }
    let updated = outcomes
        .iter()
        .any(|outcome| outcome["task"] == "update" && outcome["outcome"] == "done");
    Ok(if updated {
        "inputs updated, switched".to_string()
    } else {
        "no input changed, switched".to_string()
    })
}

/// Runs `nxf upgrade` on the machines in waves: the first machine alone, then
/// `batch` at a time. A failure lets its wave end and starts no other.
pub fn upgrade(file: &Path, hosts: &[Host], inputs: &[String], batch: usize) -> bool {
    let mut args = vec!["--json".to_string(), "upgrade".to_string()];
    args.extend(inputs.iter().cloned());
    let timeout = env::seconds(env::UPGRADE_TIMEOUT, 3 * 3600);
    let width = header(file, hosts);
    let mut failed = Vec::new();
    let mut reached = hosts.len();
    for wave in waves(hosts.len(), batch) {
        if !failed.is_empty() {
            reached = wave.start;
            break;
        }
        let names: Vec<&str> = hosts[wave.clone()]
            .iter()
            .map(|host| host.name.as_str())
            .collect();
        let pending = output::pending(&format!("Upgrading {}", names.join(", ")));
        let replies = run_for(&hosts[wave], &args, timeout);
        output::settle(pending);
        for reply in &replies {
            let name = &reply.host.name;
            match upgraded(reply) {
                Ok(said) => output::row(Mark::Done, &format!("{name:<width$}{said}")),
                Err(error) => {
                    failed.push(name.clone());
                    output::row(Mark::Failed, &format!("{name:<width$}{error}"));
                }
            }
        }
    }
    let untouched: Vec<&str> = hosts[reached..]
        .iter()
        .map(|host| host.name.as_str())
        .collect();
    for name in &untouched {
        output::row(
            Mark::Quiet,
            &format!("{name:<width$}{}", output::dim("not started")),
        );
    }
    let total = hosts.len();
    if failed.is_empty() {
        output::success(&format!("{} upgraded", machines(total)));
        return true;
    }
    output::failure(&format!(
        "Upgrade failed on {} of {}",
        failed.len(),
        machines(total)
    ));
    if !untouched.is_empty() {
        output::effect_err(&format!(
            "Not started: {}. Run the rollout again once the failure is fixed.",
            untouched.join(", ")
        ));
    }
    false
}
