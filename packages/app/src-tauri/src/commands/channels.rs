//! Tauri commands for Settings > Phone > Telegram (early). Only the app's own webview can call
//! these; the bridge has no route that turns Telegram on, pairs a chat or changes what it may do
//! (the phone can only switch it off, see `bridge/routes/channels.rs`). Every command returns the
//! new `TelegramStatus`. The bot token is never returned.
//!
//! The commands talk to Telegram (and the credential store), so they run on the blocking pool and
//! never on the window's thread.

use crate::channels::runtime::Runtime;
use crate::channels::status::TelegramStatus;
use std::sync::Arc;

const NOT_READY: &str = "Telegram is not ready yet. Try again in a moment.";

/// Runs `f` on the blocking pool with the app's runtime.
async fn with_runtime<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Arc<Runtime>) -> Result<T, String> + Send + 'static,
{
    let runtime = crate::channels::current().ok_or_else(|| NOT_READY.to_string())?.clone();
    tauri::async_runtime::spawn_blocking(move || f(&runtime)).await.map_err(|_| NOT_READY.to_string())?
}

#[tauri::command]
pub async fn telegram_status() -> Result<TelegramStatus, String> {
    with_runtime(|rt| Ok(rt.status())).await
}

/// "Check": the pasted token is looked at with Telegram and kept in the credential store.
#[tauri::command]
pub async fn telegram_set_token(token: String) -> Result<TelegramStatus, String> {
    with_runtime(move |rt| rt.set_token(&token)).await
}

#[tauri::command]
pub async fn telegram_pair_start() -> Result<TelegramStatus, String> {
    with_runtime(|rt| rt.pair_start()).await
}

/// "Yes, that's me" (`true`) or "Not me" (`false`).
#[tauri::command]
pub async fn telegram_pair_decide(accept: bool) -> Result<TelegramStatus, String> {
    with_runtime(move |rt| rt.pair_decide(accept)).await
}

#[tauri::command]
pub async fn telegram_set_enabled(on: bool) -> Result<TelegramStatus, String> {
    with_runtime(move |rt| rt.set_enabled(on)).await
}

/// What the chat may do (`watch`, `answer`, `type`; `control` is refused) and how it reports.
#[tauri::command]
pub async fn telegram_set_options(caps: Vec<String>, notify_runs: bool, short_mode: bool) -> Result<TelegramStatus, String> {
    with_runtime(move |rt| rt.set_options(&caps, notify_runs, short_mode)).await
}

/// "Disconnect": forgets the token and the chat.
#[tauri::command]
pub async fn telegram_forget() -> Result<TelegramStatus, String> {
    with_runtime(|rt| rt.forget()).await
}
