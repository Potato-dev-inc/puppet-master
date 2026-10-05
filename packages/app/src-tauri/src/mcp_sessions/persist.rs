//! Persistence of the session state so owners can reclaim after a bridge restart.

use super::state::{CoordinatorRecord, PaneControl, PaneKind, Session, State};
use crate::tool_registry::McpMode;
use serde_json::{json, Value};

/// Coordinator identity, attach tokens and run/pane leases survive a bridge restart so the
/// recorded owner can reclaim with attach_agents(attach_token). Tokens are never logged.
pub(super) fn persist_path() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        Some(crate::app_paths::app_data_dir().join("mcp_sessions.json"))
    }
}

pub(super) fn kind_str(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Agent => "agent",
        PaneKind::Shell => "shell",
        PaneKind::User => "user",
    }
}

pub(super) fn control_to_json(control: &PaneControl) -> Value {
    json!({
        "owner": control.owner,
        "coordinator_id": control.coordinator_id,
        "controller": control.controller,
        "released": control.released,
        "taken_over": control.explicitly_taken_over,
        "kind": kind_str(control.kind),
    })
}

pub(super) fn control_from_json(value: &Value) -> Option<PaneControl> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(PaneControl {
        owner: text("owner")?,
        coordinator_id: text("coordinator_id")?,
        controller: text("controller"),
        released: value.get("released").and_then(Value::as_bool).unwrap_or(false),
        explicitly_taken_over: value.get("taken_over").and_then(Value::as_bool).unwrap_or(false),
        kind: match value.get("kind").and_then(Value::as_str) {
            Some("shell") => PaneKind::Shell,
            Some("user") => PaneKind::User,
            _ => PaneKind::Agent,
        },
    })
}

pub(super) fn snapshot_state(state: &State) -> Value {
    json!({
        "version": 1,
        "sessions": state.sessions.iter().map(|(id, s)| (id.clone(), json!({
            "mode": s.mode.as_str(), "coordinator_id": s.coordinator_id, "attach_token": s.attach_token,
        }))).collect::<serde_json::Map<_, _>>(),
        "coordinators": state.coordinators.iter().map(|(id, c)| (id.clone(), json!({
            "attach_token": c.attach_token, "connection_id": c.connection_id,
        }))).collect::<serde_json::Map<_, _>>(),
        "panes": state.panes.iter().map(|(id, c)| (id.clone(), control_to_json(c))).collect::<serde_json::Map<_, _>>(),
        "agents": state.agents.iter().map(|(id, c)| (id.clone(), control_to_json(c))).collect::<serde_json::Map<_, _>>(),
    })
}

/// Inserts persisted entries; existing in-memory entries win.
pub(super) fn restore_snapshot(state: &mut State, snapshot: &Value) {
    let obj = |key: &str| snapshot.get(key).and_then(Value::as_object);
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    for (id, v) in obj("coordinators").into_iter().flatten() {
        if let (Some(attach_token), Some(connection_id)) =
            (text(v, "attach_token"), text(v, "connection_id"))
        {
            state
                .coordinators
                .entry(id.clone())
                .or_insert(CoordinatorRecord { attach_token, connection_id });
        }
    }
    for (id, v) in obj("sessions").into_iter().flatten() {
        if let (Some(coordinator_id), Some(attach_token)) =
            (text(v, "coordinator_id"), text(v, "attach_token"))
        {
            let mode = text(v, "mode")
                .and_then(|m| McpMode::parse(&m))
                .unwrap_or(McpMode::Agent);
            state
                .sessions
                .entry(id.clone())
                .or_insert(Session { mode, coordinator_id, attach_token });
        }
    }
    for (id, v) in obj("panes").into_iter().flatten() {
        if let Some(control) = control_from_json(v) {
            state.panes.entry(id.clone()).or_insert(control);
        }
    }
    for (id, v) in obj("agents").into_iter().flatten() {
        if let Some(control) = control_from_json(v) {
            state.agents.entry(id.clone()).or_insert(control);
        }
    }
}
