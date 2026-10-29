//! Tauri command surface — thin wrappers around the PTY registry.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

mod agent_install;
mod agent_store;
mod agent_wizard;
mod agent_models;
mod agents;
mod ambient;
mod askbar;
mod side_question;
mod canary;
mod app;
mod checkpoints;
mod companion;
mod tamper;
mod missions;
mod turn_summary;
mod budgets;
mod cockpit;
mod jobs;
mod coordination;
mod diagnostics;
mod loop_guard;
mod attention;
mod mcp;
mod one_brain;
mod opencode_keys;
mod pane_close;
mod pane_outbox;
mod pane_git;
mod panes;
mod secrets;
mod accounts;
mod agent_authority;
mod agent_run_mode;
mod questions;
mod standby;
mod worktrees;
mod first_run;
mod crews;
mod plain_mode;
mod plans;
mod scores;
mod second_opinion;
mod session_chat;
mod chat_index_settings;
mod chat_import;
mod chat_export;
mod clip;
mod keep_going;
mod tunnel;
mod toolbelt;
mod agent_pack;
mod point;
mod channels;
mod live_room;
pub(crate) mod night;
mod issue_intake;
mod collision;

pub use agent_install::*;
pub use agent_store::*;
pub use agent_wizard::*;
pub use agent_models::*;
pub use attention::*;
pub use agents::*;
pub use ambient::*;
pub use askbar::*;
pub use side_question::*;
pub use canary::*;
pub use app::*;
pub use checkpoints::*;
pub use companion::*;
pub use tamper::*;
pub use missions::*;
pub use turn_summary::*;
pub use budgets::*;
pub use cockpit::*;
pub use jobs::*;
pub use coordination::*;
pub use diagnostics::*;
pub use loop_guard::*;
pub use mcp::*;
pub use one_brain::*;
pub use opencode_keys::*;
pub use pane_close::*;
pub use pane_git::*;
pub use panes::*;
pub use secrets::*;
pub use accounts::*;
pub use agent_authority::*;
pub use agent_run_mode::*;
pub use questions::*;
pub use standby::*;
pub use worktrees::*;
pub use first_run::*;
pub use crews::*;
pub use plain_mode::*;
pub use plans::*;
pub use scores::*;
pub use second_opinion::*;
pub use pane_outbox::*;
pub use session_chat::*;
pub use chat_index_settings::*;
pub use chat_import::*;
pub use chat_export::*;
pub use clip::*;
pub use keep_going::*;
pub use tunnel::*;
pub use toolbelt::*;
pub use agent_pack::*;
pub use point::*;
pub use channels::*;
pub use live_room::*;
pub use night::*;
pub use issue_intake::*;
pub use collision::*;

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
