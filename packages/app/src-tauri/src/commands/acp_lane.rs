//! Tauri commands for the per-agent "How to run": Terminal (today's way, the default) or Chat (ACP).
//! Only the app's own webview can call these; the bridge has no route for them, so a phone or an agent
//! cannot change how an agent runs. The choice is read when a session starts without a lane of its own: the + menu's
//! default (`useAcpLanes`) and `run_agent` without `lane` (`acp::chat::surface::wants`).

use crate::acp::lane::{effective_lane, LaneStore, RunLane};
use crate::acp::{self, config};
use serde_json::{json, Value};

fn view() -> Value {
    let store = LaneStore::in_app_data();
    let labs = acp::chat_enabled();
    let chosen = store.all();
    let agents: Vec<Value> = config::builtin_agents()
        .iter()
        .map(|cfg| {
            let lane = chosen.get(&cfg.id).copied().unwrap_or_default();
            json!({
                "id": cfg.id,
                "label": cfg.label,
                "lane": lane.as_str(),
                "effective_lane": effective_lane(lane, labs, !cfg.disabled).as_str(),
                "disabled": cfg.disabled,
                "disabled_note": cfg.disabled_note,
            })
        })
        .collect();
    json!({ "labs_enabled": labs, "agents": agents })
}

/// `{labs_enabled, agents: [{id, label, lane, effective_lane}]}` for every agent that has an ACP definition.
/// `lane` is the saved choice, `effective_lane` what a new session would use (Chat needs the Labs switch).
#[tauri::command]
pub fn agent_lane_get() -> Value {
    view()
}

/// Saves one agent's choice (`terminal` | `acp`). Returns the new view.
#[tauri::command]
pub fn agent_lane_set(id: String, lane: String) -> Result<Value, String> {
    let lane = RunLane::parse(&lane).ok_or_else(|| format!("INVALID_ARGUMENT: unknown lane {lane}"))?;
    if lane == RunLane::Acp {
        match config::find_builtin(&id) {
            None => return Err(format!("INVALID_ARGUMENT: {id} has no chat (ACP) definition")),
            Some(cfg) if cfg.disabled => return Err(format!("INVALID_ARGUMENT: chat for {id} is off: {}", cfg.disabled_note)),
            Some(_) => {}
        }
    }
    LaneStore::in_app_data().set(&id, lane)?;
    Ok(view())
}
