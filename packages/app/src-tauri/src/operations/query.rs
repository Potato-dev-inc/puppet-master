//! Read side: single snapshots, project listings, agent-run handle resolution and
//! pane/task lookups over the durable records.

use super::model::*;
use super::store::{find_key, indexed_projects, path_for, project_key, read, root, task_index_path};
use std::collections::HashMap;
use std::fs;

pub fn get_operation(
    project_path: &str,
    operation_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    read(&path_for(project_path, operation_id)?)
}

/// Read all durable operation records for a project, ordered by creation time.
pub fn list_operations(project_path: &str) -> Result<Vec<OperationSnapshot>, OperationError> {
    let dir = root(project_path)?;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(OperationError::io(error)),
    };
    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            match read(&entry.path()) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(error) if error.code == "OPERATION_NOT_FOUND" => continue,
                Err(error) => return Err(error),
            }
        }
    }
    snapshots.sort_by_key(|snapshot| (snapshot.created_at_ms, snapshot.turn_index));
    Ok(snapshots)
}

/// Operations for `project_path` plus every other indexed project. Used when a handle
/// or listing should not be confined to the currently selected directory.
pub fn list_indexed_operations(project_path: &str) -> Result<Vec<OperationSnapshot>, OperationError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut snapshots = Vec::new();
    let mut projects = vec![project_path.to_string()];
    projects.extend(indexed_projects().unwrap_or_default());
    for project in projects {
        if !seen.insert(project_key(&project)) {
            continue;
        }
        snapshots.extend(list_operations(&project)?);
    }
    snapshots.sort_by_key(|snapshot| (snapshot.created_at_ms, snapshot.turn_index));
    Ok(snapshots)
}

/// Resolve the newest turn of a stable agent-run handle.
pub fn latest_agent_run(
    project_path: &str,
    agent_run_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    latest_in_project(project_path, agent_run_id)?
        .ok_or_else(|| crate::mcp_sessions::agent_not_found(agent_run_id))
}

/// The operation a caller's idempotency key already started in this project, if any.
pub fn find_by_idempotency_key(
    project_path: &str,
    key: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    find_key(project_path, key)
}

/// Prefer `project_path`, then any indexed project that owns this handle.
pub fn resolve_agent_run(
    project_path: &str,
    agent_run_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    if let Some(snapshot) = latest_in_project(project_path, agent_run_id)? {
        return Ok(snapshot);
    }
    for project in indexed_projects()? {
        if project_key(&project) == project_key(project_path) {
            continue;
        }
        if let Some(snapshot) = latest_in_project(&project, agent_run_id)? {
            return Ok(snapshot);
        }
    }
    Err(crate::mcp_sessions::agent_not_found(agent_run_id))
}

/// Newest turn of `agent_run_id` in each project that owns it: `preferred` first, then every
/// other indexed project. Lets a caller tell a handle that is unique from one that is not.
pub fn agent_runs_by_project(
    preferred: &str,
    agent_run_id: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut found = Vec::new();
    let mut projects = vec![preferred.to_string()];
    projects.extend(indexed_projects()?);
    for project in projects {
        if project.trim().is_empty() || !seen.insert(project_key(&project)) {
            continue;
        }
        if let Some(snapshot) = latest_in_project(&project, agent_run_id)? {
            found.push(snapshot);
        }
    }
    Ok(found)
}

pub fn turns_for_handle(
    project_path: &str,
    agent_run_id: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    let mut turns: Vec<_> = list_operations(project_path)?
        .into_iter()
        .filter(|snapshot| {
            snapshot.agent_run_id == agent_run_id
                || (snapshot.agent_run_id.is_empty() && snapshot.operation_id == agent_run_id)
        })
        .collect();
    turns.sort_by_key(|snapshot| (snapshot.turn_index, snapshot.created_at_ms));
    Ok(turns)
}

fn latest_in_project(
    project_path: &str,
    agent_run_id: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    Ok(turns_for_handle(project_path, agent_run_id)?
        .into_iter()
        .max_by_key(|snapshot| (snapshot.turn_index, snapshot.created_at_ms)))
}

/// Find the active operation bound to a task id in the supplied project.
pub fn operation_for_task(
    project: &str,
    task_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let dir = root(project)?;
    let entries = fs::read_dir(dir).map_err(OperationError::io)?;
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = match read(&entry.path()) {
                Ok(operation) => operation,
                Err(error) if error.code == "OPERATION_NOT_FOUND" => continue,
                Err(error) => return Err(error),
            };
            if op.task_id.as_deref() == Some(task_id) {
                return Ok(op);
            }
        }
    }
    Err(OperationError::new(
        "OPERATION_NOT_FOUND",
        "no operation is linked to this task",
        false,
    ))
}

pub fn operation_for_task_any_project(task_id: &str) -> Result<OperationSnapshot, OperationError> {
    let index = task_index_path()?;
    let bytes = fs::read(index).map_err(OperationError::io)?;
    let entries: HashMap<String, (String, String)> =
        serde_json::from_slice(&bytes).map_err(OperationError::io)?;
    let (project, operation_id) = entries.get(task_id).ok_or_else(|| {
        OperationError::new(
            "OPERATION_NOT_FOUND",
            "no operation is linked to this task",
            false,
        )
    })?;
    get_operation(project, operation_id)
}

/// Return the active operation currently assigned to a pane, if any.
pub fn active_operation_for_pane(
    project: &str,
    pane_id: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OperationError::io(e)),
    };
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = read(&entry.path())?;
            if op.pane_id.as_deref() == Some(pane_id) && !op.status.terminal() {
                return Ok(Some(op));
            }
        }
    }
    Ok(None)
}

/// Active occupant of `pane_id` other than `operation_id`, if any.
///
/// `create_operation` stores a requested `pane_id` before dispatch, so the
/// current operation would otherwise look busy to itself. Scan every record:
/// first-match-then-skip-self misses a real occupant listed after `self`.
pub fn conflicting_pane_operation(
    project: &str,
    pane_id: &str,
    operation_id: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OperationError::io(e)),
    };
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = read(&entry.path())?;
            if op.pane_id.as_deref() == Some(pane_id)
                && !op.status.terminal()
                && op.operation_id != operation_id
            {
                return Ok(Some(op));
            }
        }
    }
    Ok(None)
}
