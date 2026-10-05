//! Operation lifecycle entry points: delegate (create + dispatch), cancel, publish terminal
//! state, and stopping a worker. The background dispatch itself lives in `dispatch`.

use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use tauri::AppHandle;

use super::dispatch::dispatch_existing_operation;
use super::errors::{operation_error_from_http, operation_http_error};
use super::locks::{fail_operation_on_lock_conflict, release_operation_locks};
use super::pane_util::emit_panes_changed;
use super::projects::resolve_project_root;
use super::sse::push_operation_sse;
use super::state::PANE_ASSIGNMENT_SERIAL;
use super::store::append_operation_event;
use crate::events::{SystemEvent, TaskId};
use crate::pty::PaneRegistry;

pub(super) fn operation_response_value(
    snapshot: crate::operations::OperationSnapshot,
) -> serde_json::Value {
    let project_path = snapshot.project_path.clone();
    let operation_id = snapshot.operation_id.clone();
    let mut value =
        serde_json::to_value(crate::operations::snapshot_for_api(snapshot)).unwrap_or(json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "watch_command".into(),
            json!(crate::watch_command::format_watch_command(
                &operation_id,
                Some(&project_path),
            )),
        );
    }
    value
}

pub(super) fn cleanup_failed_operation(
    operation: &crate::operations::OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) {
    let _ = crate::agent_runs::pane_close::maybe_dispose_on_dispatch_failure(operation, registry);
    if let Err(err) = release_operation_locks(operation) {
        tracing::warn!(%err, "failed to release operation locks");
    }
    if let Some(task_id) = operation.task_id.as_deref() {
        if let Err(err) = append_operation_event(
            &operation.project_path,
            SystemEvent::TaskStatusUpdated {
                task_id: TaskId(task_id.to_string()),
                status: "failed".into(),
            },
        ) {
            tracing::warn!(%err, "failed to record operation failure");
        }
    }
    emit_panes_changed(registry, app);
}

pub(super) fn delegate_operation_http(
    mut req: crate::operations::DelegateWorkRequest,
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
) -> Result<(u16, serde_json::Value), (u16, serde_json::Value)> {
    if req.task.trim().is_empty() {
        return Err(operation_http_error(
            crate::operations::OperationError::new("INVALID_TASK", "task must not be empty", false),
        ));
    }
    if req.idempotency_key.trim().is_empty() {
        return Err(operation_http_error(
            crate::operations::OperationError::new(
                "INVALID_IDEMPOTENCY_KEY",
                "idempotency_key must not be empty",
                false,
            ),
        ));
    }
    if req
        .acceptance_criteria
        .as_ref()
        .is_some_and(|items| items.iter().any(|item| item.trim().is_empty()))
    {
        return Err(operation_http_error(
            crate::operations::OperationError::new(
                "INVALID_ACCEPTANCE_CRITERIA",
                "acceptance_criteria entries must be non-empty when provided",
                false,
            ),
        ));
    }
    let project = if req.project_path.trim().is_empty() {
        resolve_project_root(&registry).ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "PROJECT_PATH_REQUIRED",
                "project_path is required when no project is selected",
                false,
            ))
        })?
    } else {
        crate::project_path::prepare_project_path(std::path::Path::new(&req.project_path)).map_err(
            |err| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_PROJECT_PATH",
                    err,
                    false,
                ))
            },
        )?
    };
    req.project_path = project.to_string_lossy().into_owned();
    if req.context_policy.is_none() {
        let prior = req
            .agent_run_id
            .as_deref()
            .and_then(|handle| {
                crate::operations::list_operations(&req.project_path)
                    .ok()
                    .map(|ops| {
                        ops.iter()
                            .filter(|op| {
                                op.agent_run_id == handle && op.turn_index < req.turn_index
                            })
                            .count()
                    })
            })
            .unwrap_or(0);
        req.context_policy = Some(crate::operations::applied_delegate_context_policy(
            None,
            req.turn_index,
            prior,
        ));
    }
    let (snapshot, created) =
        crate::operations::create_operation(req).map_err(operation_http_error)?;
    if let Some(session) = snapshot.owner_session_id.as_deref() {
        crate::mcp_sessions::register_owned_agent(session, &snapshot.operation_id)
            .map_err(operation_http_error)?;
        if snapshot.agent_run_id != snapshot.operation_id {
            crate::mcp_sessions::register_owned_agent(session, &snapshot.agent_run_id)
                .map_err(operation_http_error)?;
        }
    }
    if created {
        if let Err(error) = fail_operation_on_lock_conflict(&snapshot) {
            let _ = publish_operation(
                &crate::operations::get_operation(&snapshot.project_path, &snapshot.operation_id)
                    .unwrap_or_else(|_| snapshot.clone()),
                &registry,
                &app,
            );
            return Err(operation_http_error(error));
        }
        dispatch_existing_operation(
            &registry,
            &app,
            &snapshot.project_path,
            &snapshot.operation_id,
        )
        .map_err(operation_http_error)?;
    }
    push_operation_sse(&snapshot);
    Ok((202, operation_response_value(snapshot)))
}

pub(super) fn cancel_operation_http(
    project_text: String,
    id: &str,
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
) -> Result<(u16, serde_json::Value), (u16, serde_json::Value)> {
    let registry_for_cancel = registry.clone();
    let app_for_cancel = app.clone();
    let snapshot = crate::operations::cancel_operation_with(&project_text, id, move |operation| {
        stop_operation_worker(&registry_for_cancel, &app_for_cancel, operation)
    })
    .map_err(operation_http_error)?;
    if snapshot.status == crate::operations::OperationStatus::Cancelled {
        if let Some(task_id) = snapshot.task_id.as_deref() {
            append_operation_event(
                &snapshot.project_path,
                SystemEvent::TaskStatusUpdated {
                    task_id: TaskId(task_id.to_string()),
                    status: "cancelled".into(),
                },
            )
            .map_err(|err| {
                operation_http_error(crate::operations::OperationError::new(
                    "EVENT_WRITE_FAILED",
                    err,
                    true,
                ))
            })?;
        }
        release_operation_locks(&snapshot).map_err(|err| {
            operation_http_error(crate::operations::OperationError::new(
                "LOCK_RELEASE_FAILED",
                err,
                true,
            ))
        })?;
    }
    push_operation_sse(&snapshot);
    Ok((200, operation_response_value(snapshot)))
}

pub fn delegate_operation(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    req: crate::operations::DelegateWorkRequest,
) -> Result<crate::operations::OperationSnapshot, crate::operations::OperationError> {
    let (_, value) = delegate_operation_http(req, registry.clone(), app.clone())
        .map_err(operation_error_from_http)?;
    serde_json::from_value(value).map_err(|error| {
        crate::operations::OperationError::new("INVALID_OPERATION_RESULT", error.to_string(), false)
    })
}

pub fn publish_operation(
    snapshot: &crate::operations::OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) -> Result<(), crate::operations::OperationError> {
    use crate::operations::{OperationError, OperationStatus};
    if matches!(
        snapshot.status,
        OperationStatus::Completed | OperationStatus::Failed | OperationStatus::Cancelled
    ) {
        release_operation_locks(snapshot)
            .map_err(|error| OperationError::new("LOCK_RELEASE_FAILED", error, true))?;
        if let Some(task_id) = snapshot.task_id.as_deref() {
            let event = if snapshot.status == OperationStatus::Completed {
                SystemEvent::TaskCompleted {
                    task_id: TaskId(task_id.to_owned()),
                    agent_id: snapshot
                        .pane_id
                        .clone()
                        .unwrap_or_else(|| snapshot.operation_id.clone()),
                    evidence: snapshot.result.clone().unwrap_or_default(),
                }
            } else {
                SystemEvent::TaskStatusUpdated {
                    task_id: TaskId(task_id.to_owned()),
                    status: if snapshot.status == OperationStatus::Cancelled {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into(),
                }
            };
            append_operation_event(&snapshot.project_path, event)
                .map_err(|error| OperationError::new("EVENT_WRITE_FAILED", error, true))?;
        }
        if let Some(pane_id) = snapshot.pane_id.as_deref() {
            // The live model is unreadable once the pane is gone, so persist it first.
            let live_model = crate::agent_runs::persist::live_resolved_model(registry, snapshot);
            let _ = crate::operations::record_terminal_pane_state(
                &snapshot.project_path,
                &snapshot.operation_id,
                live_model,
                false,
            );
            let disposed =
                crate::agent_runs::pane_close::maybe_dispose_after_terminal(snapshot, registry);
            if crate::agent_runs::pane_close::should_dispose_after_terminal(snapshot)
                && !registry.lock().panes.contains_key(pane_id)
            {
                let _ = crate::operations::record_terminal_pane_state(
                    &snapshot.project_path,
                    &snapshot.operation_id,
                    None,
                    true,
                );
            }
            emit_panes_changed(registry, app);
            disposed?;
        }
    }
    push_operation_sse(snapshot);
    Ok(())
}

pub fn stop_operation_worker(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    operation: &crate::operations::OperationSnapshot,
) -> Result<(), crate::operations::OperationError> {
    stop_operation_worker_with_kill(registry, app, operation, true)
}

pub fn stop_operation_worker_with_kill(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    operation: &crate::operations::OperationSnapshot,
    allow_kill_spawned_pane: bool,
) -> Result<(), crate::operations::OperationError> {
    let _cancel_guard = PANE_ASSIGNMENT_SERIAL.lock();
    let current =
        crate::operations::get_operation(&operation.project_path, &operation.operation_id)?;
    if current.pane_id.is_none() {
        return Ok(());
    }
    crate::agent_runs::pane_close::stop_worker_control(
        &current,
        registry,
        app,
        allow_kill_spawned_pane,
    )
}
