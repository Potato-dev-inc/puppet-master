//! The toolbelt list: `<app data>/toolbelt.json`.
//!
//! Secrets. This build has no OS keyring helper, so values of secret-looking env vars and headers
//! (names containing key, token, secret, password, auth, ...) are stored in this file as typed, in
//! the user's own app data folder, and are never sent back to the UI: lists show `********` and an
//! edit that leaves `********` keeps the stored value. Agent config files need the real value, so
//! the sync writes it there. The UI says so.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const FILE_VERSION: u32 = 1;
pub const MASK: &str = "********";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Global,
    Project,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Project => "project",
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolServer {
    /// Slug used in the managed key `pm-toolbelt-<id>`. Empty on input means "make one".
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub transport: Transport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub scope: Scope,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileDoc {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    servers: Vec<ToolServer>,
}

pub fn default_path() -> PathBuf {
    crate::app_paths::app_data_dir().join("toolbelt.json")
}

pub fn is_secret_key(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["key", "token", "secret", "password", "passwd", "auth", "bearer", "credential", "cookie"].iter().any(|w| n.contains(w))
}

/// Same server with secret values replaced by `********` (what the UI and the bridge see).
pub fn redacted(server: &ToolServer) -> ToolServer {
    let mask = |m: &BTreeMap<String, String>| {
        m.iter()
            .map(|(k, v)| (k.clone(), if is_secret_key(k) && !v.is_empty() { MASK.to_string() } else { v.clone() }))
            .collect()
    };
    ToolServer { env: mask(&server.env), headers: mask(&server.headers), ..server.clone() }
}

pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    let out: String = out.chars().take(40).collect();
    if out.is_empty() { "server".into() } else { out }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !id.starts_with('-')
}

pub fn load_servers(path: &Path) -> Result<Vec<ToolServer>, String> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Vec::new()),
        Ok(text) => serde_json::from_str::<FileDoc>(&text)
            .map(|d| d.servers)
            .map_err(|e| format!("toolbelt.json is not readable ({e}); fix or delete it")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

fn save(path: &Path, servers: &[ToolServer]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let doc = FileDoc { version: FILE_VERSION, servers: servers.to_vec() };
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn check(server: &ToolServer) -> Result<(), String> {
    if server.name.trim().is_empty() {
        return Err("Give the server a name.".into());
    }
    match server.transport {
        Transport::Stdio => {
            if server.command.as_deref().map(str::trim).unwrap_or("").is_empty() {
                return Err("Enter the command that starts the server.".into());
            }
        }
        Transport::Http => {
            let url = server.url.as_deref().map(str::trim).unwrap_or("");
            if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() < 9 {
                return Err("Enter the server address, starting with http:// or https://.".into());
            }
        }
    }
    for k in server.env.keys().chain(server.headers.keys()) {
        if k.trim().is_empty() {
            return Err("A variable or header has no name.".into());
        }
    }
    Ok(())
}

fn clean(mut s: ToolServer) -> ToolServer {
    s.name = s.name.trim().to_string();
    s.command = s.command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    s.url = s.url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    s.args.retain(|a| !a.is_empty());
    match s.transport {
        Transport::Stdio => {
            s.url = None;
            s.headers.clear();
        }
        Transport::Http => {
            s.command = None;
            s.args.clear();
            s.env.clear();
        }
    }
    s
}

pub fn list(path: &Path) -> Result<Vec<ToolServer>, String> {
    Ok(load_servers(path)?.iter().map(redacted).collect())
}

pub fn add(path: &Path, input: ToolServer) -> Result<ToolServer, String> {
    let mut servers = load_servers(path)?;
    let mut s = clean(input);
    check(&s)?;
    if s.id.trim().is_empty() {
        let base = slug(&s.name);
        let mut id = base.clone();
        let mut n = 2;
        while servers.iter().any(|x| x.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        s.id = id;
    } else if !valid_id(&s.id) {
        return Err("The id may only use a-z, 0-9, - and _.".into());
    } else if servers.iter().any(|x| x.id == s.id) {
        return Err(format!("A server with id {} already exists.", s.id));
    }
    servers.push(s.clone());
    save(path, &servers)?;
    Ok(redacted(&s))
}

pub fn update(path: &Path, id: &str, input: ToolServer) -> Result<ToolServer, String> {
    let mut servers = load_servers(path)?;
    let pos = servers.iter().position(|s| s.id == id).ok_or_else(|| format!("No server with id {id}."))?;
    let old = servers[pos].clone();
    let mut s = clean(ToolServer { id: id.to_string(), ..input });
    // A secret left as ******** means "keep what is stored".
    for (new, old) in [(&mut s.env, &old.env), (&mut s.headers, &old.headers)] {
        for (k, v) in new.iter_mut() {
            if v == MASK {
                match old.get(k) {
                    Some(prev) => *v = prev.clone(),
                    None => return Err(format!("{k} has no stored value; enter it again.")),
                }
            }
        }
    }
    check(&s)?;
    servers[pos] = s.clone();
    save(path, &servers)?;
    Ok(redacted(&s))
}

pub fn remove(path: &Path, id: &str) -> Result<bool, String> {
    let mut servers = load_servers(path)?;
    let before = servers.len();
    servers.retain(|s| s.id != id);
    if servers.len() == before {
        return Ok(false);
    }
    save(path, &servers)?;
    Ok(true)
}
