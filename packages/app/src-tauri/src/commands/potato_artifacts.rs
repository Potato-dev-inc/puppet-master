//! Tauri commands of the Artifacts page (Potato Bot): the list, the preview, "Save as file" on a chat message,
//! Download, Show in folder and Delete. Desktop only; the store is `potato::artifacts`.
//!
//! Errors are one string, `CODE: plain sentence`, like the other Potato Bot commands. Every command runs off the UI
//! thread (the files may be large). Downloads take the path the person picked in the native save dialog.

use std::path::Path;

use crate::potato::artifacts::{self, Artifact, Filter, ListItem, Preview};
use crate::potato::live;
use crate::potato::store::{self, StoreError};

async fn blocking<T: Send + 'static>(call: impl FnOnce() -> Result<T, StoreError> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || call().map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The files of every bot (or one bot), filtered; one row per name, newest first.
#[tauri::command]
pub async fn potato_artifact_list(filter: Option<Filter>) -> Result<Vec<ListItem>, String> {
    blocking(move || artifacts::list(&live::root(), &filter.unwrap_or_default())).await
}

/// One version: its content for the kinds the page can show, and the other versions of its name.
#[tauri::command]
pub async fn potato_artifact_open(bot_id: String, id: String) -> Result<Preview, String> {
    blocking(move || artifacts::open(&live::root(), &bot_id, &id)).await
}

/// "Save as file" on a chat message. The message must belong to this bot's thread and be a plain chat message.
#[tauri::command]
pub async fn potato_artifact_save_message(bot_id: String, message_id: String, name: Option<String>) -> Result<Artifact, String> {
    blocking(move || {
        let root = live::root();
        if !store::load_bots(&root)?.iter().any(|b| b.id == bot_id) {
            return Err(StoreError::new("POTATO_BOT_NOT_FOUND", "That bot is not known."));
        }
        let messages = store::read_messages(&root, &store::app_conversation(&bot_id), 5000, None)?;
        let message = messages
            .into_iter()
            .find(|m| m.id == message_id)
            .ok_or_else(|| StoreError::new("POTATO_ARTIFACT_NOT_FOUND", "That message is gone."))?;
        artifacts::save_message(&root, &bot_id, &message, name.as_deref(), now_ms())
    })
    .await
}

/// Download / Save as: copies the file to `dest` (a path from the save dialog).
#[tauri::command]
pub async fn potato_artifact_export(bot_id: String, id: String, dest: String) -> Result<(), String> {
    blocking(move || artifacts::export(&live::root(), &bot_id, &id, Path::new(&dest))).await
}

/// Show in folder: opens the folder with the file selected. The path is ours (never from the page).
#[tauri::command]
pub async fn potato_artifact_reveal(bot_id: String, id: String) -> Result<(), String> {
    blocking(move || {
        let (_, path) = artifacts::locate(&live::root(), &bot_id, &id)?;
        reveal(&path).map_err(|e| StoreError::new("POTATO_ARTIFACT_INVALID", format!("Could not open the folder: {e}.")))
    })
    .await
}

/// Deletes one version (its record and its file). The page asks first.
#[tauri::command]
pub async fn potato_artifact_delete(bot_id: String, id: String) -> Result<(), String> {
    blocking(move || artifacts::delete(&live::root(), &bot_id, &id)).await
}

#[cfg(windows)]
fn reveal(path: &Path) -> std::io::Result<()> {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    crate::proc_util::hidden_command("explorer").arg(arg).spawn().map(|_| ())
}

#[cfg(not(windows))]
fn reveal(path: &Path) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = crate::proc_util::hidden_command("open");
        command.arg("-R").arg(path);
        command
    } else {
        let mut command = crate::proc_util::hidden_command("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };
    command.spawn().map(|_| ())
}
