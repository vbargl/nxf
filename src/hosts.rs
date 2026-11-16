//! The file that names a set of machines to reach over SSH. nxf reads the
//! `ssh` table of each host and nothing else, so other tools can keep their
//! own keys and sections in the same file.

use std::path::Path;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub name: String,
    pub address: String,
    pub user: String,
    pub port: Option<u16>,
}

impl Host {
    /// The destination as ssh takes it.
    pub fn destination(&self) -> String {
        format!("{}@{}", self.user, self.address)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
    Yaml,
}

impl Format {
    /// The format a file name stands for. A new format is one more arm here
    /// and one in `decode`.
    pub fn of(path: &Path) -> Result<Format, String> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("toml") => Ok(Format::Toml),
            Some("json") => Ok(Format::Json),
            Some("yaml" | "yml") => Ok(Format::Yaml),
            _ => Err(format!(
                "{} has no known format; use .toml, .json, .yaml or .yml.",
                path.display()
            )),
        }
    }
}

fn decode(text: &str, format: Format) -> Result<Value, String> {
    match format {
        Format::Toml => toml::from_str(text).map_err(|err| err.to_string()),
        Format::Json => serde_json::from_str(text).map_err(|err| err.to_string()),
        Format::Yaml => serde_yaml_ng::from_str(text).map_err(|err| err.to_string()),
    }
}

/// The hosts of a file's content, in name order. Keys and sections that nxf
/// does not know are ignored on every level.
pub fn parse(text: &str, format: Format) -> Result<Vec<Host>, String> {
    let document = decode(text, format)?;
    let hosts = document
        .get("hosts")
        .and_then(Value::as_object)
        .ok_or("it has no hosts table")?;
    let mut list = Vec::new();
    for (name, entry) in hosts {
        let ssh = entry.get("ssh");
        let address = ssh
            .and_then(|ssh| ssh.get("address"))
            .and_then(Value::as_str)
            .filter(|address| !address.is_empty())
            .ok_or(format!("host {name} has no ssh.address"))?;
        let user = match ssh.and_then(|ssh| ssh.get("user")) {
            None => "root",
            Some(user) => user
                .as_str()
                .filter(|user| !user.is_empty())
                .ok_or(format!("host {name} has an invalid ssh.user"))?,
        };
        let port = match ssh.and_then(|ssh| ssh.get("port")) {
            None => None,
            Some(port) => Some(
                port.as_u64()
                    .and_then(|port| u16::try_from(port).ok())
                    .filter(|port| *port > 0)
                    .ok_or(format!("host {name} has an invalid ssh.port"))?,
            ),
        };
        list.push(Host {
            name: name.clone(),
            address: address.to_string(),
            user: user.to_string(),
            port,
        });
    }
    if list.is_empty() {
        return Err("its hosts table is empty".to_string());
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

pub fn load(path: &Path) -> Result<Vec<Host>, String> {
    let format = Format::of(path)?;
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("Cannot read {}\n{err}", path.display()))?;
    parse(&text, format).map_err(|err| format!("Cannot use {}\n{err}", path.display()))
}
