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
    let isolate = parse_isolation(&request)?;
    // A mission round joins the mission's copy instead of creating one; it is always visible.
    let joined = request.join_worktree.clone().filter(|_| request.turn_index == 0);
    let budget_asked = crate::budgets::Asked {
        turns: request.budget_turns,
        minutes: request.budget_minutes,
        parent: request.budget_parent.clone().filter(|parent| !parent.trim().is_empty()),
    };
    crate::budgets::validate(&budget_asked)?;
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
        .map(crate::project_path::prepare_project_path_coded)
        .transpose()
        .map_err(crate::project_path::PrepareError::into_operation_error)?
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
    if let Some(error) = crate::tui::registry::unavailable(&request.agent_type) {
        return Err(error);
    }
    // An unregistered id is UNKNOWN_AGENT_TYPE (with the valid ids), not the fallback profile's
    // "cannot run delegated work".
    registered_profile(&request.agent_type)?;
    // The agent's "How agent runs work" setting decides the mode only when the caller did not
    // pass `headless` itself (an explicit true/false always wins). Decided before the read-only
    // and option checks below, which depend on the mode the run will really use.
    let run_mode = crate::agent_run_mode::apply(&mut request);
    // Visible-first: an agent with no visible mode (but a headless one) runs headless, recorded
    // as `fallback.reason = no_visible_mode`. Decided before the read-only / option checks, which
    // depend on the mode the run will really use.
    let no_visible_mode = super::fallback::no_visible_mode(
        crate::tui::run_modes(&request.agent_type),
        request.headless,
        request.pane_id.is_some(),
    );
    if no_visible_mode {
        request.headless = true;
    }
    if joined.is_some() && request.headless {
        // Missions run in visible panes only: a visible-first fallback to headless is refused.
        return Err(OperationError::new(
            "MISSION_HEADLESS",
            format!("{} can't run in a visible pane, and missions run in visible panes", request.agent_type),
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
            request.account.as_deref(),
        );
        // A named worker is stored under the account-folded key, so the next call with the same
        // name and account finds it again (`worker_key`).
        if request.agent_run_id.is_none()
            && request.handle.is_none()
            && request.worker_id.is_none()
            && request.pane_id.is_none()
            && request.account.as_deref().is_some_and(|a| !a.trim().is_empty())
        {
            if let Some(folded) = key.clone() {
                request.name = Some(folded);
            }
        }
        match persist::decide_worker(
            &project_path,
            key.as_deref(),
            request.context_policy == ContextPolicy::Fresh,
        )? {
            WorkerBind::Adopt { handle, previous } => {
                let wanted = request.account.as_deref().map(str::trim).filter(|a| !a.is_empty());
                if wanted.is_some() && wanted != previous.worker.account.as_deref() {
                    return Err(OperationError::new(
                        "ACCOUNT_MISMATCH",
                        "this worker runs under a different account; start a new worker for this account",
                        false,
                    ));
                }
                if isolate
                    && operations::latest_agent_run(&project_path, &handle)
                        .map_or(true, |existing| existing.worker.worktree.is_none())
                {
                    return Err(OperationError::new(
                        "INVALID_ARGUMENT",
                        "isolation: \"worktree\" applies when a worker is created; this handle already exists without a worktree. Start a new run to isolate it.",
                        false,
                    ));
                }
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
    // The receipt reports what the profile held HERE could enforce, not whatever the live registry
    // says when the turn ends (the definition may have been edited or removed mid-run).
    worker.run_features = Some(crate::operations::RunFeatures::of(profile.as_ref()));
    // The account was folded into the worker name above (so the named worker is per account); the
    // run itself records the account and refuses a different one on a later turn of this worker.
    let requested_account = request.account.as_deref().map(str::trim).filter(|a| !a.is_empty());
    if let Some(account) = requested_account {
        if previous.as_ref().and_then(|p| p.worker.account.as_deref()).is_some_and(|prior| prior != account) {
            return Err(OperationError::new(
                "ACCOUNT_MISMATCH",
                "this worker runs under a different account; start a new worker for this account",
                false,
            ));
        }
        worker.account = Some(
            crate::accounts::validate_for_run(&agent_type, account)
                .map_err(|message| OperationError::new("INVALID_ARGUMENT", message, false))?,
        );
    }
    if no_visible_mode {
        // The existing fallback already says why this ran headless; the setting adds nothing.
        worker.fallback = Some(crate::operations::RunFallback::to_headless(
            super::fallback::NO_VISIBLE_MODE,
        ));
    } else {
        // One-task mode was chosen: say which mode ran and why. When it is not verified (any
        // more) the run is visible and the receipt carries fallback {from: one_task, ...}.
        worker.run_mode = run_mode.note;
        worker.fallback = run_mode.fallback;
    }
    if request.turn_index > 0 {
        // A follow-up continues in the mode of its run and keeps what that run reported.
        if let Some(earlier) = previous.as_ref() {
            worker.run_mode = earlier.worker.run_mode.clone();
            if earlier.worker.fallback.as_ref().is_some_and(|f| f.is_one_task_to_visible()) {
                worker.fallback = earlier.worker.fallback.clone();
            }
        }
    }
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
    // A new run's budget: its own arguments, else the Settings default (off unless set), or a share
    // of the run that started it (refused with BUDGET_EXHAUSTED when the parent has none to give).
    // Decided before the worktree exists, so a refusal leaves nothing behind. No budget by default.
    let budgeted = request.turn_index == 0
        && crate::budgets::admit(
            &project_path,
            &handle,
            &budget_asked,
            crate::budgets::Defaults::from_settings(&crate::settings_store::read_public_settings(&app)),
            crate::questions::now_ms(),
        )?
        .is_some();
    // Last step that can still be refused: create the worktree for a new isolated worker. A turn
    // after the first inherits the worktree through `previous` (see `persist::build_worker`).
    if let Some(copy) = joined {
        worker.worktree = Some(copy);
    } else if isolate && request.turn_index == 0 {
        match crate::worktrees::create(&project_path, &handle) {
            Ok(created) => worker.worktree = Some(created),
            Err(mut error) => {
                if budgeted {
                    crate::budgets::discard(&project_path, &handle);
                }
                match error.context.as_object_mut() {
                    Some(context) => {
                        context.insert("isolation".into(), "worktree".into());
                        context.insert("project_path".into(), project_path.clone().into());
                    }
                    None => error.context = serde_json::json!({"isolation": "worktree", "project_path": project_path}),
                }
                return Err(error);
            }
        }
    }
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
        placement: request.placement.clone(),
        owner_session_id: session_id,
        worker_has_mcp_tools: false,
        agent_run_id: request.agent_run_id.clone(),
        turn_index: request.turn_index,
        worker,
        context_policy: Some(request.context_policy),
        checks: validated_checks,
    };
    if !request.headless {
        let snapshot = crate::bridge::delegate_operation(&registry, &app, operation_request).inspect_err(|_| {
            if budgeted {
                crate::budgets::discard(&project_path, &handle);
            }
        })?;
        // The visible pane now exists: its auto title comes from the task it was started with.
        if let Some(pane_id) = snapshot.pane_id.as_deref() {
            crate::ambient::store::set_from_task(pane_id, &snapshot.agent_type, &request.task);
        }
        return if request.background {
            Ok(present_view(&snapshot, &registry))
        } else {
            wait_for_run(&snapshot, request.wait_ms, &registry)
        };
    }
    let (snapshot, created) = operations::create_operation(operation_request).inspect_err(|_| {
        if budgeted {
            crate::budgets::discard(&project_path, &handle);
        }
    })?;
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

/// `isolation`: absent/`none` -> false, `worktree` -> true; anything else, or combining it with an
/// existing pane/worker, is refused.
fn parse_isolation(request: &AgentRunRequest) -> Result<bool, OperationError> {
    let wants = match request.isolation.as_deref().map(str::trim) {
        None | Some("") | Some("none") => false,
        Some("worktree") => true,
        Some(other) => {
            return Err(OperationError::new(
                "INVALID_ARGUMENT",
                format!("isolation must be \"none\" or \"worktree\", got {other:?}"),
                false,
            ))
        }
    };
    if wants && request.turn_index == 0 && (request.pane_id.is_some() || request.worker_id.is_some()) {
        return Err(OperationError::new(
            "INVALID_ARGUMENT",
            "isolation: \"worktree\" starts a new worker in its own worktree; it cannot be combined with pane_id or worker_id (those adopt an existing worker)",
            false,
        ));
    }
    Ok(wants)
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

/// The registered profile for `agent_type`, or `UNKNOWN_AGENT_TYPE` listing the valid ids.
fn registered_profile(
    agent_type: &str,
) -> Result<std::sync::Arc<dyn crate::tui::TuiProfile>, OperationError> {
    crate::tui::require_registered(agent_type)
        .map_err(|unknown| OperationError::unknown_agent_type(unknown.id, unknown.valid))
}

#[cfg(test)]
mod registered_tests {
    use super::registered_profile;

    #[test]
    fn unknown_agent_is_unknown_agent_type_with_valid_ids_not_not_dispatchable() {
        let error = registered_profile("nope_agent").err().expect("unknown id is refused");
        assert_eq!(error.code, "UNKNOWN_AGENT_TYPE");
        assert!(error.context["valid"].as_array().unwrap().iter().any(|id| id == "claude"), "{:?}", error.context);
        assert!(registered_profile("claude").is_ok());
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::parse_isolation;
    use crate::agent_runs::runtime::AgentRunRequest;
    use serde_json::json;

    fn request(value: serde_json::Value) -> AgentRunRequest {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn isolation_none_or_absent_is_off_and_worktree_is_on() {
        assert!(!parse_isolation(&request(json!({"task":"t"}))).unwrap());
        assert!(!parse_isolation(&request(json!({"task":"t","isolation":"none"}))).unwrap());
        assert!(parse_isolation(&request(json!({"task":"t","isolation":"worktree"}))).unwrap());
    }

    #[test]
    fn unknown_isolation_and_adopting_a_pane_are_refused() {
        let error = parse_isolation(&request(json!({"task":"t","isolation":"docker"}))).unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT");
        let error = parse_isolation(&request(json!({"task":"t","isolation":"worktree","pane_id":"p"}))).unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT");
        assert!(error.message.contains("pane_id"));
        // a later turn on an isolated handle may carry a pane id
        assert!(parse_isolation(&request(json!({"task":"t","isolation":"worktree","pane_id":"p","turn_index":1}))).unwrap());
    }
}
