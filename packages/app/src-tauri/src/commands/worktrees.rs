//! Cockpit commands for isolated worktrees (`crate::worktrees::service`). Errors read
//! `CODE: message`; a merge conflict lists the files after the message.

use super::AppState;
use crate::operations::OperationError;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

fn text(error: OperationError) -> String {
    let files = error
        .context
        .get("files")
        .and_then(Value::as_array)
        .map(|files| files.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
        .filter(|files| !files.is_empty());
    match files {
        Some(files) => format!("{}: {} ({files})", error.code, error.message),
        None => format!("{}: {}", error.code, error.message),
    }
}

fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, OperationError> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    async move {
        tauri::async_runtime::spawn_blocking(job)
            .await
            .map_err(|error| format!("INTERNAL: {error}"))?
            .map_err(text)
    }
}

/// Open worktrees of `project_path` (every known project when omitted).
#[tauri::command]
pub async fn worktree_list(
    state: State<'_, AppState>,
    project_path: Option<String>,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        Ok(serde_json::to_value(crate::worktrees::service::list(project_path.as_deref(), &registry))
            .unwrap_or(Value::Null))
    })
    .await
}

/// The worktree a pane works in (by its working directory), or null.
#[tauri::command]
pub async fn pane_worktree(state: State<'_, AppState>, pane_id: String) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        Ok(crate::worktrees::service::for_pane(&registry, &pane_id)
            .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
            .unwrap_or(Value::Null))
    })
    .await
}

#[tauri::command]
pub async fn worktree_diff(
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
    path: Option<String>,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        crate::worktrees::service::diff(project_path.as_deref(), &handle, path.as_deref(), &registry)
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
    })
    .await
}

#[tauri::command]
pub async fn worktree_merge(
    app: AppHandle,
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    let result = blocking(move || {
        crate::worktrees::service::merge(project_path.as_deref(), &handle, &registry)
            .map(|outcome| serde_json::to_value(outcome).unwrap_or(Value::Null))
    })
    .await;
    let _ = app.emit("worktree://changed", ());
    let _ = app.emit("pty://panes-changed", ());
    result
}

#[tauri::command]
pub async fn worktree_discard(
    app: AppHandle,
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
) -> Result<(), String> {
    let registry = state.registry.clone();
    let result = blocking(move || {
        crate::worktrees::service::discard(project_path.as_deref(), &handle, &registry)
    })
    .await;
    let _ = app.emit("worktree://changed", ());
    let _ = app.emit("pty://panes-changed", ());
    result
}
