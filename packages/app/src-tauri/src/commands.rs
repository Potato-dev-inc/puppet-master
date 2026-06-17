//! Tauri command surface — thin wrappers around the PTY registry.

use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use crate::pty::{
    registry_get_project_path, registry_kill_all, registry_kill_pane, registry_read_buffer,
    registry_read_raw_buffer, registry_read_snapshot, registry_resize, registry_set_project_path,
    registry_spawn_pane, registry_write_input, PaneInfo, PaneRegistry, SpawnPaneArgs,
};

#[derive(Default)]
pub struct AppState {
    pub registry: Arc<Mutex<PaneRegistry>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }
}

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
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn write_pane_input(
    state: State<'_, AppState>,
    pane_id: String,
    args: WriteInputArgs,
) -> Result<(), String> {
    registry_write_input(&state.registry, &pane_id, &args.text, args.append_newline)
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
    registry_resize(&state.registry, &pane_id, cols, rows)
}

#[tauri::command]
pub async fn set_project_path(state: State<'_, AppState>, path: String) -> Result<(), String> {
    registry_set_project_path(&state.registry, path);
    Ok(())
}

#[tauri::command]
pub async fn get_project_path_cmd(state: State<'_, AppState>) -> Result<String, String> {
    Ok(registry_get_project_path(&state.registry))
}

#[tauri::command]
pub async fn ensure_orchestrator_mcp(backend: String, project_path: String) -> Result<crate::mcp_install::EnsureMcpResult, String> {
    crate::mcp_install::ensure_orchestrator_mcp(&backend, std::path::Path::new(&project_path))
}
