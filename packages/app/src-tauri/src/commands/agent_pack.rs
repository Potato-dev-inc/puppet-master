//! Tauri commands for "Add Puppet Master to your agents" (skills + MCP entry per detected agent).
//! Thin wrappers over `crate::agent_pack::service`.

use serde_json::Value;

#[tauri::command]
pub fn agent_pack_preview() -> Result<Value, String> {
    crate::agent_pack::service::call_live("preview")
}

#[tauri::command]
pub fn agent_pack_apply() -> Result<Value, String> {
    crate::agent_pack::service::call_live("apply")
}

#[tauri::command]
pub fn agent_pack_undo() -> Result<Value, String> {
    crate::agent_pack::service::call_live("undo")
}
