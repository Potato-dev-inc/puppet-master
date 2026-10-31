//! Tauri commands of Potato Bot (spec sections 2, 3 and 5). Desktop only: the page, the settings and the
//! brain's start and stop are never reachable through the bridge, the phone or MCP. The brain itself
//! reaches the app only through `/potato/brain/*` (`potato::auth`).
//!
//! Errors are one string, `CODE: plain sentence`, like the companion commands. The key is set through
//! `secret_set("deepseek_api_key", ..)` (masked reads only); nothing here returns it. `potato://changed`
//! (payload `{ botId }`) tells the page to read again.

use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, State};

use crate::potato::helpers::HelperStartOutcome;
use crate::potato::live;
use crate::potato::service::{PotatoState, SendOutcome, Service};
use crate::potato::store::{Bot, Message, Settings, StoreError};
use super::AppState;

fn service() -> Result<Arc<Service>, String> {
    live::service().ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())
}

fn text(error: StoreError) -> String {
    error.to_string()
}

/// Runs one service call off the UI thread (a send may start the brain; a stop waits for it to leave).
async fn run<T: Send + 'static>(call: impl FnOnce(&Service) -> Result<T, StoreError> + Send + 'static) -> Result<T, String> {
    let service = service()?;
    tauri::async_runtime::spawn_blocking(move || call(&service).map_err(text)).await.map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn potato_state() -> Result<PotatoState, String> {
    run(|s| s.state()).await
}

#[tauri::command]
pub async fn potato_messages(bot_id: String, limit: Option<usize>) -> Result<Vec<Message>, String> {
    run(move |s| s.messages(&bot_id, limit.unwrap_or(200))).await
}

/// One message from the person. Saved first; then handed to the bot's brain, which starts if it must.
#[tauri::command]
pub async fn potato_send(bot_id: String, text: String) -> Result<SendOutcome, String> {
    run(move |s| s.send(&bot_id, &text)).await
}

/// Turns a companion into a Potato Bot (the persona is the Settings default).
#[tauri::command]
pub async fn potato_bot_add(bot_id: String) -> Result<Bot, String> {
    run(move |s| s.add_bot(&bot_id)).await
}

#[tauri::command]
pub async fn potato_set_persona(bot_id: String, pack: String) -> Result<Bot, String> {
    run(move |s| s.set_persona(&bot_id, &pack)).await
}

/// "Start again": forgets the crashes and starts the brain.
#[tauri::command]
pub async fn potato_bot_start(bot_id: String) -> Result<(), String> {
    run(move |s| s.start_bot(&bot_id)).await
}

/// The page's Stop button: asks the brain to leave. The next message starts it again.
#[tauri::command]
pub async fn potato_bot_stop(bot_id: String) -> Result<(), String> {
    run(move |s| {
        s.stop_bot(&bot_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn potato_mark_read(bot_id: String) -> Result<(), String> {
    run(move |s| s.mark_read(&bot_id)).await
}

/// "Forget talk memory": the brain's folder is moved aside, never deleted.
#[tauri::command]
pub async fn potato_forget_talk_memory(bot_id: String) -> Result<(), String> {
    run(move |s| s.forget_talk_memory(&bot_id)).await
}

#[tauri::command]
pub async fn potato_settings_get() -> Result<Settings, String> {
    run(|s| s.settings()).await
}

#[tauri::command]
pub async fn potato_settings_set(settings: Settings) -> Result<Settings, String> {
    run(move |s| s.set_settings(settings)).await
}

/// The cleaned lines of the brain's log, for the "why did it stop" view. Never message text or a key.
#[tauri::command]
pub async fn potato_log(bot_id: String) -> Result<Vec<String>, String> {
    Ok(service()?.log_lines(&bot_id))
}

/// "Change folder": the names of the recent projects a helper may work in. Names only, never paths.
#[tauri::command]
pub async fn potato_helper_folders(bot_id: String) -> Result<Vec<String>, String> {
    run(move |s| s.helper_folders(&bot_id)).await
}

/// Start on a helper card. `folder` is a project NAME (from "Change folder"); without it the card's own folder
/// is used. Nothing runs without this call, and the call needs an open card.
#[tauri::command]
pub async fn potato_helper_start(bot_id: String, request_id: String, folder: Option<String>) -> Result<HelperStartOutcome, String> {
    run(move |s| s.helper_start(&bot_id, &request_id, folder.as_deref())).await
}

/// Not now on a helper card.
#[tauri::command]
pub async fn potato_helper_decline(bot_id: String, request_id: String) -> Result<(), String> {
    run(move |s| s.helper_decline(&bot_id, &request_id)).await
}

/// Settings > Potato Bot > Check key: one tiny turn. Answers "Works." or one plain sentence. The key is read
/// inside the app and goes to the model maker only.
#[tauri::command]
pub async fn potato_check_key() -> Result<String, String> {
    run(|s| s.check_key()).await
}

/// "Install Node.js" on the card: the agent installer's `node` tool, in a visible window the person watches.
#[tauri::command(async)]
pub fn potato_install_node(state: State<'_, AppState>, app: AppHandle) -> Result<Value, String> {
    crate::agent_install::install_tool(&app, &state.registry, "node")
}
