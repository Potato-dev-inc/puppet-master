//! Tauri command surface — thin wrappers around the PTY registry.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

mod agent_install;
mod agent_models;
mod agents;
mod ambient;
mod askbar;
mod app;
mod checkpoints;
mod turn_summary;
mod cockpit;
mod coordination;
mod loop_guard;
mod mcp;
mod one_brain;
mod opencode_keys;
mod pane_close;
mod panes;
mod secrets;
mod accounts;
mod questions;
mod standby;
mod worktrees;
mod second_opinion;
mod tunnel;
mod toolbelt;

pub use agent_install::*;
pub use agent_models::*;
pub use agents::*;
pub use ambient::*;
pub use askbar::*;
pub use app::*;
pub use checkpoints::*;
pub use turn_summary::*;
pub use cockpit::*;
pub use coordination::*;
pub use loop_guard::*;
pub use mcp::*;
pub use one_brain::*;
pub use opencode_keys::*;
pub use pane_close::*;
pub use panes::*;
pub use secrets::*;
pub use accounts::*;
pub use questions::*;
pub use standby::*;
pub use worktrees::*;
pub use second_opinion::*;
pub use tunnel::*;
pub use toolbelt::*;

use crate::pty::agents::AgentType;
use crate::pty::{
    registry_get_project_path, registry_kill_all, registry_kill_pane, registry_read_buffer,
    registry_read_raw_buffer, registry_read_screen_replay_at, registry_read_snapshot, registry_resize, registry_set_project_path,
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
