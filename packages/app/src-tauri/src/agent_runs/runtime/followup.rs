//! `send_agent`: starting the next turn on an existing handle, and enqueueing a follow-up behind
//! a running turn.

use super::{
    access::current_agent, handle_lock, queue::queue_followup, request::AgentRunRequest,
    view::AgentRunView,
};
use crate::agent_runs::headless::run_agent;
use crate::agent_runs::messaging::{self};
use crate::agent_runs::persist::{self, FollowupOpts};
use crate::operations::{
    self, ContextPolicy, OperationError, OperationSnapshot, OperationStatus, StateSource,
};
use parking_lot::Mutex;
use std::sync::Arc;
use uuid::Uuid;

pub fn send_agent(
    project: &str,
    handle: &str,
    task: String,
    session_id: Option<String>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
    opts: FollowupOpts,
) -> Result<AgentRunView, OperationError> {
    let handle_guard = handle_lock(handle);
    let serial = handle_guard.lock();
    let previous = current_agent(project, handle, session_id.as_deref())?;
    let project = previous.project_path.clone();
    let blocked_on_trust = crate::agent_runs::headless::blocked_on_trust(&previous);
    if !previous.status.terminal() && !blocked_on_trust {
        let queued = queue_followup(
            &project, handle, previous, task, session_id, registry, app, opts,
        );
        drop(serial);
        return queued;
    }
    if let Some(pane_id) = previous.pane_id.as_deref() {
        if !registry.lock().panes.contains_key(pane_id) {
            if let Some(error) = crate::agent_runs::pane_close::closed_after_run_in(
                std::slice::from_ref(&previous),
                pane_id,
                false,
            ) {
                return Err(error);
            }
            return Err(crate::agent_runs::pane_close::bound_pane_missing_error(
                pane_id,
            ));
        }
    }
    let headless = previous.pane_id.is_none() && previous.agent_type != "opencode_native";
    if blocked_on_trust {
        // Nothing ran: retire the parked turn so the retry is the handle's live turn.
        if let Ok(parked) = operations::mark_operation_state(
            &project,
            &previous.operation_id,
            OperationStatus::Cancelled,
            StateSource::Native,
            Some("superseded".into()),
            None,
            None,
        ) {
            let _ = crate::bridge::publish_operation(&parked, &registry, &app);
        }
    }
    let policy = persist::followup_policy(opts.context_policy, previous.worker.context_policy);
    let pending = messaging::pending_texts(&project, handle);
    let task = if pending.is_empty() {
        task
    } else {
        format!("{task}\n\nSteering messages:\n- {}", pending.join("\n- "))
    };
    let request = AgentRunRequest {
        project_path: Some(project.to_string()),
        task,
        agent_type: previous.agent_type.clone(),
        headless,
        background: opts.background,
        read_only: previous.read_only,
        keep_pane: previous.keep_pane,
        placement: None,
        isolation: None,
        worker_has_mcp_tools: false,
        timeout_ms: previous.timeout_ms,
        wait_ms: if opts.background {
            0
        } else {
            opts.wait_ms.unwrap_or(persist::FOLLOWUP_WAIT_MS)
        },
        idempotency_key: opts
            .idempotency_key
            .clone()
            .or_else(|| Some(Uuid::new_v4().to_string())),
        acceptance_criteria: opts.acceptance_criteria.clone(),
        checks: opts.checks.clone(),
        pane_id: if headless {
            None
        } else {
            previous.pane_id.clone()
        },
        worker_id: None,
        allow_broad: false,
        name: previous.worker.name.clone(),
        handle: Some(handle.to_string()),
        agent_run_id: Some(handle.to_string()),
        turn_index: previous.turn_index.saturating_add(1),
        context_policy: if blocked_on_trust {
            ContextPolicy::Fresh
        } else {
            policy
        },
        model: opts
            .requested_model
            .or(previous.worker.requested_model.clone()),
        reasoning: opts
            .requested_reasoning
            .or(previous.worker.requested_reasoning.clone()),
        role: opts.role.or(previous.worker.role.clone()),
        scope: opts.scope.or(previous.worker.scope.clone()),
        selected_history: opts.selected_history,
        locks: opts.locks.clone(),
        trust_workspace: opts.trust_workspace && headless,
        allow_shell: false,
        allowed_commands: None,
        account: previous.worker.account.clone(),
    };
    drop(serial);
    run_agent(request, registry, app, session_id)
}

pub(in crate::agent_runs) fn enqueue_followup(
    project: &str,
    handle: &str,
    previous: &OperationSnapshot,
    task: String,
    session_id: Option<String>,
    opts: FollowupOpts,
) -> Result<(OperationSnapshot, bool), OperationError> {
    if task.trim().is_empty() {
        return Err(OperationError::new(
            "INVALID_TASK",
            "task is required",
            false,
        ));
    }
    let next_turn = previous.turn_index.saturating_add(1);
    let policy = persist::followup_policy(opts.context_policy, previous.worker.context_policy);
    let choice = persist::resolve_model_choice(
        opts.requested_model
            .as_deref()
            .or(previous.worker.requested_model.as_deref()),
        opts.requested_reasoning
            .as_deref()
            .or(previous.worker.requested_reasoning.as_deref()),
        None,
    );
    let conversation = persist::conversation_facts(&previous.project_path, handle);
    let rendered = persist::render_turn_prompt(persist::TurnPromptArgs {
        agent_type: &previous.agent_type,
        policy,
        task: task.trim(),
        scope: opts.scope.as_deref().or(previous.worker.scope.as_deref()),
        prior_result: previous.result.as_deref(),
        prior_user: conversation.as_deref(),
        selected_history: opts.selected_history.as_deref().unwrap_or(&[]),
        can_resume: persist::snapshot_can_resume(previous) || persist::pane_keeps_context(previous),
        is_followup: true,
    });
    let mut worker = persist::build_worker(
        handle,
        previous.worker.name.as_deref(),
        opts.role.as_deref().or(previous.worker.role.as_deref()),
        opts.scope.as_deref().or(previous.worker.scope.as_deref()),
        Some(project),
        Some(previous),
        &rendered,
        &choice,
    );
    worker.queued_task_ids = vec![];
    worker.closed = false;
    worker.event_cursor = previous.revision.max(previous.worker.event_cursor);
    let idempotency_key = opts
        .idempotency_key
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let queued_checks = opts.checks.clone().unwrap_or_default();
    crate::agent_runs::checks::validate(std::path::Path::new(project), &queued_checks)
        .map_err(|message| OperationError::new("INVALID_ARGUMENT", message, false))?;
    operations::create_operation(operations::DelegateWorkRequest {
        project_path: project.to_string(),
        task,
        agent_type: previous.agent_type.clone(),
        pane_id: previous.pane_id.clone(),
        idempotency_key,
        acceptance_criteria: opts
            .acceptance_criteria
            .clone()
            .filter(|items| !items.is_empty()),
        task_id: None,
        exclusive: false,
        locks: opts.locks.clone(),
        timeout_ms: previous.timeout_ms,
        read_only: previous.read_only,
        keep_pane: previous.keep_pane,
        placement: None,
        owner_session_id: session_id.or(previous.owner_session_id.clone()),
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.to_string()),
        turn_index: next_turn,
        worker,
        context_policy: Some(policy),
        checks: queued_checks,
    })
}
