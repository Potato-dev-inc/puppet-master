//! Tauri command surface — thin wrappers around the PTY registry.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use crate::pty::agents::AgentType;
use crate::pty::{
    registry_get_project_path, registry_kill_all, registry_kill_pane, registry_read_buffer,
    registry_read_raw_buffer, registry_read_snapshot, registry_resize, registry_set_project_path,
    registry_spawn_pane, registry_write_input, PaneInfo, PaneRegistry, SpawnPaneArgs,
};
use crate::settings_store;

#[derive(Default)]
pub struct AppState {
    pub registry: Arc<Mutex<PaneRegistry>>,
    pub public_settings: Mutex<Value>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(PaneRegistry::new())),
            public_settings: Mutex::new(settings_store::default_public_settings()),
        }
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

fn developer_use_rust_mcp(state: &State<'_, AppState>) -> bool {
    state
        .public_settings
        .lock()
        .get("developer_use_rust_mcp")
        .and_then(Value::as_bool)
        .unwrap_or(false)
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
pub async fn list_agent_contexts() -> Result<Vec<crate::agent_contexts::AgentContextProfile>, String>
{
    Ok(crate::agent_contexts::list_agent_context_profiles())
}

#[tauri::command]
pub async fn inspect_agent_model(
    state: State<'_, AppState>,
    pane_id: String,
    lines: Option<usize>,
) -> Result<crate::agent_contexts::AgentModelInspection, String> {
    let pane = state
        .registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let agent_type = AgentType::parse(&pane.agent_type)
        .ok_or_else(|| format!("unknown agent_type: {}", pane.agent_type))?;
    let buffer = registry_read_buffer(&state.registry, &pane_id, lines.unwrap_or(200))?;
    Ok(crate::agent_contexts::inspect_agent_model(
        pane_id, agent_type, &buffer,
    ))
}

#[tauri::command]
pub async fn read_agent_context(
    state: State<'_, AppState>,
    agent_type: Option<String>,
    pane_id: Option<String>,
) -> Result<serde_json::Value, String> {
    if let Some(pane_id) = pane_id {
        let pane = state
            .registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let buffer = registry_read_buffer(&state.registry, &pane_id, 200)?;
        let context = crate::agent_contexts::build_pane_agent_context(pane, &buffer)
            .ok_or_else(|| "unknown pane agent_type".to_string())?;
        return serde_json::to_value(context)
            .map_err(|err| format!("serialize agent context: {err}"));
    }

    let agent_type = agent_type.ok_or_else(|| "agent_type or pane_id is required".to_string())?;
    let agent_type =
        AgentType::parse(&agent_type).ok_or_else(|| format!("unknown agent_type: {agent_type}"))?;
    serde_json::to_value(crate::agent_contexts::get_agent_context_profile(agent_type))
        .map_err(|err| format!("serialize agent context profile: {err}"))
}

#[tauri::command]
pub async fn replay_pane_timeline() -> Result<Vec<crate::event_log::PaneTimelineEvent>, String> {
    crate::event_log::replay_global_pane_timeline()
}

fn rebuild_read_models() -> Result<crate::projections::ReadModels, String> {
    let entries = crate::event_log::read_global_entries()?;
    Ok(crate::projections::build_read_models(&entries))
}

#[derive(Debug, Clone, Serialize)]
pub struct CoordinationStorageInfo {
    pub scope: String,
    pub project_path: Option<String>,
    pub storage_dir: String,
    pub event_log_path: String,
    pub event_count: usize,
    pub task_count: usize,
    pub lock_count: usize,
    pub exists: bool,
}

#[tauri::command]
pub async fn get_coordination_storage_info() -> Result<CoordinationStorageInfo, String> {
    let event_log_path = crate::event_log::current_event_log_path();
    let project_path = crate::event_log::active_project_path();
    let storage_dir = project_path
        .as_deref()
        .map(crate::event_log::project_storage_dir)
        .or_else(|| event_log_path.parent().map(PathBuf::from))
        .unwrap_or_else(crate::app_paths::app_data_dir);
    let exists = event_log_path.exists();
    let entries = crate::event_log::read_global_entries()?;
    let read_models = crate::projections::build_read_models(&entries);
    Ok(CoordinationStorageInfo {
        scope: if project_path.is_some() {
            "project".to_string()
        } else {
            "global_fallback".to_string()
        },
        project_path: project_path.map(|path| path.to_string_lossy().into_owned()),
        storage_dir: storage_dir.to_string_lossy().into_owned(),
        event_log_path: event_log_path.to_string_lossy().into_owned(),
        event_count: entries.len(),
        task_count: read_models.tasks.len(),
        lock_count: read_models.locks.len(),
        exists,
    })
}

#[tauri::command]
pub async fn get_workspace_state() -> Result<crate::projections::WorkspaceStateProjection, String> {
    Ok(rebuild_read_models()?.workspace)
}

#[tauri::command]
pub async fn list_tasks() -> Result<Vec<crate::projections::TaskProjection>, String> {
    Ok(rebuild_read_models()?.tasks)
}

#[tauri::command]
pub async fn list_locks() -> Result<Vec<crate::projections::LockProjection>, String> {
    Ok(rebuild_read_models()?.locks)
}

#[tauri::command]
pub async fn read_agent_inbox(
    agent_id: String,
) -> Result<crate::projections::AgentInboxProjection, String> {
    Ok(crate::projections::agent_inbox(agent_id))
}

#[tauri::command]
pub async fn get_audit() -> Result<Vec<crate::projections::AuditEntryProjection>, String> {
    Ok(rebuild_read_models()?.audit)
}

#[tauri::command]
pub async fn build_context_pack(
    request: crate::context_pack::ContextPackRequest,
) -> Result<crate::context_pack::ContextPack, String> {
    let read_models = rebuild_read_models()?;
    Ok(crate::context_pack::build_context_pack(
        request,
        &read_models,
    ))
}

#[tauri::command]
pub async fn resize_pane(
    state: State<'_, AppState>,
    pane_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let changed = registry_resize(&state.registry, &pane_id, cols, rows)?;
    if changed {
        crate::bridge::push_pane_resize_sse(&pane_id, cols, rows);
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_public_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(&settings_json)
        .map_err(|err| format!("invalid settings json: {err}"))?;
    let merged =
        settings_store::merge_public_settings(&settings_store::default_public_settings(), &parsed);
    *state.public_settings.lock() = merged;
    let _ = app;
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

#[tauri::command]
pub async fn ensure_orchestrator_mcp(
    state: State<'_, AppState>,
    backend: String,
    project_path: String,
) -> Result<crate::mcp_install::EnsureMcpResult, String> {
    crate::mcp_install::ensure_orchestrator_mcp_with_preference(
        &backend,
        std::path::Path::new(&project_path),
        developer_use_rust_mcp(&state),
    )
}

#[tauri::command]
pub async fn install_npm_mcp_configs(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<Vec<crate::mcp_install::EnsureMcpResult>, String> {
    crate::mcp_install::install_mcp_configs_with_preference(
        std::path::Path::new(&project_path),
        developer_use_rust_mcp(&state),
    )
}

#[tauri::command]
pub async fn install_global_npm_mcp_configs(
    state: State<'_, AppState>,
) -> Result<Vec<crate::mcp_install::EnsureMcpResult>, String> {
    crate::mcp_install::install_global_mcp_configs_with_preference(developer_use_rust_mcp(&state))
}

#[tauri::command]
pub async fn uninstall_npm_mcp_configs(
    project_path: String,
) -> Result<Vec<crate::mcp_install::EnsureMcpResult>, String> {
    crate::mcp_install::uninstall_npm_mcp_configs(std::path::Path::new(&project_path))
}

#[tauri::command]
pub async fn uninstall_global_npm_mcp_configs(
) -> Result<Vec<crate::mcp_install::EnsureMcpResult>, String> {
    crate::mcp_install::uninstall_global_npm_mcp_configs()
}

#[tauri::command]
pub async fn get_mcp_status(
    state: State<'_, AppState>,
    project_path: String,
    auto_repair: bool,
) -> Result<crate::mcp_status::McpStatusReport, String> {
    crate::mcp_status::get_mcp_status_with_preference(
        std::path::Path::new(&project_path),
        auto_repair,
        developer_use_rust_mcp(&state),
    )
}

/// Push a JSON chat event to all SSE clients (mobile PWA and any desktop browser).
/// The frontend calls this after each LLM chunk so the mobile PWA receives it.
#[tauri::command]
pub async fn push_chat_event(event_json: String) -> Result<(), String> {
    let payload = format!("event: chat\ndata: {event_json}\n\n");
    crate::bridge::push_sse(payload);
    Ok(())
}

/// Push public settings to mobile SSE clients after desktop-side changes.
#[tauri::command]
pub async fn push_settings_event(settings_json: String) -> Result<(), String> {
    let payload = format!("event: settings\ndata: {settings_json}\n\n");
    crate::bridge::push_sse(payload);
    Ok(())
}

#[tauri::command]
pub async fn create_mobile_pairing_session(
    bridge_url: String,
) -> Result<crate::mobile_pairing::PairingSession, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let session = store.lock().create_pairing_session(bridge_url);
    Ok(session)
}

#[tauri::command]
pub async fn list_paired_mobile_devices(
) -> Result<Vec<crate::mobile_pairing::PairedDeviceInfo>, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let devices = store.lock().list_devices();
    Ok(devices)
}

#[tauri::command]
pub async fn revoke_paired_mobile_device(device_id: String) -> Result<bool, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let revoked = store.lock().revoke_device(&device_id)?;
    Ok(revoked)
}

#[tauri::command]
pub fn get_mobile_tunnel_info() -> crate::pwa_server::DevInfoPayload {
    crate::mobile_tunnel::get_mobile_tunnel_info()
}
