//! What nxf says to programs with `--json`, and reads back from other nxf
//! programs: the outcome of a task, a refusal and the installation of a machine. Each shape has one writer and one reader.

use serde_json::{json, Value};

use crate::process;

/// The word of an outcome, as the README documents it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Done,
    /// An update that left the lockfile as it was.
    Unchanged,
    Failed,
    /// The automatic update left a task out.
    Skipped,
    /// An input whose update failed and whose strategy is `flag`.
    Flagged,
    /// The same for the strategy `report`.
    Reported,
}

impl Kind {
    const ALL: [Kind; 6] = [
        Kind::Done,
        Kind::Unchanged,
        Kind::Failed,
        Kind::Skipped,
        Kind::Flagged,
        Kind::Reported,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Done => "done",
            Kind::Unchanged => "unchanged",
            Kind::Failed => "failed",
            Kind::Skipped => "skipped",
            Kind::Flagged => "flagged",
            Kind::Reported => "reported",
        }
    }

    pub fn parse(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.as_str() == word)
    }
}

/// The result of one task, as `--json` and the automatic updater report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub task: String,
    pub inputs: Vec<String>,
    pub kind: Kind,
    /// What the task said about its outcome, one line each.
    pub details: Vec<String>,
}

fn strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect()
}

impl Outcome {
    pub fn new(task: &str, inputs: Vec<String>, kind: Kind, details: Vec<String>) -> Outcome {
        Outcome {
            task: task.to_string(),
            inputs,
            kind,
            details,
        }
    }

    pub fn failed(&self) -> bool {
        self.kind == Kind::Failed
    }

    /// An input the automatic update could not update, and is to report.
    pub fn reported(&self) -> bool {
        self.kind == Kind::Reported
    }

    pub fn to_json(&self) -> Value {
        json!({
            "task": self.task,
            "inputs": self.inputs,
            "outcome": self.kind.as_str(),
            "details": self.details,
        })
    }

    /// None for a result of another shape, such as one that an older nxf saved.
    pub fn from_json(value: &Value) -> Option<Outcome> {
        Some(Outcome {
            task: value.get("task")?.as_str()?.to_string(),
            inputs: strings(value.get("inputs")?)?,
            kind: Kind::parse(value.get("outcome")?.as_str()?)?,
            details: strings(value.get("details")?)?,
        })
    }
}

pub fn outcomes_json(outcomes: &[Outcome]) -> String {
    let list: Vec<Value> = outcomes.iter().map(Outcome::to_json).collect();
    serde_json::to_string_pretty(&list).unwrap_or_default()
}

/// The outcomes of an automatic update, with the time it saved them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub time: u64,
    pub results: Vec<Outcome>,
}

impl Saved {
    pub fn to_json(&self) -> Value {
        let results: Vec<Value> = self.results.iter().map(Outcome::to_json).collect();
        json!({"time": self.time, "results": results})
    }

    /// None when the time or any result is not of the shape that nxf writes.
    pub fn from_json(value: &Value) -> Option<Saved> {
        Some(Saved {
            time: value.get("time")?.as_u64()?,
            results: value
                .get("results")?
                .as_array()?
                .iter()
                .map(Outcome::from_json)
                .collect::<Option<_>>()?,
        })
    }
}

/// Why a run ended before any task, or any machine, was reached: the cause
/// as the outcome says it, and the lines beneath it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub error: String,
    pub details: Vec<String>,
}

impl Refusal {
    /// From an error as the commands carry it: the cause first, then the lines
    /// that help to resolve it.
    pub fn from_text(error: &str) -> Refusal {
        let error = process::cause_first(error);
        let mut lines = error.lines().map(str::trim).filter(|line| !line.is_empty());
        Refusal {
            error: lines.next().unwrap_or("Failed").to_string(),
            details: lines.map(str::to_string).collect(),
        }
    }

    pub fn to_json(&self) -> String {
        let refusal = json!({"error": self.error, "details": self.details});
        serde_json::to_string_pretty(&refusal).unwrap_or_default()
    }

    /// None for any other text, such as the standard output of a machine
    /// that is not nxf, or the list of outcomes.
    pub fn from_json(text: &str) -> Option<Refusal> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Refusal {
            error: value.get("error")?.as_str()?.to_string(),
            details: strings(value.get("details")?)?,
        })
    }
}

/// What `nxf install --json` prints, and rollout reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub host_key: String,
    /// The store path of the system, as the new machine sees it.
    pub system: Option<String>,
    /// The commit of the flake that was installed.
    pub revision: Option<String>,
}

impl Installation {
    pub fn to_json(&self) -> String {
        let installed = json!({
            "hostKey": self.host_key,
            "system": self.system,
            "revision": self.revision,
        });
        serde_json::to_string_pretty(&installed).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Option<Installation> {
        let value: Value = serde_json::from_str(text).ok()?;
        let text = |name: &str| value.get(name)?.as_str().map(str::to_string);
        Some(Installation {
            host_key: text("hostKey").filter(|key| !key.is_empty())?,
            system: text("system"),
            revision: text("revision"),
        })
    }
}
