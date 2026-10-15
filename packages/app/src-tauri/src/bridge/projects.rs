//! Project-path resolution shared by the operation, task and shell routes.

use parking_lot::Mutex;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;

use super::errors::operation_http_error;
use super::store::rebuild_operation_read_models;
use crate::events::TaskId;
use crate::pty::PaneRegistry;

pub(super) fn resolve_project_root(registry: &Arc<Mutex<PaneRegistry>>) -> Option<PathBuf> {
    crate::event_log::active_project_path().or_else(|| {
        let path = crate::pty::registry::get_project_path(registry);
        let candidate = PathBuf::from(path);
        if crate::project_path::is_valid_project_path(&candidate) {
            Some(candidate)
        } else {
            None
        }
    })
}

pub(super) fn resolve_operation_project(
    requested: Option<&str>,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<PathBuf, (u16, serde_json::Value)> {
    if let Some(path) = requested.filter(|path| !path.trim().is_empty()) {
        return crate::project_path::normalize_project_path(std::path::Path::new(path)).map_err(
            |err| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_PROJECT_PATH",
                    err,
                    false,
                ))
            },
        );
    }
    resolve_project_root(registry).ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "PROJECT_PATH_REQUIRED",
            "project_path is required when no project is selected",
            false,
        ))
    })
}

pub(super) fn resolve_task_project(
    requested: Option<&str>,
    task_id: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<PathBuf, (u16, serde_json::Value)> {
    if let Some(path) = requested.filter(|path| !path.trim().is_empty()) {
        return crate::project_path::normalize_project_path(std::path::Path::new(path)).map_err(
            |err| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_PROJECT_PATH",
                    err,
                    false,
                ))
            },
        );
    }
    if let Ok(operation) = crate::operations::operation_for_task_any_project(task_id) {
        return Ok(PathBuf::from(operation.project_path));
    }
    resolve_project_root(registry).ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "PROJECT_PATH_REQUIRED",
            "project_path is required when no project is selected",
            false,
        ))
    })
}

pub(super) fn validate_task_owner(
    project: &str,
    task_id: &str,
    agent_id: &str,
) -> Result<(), (u16, serde_json::Value)> {
    let models = rebuild_operation_read_models(project).map_err(|err| (500, json!({"error":err,"code":"STATE_UNAVAILABLE","recoverable":true,"retry_after_ms":null,"context":null})))?;
    let task = models.tasks.iter().find(|task| task.id == TaskId(task_id.to_string()))
        .ok_or_else(|| (404, json!({"error":"unknown task","code":"TASK_NOT_FOUND","recoverable":false,"retry_after_ms":null,"context":null})))?;
    if task.claimed_by.as_deref() != Some(agent_id) {
        return Err((
            409,
            json!({"error":"task is claimed by a different agent","code":"TASK_OWNER_MISMATCH","recoverable":false,"retry_after_ms":null,"context":{"task_id":task_id,"claimed_by":task.claimed_by}}),
        ));
    }
    Ok(())
}

pub(super) fn librarian_indexer_path(app: &AppHandle) -> Option<String> {
    crate::settings_store::read_public_settings(app)
        .get("librarian_indexer_path")
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn normalize_shell_cwd(
    requested: Option<&str>,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<String, crate::operations::OperationError> {
    let raw_path = requested
        .map(str::to_string)
        .unwrap_or_else(|| crate::pty::registry_get_project_path(registry));
    let normalized =
        crate::project_path::normalize_project_path(std::path::Path::new(&raw_path))
            .map_err(|error| crate::operations::OperationError::new("INVALID_CWD", error, false))?;
    let raw = normalized.to_string_lossy();
    if raw.contains('\0') || raw.contains('\r') || raw.contains('\n') {
        return Err(crate::operations::OperationError::new(
            "INVALID_CWD",
            "cwd cannot contain NUL or newline characters",
            false,
        ));
    }
    #[cfg(windows)]
    if raw.contains('"') {
        return Err(crate::operations::OperationError::new(
            "INVALID_CWD",
            "cwd contains a character that cannot be represented safely by cmd.exe",
            false,
        ));
    }
    let canonical = std::fs::canonicalize(&normalized).map_err(|error| {
        crate::operations::OperationError::new(
            "INVALID_CWD",
            format!("cwd does not exist or cannot be accessed: {error}"),
            false,
        )
    })?;
    if !canonical.is_dir() {
        return Err(crate::operations::OperationError::new(
            "INVALID_CWD",
            "cwd must name an existing directory",
            false,
        ));
    }
    let mut path = canonical.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        if let Some(stripped) = path.strip_prefix(r"\\?\UNC\") {
            path = format!(r"\\{stripped}");
        } else if let Some(stripped) = path.strip_prefix(r"\\?\") {
            path = stripped.to_string();
        }
    }
    Ok(path)
}
