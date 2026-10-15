//! The toolbelt list: `<app data>/toolbelt.json`.
//!
//! Secrets. Values of secret-looking env vars and headers (names containing key, token, secret,
//! password, auth, ...) live in the OS credential store (`crate::secrets`), not in this file. The
//! file keeps only a reference, `pm-keyring://toolbelt/<vault>/<server id>/<env|headers>/<NAME>`.
//! A value goes to the store first and is read back; only if it matches does the file get the
//! reference, so when the store is unusable the value stays in the file (as before) instead of
//! being lost. `load_servers` resolves references, so callers (the sync) see real values. Values
//! are never sent back to the UI: lists show `********` and an edit that leaves `********` keeps
//! the stored value. Agent config files need the real value, so the sync writes it there. The UI
//! says so. `migrate` moves plain values an older version wrote (idempotent, run at startup).

use crate::secrets::{self, SecretBackend};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const FILE_VERSION: u32 = 1;
pub const MASK: &str = "********";
/// Start of a value in `toolbelt.json` that points into the credential store.
pub const REF_PREFIX: &str = "pm-keyring://";

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
    /// Random id naming this file's entries in the credential store. Made with the first secret.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    vault: String,
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

fn read_doc(path: &Path) -> Result<FileDoc, String> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(FileDoc::default()),
        Ok(text) => serde_json::from_str::<FileDoc>(&text)
            .map_err(|e| format!("toolbelt.json is not readable ({e}); fix or delete it")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileDoc::default()),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

fn secret_name(vault: &str, id: &str, section: &str, key: &str) -> String {
    format!("toolbelt/{vault}/{id}/{section}/{key}")
}

fn ref_of(name: &str) -> String {
    format!("{REF_PREFIX}{name}")
}

fn name_of_ref(value: &str) -> Option<&str> {
    value.strip_prefix(REF_PREFIX)
}

fn is_plain_secret(key: &str, value: &str) -> bool {
    is_secret_key(key) && !value.is_empty() && value != MASK && name_of_ref(value).is_none()
}

/// The servers with every credential-store reference replaced by its real value.
pub fn load_servers(path: &Path) -> Result<Vec<ToolServer>, String> {
    load_servers_with(secrets::backend(), path)
}

pub fn load_servers_with(b: &dyn SecretBackend, path: &Path) -> Result<Vec<ToolServer>, String> {
    let mut servers = read_doc(path)?.servers;
    for server in &mut servers {
        for map in [&mut server.env, &mut server.headers] {
            for (key, value) in map.iter_mut() {
                let Some(name) = name_of_ref(value) else { continue };
                match b.get(name) {
                    Ok(Some(real)) => *value = real,
                    Ok(None) => {
                        tracing::warn!(server = %server.id, %key, "toolbelt secret is missing from the credential store");
                        value.clear();
                    }
                    Err(e) => {
                        return Err(format!("could not read a stored secret from {} ({e})", b.label()));
                    }
                }
            }
        }
    }
    Ok(servers)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    /// Values now held in the credential store (the file keeps a reference).
    pub moved: usize,
    /// Secret values left in the file because the store was unusable or did not read back.
    pub kept_in_file: usize,
}

/// Move every plain secret value of `servers` into the store and replace it by a reference. A
/// value is only replaced after reading it back. Existing references are left alone.
fn externalize(b: &dyn SecretBackend, vault: &str, servers: &mut [ToolServer]) -> MigrationReport {
    let mut report = MigrationReport::default();
    let usable = b.available();
    for server in servers.iter_mut() {
        let id = server.id.clone();
        for (section, map) in [("env", &mut server.env), ("headers", &mut server.headers)] {
            for (key, value) in map.iter_mut() {
                if !is_plain_secret(key, value) {
                    continue;
                }
                let name = secret_name(vault, &id, section, key);
                let stored = usable
                    && b.set(&name, value).is_ok()
                    && b.get(&name).ok().flatten().as_deref() == Some(value.as_str());
                if stored {
                    *value = ref_of(&name);
                    report.moved += 1;
                } else {
                    report.kept_in_file += 1;
                }
            }
        }
    }
    report
}

fn refs_in(servers: &[ToolServer]) -> BTreeSet<String> {
    servers
        .iter()
        .flat_map(|s| s.env.values().chain(s.headers.values()))
        .filter_map(|v| name_of_ref(v).map(str::to_string))
        .collect()
}

fn has_plain_secret(servers: &[ToolServer]) -> bool {
    servers
        .iter()
        .any(|s| s.env.iter().chain(s.headers.iter()).any(|(k, v)| is_plain_secret(k, v)))
}

fn new_vault() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn write_doc(path: &Path, doc: &FileDoc) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())? + "\n";
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Write `servers` (real values) with secrets externalised, then drop store entries that nothing
/// in the file points at any more.
pub fn save_with(b: &dyn SecretBackend, path: &Path, servers: &[ToolServer]) -> Result<MigrationReport, String> {
    let old = read_doc(path).unwrap_or_default();
    let mut vault = old.vault.clone();
    if vault.is_empty() && has_plain_secret(servers) {
        vault = new_vault();
    }
    let mut stored = servers.to_vec();
    let report = externalize(b, &vault, &mut stored);
    write_doc(path, &FileDoc { version: FILE_VERSION, vault, servers: stored.clone() })?;
    let keep = refs_in(&stored);
    for name in refs_in(&old.servers).difference(&keep) {
        let _ = b.delete(name);
    }
    Ok(report)
}

fn save(path: &Path, servers: &[ToolServer]) -> Result<(), String> {
    save_with(secrets::backend(), path, servers).map(|_| ())
}

/// One-time, idempotent: move plain secret values an older version left in `toolbelt.json` into
/// the credential store. Each is blanked in the file only after it reads back identically.
pub fn migrate(b: &dyn SecretBackend, path: &Path) -> Result<MigrationReport, String> {
    let doc = read_doc(path)?;
    let mut servers = doc.servers;
    if !has_plain_secret(&servers) {
        return Ok(MigrationReport::default());
    }
    let vault = if doc.vault.is_empty() { new_vault() } else { doc.vault };
    let report = externalize(b, &vault, &mut servers);
    if report.moved > 0 {
        write_doc(path, &FileDoc { version: FILE_VERSION, vault, servers })?;
    }
    Ok(report)
}

/// Startup hook (see `secrets::migrate_on_startup`).
pub fn migrate_on_startup() {
    match migrate(secrets::backend(), &default_path()) {
        Ok(r) if r.moved > 0 => tracing::info!(moved = r.moved, "toolbelt secrets moved to the OS credential store"),
        Ok(r) if r.kept_in_file > 0 => tracing::warn!("OS credential store unavailable: toolbelt secrets stay in toolbelt.json"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "toolbelt secret migration failed"),
    }
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

/// Every stored credential value (env vars and headers whose names look secret), for `crate::redact`.
/// Empty when the file or the credential store cannot be read.
pub fn secret_values() -> Vec<String> {
    let Ok(servers) = load_servers(&default_path()) else { return Vec::new() };
    servers
        .iter()
        .flat_map(|s| s.env.iter().chain(s.headers.iter()))
        .filter(|(k, v)| is_secret_key(k) && v.len() >= 4 && v.as_str() != MASK && name_of_ref(v).is_none())
        .map(|(_, v)| v.clone())
        .collect()
}
