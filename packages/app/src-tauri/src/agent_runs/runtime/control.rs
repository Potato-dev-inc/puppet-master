//! Stopping runs: interrupt, cancel and close, including the native OpenCode abort and the
//! per-turn bookkeeping that records an interrupted turn.

use super::{
    access::{current_agent, current_agent_for_read},
    handle_lock,
    view::AgentRunView,
};
use crate::agent_runs::messaging::{self};
use crate::agent_runs::persist::{self};
use crate::agent_runs::transcript;
use crate::operations::{self, OperationError, OperationSnapshot};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub fn cancel_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> Result<AgentRunView, OperationError> {
    let view = interrupt_agent(project, handle, session_id, registry.clone(), app)?;
    // cancel_agent means stop and release: a pane this run created for itself (keep_pane=false) is
    // closed once the turn is stopped. interrupt_agent leaves it open so the worker can be resumed.
    // The caller just stopped this run, so a read-level lookup is enough (and cannot be refused
    // because the interrupt already changed the worker's control state).
    let Ok(snapshot) = current_agent_for_read(project, handle, session_id) else {
        return Ok(view);
    };
    // `keep_pane` itself is forced on by the interrupt, so go by what the caller asked for.
    let released = snapshot.pane_created && snapshot.keep_pane_requested == Some(false);
    if snapshot.pane_id.is_none() || !released {
        return Ok(view);
    }
    if crate::agent_runs::pane_close::dispose_explicit_close(&snapshot, &registry).is_err() {
        return Ok(view);
    }
    Ok(AgentRunView::present(snapshot, Some(&registry)))
}

pub fn interrupt_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> Result<AgentRunView, OperationError> {
    interrupt_active_turns(
        project,
        handle,
        session_id,
        |snapshot| request_interrupt(snapshot, &registry, &app),
        |interrupted| crate::bridge::publish_operation(interrupted, &registry, &app),
    )
}

pub(in crate::agent_runs) fn interrupt_active_turns(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    mut request: impl FnMut(&OperationSnapshot) -> Result<(), OperationError>,
    mut publish: impl FnMut(&OperationSnapshot) -> Result<(), OperationError>,
) -> Result<AgentRunView, OperationError> {
    let handle_guard = handle_lock(handle);
    let _serial = handle_guard.lock();
    let latest = current_agent(project, handle, session_id)?;
    let project = latest.project_path.as_str();
    let turns = active_turns(operations::list_operations(project)?, handle);
    for snapshot in turns {
        request(&snapshot)?;
        let partial = snapshot.result.clone().or_else(|| {
            Some(
                if crate::agent_runs::headless::blocked_on_trust(&snapshot) {
                    // Nothing ran: the run was parked on the trust prompt and is simply closed.
                    "closed while waiting for workspace trust; nothing was run".into()
                } else {
                    "turn interrupted before a final response was recorded".into()
                },
            )
        });
        transcript::append(
            project,
            &snapshot.operation_id,
            "interrupted",
            &crate::redact::text(partial.as_deref().unwrap_or("interrupted")),
        );
        let interrupted =
            operations::mark_turn_interrupted(project, &snapshot.operation_id, partial)?;
        publish(&interrupted)?;
    }
    let final_snapshot = current_agent(project, handle, session_id)?;
    Ok(final_snapshot.into())
}

/// Warning code recorded when a stop could not reach the OpenCode server.
const OPENCODE_SERVER_UNREACHABLE: &str = "OPENCODE_SERVER_UNREACHABLE";

/// Outcome of asking OpenCode to abort a session. A server that cannot be reached has nothing
/// left to abort, so the stop still goes ahead (the operation is finalised, locks and pane are
/// released) and the unconfirmed abort is recorded as a warning. Any other failure is an error.
pub(in crate::agent_runs) fn settle_native_abort(
    project: &str,
    operation_id: &str,
    abort: Result<(), String>,
) -> Result<(), OperationError> {
    match abort {
        Ok(()) => Ok(()),
        Err(error) if crate::opencode::client::is_unreachable_error(&error) => {
            let _ = operations::add_warning(
                project,
                operation_id,
                OPENCODE_SERVER_UNREACHABLE,
                &format!(
                    "the OpenCode server could not be reached, so the abort was not delivered; the run was stopped on the Puppet Master side only ({error})"
                ),
            );
            Ok(())
        }
        Err(error) => Err(OperationError::new("INTERRUPT_FAILED", error, true)),
    }
}

fn request_interrupt(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) -> Result<(), OperationError> {
    match messaging::interrupt_action(snapshot) {
        Some(messaging::InterruptAction::AbortNative) => {
            let Some(pane_id) = snapshot.pane_id.as_deref() else {
                return Ok(());
            };
            let link = {
                let guard = registry.lock();
                guard.panes.get(pane_id).and_then(|pane| {
                    pane.opencode.as_ref().map(|link| {
                        (
                            link.base_url.clone(),
                            link.session_id.clone(),
                            link.directory.clone(),
                        )
                    })
                })
            };
            if let Some((base, session, directory)) = link {
                let abort =
                    crate::opencode::client::abort_session(&base, &session, Some(&directory));
                let reached = abort.is_ok();
                settle_native_abort(&snapshot.project_path, &snapshot.operation_id, abort)?;
                if reached {
                    let _ = crate::opencode::status::wait_until_native_accepts_prompt(
                        registry,
                        pane_id,
                        Duration::from_secs(5),
                    );
                }
            }
            Ok(())
        }
        Some(messaging::InterruptAction::StopHeadless) => {
            match crate::agent_runs::headless::cancel_headless(snapshot) {
                Ok(()) => Ok(()),
                Err(error) if error.code == "WORKER_NOT_FOUND" => Ok(()),
                Err(error) => Err(error),
            }
        }
        Some(messaging::InterruptAction::SignalTui) => {
            let Some(pane_id) = snapshot.pane_id.as_deref() else {
                return Ok(());
            };
            let sequence = crate::pty::keys::sequence("ctrl+c")
                .map_err(|error| OperationError::new("INTERRUPT_FAILED", error, false))?;
            crate::pty::registry_write_input(registry, app, pane_id, &sequence, false, false, None, crate::pty::WriteOrigin::Nudge)
                .map_err(|error| OperationError::new("INTERRUPT_FAILED", error, true))?;
            Ok(())
        }
        None => Ok(()),
    }
}

pub fn inspect_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<serde_json::Value, OperationError> {
    let snapshot = current_agent_for_read(project, handle, session_id)?;
    let siblings = operations::list_operations(&snapshot.project_path).unwrap_or_default();
    let view = AgentRunView::from(snapshot.clone());
    serde_json::to_value(view)
        .map(|mut value| {
            persist::apply_inspect_fields(&mut value, &snapshot, &siblings);
            value
        })
        .map_err(|error| OperationError::new("SERIALIZATION_ERROR", error.to_string(), false))
}

pub fn close_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> Result<AgentRunView, OperationError> {
    let latest = current_agent(project, handle, session_id)?;
    let project = latest.project_path.as_str();
    let view = interrupt_agent(project, handle, session_id, registry.clone(), app.clone())?;
    if let Ok(mut latest) = operations::resolve_agent_run(project, handle) {
        // Only the turn that spawned the pane has `pane_created`; follow-up turns reuse it. Look
        // across the run's turns so close_agent still closes a pane the run itself created.
        if !latest.pane_created {
            let spawned_here = operations::list_operations(project)
                .unwrap_or_default()
                .iter()
                .any(|turn| {
                    turn.pane_created
                        && turn.pane_id.is_some()
                        && turn.pane_id == latest.pane_id
                        && (turn.agent_run_id == latest.agent_run_id
                            || turn.operation_id == latest.operation_id)
                });
            latest.pane_created = spawned_here;
        }
        crate::agent_runs::pane_close::dispose_explicit_close(&latest, &registry)?;
    }
    if let Some(session) = session_id {
        let _ = crate::mcp_sessions::release_owned_agent(session, handle);
    }
    operations::mark_worker_closed(project, handle)
        .map(AgentRunView::from)
        .or(Ok(view))
}

pub(in crate::agent_runs) fn active_turns(
    snapshots: Vec<OperationSnapshot>,
    handle: &str,
) -> Vec<OperationSnapshot> {
    let mut active = snapshots
        .into_iter()
        .filter(|snapshot| {
            !snapshot.status.terminal()
                && (snapshot.agent_run_id == handle
                    || (snapshot.agent_run_id.is_empty() && snapshot.operation_id == handle))
        })
        .collect::<Vec<_>>();
    active.sort_by_key(|snapshot| {
        (
            std::cmp::Reverse(snapshot.turn_index),
            std::cmp::Reverse(snapshot.created_at_ms),
        )
    });
    active
}
