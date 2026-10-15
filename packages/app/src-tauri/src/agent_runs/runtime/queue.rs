//! The background thread that holds a queued follow-up until its predecessor settles, then
//! dispatches it (headless child or pane).

use super::{followup::enqueue_followup, view::AgentRunView};
use crate::agent_runs::messaging::{self};
use crate::agent_runs::persist::{self, FollowupOpts};
use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus, StateSource};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub(super) fn queue_followup(
    project: &str,
    handle: &str,
    previous: OperationSnapshot,
    task: String,
    session_id: Option<String>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
    opts: FollowupOpts,
) -> Result<AgentRunView, OperationError> {
    let headless = previous.pane_id.is_none() && previous.agent_type != "opencode_native";
    let (queued, created) =
        enqueue_followup(project, handle, &previous, task, session_id.clone(), opts)?;
    if !created {
        return Ok(AgentRunView::present(queued, Some(&registry)));
    }
    if let Some(owner) = queued.owner_session_id.as_deref() {
        if let Err(error) = crate::mcp_sessions::register_owned_agent(owner, handle) {
            let _ = operations::cancel_operation(project, &queued.operation_id);
            return Err(error);
        }
        if let Err(error) = crate::mcp_sessions::register_owned_agent(owner, &queued.operation_id) {
            let _ = operations::cancel_operation(project, &queued.operation_id);
            return Err(error);
        }
    }
    let previous_id = previous.operation_id.clone();
    let project = project.to_string();
    let handle = handle.to_string();
    let operation_id = queued.operation_id.clone();
    let agent_type = queued.agent_type.clone();
    let timeout_ms = queued.timeout_ms;
    let registry_for_worker = registry.clone();
    let app_for_worker = app.clone();
    let thread_result = thread::Builder::new()
        .name("puppet-agent-followup".into())
        .spawn(move || {
            loop {
                let queued_state = match operations::get_operation(&project, &operation_id) {
                    Ok(snapshot) => snapshot,
                    Err(_) => return,
                };
                if queued_state.status.terminal() {
                    let _ = crate::bridge::publish_operation(
                        &queued_state,
                        &registry_for_worker,
                        &app_for_worker,
                    );
                    return;
                }
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if messaging::followup_timeout_expired(
                    queued_state.status == OperationStatus::Queued,
                    queued_state.created_at_ms,
                    queued_state.started_at_ms,
                    now_ms,
                    queued_state.timeout_ms,
                    messaging::QUEUE_WAIT_TIMEOUT_MS,
                ) {
                    let error = OperationError::new(
                        "TIMEOUT",
                        "agent follow-up remained queued beyond its wall-clock deadline",
                        true,
                    );
                    if let Ok(failed) = operations::mark_operation_state(
                        &project,
                        &operation_id,
                        OperationStatus::Failed,
                        StateSource::Native,
                        Some("timeout".into()),
                        None,
                        Some(error),
                    ) {
                        let _ = crate::bridge::publish_operation(
                            &failed,
                            &registry_for_worker,
                            &app_for_worker,
                        );
                    }
                    return;
                }
                let predecessor = match operations::get_operation(&project, &previous_id) {
                    Ok(snapshot) => snapshot,
                    Err(_) => return,
                };
                if predecessor.status.terminal() {
                    break;
                }
                let wait = operations::wait_for_operations(
                    &[
                        operations::OperationWaitCursor {
                            project_path: project.clone(),
                            operation_id: previous_id.clone(),
                            after_revision: predecessor.revision,
                        },
                        operations::OperationWaitCursor {
                            project_path: project.clone(),
                            operation_id: operation_id.clone(),
                            after_revision: queued_state.revision,
                        },
                    ],
                    Some(vec![
                        OperationStatus::Completed,
                        OperationStatus::Failed,
                        OperationStatus::Cancelled,
                    ]),
                    300_000,
                    false,
                    false,
                );
                if wait.is_err() {
                    thread::sleep(Duration::from_millis(100));
                }
            }
            let queued_state = match operations::get_operation(&project, &operation_id) {
                Ok(snapshot) => snapshot,
                Err(_) => return,
            };
            if queued_state.status.terminal() {
                let _ = crate::bridge::publish_operation(
                    &queued_state,
                    &registry_for_worker,
                    &app_for_worker,
                );
                return;
            }
            if headless {
                let predecessor = operations::get_operation(&project, &previous_id).ok();
                let conversation = persist::conversation_facts(&project, &handle);
                let can_resume = predecessor
                    .as_ref()
                    .is_some_and(persist::snapshot_can_resume);
                // Enqueue copied worker state before the predecessor captured a thread id.
                if can_resume {
                    if let Some(session) = predecessor
                        .as_ref()
                        .and_then(|snapshot| snapshot.worker.provider_session_id.clone())
                        .filter(|id| !id.is_empty())
                    {
                        let _ = operations::set_provider_session(&project, &operation_id, &session);
                    }
                }
                let rendered = persist::render_turn_prompt(persist::TurnPromptArgs {
                    agent_type: &queued_state.agent_type,
                    policy: queued_state.worker.context_policy,
                    task: &queued_state.task,
                    scope: queued_state.worker.scope.as_deref(),
                    prior_result: predecessor
                        .as_ref()
                        .and_then(|snapshot| snapshot.result.as_deref()),
                    prior_user: conversation.as_deref(),
                    selected_history: &[],
                    can_resume,
                    is_followup: true,
                });
                crate::agent_runs::headless::execute_headless(
                    project,
                    operation_id,
                    agent_type,
                    rendered.prompt,
                    queued_state.read_only,
                    timeout_ms,
                    registry_for_worker,
                    app_for_worker,
                );
            } else if let Err(error) = crate::bridge::dispatch_existing_operation(
                &registry_for_worker,
                &app_for_worker,
                &project,
                &operation_id,
            ) {
                let _ = operations::mark_operation_state(
                    &project,
                    &operation_id,
                    OperationStatus::Failed,
                    StateSource::Native,
                    Some("failed".into()),
                    None,
                    Some(error),
                );
                crate::agent_runs::headless::publish(
                    &project,
                    &operation_id,
                    &registry_for_worker,
                    &app_for_worker,
                );
            }
        });
    if let Err(error) = thread_result {
        let failure = OperationError::new("THREAD_START_FAILED", error.to_string(), true);
        if let Ok(failed) = operations::mark_operation_state(
            &queued.project_path,
            &queued.operation_id,
            OperationStatus::Failed,
            StateSource::Native,
            Some("failed".into()),
            None,
            Some(failure.clone()),
        ) {
            let _ = crate::bridge::publish_operation(&failed, &registry, &app);
        }
        return Err(failure);
    }
    Ok(AgentRunView::present(queued, Some(&registry)))
}
