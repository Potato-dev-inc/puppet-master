//! Central policy for when a run may close or kill a worker pane.

use crate::operations::{OperationError, OperationSnapshot};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

/// `pane_created` on the operation — true only when dispatch spawned this pane for the run.
pub fn spawned_by_run(snapshot: &OperationSnapshot) -> bool {
    snapshot.pane_created
}

pub fn should_dispose_after_terminal(snapshot: &OperationSnapshot) -> bool {
    spawned_by_run(snapshot) && !snapshot.keep_pane
}

fn pane_busy(
    project: &str,
    pane_id: &str,
    excluding_operation_id: &str,
) -> Result<bool, OperationError> {
    Ok(crate::operations::conflicting_pane_operation(project, pane_id, excluding_operation_id)?
        .is_some())
}

/// Automatic cleanup after a terminal operation (completion, failure, cancel publish).
pub fn maybe_dispose_after_terminal(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<(), OperationError> {
    if !should_dispose_after_terminal(snapshot) {
        return Ok(());
    }
    let pane_id = snapshot
        .pane_id
        .as_deref()
        .ok_or_else(|| OperationError::new("PANE_CLEANUP_FAILED", "missing pane_id", true))?;
    if !registry.lock().panes.contains_key(pane_id) {
        return Ok(());
    }
    if pane_busy(&snapshot.project_path, pane_id, &snapshot.operation_id)? {
        tracing::info!(
            pane_id,
            operation_id = %snapshot.operation_id,
            "skipping automatic pane dispose because pane is busy"
        );
        return Ok(());
    }
    crate::pty::registry_kill_pane_with_reason(
        registry,
        pane_id,
        Some("operation_terminal_cleanup"),
    )
    .map_err(|error| OperationError::new("PANE_CLEANUP_FAILED", error, true))?;
    Ok(())
}

/// Dispatch failed after a pane was spawned for this operation.
pub fn maybe_dispose_on_dispatch_failure(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<(), OperationError> {
    if !spawned_by_run(snapshot) {
        return Ok(());
    }
    let pane_id = match snapshot.pane_id.as_deref() {
        Some(id) => id,
        None => return Ok(()),
    };
    if pane_busy(&snapshot.project_path, pane_id, &snapshot.operation_id)? {
        tracing::info!(
            pane_id,
            operation_id = %snapshot.operation_id,
            "skipping dispatch-failure pane dispose because pane is busy"
        );
        return Ok(());
    }
    crate::pty::registry_kill_pane_with_reason(
        registry,
        pane_id,
        Some("dispatch_failed_cleanup"),
    )
    .map_err(|error| OperationError::new("PANE_CLEANUP_FAILED", error, true))?;
    Ok(())
}

/// Explicit `close_agent` dispose — only spawned panes may be killed.
pub fn dispose_explicit_close(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<(), OperationError> {
    if !spawned_by_run(snapshot) {
        return Ok(());
    }
    let pane_id = snapshot
        .pane_id
        .as_deref()
        .ok_or_else(|| OperationError::new("PANE_NOT_FOUND", "missing pane_id", false))?;
    let _ = crate::pty::registry_kill_pane_with_reason(
        registry,
        pane_id,
        Some("close_agent"),
    );
    Ok(())
}

/// Worker stop for cancel / timeout — never kill adopted panes; optional kill for spawned.
pub fn stop_worker_control(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    allow_kill_spawned_pane: bool,
) -> Result<(), OperationError> {
    let pane_id = snapshot
        .pane_id
        .as_deref()
        .ok_or_else(|| OperationError::new("PANE_NOT_FOUND", "missing pane_id", false))?;
    if snapshot.agent_type == "opencode_native" {
        let link = {
            let guard = registry.lock();
            guard
                .panes
                .get(pane_id)
                .and_then(|pane| pane.opencode.as_ref())
                .map(|link| {
                    (
                        link.base_url.clone(),
                        link.session_id.clone(),
                        link.directory.clone(),
                    )
                })
        }
        .ok_or_else(|| {
            OperationError::new(
                "CANCEL_FAILED",
                "native session control is unavailable",
                true,
            )
        })?;
        crate::opencode::client::abort_session(&link.0, &link.1, Some(&link.2))
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, true))?;
    }
    if spawned_by_run(snapshot) && allow_kill_spawned_pane {
        if !pane_busy(&snapshot.project_path, pane_id, &snapshot.operation_id)? {
            crate::pty::registry_kill_pane_with_reason(
                registry,
                pane_id,
                Some("cancel_kill_spawned_pane"),
            )
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, true))?;
        }
    } else if snapshot.agent_type != "opencode_native" {
        let sequence = crate::pty::keys::sequence("ctrl+c")
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, false))?;
        crate::pty::registry_write_input(registry, app, pane_id, &sequence, false, false, None)
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, true))?;
    }
    Ok(())
}

pub fn bound_pane_missing_error(pane_id: &str) -> OperationError {
    let mut error = OperationError::new(
        "PANE_GONE",
        format!(
            "bound pane {} is no longer in the worker registry; the pane was closed or the app restarted",
            pane_id
        ),
        true,
    );
    error.context = serde_json::json!({
        "pane_id": pane_id,
        "recovery": "call list_agents or list_workers and bind a live pane_id or worker_id; do not assume the old pane_id still exists",
    });
    error
}

/// Error for a pane this app closed on its own when a run finished with `keep_pane=false`.
pub fn pane_closed_after_run_error(pane_id: &str, handle: &str) -> OperationError {
    let mut error = OperationError::new(
        "PANE_CLOSED_AFTER_RUN",
        format!(
            "pane {pane_id} was closed automatically when run {handle} finished because it was started with keep_pane=false (the default for panes the run created). take_over, switch_agent_model and live pane inspection need a live pane: start the run with keep_pane=true (and call close_agent when done). agent_transcript and inspect_agent still work for handle {handle} from the stored transcript."
        ),
        false,
    );
    error.context = serde_json::json!({
        "pane_id": pane_id,
        "handle": handle,
        "reason": "keep_pane_false",
        "recovery": "re-run with keep_pane=true; read output with agent_transcript",
    });
    error
}

/// Pure check: is `pane_id` absent because a terminal run with `keep_pane=false` disposed it?
pub fn closed_after_run_in(
    snapshots: &[OperationSnapshot],
    pane_id: &str,
    pane_alive: bool,
) -> Option<OperationError> {
    if pane_alive {
        return None;
    }
    snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.pane_id.as_deref() == Some(pane_id)
                && snapshot.status.terminal()
                && should_dispose_after_terminal(snapshot)
        })
        .max_by_key(|snapshot| snapshot.turn_index)
        .map(|snapshot| {
            let handle = if snapshot.agent_run_id.is_empty() {
                &snapshot.operation_id
            } else {
                &snapshot.agent_run_id
            };
            pane_closed_after_run_error(pane_id, handle)
        })
}

/// Registry-backed variant used by routes that need a live pane.
pub fn closed_after_run_error(
    project: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Option<OperationError> {
    let alive = registry.lock().panes.contains_key(pane_id);
    if alive {
        return None;
    }
    let snapshots = crate::operations::list_operations(project).ok()?;
    closed_after_run_in(&snapshots, pane_id, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{DelegateWorkRequest, OperationStatus, WorkerPersist};
    use crate::pty::PaneRegistry;
    use std::sync::Arc;

    fn snapshot_with_pane(
        project: &str,
        pane_id: &str,
        pane_created: bool,
        keep_pane: bool,
    ) -> OperationSnapshot {
        std::fs::create_dir_all(project).expect("project dir");
        let req = DelegateWorkRequest {
            project_path: project.to_string(),
            task: "t".into(),
            agent_type: "opencode_native".into(),
            pane_id: Some(pane_id.to_string()),
            idempotency_key: "k".into(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some("handle".into()),
            turn_index: 0,
            worker: WorkerPersist::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        let (mut op, _) = crate::operations::create_operation(req).unwrap();
        op.pane_created = pane_created;
        op.status = OperationStatus::Completed;
        op
    }

    #[test]
    fn automatic_dispose_skips_adopted_pane_even_when_keep_pane_false() {
        let project = std::env::temp_dir()
            .join(format!("pm-wp9-adopted-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let pane_id = "ui-pane";
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        registry
            .lock()
            .panes
            .insert(pane_id.to_string(), PaneRegistry::test_pane_stub(pane_id));
        let snapshot = snapshot_with_pane(&project, pane_id, false, false);
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        assert!(
            registry.lock().panes.contains_key(pane_id),
            "adopted pane must survive terminal cleanup"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn automatic_dispose_kills_idle_spawned_pane_when_keep_false() {
        let project = std::env::temp_dir()
            .join(format!("pm-wp9-spawned-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let pane_id = "spawned-pane";
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        registry
            .lock()
            .panes
            .insert(pane_id.to_string(), PaneRegistry::test_pane_stub(pane_id));
        let snapshot = snapshot_with_pane(&project, pane_id, true, false);
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        assert!(
            !registry.lock().panes.contains_key(pane_id),
            "spawned pane should be disposed when keep_pane is false"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn spawned_by_run_tracks_pane_created() {
        let project = std::env::temp_dir()
            .join(format!("pm-wp9-track-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let snap = snapshot_with_pane(&project, "p", true, false);
        assert!(spawned_by_run(&snap));
        let adopted = snapshot_with_pane(&project, "p", false, false);
        assert!(!spawned_by_run(&adopted));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn bound_pane_missing_error_is_recoverable_pane_gone() {
        let err = bound_pane_missing_error("pane-abc");
        assert_eq!(err.code, "PANE_GONE");
        assert!(err.recoverable);
    }

    #[test]
    fn closed_after_run_reports_pane_closed_after_run_only_for_disposed_panes() {
        let project = std::env::temp_dir()
            .join(format!("pm-closed-after-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let spawned = snapshot_with_pane(&project, "p1", true, false);
        let err = closed_after_run_in(&[spawned.clone()], "p1", false).expect("closed after run");
        assert_eq!(err.code, "PANE_CLOSED_AFTER_RUN");
        assert!(err.message.contains("keep_pane=true"));
        assert!(err.message.contains("agent_transcript"));
        assert!(closed_after_run_in(&[spawned], "p1", true).is_none());
        let kept = snapshot_with_pane(&format!("{project}-kept"), "p1", true, true);
        assert!(closed_after_run_in(&[kept], "p1", false).is_none());
        let adopted = snapshot_with_pane(&format!("{project}-adopted"), "p1", false, false);
        assert!(closed_after_run_in(&[adopted], "p1", false).is_none());
        for suffix in ["", "-kept", "-adopted"] {
            let _ = std::fs::remove_dir_all(format!("{project}{suffix}"));
        }
    }
}
