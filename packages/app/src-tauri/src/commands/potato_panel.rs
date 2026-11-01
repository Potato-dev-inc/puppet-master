//! Tauri commands of the bot panel's Advanced tab (Potato Bot, spec P7): Open work, Memory and Notifications. Desktop
//! only, like the rest of Potato Bot. Errors are one string, `CODE: plain sentence`. Notes are read and deleted here by
//! path, and only inside the bot's own folder (`potato::memory_notes`).

use std::sync::Arc;

use serde::Serialize;

use crate::potato::live;
use crate::potato::memory_notes::{self, Note};
use crate::potato::notify::{self, Notify};
use crate::potato::open_work::{self, HelperRow, Item};
use crate::potato::service::Service;
use crate::potato::trash;
use crate::potato::store::StoreError;

fn service() -> Result<Arc<Service>, String> {
    live::service().ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())
}

/// Runs one call off the UI thread (the files are read and written there).
async fn run<T: Send + 'static>(call: impl FnOnce(&Service) -> Result<T, StoreError> + Send + 'static) -> Result<T, String> {
    let service = service()?;
    tauri::async_runtime::spawn_blocking(move || call(&service).map_err(|e| e.to_string())).await.map_err(|e| format!("INTERNAL: {e}"))?
}

/// The Open work list: the items, and the helper jobs still running for this bot (read-only).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWorkView {
    pub items: Vec<Item>,
    pub helpers: Vec<HelperRow>,
}

#[tauri::command]
pub async fn potato_open_work_list(bot_id: String) -> Result<OpenWorkView, String> {
    run(move |s| {
        let items = open_work::list(s.root(), &bot_id)?;
        let helpers = open_work::helper_rows(&s.running_helpers(), &bot_id);
        Ok(OpenWorkView { items, helpers })
    })
    .await
}

#[tauri::command]
pub async fn potato_open_work_add(bot_id: String, text: String) -> Result<Item, String> {
    run(move |s| open_work::add(s.root(), &bot_id, &text)).await
}

#[tauri::command]
pub async fn potato_open_work_set_done(bot_id: String, id: String, done: bool) -> Result<Item, String> {
    run(move |s| open_work::set_done(s.root(), &bot_id, &id, done)).await
}

#[tauri::command]
pub async fn potato_open_work_remove(bot_id: String, id: String) -> Result<(), String> {
    run(move |s| open_work::remove(s.root(), &bot_id, &id)).await
}

/// What the bot remembers: its notes, newest first. Read-only.
#[tauri::command]
pub async fn potato_memory_list(bot_id: String) -> Result<Vec<Note>, String> {
    run(move |s| memory_notes::list(s.root(), &bot_id)).await
}

/// One note's text, by the path the list gave.
#[tauri::command]
pub async fn potato_memory_read(bot_id: String, path: String) -> Result<String, String> {
    run(move |s| memory_notes::read(s.root(), &bot_id, &path)).await
}

/// Deletes one note at once: it goes to Recently deleted (7 days). Returns the trash id, for "Undo".
#[tauri::command]
pub async fn potato_memory_delete(bot_id: String, path: String) -> Result<String, String> {
    run(move |s| trash::trash_note(s.root(), &bot_id, &path, chrono::Utc::now().timestamp_millis()).map(|item| item.id)).await
}

/// "Notify me" of one bot: `reply`, `needs_you` or `never`.
#[tauri::command]
pub async fn potato_notify_get(bot_id: String) -> Result<Notify, String> {
    run(move |s| notify::get(s.root(), &bot_id)).await
}

#[tauri::command]
pub async fn potato_notify_set(bot_id: String, value: String) -> Result<Notify, String> {
    let choice = Notify::parse(&value).ok_or_else(|| "POTATO_NOTIFY_INVALID: Pick when to hear from it.".to_string())?;
    run(move |s| notify::set(s.root(), &bot_id, choice)).await
}
