//! Tauri commands of Simple mode (`crate::plain_mode`). The UI never chooses where a task runs:
//! `plain_start` always makes a copy (see `plain_mode::run_request`).

use super::AppState;
use crate::plain_mode::{self, guard, StartInput};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

/// Whether `path` may be used as a project. `{ ok: true }` or `{ ok: false, code, message }`.
#[tauri::command]
pub async fn plain_check_folder(path: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let known = guard::KnownDirs::from_env();
        let text = crate::worktrees::folder::path_of(&path).to_string_lossy().into_owned();
        if let Some(refusal) = plain_mode::refusal_for(guard::verdict(&text, &known), &text) {
            return json!({ "ok": false, "code": refusal.code, "message": refusal.message });
        }
        let dir = std::path::Path::new(&text);
        if !dir.is_dir() {
            return json!({ "ok": false, "code": "FOLDER_MISSING", "message": format!("{text} is not a folder.") });
        }
        if !guard::small_enough(dir) {
            return json!({ "ok": false, "code": "FOLDER_TOO_BIG", "message": "This folder is too big to copy." });
        }
        json!({ "ok": true })
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))
}

/// Make the project (a new folder, or one she has), then start the agent in a COPY of it. Returns
/// `{ pane_id, handle, folder }`. Errors read `CODE: message`.
#[tauri::command]
pub async fn plain_start(app: AppHandle, state: State<'_, AppState>, input: StartInput) -> Result<Value, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let known = guard::KnownDirs::from_env();
        let plan = plain_mode::plan(&input, &known, &plain_mode::projects_root()).map_err(|r| r.to_string())?;
        plain_mode::prepare(&plan).map_err(|r| r.to_string())?;
        let run: crate::agent_runs::AgentRunRequest =
            serde_json::from_value(plan.request.clone()).map_err(|e| format!("INTERNAL: {e}"))?;
        let view = crate::agent_runs::run_agent(run, registry, app, None).map_err(|e| format!("{}: {}", e.code, e.message))?;
        crate::worktrees::folder::remember(&crate::app_paths::app_data_dir(), &plan.origin.to_string_lossy());
        Ok::<Value, String>(json!({
            "pane_id": view.pane_id,
            "handle": view.handle,
            "folder": plan.origin.to_string_lossy(),
        }))
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}

fn pane_cwd(state: &State<'_, AppState>, pane_id: &str) -> Option<std::path::PathBuf> {
    let cwd = state.registry.lock().list().into_iter().find(|p| p.id == pane_id)?.cwd;
    let path = std::path::PathBuf::from(cwd);
    path.is_dir().then_some(path)
}

/// The page the agent made in this pane's copy: `{ name, html }`, or `null` when there is none yet.
#[tauri::command]
pub async fn plain_preview(state: State<'_, AppState>, pane_id: String) -> Result<Value, String> {
    let Some(dir) = pane_cwd(&state, &pane_id) else { return Ok(Value::Null) };
    tauri::async_runtime::spawn_blocking(move || {
        plain_mode::preview::read_preview(&dir).map(|f| serde_json::to_value(f).unwrap_or(Value::Null)).unwrap_or(Value::Null)
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))
}

/// Open that page in the person's own browser.
#[tauri::command]
pub async fn plain_open_preview(state: State<'_, AppState>, pane_id: String) -> Result<(), String> {
    let Some(dir) = pane_cwd(&state, &pane_id) else { return Err("FOLDER_MISSING: the copy is gone".into()) };
    let Some(file) = plain_mode::preview::find_preview(&dir) else { return Err("NO_PAGE: nothing to open yet".into()) };
    plain_mode::open_in_default_app(&file)
}
