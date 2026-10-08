//! `run_agent`: validates an agent run request, picks or creates the worker handle, builds the
//! operation, and dispatches it (headless child, or a visible pane through the bridge).

use super::bind::bind_existing_worker;
use super::process::{execute_headless, fail_start, publish};
use super::view::{present_view, wait_for_run};
use crate::agent_runs::persist::{self, FollowupOpts, WorkerBind};
use crate::agent_runs::runtime::{self, AgentRunRequest, AgentRunView};
use crate::agent_runs::{approval_policy, capabilities, checks};
use crate::tui::RunOptionSupport;
use crate::operations::{self, ContextPolicy, DelegateWorkRequest, OperationError};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;
use uuid::Uuid;

/// Validates `allow_shell` against the backend. Returns whether the flag needs to be applied to
/// the launch (headless Cursor: `--force`); `false` means shell is already permitted and the flag
/// is an accepted no-op (OpenCode native runs bash by default).
pub(crate) fn allow_shell_applies(
    supported: &RunOptionSupport,
    agent_type: &str,
    headless: bool,
    read_only: bool,
) -> Result<bool, OperationError> {
    if !supported.allow_shell {
        return Err(OperationError::new(
            "INVALID_ARGUMENT",
            format!("allow_shell is not supported by {agent_type}: it is only needed for headless cursor_agent; OpenCode native already runs shell commands"),
            false,
        ));
    }
    if read_only {
        return Err(OperationError::new(
            "INVALID_ARGUMENT",
            "allow_shell cannot be combined with read_only",
            false,
        ));
    }
    if supported.shell_default_on {
        return Ok(false);
    }
    if !headless {
        return Err(OperationError::new(
            "INVALID_ARGUMENT",
            "allow_shell applies only to headless, non-read-only runs; in a visible pane use allowed_commands or answer_prompt",
            false,
        ));
    }
    Ok(true)
}

pub fn run_agent(
    mut request: AgentRunRequest,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
    session_id: Option<String>,
) -> Result<AgentRunView, OperationError> {
    if request.agent_type == "opencode_native" {
        request.headless = false;
    }
    if request.task.trim().is_empty() {
        return Err(OperationError::new(
            "INVALID_TASK",
            "task is required",
            false,
        ));
    }
    let explicit_project = request
        .project_path
        .clone()
        .filter(|path| !path.trim().is_empty());
    if request.project_path.is_none() {
        request.project_path = Some(registry.lock().project_path.clone());
    }
    bind_existing_worker(
        &mut request,
        &registry,
        session_id.as_deref(),
        explicit_project.as_deref(),
    )?;
    let project_path = request
        .project_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::Path::new)
        .map(crate::project_path::prepare_project_path)
        .transpose()
        .map_err(|err| OperationError::new("INVALID_PROJECT_PATH", err, false))?
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !project_path.is_empty() {
        request.project_path = Some(project_path.clone());
    }
    if request.timeout_ms == 0 || request.timeout_ms > 86_400_000 {
        return Err(OperationError::new(
            "INVALID_TIMEOUT",
            "timeout_ms must be between 1 and 86400000",
            false,
        ));
    }
    if request.wait_ms > 300_000 {
        return Err(OperationError::new(
            "INVALID_TIMEOUT",
            "wait_ms must be between 0 and 300000",
            false,
        ));
    }
    if request.worker_has_mcp_tools {
        return Err(OperationError::new(
            "WORKER_MCP_TOOLS_UNAVAILABLE",
            "this run path cannot guarantee MCP tools are configured for the worker",
            false,
        ));
    }
    // A backend that enforces read-only on a pane it spawns itself (opencode_native) may run in a
    // pane; every other backend needs a headless run. An explicitly named pane must itself have
    // been spawned read-only, and the dispatch re-checks adopted worker panes.
    let read_only_ok = if crate::tui::profile_for(&request.agent_type)
        .features()
        .read_only_spawned_pane
    {
        request.pane_id.as_deref().is_none_or(|pane| {
            capabilities::read_only_supported_for(&request.agent_type, Some(pane))
        })
    } else {
        request.headless && capabilities::read_only_supported(&request.agent_type, false)
    };
    if request.read_only && !read_only_ok {
        return Err(OperationError::new(
            "READ_ONLY_UNSUPPORTED",
            capabilities::read_only_unsupported_message(&request.agent_type, !request.headless),
            false,
        ));
    }
    let profile = crate::tui::profile_for(&request.agent_type);
    if !profile.features().dispatchable {
        return Err(OperationError::new(
            "AGENT_NOT_DISPATCHABLE",
            profile.not_dispatchable_reason(),
            false,
        ));
    }
    let agent_type = request.agent_type.clone();
    // An identical retry (same idempotency_key, same task) is a replay of the run already started,
    // whatever worker name or handle was given: do not run worker selection again, which would
    // adopt the first run's worker and open a second turn that conflicts with the key.
    if request.turn_index == 0 {
        if let Some(key) = request
            .idempotency_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            if let Ok(Some(existing)) = operations::find_by_idempotency_key(&project_path, key) {
                if existing.agent_type == agent_type
                    && existing.turn_index == 0
                    && existing.task.contains(request.task.trim())
                {
                    let fresh = operations::get_operation(&project_path, &existing.operation_id)
                        .unwrap_or(existing);
                    return if request.background {
                        Ok(present_view(&fresh, &registry))
                    } else {
                        wait_for_run(&fresh, request.wait_ms, &registry)
                    };
                }
            }
        }
    }
    if request.turn_index == 0 {
        let key = persist::worker_key(
            request.agent_run_id.as_deref(),
            request
                .handle
                .as_deref()
                .or(request.worker_id.as_deref())
                .or(request.pane_id.as_deref()),
            request.name.as_deref(),
        );
        match persist::decide_worker(
            &project_path,
            key.as_deref(),
            request.context_policy == ContextPolicy::Fresh,
        )? {
            WorkerBind::Adopt { handle, .. } => {
                return runtime::send_agent(
                    &project_path,
                    &handle,
                    request.task,
                    session_id,
                    registry,
                    app,
                    FollowupOpts {
                        context_policy: Some(request.context_policy),
                        selected_history: request.selected_history.clone(),
                        requested_model: request.model.clone(),
                        requested_reasoning: request.reasoning.clone(),
                        role: request.role.clone(),
                        scope: request.scope.clone(),
                        background: request.background,
                        wait_ms: Some(if request.background {
                            0
                        } else {
                            request.wait_ms
                        }),
                        idempotency_key: request.idempotency_key.clone(),
                        acceptance_criteria: request.acceptance_criteria.clone(),
                        checks: request.checks.clone(),
                        locks: request.locks.clone(),
                        trust_workspace: request.trust_workspace,
                    },
                );
            }
            WorkerBind::Create { handle } => {
                // No worker name/handle given: tie the new handle to the idempotency key so a
                // retry of this exact call replays the run instead of conflicting with it.
                let handle = match (key.as_deref(), request.idempotency_key.as_deref()) {
                    (None, Some(idem)) if !idem.trim().is_empty() => {
                        persist::stable_handle(&project_path, idem)
                    }
                    _ => handle,
                };
                request.agent_run_id = Some(handle);
            }
        }
    }
    let previous = request
        .agent_run_id
        .as_deref()
        .filter(|_| request.turn_index > 0)
        .and_then(|handle| operations::latest_agent_run(&project_path, handle).ok());
    let choice = persist::resolve_model_choice(
        request.model.as_deref().or(previous
            .as_ref()
            .and_then(|snapshot| snapshot.worker.requested_model.as_deref())),
        request.reasoning.as_deref().or(previous
            .as_ref()
            .and_then(|snapshot| snapshot.worker.requested_reasoning.as_deref())),
        None,
    );
    let conversation = previous.as_ref().and_then(|snapshot| {
        let handle = if snapshot.agent_run_id.is_empty() {
            snapshot.operation_id.as_str()
        } else {
            snapshot.agent_run_id.as_str()
        };
        persist::conversation_facts(&snapshot.project_path, handle)
    });
    let rendered = persist::render_turn_prompt(persist::TurnPromptArgs {
        agent_type: &agent_type,
        policy: request.context_policy,
        task: &request.task,
        scope: request.scope.as_deref().or(previous
            .as_ref()
            .and_then(|snapshot| snapshot.worker.scope.as_deref())),
        prior_result: previous
            .as_ref()
            .and_then(|snapshot| snapshot.result.as_deref()),
        prior_user: conversation.as_deref(),
        selected_history: request.selected_history.as_deref().unwrap_or(&[]),
        can_resume: previous.as_ref().is_some_and(|snapshot| {
            persist::snapshot_can_resume(snapshot)
                || (!request.headless && persist::pane_keeps_context(snapshot))
        }),
        is_followup: previous.is_some(),
    });
    let handle = request
        .agent_run_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    request.agent_run_id = Some(handle.clone());
    let mut worker = persist::build_worker(
        &handle,
        request.name.as_deref(),
        request.role.as_deref(),
        request.scope.as_deref(),
        request.project_path.as_deref(),
        previous.as_ref(),
        &rendered,
        &choice,
    );
    // APR-4/SEC-4: the file must still be what was approved and the binary the pinned one; the
    // reference is recorded on this turn (OBS-2).
    worker.agent_definition = crate::tui::registry::admit(&agent_type, &project_path)?;
    let supported = profile.run_options();
    if request.trust_workspace {
        if !supported.trust_workspace {
            return Err(OperationError::new(
                "INVALID_ARGUMENT",
                format!("trust_workspace is not supported by {agent_type}"),
                false,
            ));
        }
        if !request.headless {
            return Err(OperationError::new(
                "INVALID_ARGUMENT",
                "trust_workspace applies only to headless runs; in a visible pane answer the trust prompt with answer_prompt(choice=trust)",
                false,
            ));
        }
        worker.trust_workspace = true;
    }
    if request.allow_shell {
        worker.allow_shell = allow_shell_applies(
            &supported,
            &agent_type,
            request.headless,
            request.read_only,
        )?;
    }
    if let Some(allowed) = request
        .allowed_commands
        .as_ref()
        .filter(|items| !items.is_empty())
    {
        if !supported.allowed_commands {
            return Err(OperationError::new(
                "INVALID_ARGUMENT",
                format!("allowed_commands is not supported by {agent_type}"),
                false,
            ));
        }
        if request.headless {
            return Err(OperationError::new(
                "INVALID_ARGUMENT",
                "allowed_commands applies only to runs in a visible pane",
                false,
            ));
        }
        approval_policy::validate(allowed)
            .map_err(|message| OperationError::new("INVALID_ARGUMENT", message, false))?;
        worker.allowed_commands = allowed.iter().map(|item| item.trim().to_string()).collect();
    }
    let idempotency_key = request
        .idempotency_key
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let validated_checks = request.checks.clone().unwrap_or_default();
    checks::validate(std::path::Path::new(&project_path), &validated_checks)
        .map_err(|message| OperationError::new("INVALID_ARGUMENT", message, false))?;
    let requested_criteria = request.acceptance_criteria.as_deref();
    let acceptance_criteria = stored_acceptance_criteria(requested_criteria);
    let task_prompt = wrap_acceptance_criteria(&rendered.prompt, requested_criteria);
    let operation_request = DelegateWorkRequest {
        project_path: project_path.clone(),
        task: task_prompt.clone(),
        agent_type,
        pane_id: request.pane_id.clone(),
        idempotency_key,
        acceptance_criteria: if acceptance_criteria.is_empty() {
            None
        } else {
            Some(acceptance_criteria)
        },
        task_id: None,
        exclusive: false,
        locks: request.locks.clone(),
        timeout_ms: request.timeout_ms,
        read_only: request.read_only,
        keep_pane: request.keep_pane,
        owner_session_id: session_id,
        worker_has_mcp_tools: false,
        agent_run_id: request.agent_run_id.clone(),
        turn_index: request.turn_index,
        worker,
        context_policy: Some(request.context_policy),
        checks: validated_checks,
    };
    if !request.headless {
        let snapshot = crate::bridge::delegate_operation(&registry, &app, operation_request)?;
        return if request.background {
            Ok(present_view(&snapshot, &registry))
        } else {
            wait_for_run(&snapshot, request.wait_ms, &registry)
        };
    }
    let (snapshot, created) = operations::create_operation(operation_request)?;
    if !created {
        return if request.background {
            Ok(present_view(&snapshot, &registry))
        } else {
            wait_for_run(&snapshot, request.wait_ms, &registry)
        };
    }
    if let Err(error) = crate::bridge::fail_operation_on_lock_conflict(&snapshot) {
        publish(&project_path, &snapshot.operation_id, &registry, &app);
        return Err(error);
    }
    if let Some(owner) = snapshot.owner_session_id.as_deref() {
        if let Err(error) = crate::mcp_sessions::register_owned_agent(owner, &snapshot.agent_run_id)
        {
            if let Ok(cancelled) =
                operations::cancel_operation(&project_path, &snapshot.operation_id)
            {
                let _ = crate::bridge::publish_operation(&cancelled, &registry, &app);
            }
            return Err(error);
        }
    }
    let project = project_path.clone();
    let operation_id = snapshot.operation_id.clone();
    let registry_for_worker = registry.clone();
    let app_for_worker = app.clone();
    let agent_type = request.agent_type;
    let task = task_prompt;
    let read_only = request.read_only;
    let timeout_ms = request.timeout_ms;
    let failure_project = project.clone();
    let failure_id = operation_id.clone();
    let failure_registry = registry.clone();
    let failure_app = app.clone();
    if let Err(error) = thread::Builder::new()
        .name("puppet-agent-headless".into())
        .spawn(move || {
            execute_headless(
                project,
                operation_id,
                agent_type,
                task,
                read_only,
                timeout_ms,
                registry_for_worker,
                app_for_worker,
            );
        })
    {
        let failure = OperationError::new("THREAD_START_FAILED", error.to_string(), true);
        fail_start(
            &failure_project,
            &failure_id,
            &failure.code,
            &failure.message,
        );
        publish(
            &failure_project,
            &failure_id,
            &failure_registry,
            &failure_app,
        );
        return Err(failure);
    }
    if request.background {
        Ok(present_view(&snapshot, &registry))
    } else {
        wait_for_run(&snapshot, request.wait_ms, &registry)
    }
}

fn nonempty_criteria(requested: Option<&[String]>) -> Vec<String> {
    requested
        .unwrap_or(&[])
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn stored_acceptance_criteria(requested: Option<&[String]>) -> Vec<String> {
    nonempty_criteria(requested)
}

fn wrap_acceptance_criteria(prompt: &str, requested: Option<&[String]>) -> String {
    let explicit = nonempty_criteria(requested);
    if explicit.is_empty() {
        prompt.to_string()
    } else {
        format!(
            "{prompt}\n\nAcceptance criteria:\n- {}",
            explicit.join("\n- ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_acceptance_stores_empty_without_wrapping_the_prompt() {
        let stored = stored_acceptance_criteria(None);
        assert!(stored.is_empty());
        assert_eq!(
            wrap_acceptance_criteria("analyze ORBIT-42 in 50 words", None),
            "analyze ORBIT-42 in 50 words"
        );
        assert_eq!(
            wrap_acceptance_criteria("analyze ORBIT-42 in 50 words", Some(&["  ".into()])),
            "analyze ORBIT-42 in 50 words"
        );
    }

    #[test]
    fn explicit_acceptance_wraps_the_prompt() {
        let criteria = vec!["return GREEN ORBIT-42".into()];
        assert_eq!(
            stored_acceptance_criteria(Some(criteria.as_slice())),
            criteria
        );
        assert!(wrap_acceptance_criteria("task", Some(criteria.as_slice()))
            .contains("Acceptance criteria:\n- return GREEN ORBIT-42"));
    }
}
