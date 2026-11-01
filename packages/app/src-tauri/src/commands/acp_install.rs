//! Tauri commands for the chat add-on (A5): is an agent's adapter installed, install it (progress comes as events),
//! remove it. Only the app's own webview calls these; the bridge has no route for them, so a phone or an agent cannot
//! install anything. Behind the Labs switch like the rest of the chat lane. All the work is in `acp::install`.

use super::*;
use crate::acp::config::{self, AgentConfig, RealHost};
use crate::acp::install::{self, installable, Addons, InstallError, RealRunner};
use serde_json::{json, Value};

/// Event: `{agent, status}` whenever an install makes progress, finishes or fails.
pub const EVENT: &str = "acp-addon-changed";

fn cfg_for(id: &str) -> Result<AgentConfig, String> {
    match config::find_builtin(id) {
        Some(cfg) if installable(&cfg) => Ok(cfg),
        Some(_) => Err(format!("INVALID_ARGUMENT: {id} has no chat add-on to install")),
        None => Err(format!("INVALID_ARGUMENT: {id} has no chat (ACP) definition")),
    }
}

fn labs_gate() -> Result<(), String> {
    if crate::acp::chat_enabled() {
        Ok(())
    } else {
        Err("ACP_LABS_OFF: Chat sessions are off. Turn on Chat (ACP) in Labs first.".to_string())
    }
}

fn status_of(addons: &Addons, cfg: &AgentConfig) -> Value {
    serde_json::to_value(addons.status(cfg, &RealHost)).unwrap_or(Value::Null)
}

fn view(addons: &Addons) -> Value {
    let list: Vec<Value> = config::builtin_agents().iter().filter(|cfg| installable(cfg)).map(|cfg| status_of(addons, cfg)).collect();
    json!({ "labs_enabled": crate::acp::chat_enabled(), "addons": list })
}

/// `{labs_enabled, addons: [{agent, label, state, version, pinned_version, detail, size_mb, lines, signin_hint}]}` for
/// every agent that has a chat add-on. `state`: not_installed | installing | installed | failed.
#[tauri::command]
pub fn acp_addon_status() -> Value {
    view(install::addons())
}

/// Starts installing one agent's add-on in the background; returns the status (`installing`). Progress and the end
/// come as `acp-addon-changed` events.
#[tauri::command]
pub async fn acp_addon_install(app: AppHandle, agent_id: String) -> Result<Value, String> {
    labs_gate()?;
    let cfg = cfg_for(&agent_id)?;
    let addons = install::addons();
    addons.begin(&cfg.id).map_err(|e| format!("ACP_ADDON_BUSY: {e}"))?;
    let started = status_of(addons, &cfg);
    let (app2, cfg2) = (app.clone(), cfg.clone());
    std::thread::Builder::new()
        .name(format!("acp-addon-{}", cfg.id))
        .spawn(move || {
            let changed = || {
                let _ = app2.emit(EVENT, json!({ "agent": cfg2.id, "status": status_of(install::addons(), &cfg2) }));
            };
            let result = install::addons().install_blocking(&cfg2, &RealHost, &RealRunner, &changed);
            match result {
                Ok(version) => tracing::info!(agent = %cfg2.id, "acp add-on installed: {version}"),
                Err(error) => tracing::warn!(agent = %cfg2.id, "acp add-on install failed: {error}"),
            }
        })
        .map_err(|e| format!("ACP_ADDON_FAILED: {e}"))?;
    Ok(started)
}

/// Moves the add-on to `.removed` (nothing is deleted). Returns the new status.
#[tauri::command]
pub async fn acp_addon_remove(app: AppHandle, agent_id: String) -> Result<Value, String> {
    let cfg = cfg_for(&agent_id)?;
    let addons = install::addons();
    addons.remove(&cfg, &RealHost).map_err(|e| match e {
        InstallError::Busy => format!("ACP_ADDON_BUSY: {e}"),
        other => format!("ACP_ADDON_FAILED: {other}"),
    })?;
    let status = status_of(addons, &cfg);
    let _ = app.emit(EVENT, json!({ "agent": cfg.id, "status": status }));
    Ok(status)
}
