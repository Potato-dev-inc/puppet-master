//! Tauri commands of Recently deleted (UX-2): the list of what was deleted in the last 7 days, and "Bring back".
//! Desktop only, like the rest of Potato Bot. Errors are one string, `CODE: plain sentence`. The store is
//! `potato::trash`; the purge runs by itself (the routines tick), not from here.

use crate::potato::live;
use crate::potato::trash::{self, Item};

fn root() -> Result<std::path::PathBuf, String> {
    live::service().map(|s| s.root().to_path_buf()).ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())
}

/// What was deleted in the last 7 days, newest first.
#[tauri::command]
pub async fn potato_trash_list() -> Result<Vec<Item>, String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || trash::list(&root).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// Puts one deleted item back as it was (a note, a routine or a whole bot with its memory and routines).
#[tauri::command]
pub async fn potato_trash_restore(id: String) -> Result<(), String> {
    let root = root()?;
    tauri::async_runtime::spawn_blocking(move || trash::restore(&root, &id).map(|_| ()).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}
