//! The page's commands for the link between computers. Desktop window only: nothing on the bridge, the MCP tools or
//! the phone routes mentions them (`guard_tests`), so no agent can start sharing, pause it, or move a computer.
//! Every one says `FEATURE_OFF` unless Labs `desk_enabled` is on, and returns the new [`LinkView`].
//!
//! `desk_link_view` -> `{seq, devices:[{node,name,state,side,mouse_here,controls_me,input_on,clipboard_on}], mouse_on,
//! controlled_by, paused, sharing, tray, admin_note, error, clipboard_note, supported}`; the same value arrives as the
//! event `desk://link` whenever it changes. `desk_link_set_side(node, side)` (`left|right|top|bottom`),
//! `desk_link_pause(paused)`, `desk_link_panic()`.
//!
//! The layout: `desk_link_layout()` -> `{computers:[{node,name,me,online,known,x,y,width,height,screens:[{id,name,x,y,
//! width,height,primary}],rev}], touch_gap}` (every computer's screens in one space; this computer's box starts at
//! 0, 0), also pushed as the event `desk://layout`. `desk_link_set_place(node, x, y)` puts a computer's box at (x, y)
//! in that space and returns the new layout; the other computer is told and agrees. `desk_link_set_side` is a snapped
//! placement (touching on that side).
//!
//! The switches themselves ("Mouse and keyboard", "Clipboard") are `trust_set_permission`; this module only reads them.

use super::{LayoutView, LinkView};
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

#[tauri::command]
pub async fn desk_link_layout(app: tauri::AppHandle) -> Result<LayoutView, String> {
    Ok(link_ready(&app)?.layout())
}

#[tauri::command]
pub async fn desk_link_set_place(app: tauri::AppHandle, node: String, x: i32, y: i32) -> Result<LayoutView, String> {
    let link = link_ready(&app)?;
    link.set_position(&node_of(&node)?, x, y)?;
    Ok(link.layout())
}
