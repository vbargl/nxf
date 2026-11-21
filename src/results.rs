//! What nxf says to programs with `--json`, and reads back from other nxf
//! programs: the outcome of a task. Each shape has one writer and one reader.

use serde_json::{json, Value};

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
