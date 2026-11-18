//! How the automatic update treats each input of the flake. The configuration
//! in the checkout says so, in `services.nxf.autoUpdate`: what is committed
//! decides, not the system that runs, so a changed strategy needs no switch.

use std::path::Path;

use serde_json::Value;

use crate::flake;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// The automatic update leaves the input alone; it is updated by hand.
    Off,
    /// A failed update is recorded, and the run goes on as if it had not been tried.
    Flag,
    /// As `Flag`, and the failure hook runs after the run.
    Report,
}

impl Strategy {
    fn parse(word: &str) -> Result<Strategy, String> {
        match word {
            "off" => Ok(Strategy::Off),
            "flag" => Ok(Strategy::Flag),
            "report" => Ok(Strategy::Report),
            other => Err(format!(
                "unknown update strategy '{other}'; it is off, flag or report"
            )),
        }
    }
}

/// The strategy of every input: its own, or the one for inputs without one.
#[derive(Debug, PartialEq, Eq)]
pub struct Strategies {
    default: Strategy,
    inputs: Vec<(String, Strategy)>,
}

impl Default for Strategies {
    fn default() -> Self {
        Strategies {
            default: Strategy::Report,
            inputs: Vec::new(),
        }
    }
}

impl Strategies {
    pub fn of(&self, input: &str) -> Strategy {
        self.inputs
            .iter()
            .find(|(name, _)| name == input)
            .map_or(self.default, |(_, strategy)| *strategy)
    }

    /// From `{"strategy": ..., "inputs": {...}}`; what is null or missing
    /// keeps its default.
    pub fn from_settings(settings: &Value) -> Result<Strategies, String> {
        let mut strategies = Strategies::default();
        if let Some(word) = settings.get("strategy").and_then(Value::as_str) {
            strategies.default = Strategy::parse(word)?;
        }
        if let Some(inputs) = settings.get("inputs").and_then(Value::as_object) {
            for (name, word) in inputs {
                let word = word.as_str().ok_or_else(|| {
                    format!("the update strategy of input '{name}' is not a word")
                })?;
                strategies
                    .inputs
                    .push((name.clone(), Strategy::parse(word)?));
            }
        }
        Ok(strategies)
    }
}

/// The strategies the configuration in the checkout asks for.
pub fn read(dir: &Path, configuration: &str) -> Result<Strategies, String> {
    let settings = flake::update_settings(dir, configuration)
        .map_err(|err| format!("Cannot read the update strategies of the configuration\n{err}"))?;
    Strategies::from_settings(&settings)
}
