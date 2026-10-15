//! Tauri commands for file checkpoints (see `docs/features/checkpoints.md`). Errors are plain strings.

use crate::checkpoints::{self, FileChange, ListView, RestoreReport};

#[tauri::command]
pub async fn checkpoints_list(pane_id: String) -> Result<ListView, String> {
    tauri::async_runtime::spawn_blocking(move || checkpoints::list_view(&pane_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn checkpoint_diff(pane_id: String, id: String) -> Result<Vec<FileChange>, String> {
    tauri::async_runtime::spawn_blocking(move || checkpoints::live().diff(&pane_id, &id).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn checkpoint_restore(pane_id: String, id: String) -> Result<RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || checkpoints::live().restore(&pane_id, &id).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// "Undo last turn": restores the start of the newest turn.
#[tauri::command]
pub async fn checkpoint_undo_last(pane_id: String) -> Result<RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        checkpoints::live().undo_last_turn(&pane_id).map_err(|e| match e {
            checkpoints::CheckpointError::NotFound => "There's nothing to undo yet.".to_string(),
            other => other.to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

