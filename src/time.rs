//! Times: the clock, a time as the person writes and reads it, and how long
//! ago a time was.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::process;

/// Seconds since the epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A point in time as `date --date` understands it, in the local time zone.
pub fn parse(text: &str) -> Result<u64, String> {
    let output = process::capture(Command::new("date").args(["--date", text, "+%s"]))?;
    let parsed = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>();
    match parsed {
        Ok(time) if output.status.success() => Ok(time),
        _ => Err(format!(
            "Invalid time '{text}'; use a date such as 2030-01-31 or '2030-01-31 18:00'."
        )),
    }
}

/// A time for people, in the local time zone.
pub fn shown(time: u64) -> String {
    process::capture(Command::new("date").args(["--date", &format!("@{time}"), "+%Y-%m-%d %H:%M"]))
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| format!("@{time}"))
}

/// How long ago a time was, for people.
pub fn age(time: u64) -> String {
    let seconds = now().saturating_sub(time);
    let (count, unit) = match seconds {
        0..=59 => return "just now".to_string(),
        60..=3599 => (seconds / 60, "minute"),
        3600..=86399 => (seconds / 3600, "hour"),
        _ => (seconds / 86400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}
