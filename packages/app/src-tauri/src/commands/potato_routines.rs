//! Tauri commands of Potato Bot's routines (phase P9a). Desktop only: a routine is made and changed by the person
//! at this computer, never through the bridge, the phone or MCP. The list, create, update, pause, resume, delete and
//! "run now" call `potato::routines`; the 30 s tick lives in `potato::live`.
//!
//! Errors are one string, `CODE: plain sentence`, like the other Potato Bot commands.

use crate::potato::live;
use crate::potato::routines::{self, RoutineInput, RoutineView};
use crate::potato::service::SendOutcome;
use crate::potato::store::StoreError;

fn text(error: StoreError) -> String {
    error.to_string()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn root() -> Result<std::path::PathBuf, String> {
    live::service().map(|s| s.root().to_path_buf()).ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())
}

/// The routines of one bot, or of every bot when `bot_id` is missing.
#[tauri::command]
pub async fn potato_routines_list(bot_id: Option<String>) -> Result<Vec<RoutineView>, String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || routines::list(&root, bot_id.as_deref()).map_err(text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn potato_routine_create(bot_id: String, input: RoutineInput) -> Result<RoutineView, String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || routines::create(&root, &bot_id, &input, now_ms()).map_err(text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn potato_routine_update(routine_id: String, input: RoutineInput) -> Result<RoutineView, String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || routines::update(&root, &routine_id, &input, now_ms()).map_err(text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn potato_routine_pause(routine_id: String, paused: bool) -> Result<RoutineView, String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || routines::set_paused(&root, &routine_id, paused, now_ms()).map_err(text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn potato_routine_delete(routine_id: String) -> Result<(), String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || routines::delete(&root, &routine_id).map_err(text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// "Run now": the routine's prompt goes to its bot at once. The schedule does not move.
#[tauri::command]
pub async fn potato_routine_run_now(routine_id: String) -> Result<SendOutcome, String> {
    let service = live::service().ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || routines::run_now(&service, &routine_id, now_ms()).map_err(text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}
