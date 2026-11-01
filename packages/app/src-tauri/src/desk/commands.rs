//! The two Tauri commands of My computers. Desktop window only: nothing on the bridge, the MCP tools or the
//! phone routes calls them (`guard_tests`). Both say `FEATURE_OFF` unless Labs `desk_enabled` is on.
//!
//! `desk_snapshot` -> `Snapshot` (screens, pointer, hotkeys, status).
//! `desk_command({ cmd })` -> `Snapshot`, where `cmd` is one of
//! `{type:"refresh"}`, `{type:"hop_next"}`, `{type:"hop_prev"}`, `{type:"hop_to", screen:N}`,
//! `{type:"find_pointer"}`, `{type:"set_hotkeys", hotkeys:{...}}`, `{type:"reset_hotkeys"}`,
//! `{type:"set_hops_enabled", enabled}`, `{type:"pause", paused}`, `{type:"set_hop_mode", mode}`.
//! Every change also arrives as the event `desk://snapshot` with the new `Snapshot`.

use std::sync::Once;

use tauri::{AppHandle, Emitter};

use super::engine::{Cmd, Snapshot};

static ATTACH: Once = Once::new();

/// Send every changed snapshot to the webview, once per app run.
fn attach(app: &AppHandle) {
    let app = app.clone();
    ATTACH.call_once(move || {
        super::global().set_notifier(std::sync::Arc::new(move |snap: &Snapshot| {
            let _ = app.emit("desk://snapshot", snap);
        }));
    });
}

#[tauri::command]
pub async fn desk_snapshot(app: AppHandle) -> Result<Snapshot, String> {
    attach(&app);
    tauri::async_runtime::spawn_blocking(|| super::global().snapshot())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn desk_command(app: AppHandle, cmd: Cmd) -> Result<Snapshot, String> {
    attach(&app);
    tauri::async_runtime::spawn_blocking(move || super::global().command(cmd))
        .await
        .map_err(|e| e.to_string())?
}
