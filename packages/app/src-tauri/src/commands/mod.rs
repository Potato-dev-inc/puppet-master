//! Tauri command surface — thin wrappers around the PTY registry.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

mod agent_models;
mod agents;
mod app;
mod cockpit;
mod coordination;
mod mcp;
mod opencode_keys;
mod panes;
mod standby;

pub use agent_models::*;
pub use agents::*;
pub use app::*;
pub use cockpit::*;
pub use coordination::*;
pub use mcp::*;
pub use opencode_keys::*;
pub use panes::*;
pub use standby::*;

use crate::pty::agents::AgentType;
use crate::pty::{
    registry_get_project_path, registry_kill_all, registry_kill_pane, registry_read_buffer,
    registry_read_raw_buffer, registry_read_screen_replay, registry_read_snapshot, registry_resize, registry_set_project_path,
    registry_spawn_pane, registry_write_input, PaneInfo, PaneRegistry, SpawnPaneArgs,
};
use crate::settings_store;

#[derive(Default)]
pub struct AppState {
    pub registry: Arc<Mutex<PaneRegistry>>,
    pub public_settings: Mutex<Value>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(PaneRegistry::new())),
            public_settings: Mutex::new(settings_store::default_public_settings()),
        }
    }
}

fn developer_use_rust_mcp(state: &State<'_, AppState>) -> bool {
    state
        .public_settings
        .lock()
        .get("developer_use_rust_mcp")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
