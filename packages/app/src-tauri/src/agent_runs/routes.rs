use super::runtime::{self, AgentRunRequest, AgentRunView};
use crate::operations::{self, OperationError, OperationStatus, OperationWaitCursor, StateSource};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tauri::AppHandle;

pub fn handle_request(
    method: &str,
    segments: &[&str],
    query: &str,
    body: &[u8],
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    session_id: Option<&str>,
) -> Option<Result<Value, OperationError>> {
    let result = match (method, segments) {
        ("POST", ["agents", "run"]) => parse::<AgentRunRequest>(body).and_then(|mut request| {
            if request
                .project_path
                .as_deref()
                .is_none_or(|path| path.trim().is_empty())
            {
                request.project_path = query_value(query, "project_path");
            }
            runtime::run_agent(
                request,
                registry.clone(),
                app.clone(),
                session_id.map(str::to_string),
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("POST", ["agents", "wait"]) => parse::<WaitAgentsRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            wait_agents(&project, request, session_id, registry)
        }),
        ("POST", ["agents", "send"]) => parse::<SendAgentRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            let message = request.message.or(request.task).unwrap_or_default();
            if message.trim().is_empty() {
                return Err(OperationError::new(
                    "INVALID_TASK",
                    "message is required",
                    false,
                ));
            }
            runtime::send_message(
                &project,
                &request.handle,
                message,
                session_id,
                request.delivery.as_deref(),
                request.idempotency_key.as_deref(),
                registry.clone(),
                app.clone(),
            )
        }),
        ("POST", ["agents", "followup"]) => parse::<SendAgentRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            let task = request.task.or(request.message).unwrap_or_default();
            if task.trim().is_empty() {
                return Err(OperationError::new(
                    "INVALID_TASK",
                    "task is required",
                    false,
                ));
            }
            runtime::send_agent(
                &project,
                &request.handle,
                task,
                session_id.map(str::to_string),
                registry.clone(),
                app.clone(),
                super::persist::FollowupOpts {
                    context_policy: request.context_policy,
                    selected_history: request.selected_history,
                    requested_model: request.model,
                    requested_reasoning: request.reasoning,
                    role: None,
                    scope: request.scope,
                    background: false,
                    wait_ms: None,
                    idempotency_key: request.idempotency_key,
                },
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("POST", ["agents", "answer"]) => parse::<AnswerPromptRequest>(body).and_then(|request| {
            if request.handle.is_none() {
                let pane_id = request.pane_id.as_deref().filter(|id| !id.is_empty()).ok_or_else(|| {
                    OperationError::new(
                        "INVALID_ARGUMENT",
                        "answer_prompt needs `handle` (agent run) or `pane_id` (full pane id from wait_for_panes)",
                        false,
                    )
                })?;
                return crate::pane_prompt::answer(
                    registry,
                    app,
                    pane_id,
                    &request.prompt_id,
                    &request.choice,
                    request.allow_broad,
                );
            }
            let project = project_path(request.project_path.as_deref(), query, registry);
            answer_prompt(&project, &request, session_id, registry, app)
        }),
        ("POST", ["agents", "cancel"]) => parse::<AgentHandleRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            runtime::cancel_agent(
                &project,
                &request.handle,
                session_id,
                registry.clone(),
                app.clone(),
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("GET", ["agents", "steer-receipt"]) => {
            let handle = query_value(query, "handle").filter(|value| !value.is_empty());
            let idempotency_key = query_value(query, "idempotency_key")
                .filter(|value| !value.is_empty());
            let (Some(handle), Some(idempotency_key)) = (handle, idempotency_key) else {
                return Some(Err(OperationError::new(
                    "INVALID_ARGUMENT",
                    "handle and idempotency_key are required",
                    false,
                )));
            };
            let project = project_path(
                query_value(query, "project_path").as_deref(),
                query,
                registry,
            );
            super::messaging::find_receipt_by_key(&project, &handle, &idempotency_key)
                .map(|receipt| super::messaging::receipt_value(&receipt))
                .ok_or_else(|| {
                    OperationError::new(
                        "STEER_RECEIPT_NOT_FOUND",
                        "steer receipt not found",
                        false,
                    )
                })
        }
        ("GET", ["agents"]) => {
            let explicit = query_value(query, "project_path");
            let project = project_path(explicit.as_deref(), query, registry);
            list_agents(&project, session_id, registry, explicit.is_some())
        }
        ("GET", ["agents", handle]) => {
            let project = project_path(None, query, registry);
            resolve_run_handle(&project, handle, registry).and_then(|handle| {
                let mut value = inspect_agent(&project, &handle, session_id)?;
                if let Ok(snapshot) = runtime::current_agent_for_read(&project, &handle, session_id) {
                    super::persist::overlay_live_model(&mut value, &snapshot, Some(registry));
                }
                Ok(value)
            })
        }
        ("POST", ["agents", "close"]) => parse::<AgentHandleRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            runtime::close_agent(
                &project,
                &request.handle,
                session_id,
                registry.clone(),
                app.clone(),
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("GET", ["agents", handle, "transcript"]) => {
            let project = project_path(None, query, registry);
            resolve_run_handle(&project, handle, registry)
                .and_then(|handle| transcript(&project, &handle, query, session_id))
        }
        _ => return None,
    };
    Some(result)
}

#[derive(Debug, Deserialize)]
struct WaitAgentsRequest {
    #[serde(default)]
    project_path: Option<String>,
    handles: Vec<String>,
    #[serde(default)]
    after_revisions: HashMap<String, u64>,
    #[serde(default)]
    after_cursor: Option<u64>,
    #[serde(default)]
    until: Option<Vec<OperationStatus>>,
    #[serde(default = "default_wait_timeout", alias = "timeout_ms")]
    wait_ms: u64,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    wake_on_progress: bool,
}

#[derive(Debug, Deserialize)]
struct SendAgentRequest {
    #[serde(default)]
    project_path: Option<String>,
    handle: String,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    delivery: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    context_policy: Option<crate::operations::ContextPolicy>,
    #[serde(default)]
    selected_history: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentHandleRequest {
    #[serde(default)]
    project_path: Option<String>,
    handle: String,
}

#[derive(Debug, Deserialize)]
struct AnswerPromptRequest {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    pane_id: Option<String>,
    prompt_id: String,
    choice: String,
    #[serde(default)]
    allow_broad: bool,
}

fn default_wait_timeout() -> u64 {
    120_000
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, OperationError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|error| OperationError::new("INVALID_JSON", error.to_string(), false))?;
    runtime::normalize_context_policy_fields(&mut value)?;
    serde_json::from_value(value)
        .map_err(|error| OperationError::new("INVALID_JSON", error.to_string(), false))
}

fn project_path(
    requested: Option<&str>,
    query: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> String {
    requested
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
        .or_else(|| query_value(query, "project_path"))
        .unwrap_or_else(|| registry.lock().project_path.clone())
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| percent_decode(value))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    decoded.push(high * 16 + low);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Ended runs (completed/failed/cancelled) stay listed this long after they finish.
const ENDED_RUN_RETENTION_MS: u64 = 24 * 60 * 60 * 1000;

/// Latest turn of every run that is active or ended within the retention window, paired with
/// whether `session_id` owns it. Runs started by other connections are listed with
/// `owned == false`; listing them grants no control (take_over still needs grant=true).
pub(crate) fn visible_runs(
    operations: Vec<crate::operations::OperationSnapshot>,
    session_id: Option<&str>,
    now_ms: u64,
) -> Vec<(crate::operations::OperationSnapshot, bool)> {
    let mut latest = BTreeMap::<String, crate::operations::OperationSnapshot>::new();
    for snapshot in operations {
        let handle = if snapshot.agent_run_id.is_empty() {
            snapshot.operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        };
        let should_replace = latest
            .get(&handle)
            .map_or(true, |existing| snapshot.turn_index >= existing.turn_index);
        if should_replace {
            latest.insert(handle, snapshot);
        }
    }
    latest
        .into_values()
        .filter(|snapshot| {
            let ended = matches!(
                snapshot.status,
                OperationStatus::Completed | OperationStatus::Failed | OperationStatus::Cancelled
            );
            !ended
                || snapshot.finished_at_ms.map_or(true, |finished| {
                    now_ms.saturating_sub(finished) <= ENDED_RUN_RETENTION_MS
                })
        })
        .map(|snapshot| {
            let handle = if snapshot.agent_run_id.is_empty() {
                snapshot.operation_id.as_str()
            } else {
                snapshot.agent_run_id.as_str()
            };
            let owned = session_id.map_or(true, |session| {
                snapshot.owner_session_id.as_deref() == Some(session)
                    || crate::mcp_sessions::check_run_access(session, handle).is_ok()
            });
            (snapshot, owned)
        })
        .collect()
}

fn inspect_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<Value, OperationError> {
    runtime::inspect_agent(project, handle, session_id)
}

fn list_agents(
    project: &str,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    explicit_project: bool,
) -> Result<Value, OperationError> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let operations = if explicit_project {
        operations::list_operations(project)?
    } else {
        operations::list_indexed_operations(project)?
    };
    let live: BTreeMap<String, crate::pty::PaneInfo> = registry
        .lock()
        .list()
        .into_iter()
        .filter(|pane| !pane.id.starts_with("puppet-master-orchestrator-"))
        .map(|pane| (pane.id.clone(), pane))
        .collect();
    let mut claimed = std::collections::BTreeSet::new();
    let mut live_workers = Vec::new();
    let mut other = Vec::new();
    for (snapshot, owned) in visible_runs(operations, session_id, now_ms) {
        if should_hide_from_agent_list(&snapshot) {
            continue;
        }
        let live_pane = snapshot
            .pane_id
            .as_deref()
            .and_then(|pane_id| live.get(pane_id));
        if let Some(pane) = live_pane {
            if snapshot.status.terminal() {
                continue;
            }
            claimed.insert(pane.id.clone());
            live_workers.push(run_worker_entry(&snapshot, owned, session_id, registry));
            continue;
        }
        if snapshot.status.terminal() && !explicit_project {
            continue;
        }
        other.push(run_worker_entry(&snapshot, owned, session_id, registry));
    }
    for pane in live.values() {
        if !claimed.contains(&pane.id) {
            live_workers.push(pane_worker_entry(pane, session_id));
        }
    }
    live_workers.extend(other);
    Ok(json!({"workers": live_workers}))
}

fn should_hide_from_agent_list(snapshot: &crate::operations::OperationSnapshot) -> bool {
    matches!(
        snapshot.stage.as_deref(),
        Some("interrupted") | Some("closed")
    )
}

fn run_worker_entry(
    snapshot: &crate::operations::OperationSnapshot,
    owned: bool,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Value {
    let view = AgentRunView::from(snapshot.clone());
    let grant_required = snapshot.pane_id.as_deref().is_some_and(|pane_id| {
        session_id.is_some_and(|session| !crate::mcp_sessions::session_controls_pane(session, pane_id))
    });
    let workspace = snapshot
        .worker
        .workspace
        .clone()
        .unwrap_or_else(|| snapshot.project_path.clone());
    let pane = snapshot.pane_id.as_deref().and_then(|id| {
        registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == id)
    });
    json!({
        "worker_id": view.handle,
        "handle": view.handle,
        "name": snapshot.worker.name,
        "pane_id": snapshot.pane_id,
        "backend": snapshot.agent_type,
        "workspace": pane.as_ref().map(|pane| pane.cwd.clone()).unwrap_or(workspace),
        "status": view.status,
        "model": snapshot.worker.resolved_model.clone().or(snapshot.worker.requested_model.clone()),
        "owned": owned,
        "grant_required": grant_required,
        "read_only_supported": super::capabilities::read_only_supported(&snapshot.agent_type, snapshot.pane_id.is_some()),
        "adoptable": pane
            .as_ref()
            .map(|pane| pane.status != "error")
            .unwrap_or(owned || !grant_required),
    })
}

fn pane_worker_entry(pane: &crate::pty::PaneInfo, session_id: Option<&str>) -> Value {
    let grant_required = session_id
        .is_some_and(|session| !crate::mcp_sessions::session_controls_pane(session, &pane.id));
    json!({
        "worker_id": pane.id,
        "handle": pane.id,
        "pane_id": pane.id,
        "backend": pane.agent_type,
        "workspace": pane.cwd,
        "status": pane.status,
        "model": Value::Null,
        "owned": !grant_required,
        "grant_required": grant_required,
        "read_only_supported": super::capabilities::read_only_supported(&pane.agent_type, true),
        "adoptable": pane.status != "error",
    })
}

pub(crate) fn latest_handle_for_pane(
    project: &str,
    pane_id: &str,
) -> Result<String, OperationError> {
    let mut best: Option<crate::operations::OperationSnapshot> = None;
    for snapshot in operations::list_operations(project)? {
        if snapshot.pane_id.as_deref() != Some(pane_id) {
            continue;
        }
        let replace = best
            .as_ref()
            .map_or(true, |existing| snapshot.turn_index >= existing.turn_index);
        if replace {
            best = Some(snapshot);
        }
    }
    best.map(|snapshot| {
        if snapshot.agent_run_id.is_empty() {
            snapshot.operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        }
    })
    .ok_or_else(|| {
        OperationError::new(
            "AGENT_NOT_FOUND",
            format!(
                "no agent run is bound to pane {pane_id}; use list_agents for the run handle"
            ),
            false,
        )
    })
}

fn resolve_run_handle(
    project: &str,
    id: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Result<String, OperationError> {
    if operations::resolve_agent_run(project, id).is_ok() {
        return Ok(id.to_string());
    }
    let pane_ids: Vec<String> = registry.lock().panes.keys().cloned().collect();
    if let Ok(pane_id) = crate::mcp_sessions::resolve_pane_id(id, &pane_ids) {
        let kind = if pane_id == id {
            "a pane id"
        } else {
            "a pane id prefix"
        };
        let run_handle = latest_handle_for_pane(project, &pane_id).ok();
        let mut error = OperationError::new(
            "INVALID_AGENT_HANDLE",
            crate::mcp_sessions::pane_id_as_handle_message(
                id,
                kind,
                &pane_id,
                run_handle.as_deref(),
            ),
            false,
        );
        error.context = json!({"handle": id, "expected": "agent_run_handle", "actual_kind": "pane_id", "pane_id": pane_id, "run_handle": run_handle});
        return Err(error);
    }
    match operations::resolve_agent_run(project, id) {
        Ok(_) => Ok(id.to_string()),
        Err(error) if error.code == "AGENT_NOT_FOUND" => {
            // Stable worker names survive restarts; accept one that matches a single run.
            match super::persist::resolve_worker_name(project, id)? {
                Some(handle) => Ok(handle),
                None => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn wait_agents(
    project: &str,
    request: WaitAgentsRequest,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Result<Value, OperationError> {
    if request.handles.is_empty() {
        return Err(OperationError::new(
            "INVALID_AGENT_HANDLES",
            "handles must not be empty",
            false,
        ));
    }
    let mut cursors = Vec::new();
    let mut caller_cursors = Vec::new();
    let mut latest_by_id = HashMap::new();
    for raw in &request.handles {
        let handle = resolve_run_handle(project, raw, registry)?;
        let snapshot = runtime::current_agent_for_wait(project, &handle, session_id)?;
        let explicit = request
            .after_revisions
            .get(raw.as_str())
            .or_else(|| request.after_revisions.get(&handle))
            .copied()
            .or(request.after_cursor);
        caller_cursors.push(explicit);
        cursors.push(OperationWaitCursor {
            project_path: snapshot.project_path.clone(),
            operation_id: snapshot.operation_id.clone(),
            after_revision: explicit
                .unwrap_or(snapshot.worker.event_cursor.max(snapshot.revision)),
        });
        latest_by_id.insert(snapshot.operation_id.clone(), handle);
    }
    let wait_all = request.mode.as_deref() == Some("all");
    if request
        .mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "all" | "any"))
    {
        return Err(OperationError::new(
            "INVALID_WAIT_MODE",
            "mode must be 'any' or 'all'",
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
    let result = operations::wait_for_operations(
        &cursors,
        request.until,
        request.wait_ms,
        wait_all,
        request.wake_on_progress,
    )?;
    let agents = result
        .snapshots
        .into_iter()
        .zip(caller_cursors.iter())
        .zip(result.wake_reasons.iter())
        .map(|((snapshot, caller_cursor), wake_reason)| {
            let overlay_snapshot = snapshot.clone();
            let view = runtime::agent_view_after_wait(
                snapshot,
                *caller_cursor,
                Some(wake_reason.clone()),
            );
            let mut value = serde_json::to_value(view).unwrap_or(Value::Null);
            super::persist::overlay_live_model(&mut value, &overlay_snapshot, Some(registry));
            value
        })
        .collect::<Vec<_>>();
    Ok(json!({"agents": agents, "reason": result.reason, "mode": if wait_all {"all"} else {"any"}}))
}

fn transcript(
    project: &str,
    handle: &str,
    query: &str,
    session_id: Option<&str>,
) -> Result<Value, OperationError> {
    let snapshot = runtime::current_agent_for_read(project, handle, session_id)?;
    let project = snapshot.project_path.as_str();
    let after = query_value(query, "after")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let page = if let Some(operation_id) = query_value(query, "operation_id") {
        let selected = operations::get_operation(project, &operation_id)?;
        if selected.agent_run_id != handle && selected.operation_id != handle {
            return Err(OperationError::new(
                "AGENT_NOT_FOUND",
                "operation does not belong to this agent handle",
                false,
            ));
        }
        runtime::transcript_page(project, &operation_id, after)?
    } else {
        runtime::transcript_for_handle(project, handle, after)?
    };
    serde_json::to_value(page)
        .map_err(|error| OperationError::new("SERIALIZATION_ERROR", error.to_string(), false))
}

fn answer_prompt(
    project: &str,
    request: &AnswerPromptRequest,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
) -> Result<Value, OperationError> {
    let handle = request.handle.as_deref().unwrap_or_default();
    let snapshot = runtime::current_agent(project, handle, session_id)?;
    let action = snapshot.required_action.as_ref().ok_or_else(|| {
        OperationError::new("PROMPT_NOT_PENDING", "agent has no pending prompt", false)
    })?;
    if snapshot.status != OperationStatus::WaitingInput {
        return Err(OperationError::new(
            "PROMPT_NOT_PENDING",
            "agent is not waiting for input",
            false,
        ));
    }
    let matches_prompt = ["prompt_id", "request_id", "message_id", "id"]
        .iter()
        .filter_map(|key| action.get(*key).and_then(Value::as_str))
        .any(|id| id == request.prompt_id)
        || action
            .get("permission_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&request.prompt_id)));
    if !matches_prompt {
        return Err(OperationError::new(
            "STALE_PROMPT",
            "prompt id is no longer current",
            false,
        ));
    }
    if matches!(
        request.choice.as_str(),
        "allow_all" | "allow_broad" | "always"
    ) && !request.allow_broad
    {
        return Err(OperationError::new(
            "BROAD_APPROVAL_REQUIRES_OPT_IN",
            "allow_all requires allow_broad=true",
            false,
        ));
    }
    let pane_id = snapshot.pane_id.as_deref().ok_or_else(|| {
        OperationError::new(
            "PROMPT_UNAVAILABLE",
            "headless worker does not accept interactive prompt replies; for workspace_trust, trust the folder in an interactive agent yourself, then retry with send_agent",
            false,
        )
    })?;
    let kind = action
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if snapshot.agent_type == "opencode_native" && kind == "permission_required" {
        let choice = match request.choice.as_str() {
            "allow_once" | "once" => "once",
            "deny" | "reject" => "reject",
            "allow_broad" | "always" if request.allow_broad => "always",
            _ => return Err(OperationError::new("INVALID_PROMPT_CHOICE", "permission choice must be allow_once or deny; allow_broad requires allow_broad=true", false)),
        };
        if choice == "always" && !request.allow_broad {
            return Err(OperationError::new(
                "INVALID_PROMPT_CHOICE",
                "broad permission requires allow_broad=true",
                false,
            ));
        }
        crate::opencode::reply_pane_permission(registry, pane_id, &request.prompt_id, choice)
            .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
    } else if snapshot.agent_type == "opencode_native" && kind == "question_required" {
        let question = crate::opencode::messages::pending_question_for_pane(registry, pane_id)
            .ok_or_else(|| {
                OperationError::new(
                    "STALE_PROMPT",
                    "native question is no longer pending",
                    false,
                )
            })?;
        let allowed = question
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>();
        if !allowed.is_empty() && !allowed.contains(&request.choice.as_str()) {
            return Err(OperationError::new(
                "INVALID_PROMPT_CHOICE",
                "answer must match one of the current explicit options",
                false,
            ));
        }
        crate::opencode::reply_pane_question(
            registry,
            pane_id,
            &request.choice,
            Some(&request.prompt_id),
        )
        .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
    } else {
        let screen = {
            let locked = registry.lock();
            let pane = locked.panes.get(pane_id).ok_or_else(|| {
                OperationError::new("PANE_NOT_FOUND", "agent pane is no longer available", false)
            })?;
            let contents = pane.screen.lock().screen().contents();
            contents
        };
        let adapter = crate::agent_adapters::adapter_for(&snapshot.agent_type);
        let prompt = adapter
            .detect_prompt(&screen, request.allow_broad)
            .ok_or_else(|| {
                OperationError::new(
                    "STALE_PROMPT",
                    "current screen no longer contains a supported prompt",
                    false,
                )
            })?;
        if super::native::stable_prompt_id(&prompt).as_deref() != Some(request.prompt_id.as_str()) {
            return Err(OperationError::new(
                "STALE_PROMPT",
                "current terminal prompt changed",
                false,
            ));
        }
        let choice = match request.choice.as_str() {
            "once" => "allow_once",
            "reject" => "deny",
            "always" | "allow_broad" if request.allow_broad => "allow_all",
            choice => choice,
        };
        let reply = adapter
            .prompt_reply(&prompt, choice, request.allow_broad)
            .map_err(|error| OperationError::new("INVALID_PROMPT_CHOICE", error, false))?;
        let text = reply.key.or(reply.text).ok_or_else(|| {
            OperationError::new(
                "INVALID_PROMPT_CHOICE",
                "adapter did not produce a reply",
                false,
            )
        })?;
        crate::pty::registry::write_input(registry, app, pane_id, &text, true, false, None)
            .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
    }
    let updated = operations::mark_operation_observation(
        project,
        &snapshot.operation_id,
        Some("running".into()),
        None,
        StateSource::Native,
    )?;
    crate::bridge::publish_operation(&updated, registry, app)?;
    serde_json::to_value(AgentRunView::from(updated))
        .map_err(|error| OperationError::new("SERIALIZATION_ERROR", error.to_string(), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_handles_project_paths_and_plus() {
        assert_eq!(
            percent_decode("C%3A%2FMy+Project%2Frepo"),
            "C:/My Project/repo"
        );
    }

    #[test]
    fn pane_worker_entry_exposes_adoption_fields() {
        let pane = crate::pty::PaneInfo {
            id: "30313db4-8896-46ee-8bd6-556b9cb0a014".into(),
            agent_type: "opencode_native".into(),
            pid: 1,
            status: "running".into(),
            created_at: 0,
            last_output_at: None,
            cwd: "C:/work/puppet-master/packages/app/src-tauri".into(),
            cols: 80,
            rows: 24,
        };
        let entry = pane_worker_entry(&pane, Some("mcp-session"));
        assert_eq!(entry["worker_id"], pane.id);
        assert_eq!(entry["backend"], "opencode_native");
        assert_eq!(entry["workspace"], pane.cwd);
        assert_eq!(entry["grant_required"], true);
        assert_eq!(entry["read_only_supported"], false);
        assert_eq!(entry["adoptable"], true);
    }

    fn pane_bound_run(
        with_run: bool,
    ) -> (String, Arc<Mutex<crate::pty::PaneRegistry>>, String) {
        let project = std::env::temp_dir()
            .join(format!("pm-pane-as-handle-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&project).unwrap();
        let pane_id = format!("pane-{}", uuid::Uuid::new_v4());
        let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::default()));
        registry.lock().panes.insert(
            pane_id.clone(),
            crate::pty::PaneRegistry::test_pane_stub(&pane_id),
        );
        if with_run {
            let req = crate::operations::DelegateWorkRequest {
                project_path: project.clone(),
                task: "t".into(),
                agent_type: "opencode_native".into(),
                pane_id: Some(pane_id.clone()),
                idempotency_key: format!("k-{pane_id}"),
                acceptance_criteria: None,
                task_id: None,
                exclusive: false,
                locks: vec![],
                timeout_ms: 60_000,
                read_only: false,
                keep_pane: true,
                owner_session_id: None,
                worker_has_mcp_tools: false,
                agent_run_id: Some("run-handle-1".into()),
                turn_index: 0,
                worker: crate::operations::WorkerPersist::default(),
                context_policy: None,
                checks: Vec::new(),
            };
            crate::operations::create_operation(req).unwrap();
        }
        (project, registry, pane_id)
    }

    fn assert_pane_as_handle_message(error: &OperationError, pane_id: &str, run: Option<&str>) {
        assert_eq!(error.code, "INVALID_AGENT_HANDLE");
        assert!(error.message.contains("a pane id"), "{}", error.message);
        assert!(error.message.contains("not an agent run handle"));
        assert!(error.message.contains("run_agent result field `handle`"));
        assert!(error.message.contains("list_agents `handle`"));
        assert!(error.message.contains(pane_id));
        match run {
            Some(handle) => assert!(error.message.contains(&format!("`{handle}`"))),
            None => assert!(error.message.contains("no known run")),
        }
    }

    #[test]
    fn wait_agents_inspect_and_transcript_explain_pane_id_given_as_handle() {
        let (project, registry, pane_id) = pane_bound_run(true);
        let request: WaitAgentsRequest =
            serde_json::from_value(json!({"handles": [pane_id.clone()]})).unwrap();
        let wait = wait_agents(&project, request, None, &registry).unwrap_err();
        assert_pane_as_handle_message(&wait, &pane_id, Some("run-handle-1"));
        let inspect = resolve_run_handle(&project, &pane_id, &registry)
            .and_then(|handle| inspect_agent(&project, &handle, None))
            .unwrap_err();
        assert_pane_as_handle_message(&inspect, &pane_id, Some("run-handle-1"));
        let transcript = resolve_run_handle(&project, &pane_id, &registry)
            .and_then(|handle| transcript(&project, &handle, "", None))
            .unwrap_err();
        assert_pane_as_handle_message(&transcript, &pane_id, Some("run-handle-1"));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn pane_id_without_known_run_still_names_kinds_and_sources() {
        let (project, registry, pane_id) = pane_bound_run(false);
        let error = resolve_run_handle(&project, &pane_id, &registry).unwrap_err();
        assert_pane_as_handle_message(&error, &pane_id, None);
        let _ = std::fs::remove_dir_all(&project);
    }

    fn named_run(project: &str, handle: &str, name: &str, turn: u32) {
        let req = crate::operations::DelegateWorkRequest {
            project_path: project.to_string(),
            task: "t".into(),
            agent_type: "opencode_native".into(),
            pane_id: None,
            idempotency_key: format!("k-{handle}-{turn}"),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane: true,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some(handle.into()),
            turn_index: turn,
            worker: crate::operations::WorkerPersist {
                name: Some(name.into()),
                ..Default::default()
            },
            context_policy: None,
            checks: Vec::new(),
        };
        crate::operations::create_operation(req).unwrap();
    }

    #[test]
    fn persisted_worker_name_resolves_to_handle_when_unique() {
        let (project, registry, _pane) = pane_bound_run(false);
        named_run(&project, "run-luna", "luna", 0);
        named_run(&project, "run-luna", "luna", 1);
        named_run(&project, "run-nova", "nova", 0);
        // Same resolution a fresh process uses after restart: name -> persisted handle.
        assert_eq!(
            resolve_run_handle(&project, "luna", &registry).unwrap(),
            "run-luna"
        );
        assert_eq!(
            super::super::persist::find_worker(&project, "luna")
                .unwrap()
                .agent_run_id,
            "run-luna"
        );
        assert!(resolve_run_handle(&project, "unknown-name", &registry).is_err());
        named_run(&project, "run-luna-2", "luna", 0);
        let error = resolve_run_handle(&project, "luna", &registry).unwrap_err();
        assert_eq!(error.code, "AMBIGUOUS_WORKER_NAME");
        let _ = std::fs::remove_dir_all(&project);
    }
}
