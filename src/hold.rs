//! Holding automatic updates back: an explicit hold with a reason and an
//! optional end, kept in the updater's state so that it survives a reboot.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::{process, state, time};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub reason: String,
    pub since: u64,
    pub until: Option<u64>,
}

fn file() -> PathBuf {
    state::updater_dir().join("hold.json")
}

/// The hold as saved, even when it has ended.
fn saved() -> Option<Hold> {
    let value: Value = serde_json::from_str(&std::fs::read_to_string(file()).ok()?).ok()?;
    Some(Hold {
        reason: value.get("reason")?.as_str()?.to_string(),
        since: value.get("since").and_then(Value::as_u64).unwrap_or(0),
        until: value.get("until").and_then(Value::as_u64),
    })
}

/// The hold in effect now, if any.
pub fn active() -> Option<Hold> {
    saved().filter(|hold| hold.until.is_none_or(|until| time::now() < until))
}

fn denied(action: &str, err: std::io::Error) -> String {
    format!(
        "cannot {action} {}: {err}; run as a member of the nxf group or with sudo",
        file().display()
    )
}

pub fn set(reason: &str, until: Option<u64>) -> Result<Hold, String> {
    let hold = Hold {
        reason: reason.to_string(),
        since: time::now(),
        until,
    };
    let text = format!("{:#}\n", hold.to_json());
    process::write_atomic(&file(), &text).map_err(|err| denied("write", err))?;
    Ok(hold)
}

/// Removes the hold and returns whether there was one.
pub fn clear() -> Result<bool, String> {
    match std::fs::remove_file(file()) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(denied("remove", err)),
    }
}

/// Removes a hold that has ended; a failure only leaves the stale file.
pub fn forget_ended() {
    if saved().is_some() && active().is_none() {
        let _ = std::fs::remove_file(file());
    }
}

/// A duration such as `90m`, `12h`, `3d` or `2w`, in seconds.
pub fn duration(text: &str) -> Result<u64, String> {
    let invalid =
        || format!("Invalid duration '{text}'; use a number with m, h, d or w, such as 3d.");
    let unit = text.chars().next_back().ok_or_else(invalid)?;
    let count: u64 = text[..text.len() - unit.len_utf8()]
        .parse()
        .map_err(|_| invalid())?;
    let seconds = match unit {
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        'w' => 604800,
        _ => return Err(invalid()),
    };
    count
        .checked_mul(seconds)
        .filter(|total| *total > 0)
        .ok_or_else(invalid)
}

/// The end of a hold from the options of the command: a time, a duration from
/// now, or none. It lies in the future.
pub fn end(until: Option<String>, duration: Option<String>) -> Result<Option<u64>, String> {
    let end = match (until, duration) {
        (Some(until), _) => Some(time::parse(&until)?),
        (None, Some(duration)) => Some(time::now() + self::duration(&duration)?),
        (None, None) => None,
    };
    if end.is_some_and(|end| end <= time::now()) {
        return Err("The hold would end in the past.".to_string());
    }
    Ok(end)
}

impl Hold {
    /// The reason, and when the hold ends.
    pub fn describe(&self) -> String {
        match self.until {
            Some(until) => format!("{} (until {})", self.reason, time::shown(until)),
            None => format!("{} (until released)", self.reason),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({"reason": self.reason, "since": self.since, "until": self.until})
    }
}
