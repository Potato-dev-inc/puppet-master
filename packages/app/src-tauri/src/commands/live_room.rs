//! Live Room commands (SPEC-n32 3.8, 4 #2): preview what viewers would see, start, stop, pause and resume a room, read
//! its status, copy its link, and read or save its settings. Desktop only: these are Tauri commands, not bridge
//! routes and not MCP tools, so an agent, the companion or a paired phone cannot start a room.
//!
//! Anything that shows or changes what is shared (preview, start, link, settings) is refused unless the calling
//! webview is an app window (`guard`): a web page the app shows, like a pane preview, has another label. Stop, pause,
//! resume and status only reduce exposure or read two numbers, so any app window may call them. The link is returned
//! to the caller and never logged.

use super::session_chat::SessionChatState;
use super::AppState;
use crate::live_room::config::{self, LiveRoomConfig};
use crate::live_room::publisher::{LiveStatus, Publisher, StopReason};
use crate::live_room::service::{self, Env, LiveRoomState, StartRequest, StartedRoom, WatchSource, NO_RELAY};
use crate::live_room::source::ClientSink;
use crate::live_room::view::{LiveItem, LivePane};
use crate::live_room::HOLDER;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// The windows that may start a room, see its preview and read its link.
const APP_WINDOWS: [&str; 2] = ["cockpit", "main"];
/// The windows that may read and save the settings: the Settings window hosts the Live Room block.
const SETTINGS_WINDOWS: [&str; 3] = ["cockpit", "main", "settings"];
/// A preview reads the panes under its own holder name, so letting go of it never ends a running room's reader.
const PREVIEW_HOLDER: &str = "live-room-preview";

/// What the Go live sheet shows: the exact first snapshot viewers would get.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub panes: Vec<LivePane>,
    pub items: Vec<LiveItem>,
    pub hidden: u32,
}

fn guard(webview: &tauri::Webview, allowed: &[&str]) -> Result<(), String> {
    service::check_window(webview.label(), allowed)
}

fn data_dir() -> PathBuf {
    crate::app_paths::app_data_dir()
}

/// The checked pane list, from the registry.
fn checked_panes(state: &AppState, pane_ids: &[String]) -> Result<Vec<String>, String> {
    service::check_panes(pane_ids, |id| state.registry.lock().get(id).map(|pane| pane.info.agent_type.clone()))
}

/// The first snapshot viewers would get for `pane_ids`: panes with their lines, and the cleaned chat rows. Reads the
/// panes the way a room does, then lets go of them.
#[tauri::command]
pub async fn live_room_preview(
    app: AppHandle,
    webview: tauri::Webview,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    pane_ids: Vec<String>,
) -> Result<PreviewView, String> {
    guard(&webview, &APP_WINDOWS)?;
    let panes = checked_panes(&state, &pane_ids)?;
    let source = WatchSource::new(chat.watchers(&app, &state.registry), state.registry.clone(), PREVIEW_HOLDER, panes.clone());
    let ctx = (*crate::redact::known::context()).clone();
    // The first look at a pane reads its session file: off the async threads.
    tauri::async_runtime::spawn_blocking(move || {
        let mut source = source;
        let (panes, items) = Publisher::<WatchSource, ClientSink>::preview(&mut source, &panes, &ctx);
        PreviewView { panes, items, hidden: 0 }
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))
}

/// Starts the room. The relay is the Live Room setting, else the team relay. Returns when the first snapshot is on the
/// relay; the window then asks for the link (`live_room_copy_link`).
#[tauri::command]
pub async fn live_room_start(
    app: AppHandle,
    webview: tauri::Webview,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    room: State<'_, LiveRoomState>,
    req: StartRequest,
) -> Result<StartedRoom, String> {
    guard(&webview, &APP_WINDOWS)?;
    let panes = checked_panes(&state, &req.pane_ids)?;
    let dir = data_dir();
    let relay = relay_for(&dir).ok_or_else(|| NO_RELAY.to_string())?;
    let source = WatchSource::new(chat.watchers(&app, &state.registry), state.registry.clone(), HOLDER, panes.clone());
    let env = Env { data_dir: dir, relay, emit: emitter(&app), ctx: (*crate::redact::known::context()).clone() };
    let room = room.inner().clone();
    tauri::async_runtime::spawn_blocking(move || room.start(&env, &req, panes, source))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// Ends the room now. Any app window may call it.
#[tauri::command]
pub async fn live_room_stop(room: State<'_, LiveRoomState>) -> Result<(), String> {
    let room = room.inner().clone();
    tauri::async_runtime::spawn_blocking(move || room.stop_all(StopReason::Owner)).await.map_err(|e| format!("INTERNAL: {e}"))
}

/// Pauses the room: what the panes show from now on is thrown away until Resume.
#[tauri::command]
pub async fn live_room_pause(room: State<'_, LiveRoomState>) -> Result<(), String> {
    room.pause()
}

#[tauri::command]
pub async fn live_room_resume(room: State<'_, LiveRoomState>) -> Result<(), String> {
    room.resume()
}

/// The room as the bar shows it (`live: false` when there is none).
#[tauri::command]
pub async fn live_room_status(room: State<'_, LiveRoomState>) -> Result<LiveStatus, String> {
    Ok(room.status())
}

/// The full link of the running room. The window copies it; nothing here logs it.
#[tauri::command]
pub async fn live_room_copy_link(webview: tauri::Webview, room: State<'_, LiveRoomState>) -> Result<String, String> {
    guard(&webview, &APP_WINDOWS)?;
    room.link()
}

#[tauri::command]
pub async fn live_room_config_get(webview: tauri::Webview) -> Result<LiveRoomConfig, String> {
    guard(&webview, &SETTINGS_WINDOWS)?;
    Ok(config::load(&data_dir()))
}

/// Saves the settings. A relay address the rules refuse (for example `http://` to another computer) is an error
/// and nothing is written.
#[tauri::command]
pub async fn live_room_config_set(webview: tauri::Webview, cfg: LiveRoomConfig) -> Result<(), String> {
    guard(&webview, &SETTINGS_WINDOWS)?;
    config::save(&data_dir(), &cfg)
}

/// The relay a room uses: the Live Room setting, else the team's relay.
fn relay_for(dir: &std::path::Path) -> Option<String> {
    let team = crate::team::config::load_checked(&crate::team::config::path_in(dir)).map(|team| team.relay).unwrap_or_default();
    config::effective_relay(&config::load(dir), &team)
}

/// Tells every window the room changed (`live://state`), and the tray when the room starts or ends.
fn emitter(app: &AppHandle) -> service::Emit {
    let app = app.clone();
    let was_live = Arc::new(std::sync::atomic::AtomicBool::new(false));
    Arc::new(move |status: &LiveStatus| {
        let _ = app.emit(service::STATE_EVENT, status);
        if was_live.swap(status.live, std::sync::atomic::Ordering::SeqCst) != status.live {
            // Not in unit tests: the tray's windowing code would load in the test exe (as in `attention`).
            #[cfg(not(test))]
            crate::tray::request_refresh();
        }
    })
}
