//! Nix internal-json activity events. Numeric types come from Nix's
//! src/libutil/include/nix/util/logging.hh; unknown events are ignored.

use serde_json::Value;

use crate::text::{strip_ansi, strip_controls};

const BUILDS: i64 = 104;
// BUILD and PROGRESS are both 105: they come from different enums of Nix, the
// type of an activity and the type of a result, so each is read for its kind.
const BUILD: i64 = 105;
const BUILD_LOG_LINE: i64 = 101;
const POST_BUILD_LOG_LINE: i64 = 107;
const PROGRESS: i64 = 105;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub done: i64,
    pub expected: i64,
    pub closed: bool,
}

/// Maps keep insertion order: the newest active build is the last one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub builds: Vec<(String, Build)>,
    pub active: Vec<(String, String)>,
    pub logs: Vec<(String, String)>,
    pub current: Option<String>,
    pub inputs: Vec<String>,
    pub input: Option<String>,
}

fn upsert<T>(map: &mut Vec<(String, T)>, key: &str, value: T) {
    match map.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = value,
        None => map.push((key.to_string(), value)),
    }
}

fn get<'a, T>(map: &'a [(String, T)], key: &str) -> Option<&'a T> {
    map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// The event on a line of tool output, if it is one.
pub fn event(line: &str) -> Option<Value> {
    let line = strip_ansi(line);
    let json = line.trim().strip_prefix("@nix ")?;
    serde_json::from_str::<Value>(json)
        .ok()
        .filter(Value::is_object)
}

pub fn initial(inputs: &[String]) -> State {
    State {
        inputs: inputs.to_vec(),
        ..State::default()
    }
}

fn computed_input(message: &str) -> Option<&str> {
    let name = message
        .strip_prefix("computing input '")?
        .strip_suffix('\'')?;
    (!name.contains('\'')).then_some(name)
}

pub fn advance(state: &State, event: Option<&Value>) -> State {
    let Some(event) = event else {
        return state.clone();
    };
    let mut next = state.clone();
    let action = event.get("action").and_then(Value::as_str).unwrap_or("");
    let kind = event.get("type").and_then(Value::as_i64);

    // Nix emits this message before fetching or resolving an input. Only
    // requested root inputs are shown; pinned dependencies stay quiet.
    if action == "msg" {
        if let Some(message) = event.get("msg").and_then(Value::as_str) {
            if let Some(name) = computed_input(&strip_ansi(message)) {
                let name = name.split('/').next().unwrap_or(name);
                if state.inputs.is_empty() || state.inputs.iter().any(|input| input == name) {
                    next.input = Some(name.to_string());
                    return next;
                }
            }
        }
    }
    let id = match event.get("id") {
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        _ => return next,
    };
    let fields = match event.get("fields") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(fields)) => fields.as_slice(),
        Some(_) => return next,
    };

    if action == "start" && kind == Some(BUILDS) {
        upsert(
            &mut next.builds,
            &id,
            Build {
                done: 0,
                expected: 0,
                closed: false,
            },
        );
    } else if action == "result" && kind == Some(PROGRESS) && get(&state.builds, &id).is_some() {
        let (Some(done), Some(expected)) = (
            fields.first().and_then(Value::as_i64),
            fields.get(1).and_then(Value::as_i64),
        ) else {
            return next;
        };
        if done < 0 || expected < done {
            return next;
        }
        upsert(
            &mut next.builds,
            &id,
            Build {
                done,
                expected,
                closed: false,
            },
        );
    } else if action == "start" && kind == Some(BUILD) {
        let Some(drv) = fields
            .first()
            .and_then(Value::as_str)
            .filter(|drv| drv.ends_with(".drv"))
        else {
            return next;
        };
        upsert(&mut next.active, &id, derivation_name(drv));
        next.current = Some(id);
    } else if action == "result"
        && matches!(kind, Some(BUILD_LOG_LINE) | Some(POST_BUILD_LOG_LINE))
        && get(&state.active, &id).is_some()
    {
        let Some(text) = fields.first().and_then(Value::as_str) else {
            return next;
        };
        let last = text
            .replace('\r', "\n")
            .lines()
            .rfind(|line| !line.is_empty())
            .map(|line| strip_controls(&strip_ansi(line)).trim().to_string())
            .unwrap_or_default();
        if last.is_empty() {
            return next;
        }
        upsert(&mut next.logs, &id, last);
        next.current = Some(id);
    } else if action == "stop" {
        next.active.retain(|(key, _)| *key != id);
        next.logs.retain(|(key, _)| *key != id);
        if next.current.as_deref() == Some(id.as_str()) {
            next.current = next.active.last().map(|(key, _)| key.clone());
        }
        if let Some(build) = next
            .builds
            .iter_mut()
            .find(|(key, _)| *key == id)
            .map(|(_, build)| build)
        {
            build.closed = true;
        }
    }
    next
}

fn derivation_name(drv: &str) -> String {
    let base = drv.rsplit('/').next().unwrap_or(drv);
    let base = base.strip_suffix(".drv").unwrap_or(base);
    let hashed = base.len() > 33
        && base.as_bytes()[32] == b'-'
        && base[..32]
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if hashed { &base[33..] } else { base }.to_string()
}

/// The lines describing the state. With `logs`, the current build carries its
/// latest builder line; without, up to two builds are named.
pub fn snapshot(state: &State, logs: bool) -> Vec<String> {
    let done: i64 = state.builds.iter().map(|(_, build)| build.done).sum();
    let expected: i64 = state
        .builds
        .iter()
        .map(|(_, build)| {
            if build.closed {
                build.done
            } else {
                build.expected
            }
        })
        .sum();
    let mut active: Vec<&str> = Vec::new();
    for (_, name) in &state.active {
        if !active.contains(&name.as_str()) {
            active.push(name);
        }
    }
    let mut lines = Vec::new();
    if let Some(input) = &state.input {
        if expected == 0 && active.is_empty() {
            lines.push(format!("Checking input {input}"));
        }
    }
    if expected > 0 {
        lines.push(if state.builds.iter().all(|(_, build)| build.closed) {
            let noun = if done == 1 {
                "derivation"
            } else {
                "derivations"
            };
            format!("{done} {noun} built")
        } else {
            format!("{done} of {expected} derivations built")
        });
    }
    if !active.is_empty() {
        let current = state.current.as_deref().unwrap_or("");
        let (names, listed) = if logs {
            let name = get(&state.active, current)
                .map(String::as_str)
                .unwrap_or(active[active.len() - 1]);
            (name.to_string(), 1)
        } else {
            (
                active
                    .iter()
                    .take(2)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", "),
                2,
            )
        };
        let more = if active.len() > listed {
            format!(" and {} more", active.len() - listed)
        } else {
            String::new()
        };
        let suffix = match get(&state.logs, current) {
            Some(line) if logs && !line.is_empty() => format!(" • {line}"),
            _ => String::new(),
        };
        lines.push(format!("Building {names}{more}{suffix}"));
    }
    lines
}

/// What a line of failed tool output says: errors and builder logs are
/// decoded, the internal-json envelope is never exposed.
pub fn diagnostic(line: &str) -> String {
    let Some(event) = event(line) else {
        return if line.trim().starts_with("@nix ") {
            String::new()
        } else {
            line.to_string()
        };
    };
    let action = event.get("action").and_then(Value::as_str).unwrap_or("");
    let kind = event.get("type").and_then(Value::as_i64);
    if action == "msg" && event.get("level").and_then(Value::as_i64).unwrap_or(0) <= 1 {
        strip_ansi(event.get("msg").and_then(Value::as_str).unwrap_or(""))
    } else if action == "result" && matches!(kind, Some(BUILD_LOG_LINE) | Some(POST_BUILD_LOG_LINE))
    {
        let field = event.get("fields").and_then(|fields| fields.get(0));
        match field {
            Some(Value::String(text)) => strip_ansi(text),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    } else {
        String::new()
    }
}
