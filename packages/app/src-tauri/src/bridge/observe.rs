//! Reacts to pane status changes for the operation that owns the pane: records errors,
//! unhealthy native workers and pending prompts on the operation and broadcasts the update.

use serde_json::json;

use super::locks::release_operation_locks;
use super::sse::push_operation_sse;
use super::state::BRIDGE_REGISTRY;

pub(super) fn observe_operation_pane_status(pane_id: &str, status: &str) {
    let Some(registry) = BRIDGE_REGISTRY.get() else {
        return;
    };
    let pane = registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id);
    let Some(pane) = pane else {
        return;
    };
    let Ok(project) = crate::project_path::normalize_project_path(std::path::Path::new(&pane.cwd))
    else {
        return;
    };
    let project_text = project.to_string_lossy();
    let Ok(Some(operation)) = crate::operations::active_operation_for_pane(&project_text, pane_id)
    else {
        return;
    };
    if status == "error" {
        let error = crate::operations::OperationError::new(
            "PANE_ERROR",
            "worker pane entered an error state before task completion",
            true,
        );
        if let Ok(snapshot) = crate::operations::mark_operation_state(
            &project_text,
            &operation.operation_id,
            crate::operations::OperationStatus::Failed,
            crate::operations::StateSource::Inferred,
            Some("pane_error".into()),
            None,
            Some(error),
        ) {
            let _ = release_operation_locks(&snapshot);
            push_operation_sse(&snapshot);
        }
        return;
    }
    let native_status = (pane.agent_type == "opencode_native")
        .then(|| crate::opencode::status::worker_status(registry, pane_id).ok())
        .flatten();
    if native_status
        .as_ref()
        .is_some_and(|state| !state.serve_healthy)
    {
        let error = crate::operations::OperationError::new(
            "WORKER_UNHEALTHY",
            "native worker service is unhealthy",
            true,
        );
        if let Ok(snapshot) = crate::operations::mark_operation_state(
            &project_text,
            &operation.operation_id,
            crate::operations::OperationStatus::Failed,
            crate::operations::StateSource::Native,
            Some("worker_unhealthy".into()),
            None,
            Some(error),
        ) {
            let _ = release_operation_locks(&snapshot);
            push_operation_sse(&snapshot);
        }
        return;
    }
    let native_action = native_status.as_ref().filter(|state| !state.pending_permission_ids.is_empty()).map(|state| json!({
        "kind":"permission_required", "pane_id":pane_id, "permission_ids":state.pending_permission_ids
    }));
    // A dialog the TUI profile recognises is blocking whatever the status heuristic says (status
    // trails the screen, and a run parked on it at startup must not be un-parked by a stale
    // "running"). It is recorded with its id and command straight away; the id-less manual
    // fallback is only for blocked panes whose prompt is not recognised.
    let screen_action = if pane.agent_type == "opencode_native" {
        None
    } else {
        crate::pane_prompt::required_action(registry, pane_id)
    };
    let observed_state = if native_action.is_some() || screen_action.is_some() {
        "waiting_input"
    } else {
        status
    };
    let action = native_action.or(screen_action).or_else(|| {
        (status == "waiting_input").then(|| {
            json!({
                "kind": "manual_input_required",
                "pane_id": pane_id,
                "detail": "Inspect the pane and resolve the prompt manually. No permission was approved automatically."
            })
        })
    });
    if let Ok(snapshot) = crate::operations::mark_operation_observation(
        &project_text,
        &operation.operation_id,
        Some(observed_state.to_string()),
        action,
        if native_status.is_some() {
            crate::operations::StateSource::Native
        } else {
            crate::operations::StateSource::Inferred
        },
    ) {
        push_operation_sse(&snapshot);
    }
}

/// What checkpoints need to know about a pane (folder, agent, status).
pub(crate) fn checkpoint_pane_facts(pane_id: &str) -> Option<crate::checkpoints::PaneFacts> {
    let registry = BRIDGE_REGISTRY.get()?;
    let guard = registry.lock();
    let pane = guard.panes.get(pane_id)?;
    Some(crate::checkpoints::PaneFacts {
        cwd: pane.info.cwd.clone(),
        agent_type: pane.info.agent_type.clone(),
        status: pane.info.status.clone(),
    })
}

/// The public settings snapshot (`checkpoints_enabled`, `checkpoints_keep`), when the app is up.
pub(crate) fn checkpoint_public_settings() -> Option<serde_json::Value> {
    let app = super::state::BRIDGE_APP.get()?;
    Some(crate::settings_store::read_public_settings(app))
}
