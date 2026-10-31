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
            // A run parked at a usage limit stays parked while the limit is on screen.
            .or_else(|| crate::handoff::active_limit_action(pane_id))
    };
    // The status can turn `waiting_input` a moment before the dialog is fully drawn (the question line is there, its
    // options are still arriving): nothing is recognised yet. Recording the id-less manual fallback right then showed a
    // run that `needs_input` with no prompt id, and it stayed that way (no later status event re-reads the screen). So
    // a waiting pane with nothing recognised is given a moment to finish drawing before the fallback is recorded.
    if native_action.is_none() && screen_action.is_none() && status == "waiting_input" && pane.agent_type != "opencode_native" {
        let (registry, pane_id, project_text, operation_id) =
            (registry.clone(), pane_id.to_string(), project_text.to_string(), operation.operation_id.clone());
        std::thread::spawn(move || {
            let look = || {
                let still_waiting = registry
                    .lock()
                    .panes
                    .get(&pane_id)
                    .is_some_and(|pane| pane.status.lock().as_str() == "waiting_input");
                if !still_waiting {
                    return Look::Moved;
                }
                match crate::pane_prompt::required_action(&registry, &pane_id).or_else(|| crate::handoff::active_limit_action(&pane_id)) {
                    Some(action) => Look::Prompt(action),
                    None => Look::Nothing,
                }
            };
            match settle(look, SETTLE_LOOKS, || std::thread::sleep(SETTLE_STEP)) {
                Look::Moved => {}
                Look::Prompt(action) => record_with(&project_text, &operation_id, "waiting_input", Some(action), crate::operations::StateSource::Inferred),
                Look::Nothing => record_with(&project_text, &operation_id, "waiting_input", Some(manual_action(&pane_id)), crate::operations::StateSource::Inferred),
            }
        });
        return;
    }
    let observed_state = if native_action.is_some() || screen_action.is_some() {
        "waiting_input"
    } else {
        status
    };
    let action = native_action.or(screen_action).or_else(|| (status == "waiting_input").then(|| manual_action(pane_id)));
    record_with(
        &project_text,
        &operation.operation_id,
        observed_state,
        action,
        if native_status.is_some() {
            crate::operations::StateSource::Native
        } else {
            crate::operations::StateSource::Inferred
        },
    );
}

/// How many times a waiting pane with nothing recognised is looked at again, and the pause between looks.
const SETTLE_LOOKS: u32 = 15;
const SETTLE_STEP: std::time::Duration = std::time::Duration::from_millis(100);

/// What one look at a waiting pane found.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Look {
    /// A prompt Puppet Master recognises (its id and options).
    Prompt(serde_json::Value),
    /// The pane still waits and nothing on it is recognised.
    Nothing,
    /// The pane is not waiting any more (a later status event records whatever it is now).
    Moved,
}

/// Look until a prompt shows, the pane stops waiting, or `looks` looks have found nothing (then `Nothing`: the prompt is
/// not one Puppet Master reads and a person has to).
pub(super) fn settle(mut look: impl FnMut() -> Look, looks: u32, mut pause: impl FnMut()) -> Look {
    for n in 0..looks.max(1) {
        if n > 0 {
            pause();
        }
        match look() {
            Look::Nothing => {}
            found => return found,
        }
    }
    Look::Nothing
}

/// The fallback for a waiting pane no profile and no menu shape reads. It carries no prompt id on purpose.
fn manual_action(pane_id: &str) -> serde_json::Value {
    json!({
        "kind": "manual_input_required",
        "pane_id": pane_id,
        "detail": "Inspect the pane and resolve the prompt manually. No permission was approved automatically."
    })
}

fn record_with(
    project_text: &str,
    operation_id: &str,
    observed_state: &str,
    action: Option<serde_json::Value>,
    source: crate::operations::StateSource,
) {
    if let Ok(snapshot) = crate::operations::mark_operation_observation(
        project_text,
        operation_id,
        Some(observed_state.to_string()),
        action,
        source,
    ) {
        push_operation_sse(&snapshot);
    }
}

#[cfg(test)]
mod settle_tests {
    use super::*;

    #[test]
    fn a_dialog_that_finishes_drawing_is_taken_not_the_id_less_fallback() {
        let mut looks = vec![Look::Nothing, Look::Nothing, Look::Prompt(json!({"id": "prompt-1"}))].into_iter();
        let mut pauses = 0;
        let got = settle(|| looks.next().unwrap_or(Look::Nothing), 15, || pauses += 1);
        assert_eq!(got, Look::Prompt(json!({"id": "prompt-1"})));
        assert_eq!(pauses, 2, "it looked again only while nothing was there");
    }

    #[test]
    fn a_prompt_nobody_reads_ends_as_nothing_after_the_last_look() {
        let mut seen = 0;
        let got = settle(|| { seen += 1; Look::Nothing }, 5, || {});
        assert_eq!((got, seen), (Look::Nothing, 5));
    }

    #[test]
    fn a_pane_that_stopped_waiting_is_left_to_its_next_status_event() {
        let mut looks = vec![Look::Nothing, Look::Moved].into_iter();
        assert_eq!(settle(|| looks.next().unwrap_or(Look::Nothing), 15, || {}), Look::Moved);
    }
}

/// What checkpoints need to know about a pane (folder, agent, status).
pub(crate) fn checkpoint_pane_facts(pane_id: &str) -> Option<crate::checkpoints::PaneFacts> {
    let registry = BRIDGE_REGISTRY.get()?;
    let guard = registry.lock();
    guard.panes.get(pane_id).map(pane_facts)
}

/// Folder, agent and LIVE status of a pane. `PaneState::info` is a spawn-time snapshot whose
/// `status` stays `"running"` forever; the real status lives behind `PaneState::status`.
pub(crate) fn pane_facts(pane: &crate::pty::registry::PaneState) -> crate::checkpoints::PaneFacts {
    crate::checkpoints::PaneFacts {
        cwd: pane.info.cwd.clone(),
        agent_type: pane.info.agent_type.clone(),
        status: pane.status.lock().as_str().to_string(),
    }
}

/// Whether an agent run (operation) is still open on this pane.
pub(crate) fn pane_has_open_run(pane_id: &str, cwd: &str) -> bool {
    let Ok(project) = crate::project_path::normalize_project_path(std::path::Path::new(cwd)) else {
        return false;
    };
    matches!(
        crate::operations::active_operation_for_pane(&project.to_string_lossy(), pane_id),
        Ok(Some(_))
    )
}

/// The public settings snapshot (`checkpoints_enabled`, `checkpoints_keep`), when the app is up.
pub(crate) fn checkpoint_public_settings() -> Option<serde_json::Value> {
    let app = super::state::BRIDGE_APP.get()?;
    Some(crate::settings_store::read_public_settings(app))
}

/// The pane registry the bridge serves, when the app is up (turn summaries and the away digest).
pub(crate) fn live_registry() -> Option<std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>> {
    BRIDGE_REGISTRY.get().cloned()
}

/// The app handle, when the app is up (Tauri events from background threads).
pub(crate) fn live_app() -> Option<tauri::AppHandle> {
    super::state::BRIDGE_APP.get().cloned()
}
