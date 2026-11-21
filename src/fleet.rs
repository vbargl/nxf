//! Commands against the machines of a hosts file. nxf runs itself on each
//! machine over ssh and reports one row for each; it knows SSH destinations
//! and nothing more. The upgrade goes in waves: the first machine alone, then
//! a batch at a time.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::env;
use crate::hosts::{self, Host};
use crate::output::{self, Mark, Style};
use crate::results::{self, Kind, Machine, Outcome, Refusal, Saved, State};
use crate::ssh::{self, Link, Trust};
use crate::tasks::UPDATE;
use crate::text::{last_line, plain};
use crate::{process, time};

/// What a machine answered. No code means that ssh did not end within the limit.
struct Reply {
    host: Host,
    code: Option<i32>,
    limit: Duration,
    stdout: String,
    stderr: String,
}

impl Reply {
    /// Why the machine gave no usable answer, if it did not. A refusal that
    /// nxf printed as JSON is the cause, before the last line of text.
    fn error(&self) -> Option<String> {
        let said = || last_line(&plain(&self.stderr)).or_else(|| last_line(&plain(&self.stdout)));
        match self.code {
            Some(0) => None,
            // ssh itself exits with 255 when it cannot connect.
            Some(255) => Some(format!(
                "unreachable: {}",
                said().unwrap_or_else(|| "ssh failed".to_string())
            )),
            Some(code) => Some(
                Refusal::from_json(&self.stdout)
                    .map(|refusal| refusal.error)
                    .or_else(said)
                    .unwrap_or_else(|| format!("nxf exited with status {code}")),
            ),
            // The machine may have been reached: it did not finish, which is not unreachable.
            None => Some(match self.limit.as_secs() {
                1 => "timed out after 1 second".to_string(),
                seconds => format!("timed out after {seconds} seconds"),
            }),
        }
    }
}

fn ask(host: &Host, args: &[String], timeout: Duration) -> Reply {
    let remote = std::iter::once("nxf".to_string())
        .chain(args.iter().map(|arg| ssh::quoted(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let link = Link {
        host: host.clone(),
        trust: Trust::Keys,
    };
    let mut ssh = link.command(&remote);
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
fn run(hosts: &[Host], args: &[String]) -> Vec<Reply> {
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

fn header(file: &Path, hosts: &[Host]) -> usize {
    output::context(&format!(
        "{} · {}",
        file.display(),
        output::machines(hosts.len())
    ));
    output::line("");
    output::widened(hosts.iter().map(|host| host.name.as_str()))
}

/// What needs attention on a machine, from its `status --json`.
fn attention(status: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    if status["rebootRequired"] == true {
        notes.push("restart needed".to_string());
    }
    if let Some(reason) = status["hold"]["reason"].as_str() {
        notes.push(match status["hold"]["until"].as_u64() {
            Some(until) => format!("held: {reason} (until {})", time::shown(until)),
            None => format!("held: {reason}"),
        });
    }
    if !status["bad"].is_null() {
        notes.push("failed its health check".to_string());
    }
    let failed = Saved::from_json(&status["lastUpdate"])
        .is_some_and(|saved| saved.results.iter().any(Outcome::failed));
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
pub fn status(file: &Path, as_json: bool) -> Result<bool, String> {
    let hosts = &hosts::load(file)?;
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
        output::data(&serde_json::to_string_pretty(&all).unwrap_or_default());
        return Ok(silent == 0);
    }

    let width = header(file, hosts);
    let mut attentive = 0;
    for (reply, answer) in &answers {
        let name = &reply.host.name;
        match answer {
            Err(error) => output::row(Mark::Failed, &format!("{name:<width$}{error}")),
            Ok(status) => {
                let revision = status["revision"]
                    .as_str()
                    .map_or("unknown".to_string(), output::revision);
                let updated = match status["lastUpdate"]["time"].as_u64() {
                    Some(time) => format!("updated {}", time::age(time)),
                    None => "no automatic update yet".to_string(),
                };
                let notes = attention(status);
                let mark = if notes.is_empty() {
                    Mark::Done
                } else {
                    attentive += 1;
                    Mark::Attention
                };
                let mut text = format!(
                    "{name:<width$}{revision:<shown$}  {}",
                    output::dim(&updated),
                    shown = output::REVISION
                );
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
        output::failure(&format!(
            "{silent} of {} gave no status",
            output::machines(total)
        ));
        if attentive > 0 {
            output::effect_err(&format!("{} need attention.", output::machines(attentive)));
        }
    } else if attentive > 0 {
        output::caution(&format!(
            "{attentive} of {} need attention",
            output::machines(total)
        ));
    } else {
        output::success(&format!("{} up to date", output::machines(total)));
    }
    Ok(silent == 0)
}

/// Runs one command on every machine of the hosts file and reports what each
/// said last. `done` names the outcome when all succeed, and `failed` the
/// failure of some.
pub fn each(file: &Path, args: &[&str], done: &str, failed: &str) -> Result<bool, String> {
    let hosts = &hosts::load(file)?;
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let replies = run(hosts, &args);
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
        output::failure(&format!(
            "{failed} on {failures} of {}",
            output::machines(total)
        ));
    } else {
        output::success(&format!("{done} on {}", output::machines(total)));
    }
    Ok(failures == 0)
}

/// Holds automatic updates back on every machine.
pub fn hold(file: &Path, reason: &str, until: Option<u64>) -> Result<bool, String> {
    // The end is one point in time, whatever each machine's clock zone is.
    let until = until.map(|until| format!("@{until}"));
    let mut args = vec!["hold", "--reason", reason];
    if let Some(until) = &until {
        args.extend(["--until", until]);
    }
    each(file, &args, "Automatic updates are held", "Not held")
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

/// The release that writes the outcomes as nxf reads them.
const RELEASE: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

/// What the upgrade did on one machine, from the outcomes its `--json` printed.
fn upgraded(reply: &Reply) -> Result<String, String> {
    if let (None, Some(error)) = (reply.code, reply.error()) {
        // Ending ssh here does not end the task there.
        return Err(format!("{error}; the upgrade may still run on the machine"));
    }
    let list = match reply.code {
        Some(255) | None => None,
        Some(_) => serde_json::from_str::<Value>(&reply.stdout).ok(),
    };
    let outcomes: Option<Vec<Outcome>> = list
        .as_ref()
        .and_then(|list| list.as_array()?.iter().map(Outcome::from_json).collect());
    let Some(outcomes) = outcomes else {
        // A list of another shape is what an older nxf prints; its state is unknown.
        if list.is_some_and(|list| list.is_array()) {
            return Err(format!(
                "unreadable answer: nxf on the machine is older than {RELEASE}; the upgrade may have run"
            ));
        }
        // Otherwise nxf refused before any task ran, and its refusal is the cause.
        return Err(reply
            .error()
            .unwrap_or_else(|| "unreadable answer".to_string()));
    };
    if let Some(failed) = outcomes.iter().find(|outcome| outcome.failed()) {
        // The details are the task's own failure, whose mark the row repeats.
        let details = failed.details.join(" | ");
        let detail = details.trim_start_matches(['✗', ' ']);
        return Err(if detail.is_empty() {
            format!("{} failed", failed.task)
        } else {
            format!("{} failed: {detail}", failed.task)
        });
    }
    if reply.code != Some(0) {
        return Err(reply
            .error()
            .unwrap_or_else(|| "the upgrade failed".to_string()));
    }
    let updated = outcomes
        .iter()
        .any(|outcome| outcome.task == UPDATE && outcome.kind == Kind::Done);
    Ok(if updated {
        "inputs updated, switched".to_string()
    } else {
        "no input changed, switched".to_string()
    })
}

/// Runs `nxf upgrade` on the machines in waves: the first machine alone, then
/// `batch` at a time. A failure lets its wave end and starts no other. With
/// `json` the machines and the outcome are printed as JSON on standard output, and the text on standard error.
pub fn upgrade(file: &Path, hosts: &[Host], inputs: &[String], batch: usize, json: bool) -> bool {
    if json {
        output::divert();
    }
    let mut args = vec!["--json".to_string(), "upgrade".to_string()];
    args.extend(inputs.iter().cloned());
    let timeout = env::seconds(env::UPGRADE_TIMEOUT, 3 * 3600);
    let width = header(file, hosts);
    let mut failed = Vec::new();
    let mut machines = Vec::new();
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
                Ok(said) => {
                    output::row(Mark::Done, &format!("{name:<width$}{said}"));
                    machines.push(Machine::new(name, State::Upgraded));
                }
                Err(error) => {
                    failed.push(name.clone());
                    output::row(Mark::Failed, &format!("{name:<width$}{error}"));
                    machines.push(Machine {
                        error: Some(error),
                        ..Machine::new(name, State::Failed)
                    });
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
        machines.push(Machine::new(name, State::NotStarted));
    }
    let total = hosts.len();
    if failed.is_empty() {
        output::success(&format!("{} upgraded", output::machines(total)));
    } else {
        output::failure(&format!(
            "Upgrade failed on {} of {}",
            failed.len(),
            output::machines(total)
        ));
        if !untouched.is_empty() {
            output::effect_err(&format!(
                "Not started: {}. Run the rollout again once the failure is fixed.",
                untouched.join(", ")
            ));
        }
    }
    if json {
        output::data(&results::machines_json(failed.is_empty(), &machines));
    }
    failed.is_empty()
}
