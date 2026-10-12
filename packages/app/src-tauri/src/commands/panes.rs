//! Tauri commands for panes: list, spawn, kill, input, buffers, resize and the project path.

use super::*;

#[tauri::command]
pub async fn list_panes(state: State<'_, AppState>) -> Result<Vec<PaneInfo>, String> {
    Ok(state.registry.lock().list())
}

#[tauri::command]
pub async fn spawn_pane(
    app: AppHandle,
    state: State<'_, AppState>,
    args: SpawnPaneArgs,
) -> Result<String, String> {
    registry_spawn_pane(&state.registry, &app, args)
}

#[tauri::command]
pub async fn kill_pane_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
) -> Result<(), String> {
    registry_kill_pane(&state.registry, &pane_id)?;
    let _ = app.emit("pty://panes-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn kill_all_panes(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    registry_kill_all(&state.registry);
    let _ = app.emit("pty://panes-changed", ());
    Ok(())
}

#[derive(Deserialize)]
pub struct WriteInputArgs {
    pub text: String,
    #[serde(default = "default_true")]
    pub append_newline: bool,
    /// MCP/orchestrator: send plain-text prompts via OpenCode prompt_async (opencode_native only).
    #[serde(default)]
    pub via_opencode_api: bool,
    #[serde(default)]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
}

pub(super) fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn write_pane_input(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    args: WriteInputArgs,
) -> Result<(), String> {
    let settings = state.public_settings.lock().clone();
    let model = crate::opencode::resolve_model(
        args.model_provider.as_deref(),
        args.model_id.as_deref(),
        &settings,
    );
    registry_write_input(
        &state.registry,
        &app,
        &pane_id,
        &args.text,
        args.append_newline,
        args.via_opencode_api,
        model,
    )
}

#[tauri::command]
pub async fn read_pane_buffer(
    state: State<'_, AppState>,
    pane_id: String,
    lines: usize,
) -> Result<String, String> {
    registry_read_buffer(&state.registry, &pane_id, lines)
}

#[tauri::command]
pub async fn read_pane_snapshot(
    state: State<'_, AppState>,
    pane_id: String,
) -> Result<String, String> {
    registry_read_snapshot(&state.registry, &pane_id)
}

#[tauri::command]
pub async fn read_pane_raw_buffer(
    state: State<'_, AppState>,
    pane_id: String,
    lines: usize,
) -> Result<Vec<u8>, String> {
    registry_read_raw_buffer(&state.registry, &pane_id, lines)
}

#[tauri::command]
pub async fn resize_pane(
    state: State<'_, AppState>,
    pane_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    // A phone viewing this pane holds its size; the desktop size is applied when it lets go.
    if crate::pty::viewer_size::desktop_resize(&pane_id, cols, rows) {
        return Ok(());
    }
    let changed = registry_resize(&state.registry, &pane_id, cols, rows)?;
    if changed {
        // Phone viewers (SSE / long-poll) and desktop windows (re-read pane info) see the size.
        crate::bridge::notify_pane_resized(&pane_id, cols, rows);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_project_path(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let normalized = crate::project_path::normalize_project_path(std::path::Path::new(&path))?;
    registry_set_project_path(&state.registry, normalized.to_string_lossy().into_owned());
    crate::event_log::set_active_project_path(Some(normalized));
    Ok(())
}

#[tauri::command]
pub async fn get_project_path_cmd(state: State<'_, AppState>) -> Result<String, String> {
    Ok(registry_get_project_path(&state.registry))
}
