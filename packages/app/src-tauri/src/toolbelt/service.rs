//! JSON-in / JSON-out front for the toolbelt, shared by the Tauri commands and the bridge routes.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::import;
use super::store::{self, ToolServer};
use super::sync::{self, HostSpec, SyncOptions, Unsupported};
use crate::mcp_install::declarative::HostLaunch;

/// Everything the actions depend on, so tests can point them at temp folders.
pub struct Env {
    pub store_path: PathBuf,
    pub hosts: Vec<HostSpec>,
    pub unsupported: Vec<Unsupported>,
    pub home: PathBuf,
    /// Launch spec for Puppet Master's own entry when a sync asks for it.
    pub pm_launch: Option<HostLaunch>,
}

impl Env {
    pub fn live() -> Result<Self, String> {
        let (hosts, unsupported) = sync::collect_hosts();
        Ok(Self {
            store_path: store::default_path(),
            hosts,
            unsupported,
            home: crate::project_path::home_dir().ok_or_else(|| "could not resolve the home folder".to_string())?,
            pm_launch: Some(HostLaunch::current()),
        })
    }

    fn options(&self, body: &Value) -> SyncOptions {
        let project = body.get("project").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from);
        let include_pm = body.get("include_pm").and_then(Value::as_bool).unwrap_or(false);
        SyncOptions { home: self.home.clone(), project, pm_launch: if include_pm { self.pm_launch.clone() } else { None } }
    }
}

fn server_of(v: &Value) -> Result<ToolServer, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("That server isn't valid: {e}"))
}

fn text<'a>(body: &'a Value, key: &str) -> Result<&'a str, String> {
    body.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("{key} is required"))
}

/// `list`, `add`, `update`, `remove`, `sync_preview`, `sync_apply`, `import_preview`, `import_apply`.
pub fn call(env: &Env, action: &str, body: &Value) -> Result<Value, String> {
    let path: &Path = &env.store_path;
    match action {
        "list" => {
            let (hosts, unsupported) = (&env.hosts, &env.unsupported);
            let agents: Vec<Value> = hosts
                .iter()
                .map(|h| json!({"id": h.agent_id, "label": h.label, "installed": h.installed, "supports_url": h.table.entry_http.is_some()}))
                .collect();
            Ok(json!({"servers": store::list(path)?, "agents": agents, "unsupported": unsupported}))
        }
        "add" => Ok(json!({"server": store::add(path, server_of(body.get("server").unwrap_or(body))?)?})),
        "update" => Ok(json!({"server": store::update(path, text(body, "id")?, server_of(body.get("server").ok_or("server is required")?)?)?})),
        "remove" => Ok(json!({"removed": store::remove(path, text(body, "id")?)?})),
        "sync_preview" | "sync_apply" => {
            let servers = store::load_servers(path)?;
            let report = sync::run(&servers, &env.hosts, env.unsupported.clone(), &env.options(body), action == "sync_apply");
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        "import_preview" => {
            let existing = store::load_servers(path)?;
            Ok(json!({"candidates": import::preview(&env.hosts, &env.options(body), &existing)}))
        }
        "import_apply" => {
            let refs: Vec<String> = body
                .get("references")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            if refs.is_empty() {
                return Err("Pick at least one server to import.".into());
            }
            Ok(json!({"added": import::apply(&env.hosts, &env.options(body), path, &refs)?}))
        }
        other => Err(format!("unknown toolbelt action {other}")),
    }
}

/// The live app: real agent registry, real app data folder.
pub fn call_live(action: &str, body: &Value) -> Result<Value, String> {
    call(&Env::live()?, action, body)
}
