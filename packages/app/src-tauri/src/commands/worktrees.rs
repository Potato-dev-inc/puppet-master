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

// -------------------------------------------------- the "+" menu's folder step and Copies tab

use crate::worktrees::{folder, preset, service};
use std::path::PathBuf;
use std::time::Duration;

fn app_data() -> PathBuf {
    crate::app_paths::app_data_dir()
}

/// Recent projects: the folders chosen here, then every project that had runs, then the default
/// project. Each with its Git state.
#[tauri::command]
pub async fn project_recent(state: State<'_, AppState>, limit: Option<usize>) -> Result<Value, String> {
    let default_project = crate::standby::default_project(&state.registry);
    blocking(move || {
        let mut others = crate::operations::indexed_project_paths();
        others.push(default_project);
        Ok(serde_json::to_value(folder::recent(&app_data(), &others, limit.unwrap_or(8))).unwrap_or(Value::Null))
    })
    .await
}

#[tauri::command]
pub async fn project_forget(path: String) -> Result<(), String> {
    folder::forget(&app_data(), &path);
    Ok(())
}

/// Remember `path` as the most recent project (called when an agent starts there).
#[tauri::command]
pub async fn project_remember(path: String) -> Result<(), String> {
    folder::remember(&app_data(), &path);
    Ok(())
}

/// The system folder picker. `null` when the person cancels. The chosen folder is remembered.
#[tauri::command]
pub async fn project_pick_folder(app: AppHandle) -> Result<Option<Value>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title("Choose a project folder").blocking_pick_folder()
    })
    .await
    .map_err(|error| format!("INTERNAL: {error}"))?;
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|error| format!("INVALID_ARGUMENT: {error}"))?;
    // The folder as the person picked it, canonical and without a `\\?\` prefix.
    let text = crate::project_path::selected_folder(&path).to_string_lossy().into_owned();
    folder::remember(&app_data(), &text);
    let entry = folder::recent(&app_data(), &[], 1)
        .into_iter()
        .next()
        .map(|item| serde_json::to_value(item).unwrap_or(Value::Null));
    Ok(entry)
}

#[tauri::command]
pub async fn project_git_state(path: String) -> Result<String, String> {
    blocking(move || Ok(folder::git_state(&folder::path_of(&path)).to_string())).await
}

#[tauri::command]
pub async fn project_git_init_plan(path: String) -> Result<Value, String> {
    blocking(move || {
        folder::init_plan(&folder::path_of(&path)).map(|plan| serde_json::to_value(plan).unwrap_or(Value::Null))
    })
    .await
}

#[tauri::command]
pub async fn project_git_init(path: String) -> Result<Value, String> {
    blocking(move || {
        let folder_path = folder::path_of(&path);
        let outcome = folder::init(&folder_path)?;
        folder::remember(&app_data(), &folder_path.to_string_lossy());
        Ok(serde_json::to_value(outcome).unwrap_or(Value::Null))
    })
    .await
}

fn plain(message: String) -> OperationError {
    OperationError::new("INVALID_ARGUMENT", message, false)
}

/// Setup commands, files to copy and the start command of a project (Project, Copies).
#[tauri::command]
pub async fn project_preset_get(project_path: String) -> Result<Value, String> {
    blocking(move || {
        Ok(serde_json::to_value(preset::view(&folder::path_of(&project_path))).unwrap_or(Value::Null))
    })
    .await
}

#[tauri::command]
pub async fn project_preset_set(
    project_path: String,
    setup: Vec<String>,
    copy: Vec<String>,
    run: Option<String>,
) -> Result<Value, String> {
    blocking(move || {
        preset::set_from_text(&folder::path_of(&project_path), &setup, &copy, run.as_deref().unwrap_or(""))
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
            .map_err(plain)
    })
    .await
}

#[tauri::command]
pub async fn project_preset_import_conductor(project_path: String) -> Result<Value, String> {
    blocking(move || {
        preset::import_conductor(&folder::path_of(&project_path))
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
            .map_err(plain)
    })
    .await
}

/// The facts and the prefilled title and body for the Create PR sheet.
#[tauri::command]
pub async fn worktree_pr_prepare(
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        service::pr_prepare(project_path.as_deref(), &handle, &registry, &Default::default())
            .map(|prepared| serde_json::to_value(prepared).unwrap_or(Value::Null))
    })
    .await
}

#[tauri::command]
pub async fn worktree_pr_create(
    app: AppHandle,
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
    title: String,
    body: String,
    base: String,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    let result = blocking(move || {
        service::pr_create(project_path.as_deref(), &handle, &title, &body, &base, &registry, &Default::default())
            .map(|pr| serde_json::to_value(pr).unwrap_or(Value::Null))
    })
    .await;
    let _ = app.emit("worktree://changed", ());
    result
}

/// Read the pull request's checks, waiting up to `wait_secs` (at most 60) while some are pending.
#[tauri::command]
pub async fn worktree_pr_checks(
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
    wait_secs: Option<u64>,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        let wait = Duration::from_secs(wait_secs.unwrap_or(0).min(60));
        service::pr_checks(project_path.as_deref(), &handle, wait, &registry, &Default::default())
            .map(|pr| serde_json::to_value(pr).unwrap_or(Value::Null))
    })
    .await
}

/// What "Send back" would say to the agent. Nothing is sent.
#[tauri::command]
pub async fn worktree_pr_send_back_preview(
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        service::pr_send_back_preview(project_path.as_deref(), &handle, &registry, &Default::default())
            .map(|preview| serde_json::to_value(preview).unwrap_or(Value::Null))
    })
    .await
}

/// Send the previewed message to the agent. Only called after the person pressed Send.
#[tauri::command]
pub async fn worktree_pr_send_back(
    app: AppHandle,
    state: State<'_, AppState>,
    project_path: Option<String>,
    handle: String,
    text: String,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || {
        service::pr_send_back(project_path.as_deref(), &handle, &text, &registry, app)
            .map(|sent| serde_json::to_value(sent).unwrap_or(Value::Null))
    })
    .await
}
