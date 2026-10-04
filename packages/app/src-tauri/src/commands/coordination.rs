//! Tauri commands over the coordination event log and its projections (timeline, tasks, locks, audit, context packs).

use super::*;

#[tauri::command]
pub async fn replay_pane_timeline() -> Result<Vec<crate::event_log::PaneTimelineEvent>, String> {
    crate::event_log::replay_global_pane_timeline()
}

pub(super) fn rebuild_read_models() -> Result<crate::projections::ReadModels, String> {
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
    let project_root = crate::event_log::active_project_path();
    Ok(crate::context_pack::build_context_pack(
        request,
        &read_models,
        project_root.as_deref(),
        None,
    ))
}
