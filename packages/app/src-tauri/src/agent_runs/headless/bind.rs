//! Adopting an existing worker (persisted run or live pane) into a new run request, with the
//! workspace and backend consistency checks that go with it.

use crate::agent_runs::persist;
use crate::agent_runs::runtime::{self, AgentRunRequest};
use crate::operations::OperationError;
use parking_lot::Mutex;
use std::sync::Arc;

fn workspace_conflict(supplied: &str, worker: &str) -> bool {
    let supplied_path = std::path::Path::new(supplied);
    let worker_path = std::path::Path::new(worker);
    !crate::project_path::workspace_covers(supplied_path, worker_path)
        && !crate::project_path::workspace_covers(worker_path, supplied_path)
}

fn workspace_mismatch(expected: &str, supplied: &str) -> OperationError {
    let mut error = OperationError::new(
        "WORKSPACE_MISMATCH",
        format!("worker workspace is {expected}; supplied {supplied}"),
        false,
    );
    error.context = serde_json::json!({
        "expected": expected,
        "supplied": supplied,
    });
    error
}

pub(super) fn bind_existing_worker(
    request: &mut AgentRunRequest,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    session_id: Option<&str>,
    explicit_project: Option<&str>,
) -> Result<(), OperationError> {
    let pane_ref = request
        .pane_id
        .clone()
        .or_else(|| request.worker_id.clone())
        .or_else(|| request.handle.clone());
    let Some(pane_ref) = pane_ref else {
        return Ok(());
    };
    let live_pane_ids: Vec<String> = registry
        .lock()
        .list()
        .into_iter()
        .map(|pane| pane.id)
        .collect();
    let persisted = worker_lookup_key(request, &pane_ref, &live_pane_ids).and_then(|key| {
        persist::find_worker(request.project_path.as_deref().unwrap_or_default(), key).ok()
    });
    if let Some(existing) = persisted {
        let worker_dir = existing
            .worker
            .workspace
            .as_deref()
            .unwrap_or(&existing.project_path);
        if let Some(supplied) = explicit_project {
            if workspace_conflict(supplied, worker_dir) {
                return Err(workspace_mismatch(worker_dir, supplied));
            }
        }
        request.handle = Some(existing.agent_run_id.clone());
        request.agent_run_id = Some(existing.agent_run_id.clone());
        request.agent_type = existing.agent_type.clone();
        request.pane_id = existing.pane_id.clone().or(request.pane_id.clone());
        request.project_path = Some(worker_dir.to_string());
        request.headless = request.pane_id.is_none();
        if request.pane_id.is_some() {
            request.keep_pane = true;
        }
        if let (Some(session), Some(pane_id)) = (session_id, request.pane_id.as_deref()) {
            if find_live_pane(registry, pane_id).is_some() {
                crate::mcp_sessions::take_over_pane(session, pane_id, true)?;
            }
        }
        return Ok(());
    }
    let Some(pane) = find_live_pane(registry, &pane_ref) else {
        return Ok(());
    };
    if let Some(supplied) = explicit_project {
        if workspace_conflict(supplied, &pane.cwd) {
            return Err(workspace_mismatch(&pane.cwd, supplied));
        }
    }
    if request.agent_type != pane.agent_type && request.agent_type != runtime::default_agent() {
        let mut error = OperationError::new(
            "AGENT_TYPE_MISMATCH",
            format!(
                "worker backend is {}; requested {}",
                pane.agent_type, request.agent_type
            ),
            false,
        );
        error.context = serde_json::json!({
            "expected": pane.agent_type,
            "supplied": request.agent_type,
        });
        return Err(error);
    }
    let supplied_account = request.account.as_deref().map(str::trim).filter(|a| !a.is_empty());
    if supplied_account.is_some() && supplied_account != pane.account.as_deref() {
        let mut error = OperationError::new(
            "ACCOUNT_MISMATCH",
            "that pane runs under a different account; open a new pane for this account",
            false,
        );
        error.context = serde_json::json!({"pane_id": pane.id, "pane_account": pane.account, "supplied": supplied_account});
        return Err(error);
    }
    // The run takes the account the pane was started under.
    request.account = pane.account.clone();
    request.agent_type = pane.agent_type.clone();
    request.pane_id = Some(pane.id.clone());
    request.project_path = Some(pane.cwd.clone());
    request.headless = false;
    request.keep_pane = true;
    if request.read_only {
        return Err(OperationError::new(
            "READ_ONLY_UNSUPPORTED",
            "read-only mode cannot be enforced in a shared terminal pane",
            false,
        ));
    }
    if let Some(session) = session_id {
        crate::mcp_sessions::take_over_pane(session, &pane.id, true)?;
    }
    Ok(())
}

/// The key to look a persisted worker up by, or `None` when no worker can match.
///
/// A miss in `find_worker` scans every operation of every indexed project (0.7s here, and it grows
/// with history), so the cheap, certain cases skip it: a follow-up already names its handle, and a
/// reference that is exactly a live pane's id is a pane, never a handle or a worker name.
fn worker_lookup_key<'a>(
    request: &'a AgentRunRequest,
    pane_ref: &'a str,
    live_pane_ids: &[String],
) -> Option<&'a str> {
    if request.turn_index > 0 {
        if let Some(handle) = request.handle.as_deref().filter(|handle| !handle.is_empty()) {
            return Some(handle);
        }
    }
    if live_pane_ids.iter().any(|id| id == pane_ref) {
        return None;
    }
    Some(pane_ref)
}

fn find_live_pane(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    id: &str,
) -> Option<crate::pty::PaneInfo> {
    let panes = registry.lock().list();
    let ids: Vec<String> = panes.iter().map(|pane| pane.id.clone()).collect();
    let resolved = crate::mcp_sessions::resolve_pane_id(id, &ids).ok()?;
    panes
        .into_iter()
        .find(|pane| pane.id == resolved)
        .filter(|pane| !pane.id.starts_with("puppet-master-orchestrator-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::AgentRunRequest;
    use crate::operations::DelegateWorkRequest;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn persist_req(project: &str, handle: &str, turn: u32, task: &str) -> DelegateWorkRequest {
        DelegateWorkRequest {
            project_path: project.to_string(),
            task: task.into(),
            agent_type: "codex".into(),
            pane_id: None,
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            acceptance_criteria: Some(vec!["return a result".into()]),
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 900_000,
            read_only: false,
            keep_pane: false,
            placement: None,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some(handle.into()),
            turn_index: turn,
            worker: Default::default(),
            context_policy: None,
            checks: Vec::new(),
        }
    }

    fn request_with(value: serde_json::Value) -> AgentRunRequest {
        let mut base = serde_json::json!({"task": "t", "agent_type": "codex"});
        base.as_object_mut().unwrap().extend(value.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn worker_lookup_skips_the_all_projects_scan_when_the_answer_is_already_known() {
        let live = vec!["pane-live-1".to_string()];
        // A follow-up already carries its handle: look that up (one project), not the pane id.
        let mut follow_up = request_with(serde_json::json!({"pane_id": "pane-live-1", "handle": "luna"}));
        follow_up.turn_index = 1;
        assert_eq!(worker_lookup_key(&follow_up, "pane-live-1", &live), Some("luna"));
        // A first turn aimed at a live pane's exact id is a pane: no worker can be named that.
        let adopt = request_with(serde_json::json!({"pane_id": "pane-live-1"}));
        assert_eq!(worker_lookup_key(&adopt, "pane-live-1", &live), None);
        // A worker name or handle is still looked up, as is an id that is not (or no longer) live.
        let named = request_with(serde_json::json!({"worker_id": "luna"}));
        assert_eq!(worker_lookup_key(&named, "luna", &live), Some("luna"));
        let stale = request_with(serde_json::json!({"pane_id": "pane-gone"}));
        assert_eq!(worker_lookup_key(&stale, "pane-gone", &live), Some("pane-gone"));
    }

    #[test]
    fn bind_existing_worker_reuses_persisted_run_without_live_pane() {
        let project = std::env::temp_dir().join(format!("pm-bind-{}", uuid::Uuid::new_v4()));
        let project_path = project.to_string_lossy().into_owned();
        let mut request = persist_req(&project_path, "luna", 0, "first");
        request.agent_type = "opencode_native".into();
        request.pane_id = Some("pane-native".into());
        request.idempotency_key = "turn-0".into();
        request.worker.provider_session_id = Some("ses-1".into());
        let _ = crate::operations::create_operation(request).unwrap();
        let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::new()));
        let mut run: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "task": "continue",
            "agent_type": "codex",
            "worker_id": "luna",
            "project_path": project_path
        }))
        .unwrap();
        bind_existing_worker(&mut run, &registry, Some("fresh-session"), None).unwrap();
        assert_eq!(run.agent_run_id.as_deref(), Some("luna"));
        assert_eq!(run.agent_type, "opencode_native");
        assert_eq!(run.pane_id.as_deref(), Some("pane-native"));
        assert!(!run.headless);
        let _ = std::fs::remove_dir_all(project);
    }
}
