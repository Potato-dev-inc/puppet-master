use super::persist::{self, FollowupOpts, WorkerBind};
use super::runtime::{AgentRunRequest, AgentRunView};
use super::transcript;
use crate::operations::{
    self, ContextPolicy, DelegateWorkRequest, OperationError, OperationSnapshot, OperationStatus,
    StateSource,
};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
struct HeadlessControl {
    child: Mutex<Child>,
    cancelled: AtomicBool,
}

static CHILDREN: OnceLock<Mutex<HashMap<String, Arc<HeadlessControl>>>> = OnceLock::new();

fn children() -> &'static Mutex<HashMap<String, Arc<HeadlessControl>>> {
    CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
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
    if request.read_only
        && (!request.headless
            || !super::capabilities::read_only_supported(&request.agent_type, false))
    {
        return Err(OperationError::new(
            "READ_ONLY_UNSUPPORTED",
            super::capabilities::read_only_unsupported_message(
                &request.agent_type,
                !request.headless,
            ),
            false,
        ));
    }
    let agent_type = if request.agent_type == "cursor" {
        return Err(OperationError::new(
            "AGENT_NOT_DISPATCHABLE",
            "cursor opens the IDE; use cursor_agent for headless agent runs",
            false,
        ));
    } else {
        request.agent_type.clone()
    };
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
                return super::runtime::send_agent(
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
                        wait_ms: Some(if request.background { 0 } else { request.wait_ms }),
                        idempotency_key: request.idempotency_key.clone(),
                    },
                );
            }
            WorkerBind::Create { handle } => {
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
        request
            .model
            .as_deref()
            .or(previous
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
        policy: request.context_policy,
        task: &request.task,
        scope: request
            .scope
            .as_deref()
            .or(previous
                .as_ref()
                .and_then(|snapshot| snapshot.worker.scope.as_deref())),
        prior_result: previous
            .as_ref()
            .and_then(|snapshot| snapshot.result.as_deref()),
        prior_user: conversation.as_deref(),
        selected_history: request.selected_history.as_deref().unwrap_or(&[]),
        can_resume: previous
            .as_ref()
            .is_some_and(persist::snapshot_can_resume),
        is_followup: previous.is_some(),
    });
    let handle = request
        .agent_run_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    request.agent_run_id = Some(handle.clone());
    let worker = persist::build_worker(
        &handle,
        request.name.as_deref(),
        request.role.as_deref(),
        request.scope.as_deref(),
        request.project_path.as_deref(),
        previous.as_ref(),
        &rendered,
        &choice,
    );
    let idempotency_key = request
        .idempotency_key
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let validated_checks = request.checks.clone().unwrap_or_default();
    super::checks::validate(std::path::Path::new(&project_path), &validated_checks)
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

fn present_view(snapshot: &OperationSnapshot, registry: &Arc<Mutex<PaneRegistry>>) -> AgentRunView {
    AgentRunView::present(snapshot.clone(), Some(registry))
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
        format!("{prompt}\n\nAcceptance criteria:\n- {}", explicit.join("\n- "))
    }
}

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

pub(crate) fn bind_existing_worker(
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
    if let Ok(existing) = persist::find_worker(
        request.project_path.as_deref().unwrap_or_default(),
        &pane_ref,
    ) {
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
    if request.agent_type != pane.agent_type && request.agent_type != super::runtime::default_agent() {
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

fn wait_for_run(
    snapshot: &OperationSnapshot,
    wait_ms: u64,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<AgentRunView, OperationError> {
    if wait_ms == 0 {
        return Ok(present_view(snapshot, registry));
    }
    let result = operations::wait_for_operation_with_stage(
        &snapshot.project_path,
        &snapshot.operation_id,
        snapshot.revision,
        Some(vec![
            OperationStatus::WaitingInput,
            OperationStatus::Completed,
            OperationStatus::Failed,
            OperationStatus::Cancelled,
        ]),
        Some("settled_unverified"),
        wait_ms.min(300_000),
    )?;
    Ok(present_view(&result.snapshot, registry))
}

pub(super) fn compact_prompt(action: serde_json::Value) -> serde_json::Value {
    let id = ["prompt_id", "request_id", "message_id", "id"]
        .iter()
        .find_map(|key| action.get(*key).cloned())
        .or_else(|| {
            action
                .get("permission_ids")
                .and_then(serde_json::Value::as_array)
                .and_then(|ids| ids.first().cloned())
        })
        .unwrap_or(serde_json::Value::Null);
    let kind = action
        .get("kind")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let summary = action
        .get("question")
        .or_else(|| action.get("detail"))
        .or_else(|| action.get("text"))
        .cloned()
        .unwrap_or_else(|| kind.clone());
    let options = action
        .get("options")
        .and_then(serde_json::Value::as_array)
        .map(|options| {
            serde_json::Value::Array(
                options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        if let Some(label) = option.as_str() {
                            serde_json::json!({"id": label, "label": label})
                        } else if let Some(object) = option.as_object() {
                            let label = object
                                .get("label")
                                .or_else(|| object.get("text"))
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            let id = object.get("id").cloned().unwrap_or_else(|| label.clone());
                            let mut normalized = serde_json::Map::new();
                            normalized.insert("id".into(), id);
                            normalized.insert("label".into(), label);
                            if let Some(description) =
                                object.get("description").or_else(|| object.get("detail"))
                            {
                                normalized.insert("description".into(), description.clone());
                            }
                            serde_json::Value::Object(normalized)
                        } else {
                            serde_json::json!({"id": index.to_string(), "label": option})
                        }
                    })
                    .collect(),
            )
        })
        .or_else(
            || match action.get("kind").and_then(serde_json::Value::as_str) {
                Some("permission_required") => Some(serde_json::json!([
                    {"id":"allow_once","label":"Allow once"},
                    {"id":"deny","label":"Deny"},
                    {"id":"allow_broad","label":"Always allow"}
                ])),
                Some("manual_approval_required") => Some(serde_json::json!([])),
                _ => None,
            },
        )
        .unwrap_or_else(|| serde_json::json!([]));
    let mut prompt = serde_json::json!({"id":id,"kind":kind,"summary":summary,"options":options});
    if let Some(object) = prompt.as_object_mut() {
        for key in [
            "action",
            "resource",
            "scope",
            "path",
            "permission",
            "previous_session",
            "session_id",
            "pane_id",
        ] {
            if let Some(value) = action.get(key).cloned().filter(|value| !value.is_null()) {
                object.insert(key.to_string(), value);
            }
        }
    }
    prompt
}

fn read_tui_screen(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
) -> Option<String> {
    crate::pty::registry_read_snapshot(registry.as_ref(), pane_id).ok()
}

pub fn supervise_existing(
    snapshot: OperationSnapshot,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) {
    let _ = thread::Builder::new()
        .name("puppet-agent-deadline".into())
        .spawn(move || {
            let start = snapshot.started_at_ms.unwrap_or(snapshot.created_at_ms);
            let deadline = Duration::from_millis(snapshot.timeout_ms);
            let mut watch = super::completion::TuiTurnWatch::from_baseline(
                snapshot.output_baseline.clone().unwrap_or_default(),
            )
            .with_task(snapshot.task.clone());
            loop {
                let current =
                    match operations::get_operation(&snapshot.project_path, &snapshot.operation_id)
                    {
                        Ok(current) => current,
                        Err(_) => return,
                    };
                if current.status.terminal() {
                    return;
                }
                if watch.baseline.is_empty() {
                    if let Some(pane_id) = current.pane_id.as_deref() {
                        let pre_turn = current.status != OperationStatus::Running
                            || current.stage.as_deref() == Some("waiting_for_pane_readiness");
                        if pre_turn {
                            if let Some(screen) = read_tui_screen(&registry, pane_id) {
                                watch.baseline = screen;
                            }
                        }
                    }
                }
                if current.agent_type == "opencode_native" {
                    super::native::supervise_native_observation(&current, &registry, &app);
                } else if matches!(
                    current.status,
                    OperationStatus::Running | OperationStatus::WaitingInput
                ) {
                    if let Some(pane_id) = current.pane_id.as_deref() {
                        let pane_status = registry
                            .lock()
                            .list()
                            .into_iter()
                            .find(|pane| pane.id == pane_id)
                            .map(|pane| pane.status)
                            .unwrap_or_default();
                        let action = super::native::infer_tui_prompt(
                            &registry,
                            pane_id,
                            &current.agent_type,
                        );
                        let has_prompt = action.is_some() || pane_status == "waiting_input";
                        let screen = read_tui_screen(&registry, pane_id).unwrap_or_default();
                        match super::completion::observe_tui_turn(
                            &mut watch,
                            &pane_status,
                            &screen,
                            has_prompt,
                        ) {
                            super::completion::TuiTurnDecision::WaitingInput => {
                                if let Some(action) = action {
                                    if current.status != OperationStatus::WaitingInput
                                        || current.required_action.as_ref() != Some(&action)
                                    {
                                        transcript::append(
                                            &current.project_path,
                                            &current.operation_id,
                                            "permission",
                                            action
                                                .get("summary")
                                                .and_then(serde_json::Value::as_str)
                                                .unwrap_or("permission required"),
                                        );
                                        if let Ok(waiting) =
                                            operations::mark_operation_waiting_input(
                                                &current.project_path,
                                                &current.operation_id,
                                                Some("waiting_input".into()),
                                                action,
                                                StateSource::Inferred,
                                            )
                                        {
                                            let _ = crate::bridge::publish_operation(
                                                &waiting, &registry, &app,
                                            );
                                        }
                                    }
                                }
                            }
                            super::completion::TuiTurnDecision::Complete { result, capture } => {
                                transcript::append(
                                    &current.project_path,
                                    &current.operation_id,
                                    "assistant_final",
                                    &result,
                                );
                                if let Ok(completed) = operations::mark_operation_finished(
                                    &current.project_path,
                                    &current.operation_id,
                                    Some(result),
                                    capture,
                                    false,
                                ) {
                                    let _ = crate::bridge::publish_operation(
                                        &completed, &registry, &app,
                                    );
                                    return;
                                }
                            }
                            super::completion::TuiTurnDecision::AmbiguousIdle => {
                                if let Ok(completed) = operations::mark_operation_finished(
                                    &current.project_path,
                                    &current.operation_id,
                                    None,
                                    crate::operations::ResultCapture::Missing,
                                    false,
                                ) {
                                    let _ = crate::bridge::publish_operation(
                                        &completed, &registry, &app,
                                    );
                                    return;
                                }
                            }
                            super::completion::TuiTurnDecision::Continue => {}
                        }
                    }
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let elapsed = now.saturating_sub(start);
                if elapsed >= deadline.as_millis() as u64 {
                    let mut error = OperationError::new(
                        "TIMEOUT",
                        "agent exceeded its wall-clock deadline and was terminated",
                        true,
                    );
                    error.context = serde_json::json!({"timeout_ms": current.timeout_ms});
                    let registry_for_stop = registry.clone();
                    let app_for_stop = app.clone();
                    if let Err(stop_error) = crate::bridge::stop_operation_worker_with_kill(
                        &registry_for_stop,
                        &app_for_stop,
                        &current,
                        false,
                    ) {
                        let error =
                            OperationError::new("WORKER_STOP_FAILED", stop_error.message, true);
                        if let Ok(failed) = operations::mark_operation_state(
                            &current.project_path,
                            &current.operation_id,
                            OperationStatus::Failed,
                            StateSource::Native,
                            Some("stop_failed".into()),
                            None,
                            Some(error),
                        ) {
                            let _ = crate::bridge::publish_operation(
                                &failed,
                                &registry_for_stop,
                                &app_for_stop,
                            );
                        }
                        return;
                    }
                    if let Ok(failed) = operations::mark_operation_state(
                        &current.project_path,
                        &current.operation_id,
                        OperationStatus::Failed,
                        StateSource::Native,
                        Some("timeout".into()),
                        None,
                        Some(error),
                    ) {
                        let _ = crate::bridge::publish_operation(
                            &failed,
                            &registry_for_stop,
                            &app_for_stop,
                        );
                    }
                    return;
                }
                thread::sleep(Duration::from_millis(
                    if snapshot.agent_type == "opencode_native" {
                        1_000
                    } else {
                        250
                    },
                ));
            }
        });
}

pub(super) fn execute_headless(
    project: String,
    operation_id: String,
    agent_type: String,
    task: String,
    read_only: bool,
    timeout_ms: u64,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) {
    transcript::append(&project, &operation_id, "user_task", &task);
    let snapshot = operations::get_operation(&project, &operation_id).ok();
    let resume_session = snapshot
        .as_ref()
        .filter(|current| {
            persist::snapshot_can_resume(current)
                && current.worker.context_policy != ContextPolicy::Fresh
        })
        .and_then(|current| current.worker.provider_session_id.clone())
        .filter(|id| !id.is_empty());
    let spec =
        match crate::agent_adapters::launch::launch_spec_resuming(
            &agent_type,
            &project,
            &task,
            read_only,
            resume_session.as_deref(),
        ) {
            Ok(spec) if !read_only || spec.read_only_enforced => spec,
            Ok(_) => {
                fail_start(
                    &project,
                    &operation_id,
                    "READ_ONLY_UNSUPPORTED",
                    "adapter did not enforce read-only mode",
                );
                publish(&project, &operation_id, &registry, &app);
                return;
            }
            Err(error) => {
                let (code, message) = error
                    .strip_prefix("READ_ONLY_UNSUPPORTED: ")
                    .map(|message| ("READ_ONLY_UNSUPPORTED", message))
                    .unwrap_or(("LAUNCH_FAILED", error.as_str()));
                fail_start(&project, &operation_id, code, message);
                publish(&project, &operation_id, &registry, &app);
                return;
            }
        };
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut readers = Vec::new();
    let dispatch = operations::run_operation_dispatch(&project, &operation_id, |_| {
        operations::dispatch_input_if_active(&project, &operation_id, |_| {
            let child = command.spawn().map_err(|error| {
                let mut op_error = OperationError::new(
                    "LAUNCH_FAILED",
                    format!(
                        "spawn failed (program {:?}, cwd {}): {error}",
                        spec.program,
                        spec.cwd.display()
                    ),
                    true,
                );
                op_error.context = serde_json::json!({"program": spec.program, "cwd": spec.cwd.display().to_string()});
                op_error
            })?;
            let control = Arc::new(HeadlessControl {
                child: Mutex::new(child),
                cancelled: AtomicBool::new(false),
            });
            children()
                .lock()
                .insert(operation_id.clone(), control.clone());
            let (stdout, stderr) = {
                let mut child = control.child.lock();
                (child.stdout.take(), child.stderr.take())
            };
            if let Some(stdout) = stdout {
                if let Some(reader) =
                    spawn_reader(project.clone(), operation_id.clone(), "stdout", stdout)
                {
                    readers.push(reader);
                }
            }
            if let Some(stderr) = stderr {
                if let Some(reader) =
                    spawn_reader(project.clone(), operation_id.clone(), "stderr", stderr)
                {
                    readers.push(reader);
                }
            }
            Ok(())
        })?;
        Ok(())
    });
    if dispatch.is_err() {
        children().lock().remove(&operation_id);
        publish(&project, &operation_id, &registry, &app);
        return;
    }
    let Some(control) = children().lock().get(&operation_id).cloned() else {
        return;
    };
    let started = Instant::now();
    let exit_status = loop {
        if control.cancelled.load(Ordering::Acquire) {
            break None;
        }
        let mut child = control.child.lock();
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                let _ = operations::mark_operation_state(
                    &project,
                    &operation_id,
                    OperationStatus::Failed,
                    StateSource::Native,
                    Some("failed".into()),
                    None,
                    Some(OperationError::new(
                        "WORKER_WAIT_FAILED",
                        error.to_string(),
                        true,
                    )),
                );
                break None;
            }
        }
        drop(child);
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            if let Err(stop_error) = terminate(&control) {
                let failure = OperationError::new("WORKER_STOP_FAILED", stop_error.message, true);
                let _ = operations::mark_operation_state(
                    &project,
                    &operation_id,
                    OperationStatus::Failed,
                    StateSource::Native,
                    Some("stop_failed".into()),
                    None,
                    Some(failure),
                );
                break None;
            }
            let mut error = OperationError::new(
                "TIMEOUT",
                "agent exceeded its wall-clock deadline and was terminated",
                true,
            );
            error.context = serde_json::json!({"timeout_ms": timeout_ms});
            let _ = operations::mark_operation_state(
                &project,
                &operation_id,
                OperationStatus::Failed,
                StateSource::Native,
                Some("timeout".into()),
                None,
                Some(error),
            );
            break None;
        }
        thread::sleep(Duration::from_millis(75));
    };
    for reader in readers {
        let _ = reader.join();
    }
    let current = operations::get_operation(&project, &operation_id).ok();
    if current.as_ref().is_some_and(|op| op.status.terminal()) {
        children().lock().remove(&operation_id);
        publish(&project, &operation_id, &registry, &app);
        return;
    }
    if let Some(status) = exit_status {
        let chunks = transcript::read(&project, &operation_id, 0)
            .map(|page| page.chunks)
            .unwrap_or_default();
        let collect = |stream: &str| {
            chunks
                .iter()
                .filter(|chunk| chunk.stream == stream)
                .map(|chunk| chunk.text.as_str())
                .collect::<String>()
        };
        let transcript = collect("stdout");
        let stderr = collect("stderr");
        let code = status.code().unwrap_or(-1);
        if let Some(session) =
            crate::agent_adapters::launch::extract_provider_session(&agent_type, &transcript)
        {
            let _ = operations::set_provider_session(&project, &operation_id, &session);
        }
        if let Some(result) =
            crate::agent_adapters::launch::extract_result(&agent_type, &transcript, code)
        {
            transcript::append(&project, &operation_id, "assistant_final", &result);
            if let Ok(snapshot) = operations::get_operation(&project, &operation_id) {
                let handle = if snapshot.agent_run_id.is_empty() {
                    snapshot.operation_id.as_str()
                } else {
                    snapshot.agent_run_id.as_str()
                };
                super::messaging::mark_open_steers_processed(&project, handle, &result);
            }
            let _ = operations::mark_operation_completed(&project, &operation_id, &result, true);
        } else if let Some(mut prompt) = crate::agent_adapters::launch::detect_headless_trust(
            &agent_type,
            &format!(
                "{stderr}
{transcript}"
            ),
        ) {
            if let Some(id) = super::native::stable_prompt_id(&prompt) {
                prompt["prompt_id"] = serde_json::Value::String(id);
            }
            if let Err(error) = operations::mark_operation_waiting_input(
                &project,
                &operation_id,
                Some("waiting_input".into()),
                prompt,
                StateSource::Native,
            ) {
                tracing::warn!(operation_id = %operation_id, code = %error.code, message = %error.message, "could not park headless run on workspace trust prompt");
            }
        } else {
            let error = OperationError::new(
                if code == 0 {
                    "UNVERIFIED_RESULT"
                } else {
                    "WORKER_EXIT_FAILED"
                },
                failure_message(code, &stderr, Some(&spec.program)),
                code != 0,
            );
            let _ = operations::mark_operation_state(
                &project,
                &operation_id,
                OperationStatus::Failed,
                StateSource::Native,
                Some("failed".into()),
                None,
                Some(error),
            );
        }
    }
    children().lock().remove(&operation_id);
    publish(&project, &operation_id, &registry, &app);
}

/// Builds the run error text, surfacing the first useful CLI diagnostic so
/// `--help` filler does not hide the actual cause.
pub(super) fn failure_message(code: i32, stderr: &str, program: Option<&str>) -> String {
    let base = if code == 0 {
        "agent exited successfully without structured completion evidence"
    } else {
        "agent process exited with a failure status"
    };
    let Some(diagnostic) = first_cli_diagnostic(stderr) else {
        return base.to_string();
    };
    let mut message = format!("{base}: {diagnostic}");
    if diagnostic.to_ascii_lowercase().contains("trust") {
        message.push_str(" (trust the workspace folder in the agent first, then retry)");
    }
    if diagnostic.to_ascii_lowercase().contains("unexpected argument")
        || diagnostic.to_ascii_lowercase().contains("unknown option")
    {
        if let Some(program) = program {
            if let Some(version) = cli_version(program) {
                message.push_str(&format!(" ({version})"));
            }
            message.push_str(&format!(
                "; check `{program} exec --help` — top-level flags belong before `exec`"
            ));
        }
    }
    message
}

fn first_cli_diagnostic(stderr: &str) -> Option<&str> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let is_help = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.contains("try '--help'")
            || lower.contains("for more information")
            || lower.contains("usage:")
    };
    let is_cause = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.contains("unexpected argument")
            || lower.contains("unknown option")
            || lower.contains("unrecognized")
            || lower.contains("error:")
            || lower.contains("not trusted")
            || lower.contains("failed")
    };
    lines
        .iter()
        .copied()
        .find(|line| is_cause(line) && !is_help(line))
        .or_else(|| lines.iter().copied().rev().find(|line| !is_help(line)))
        .or_else(|| lines.first().copied())
}

fn cli_version(program: &str) -> Option<String> {
    let output = std::process::Command::new(program)
        .arg("--version")
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn spawn_reader<R: Read + Send + 'static>(
    project: String,
    operation_id: String,
    stream: &'static str,
    mut reader: R,
) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name(format!("puppet-agent-{stream}"))
        .spawn(move || {
            let mut bytes = [0u8; 8192];
            let mut pending = Vec::new();
            loop {
                let Ok(read) = reader.read(&mut bytes) else {
                    break;
                };
                if read == 0 {
                    break;
                }
                pending.extend_from_slice(&bytes[..read]);
                append_valid_utf8(&project, &operation_id, stream, &mut pending, false);
            }
            append_valid_utf8(&project, &operation_id, stream, &mut pending, true);
        })
        .ok()
}

pub(super) fn append_valid_utf8(
    project: &str,
    operation_id: &str,
    stream: &str,
    pending: &mut Vec<u8>,
    flush_incomplete: bool,
) {
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                if !text.is_empty() {
                    transcript::append(project, operation_id, stream, text);
                }
                pending.clear();
                return;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    let text = std::str::from_utf8(&pending[..valid]).unwrap_or_default();
                    transcript::append(project, operation_id, stream, text);
                    pending.drain(..valid);
                }
                match error.error_len() {
                    Some(length) => {
                        transcript::append(project, operation_id, stream, "\u{fffd}");
                        pending.drain(..length.min(pending.len()));
                    }
                    None if flush_incomplete => {
                        if !pending.is_empty() {
                            transcript::append(project, operation_id, stream, "\u{fffd}");
                            pending.clear();
                        }
                        return;
                    }
                    None => return,
                }
            }
        }
    }
}

fn terminate(control: &HeadlessControl) -> Result<(), OperationError> {
    let mut child = control.child.lock();
    let pid = child.id();
    if child
        .try_wait()
        .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?
        .is_some()
    {
        control.cancelled.store(true, Ordering::Release);
        return Ok(());
    }
    #[cfg(windows)]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
        if !output.status.success() && child.try_wait().ok().flatten().is_none() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut error = OperationError::new(
                "WORKER_STOP_FAILED",
                "taskkill could not stop the worker process tree",
                true,
            );
            error.context = json!({
                "exit_code": output.status.code(),
                "stderr": stderr.trim(),
                "pid": pid,
                "stopped": false
            });
            return Err(error);
        }
    }
    #[cfg(unix)]
    {
        let process_group = format!("-{pid}");
        let term_status = Command::new("kill")
            .args(["-TERM", process_group.as_str()])
            .status()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
        if !term_status.success() && child.try_wait().ok().flatten().is_none() {
            return Err(OperationError::new(
                "WORKER_STOP_FAILED",
                "could not signal worker process group",
                true,
            ));
        }
        thread::sleep(Duration::from_millis(150));
        let kill_status = Command::new("kill")
            .args(["-KILL", process_group.as_str()])
            .status()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
        if !kill_status.success() && child.try_wait().ok().flatten().is_none() {
            return Err(OperationError::new(
                "WORKER_STOP_FAILED",
                "could not terminate worker process group",
                true,
            ));
        }
    }
    if child
        .try_wait()
        .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?
        .is_none()
    {
        child
            .kill()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
    }
    child
        .wait()
        .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
    control.cancelled.store(true, Ordering::Release);
    Ok(())
}

fn fail_start(project: &str, id: &str, code: &str, message: &str) {
    let error = OperationError::new(code, message, false);
    let _ = operations::mark_operation_state(
        project,
        id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("failed".into()),
        None,
        Some(error),
    );
}

pub(super) fn publish(
    project: &str,
    id: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) {
    if let Ok(snapshot) = operations::get_operation(project, id) {
        let _ = crate::bridge::publish_operation(&snapshot, registry, app);
    }
}

pub(super) fn cancel_headless(operation: &OperationSnapshot) -> Result<(), OperationError> {
    let control = children().lock().get(&operation.operation_id).cloned();
    let Some(control) = control else {
        // A run parked on a workspace-trust prompt has already exited; cancelling just closes it.
        if parked_on_trust(operation) {
            return Ok(());
        }
        return Err(OperationError::new(
            "WORKER_NOT_FOUND",
            "worker process is not active",
            false,
        ));
    };
    terminate(&control)
}

/// A headless run whose process exited on the workspace-trust screen. No worker is alive, so
/// the only ways forward are a retry or a cancel. Status is not checked here because the cancel
/// hook runs after the operation has already moved to `Cancelling`.
fn parked_on_trust(operation: &OperationSnapshot) -> bool {
    operation.pane_id.is_none()
        && matches!(
            operation.status,
            OperationStatus::WaitingInput | OperationStatus::Cancelling
        )
        && operation
            .required_action
            .as_ref()
            .and_then(|action| action.get("kind"))
            .and_then(serde_json::Value::as_str)
            == Some("workspace_trust")
        && !children().lock().contains_key(&operation.operation_id)
}

/// Parked on trust and still waiting (not already being cancelled): eligible for a send_agent retry.
pub(super) fn blocked_on_trust(operation: &OperationSnapshot) -> bool {
    operation.status == OperationStatus::WaitingInput && parked_on_trust(operation)
}

#[cfg(test)]
mod acceptance_tests {
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

#[cfg(test)]
mod bind_tests {
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
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some(handle.into()),
            turn_index: turn,
            worker: Default::default(),
            context_policy: None,
            checks: Vec::new(),
        }
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
