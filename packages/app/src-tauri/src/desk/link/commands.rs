//! The page's commands for the link between computers. Desktop window only: nothing on the bridge, the MCP tools or
//! the phone routes mentions them (`guard_tests`), so no agent can start sharing, pause it, or move a computer.
//! Every one says `FEATURE_OFF` unless Labs `desk_enabled` is on, and returns the new [`LinkView`].
//!
//! `desk_link_view` -> `{seq, devices:[{node,name,state,side,mouse_here,controls_me,input_on,clipboard_on}], mouse_on,
//! controlled_by, paused, sharing, tray, admin_note, error, clipboard_note, supported}`; the same value arrives as the
//! event `desk://link` whenever it changes. `desk_link_set_side(node, side)` (`left|right|top|bottom`),
//! `desk_link_pause(paused)`, `desk_link_panic()`.
//!
//! The switches themselves ("Mouse and keyboard", "Clipboard") are `trust_set_permission`; this module only reads them.

use super::LinkView;
use crate::pairing_commands::{link_ready, node_of};

#[tauri::command]
pub async fn desk_link_view(app: tauri::AppHandle) -> Result<LinkView, String> {
    Ok(link_ready(&app)?.view())
}

#[tauri::command]
pub async fn desk_link_set_side(app: tauri::AppHandle, node: String, side: String) -> Result<LinkView, String> {
    let link = link_ready(&app)?;
    link.set_side(&node_of(&node)?, &side)?;
    Ok(link.view())
}

#[tauri::command]
pub async fn desk_link_pause(app: tauri::AppHandle, paused: bool) -> Result<LinkView, String> {
    let link = link_ready(&app)?;
    link.pause(paused);
    Ok(link.view())
}

#[tauri::command]
pub async fn desk_link_panic(app: tauri::AppHandle) -> Result<LinkView, String> {
    let link = link_ready(&app)?;
    link.panic();
    Ok(link.view())
}
