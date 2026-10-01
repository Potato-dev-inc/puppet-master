use super::capabilities::{self, BackendCapabilities};
use super::transcript;
use super::messaging::{self, DeliveryMode, Disposition};
use super::persist::{self, FollowupOpts};
use crate::operations::{
    self, AcceptanceStatus, ContextPolicy, OperationError, OperationSnapshot, OperationStatus,
    ResultCapture, StateSource,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use uuid::Uuid;

fn is_false(value: &bool) -> bool {
    !*value
}

const DEFAULT_TIMEOUT_MS: u64 = 900_000;
static HANDLE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn handle_lock(handle: &str) -> Arc<Mutex<()>> {
    HANDLE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .entry(handle.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

pub use super::headless::{run_agent, supervise_existing};

pub(super) fn default_agent() -> String {
    "codex".into()
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}
fn default_wait() -> u64 {
    persist::FOREGROUND_WAIT_MS
}
fn default_headless() -> bool {
    true
}

/// Merge `context_mode` (alias) into `context_policy` before deserializing bodies.
/// Either key alone, neither, or both with the same value are accepted. Two different values
/// are an error unless one is the legacy `packet` schema default, which yields to the other.
/// Null counts as absent.
pub(crate) fn normalize_context_policy_fields(value: &mut Value) -> Result<(), OperationError> {
    let map = match value.as_object_mut() {
        Some(map) => map,
        None => return Ok(()),
    };
    let mode = map.remove("context_mode").filter(|v| !v.is_null());
    let policy = map.remove("context_policy").filter(|v| !v.is_null());
    let merged = match (mode, policy) {
        (None, None) => return Ok(()),
        (Some(v), None) | (None, Some(v)) => v,
        (Some(mode), Some(policy)) => {
            // `packet` was the schema default of both fields. A host (or cached schema) that
            // fills the untouched alias with it must not turn one explicit choice into a conflict.
            let is_default = |v: &Value| v.as_str() == Some("packet");
            if mode != policy && is_default(&policy) {
                mode
            } else if mode != policy && is_default(&mode) {
                policy
            } else if mode != policy {
                return Err(OperationError::new(
                    "INVALID_ARGUMENT",
                    format!(
                        "context_mode and context_policy disagree ({} vs {}); they are aliases, set only one",
                        mode.as_str().unwrap_or("?"),
                        policy.as_str().unwrap_or("?")
                    ),
                    false,
                ));
            } else {
                policy
            }
        }
    };
    map.insert("context_policy".into(), merged);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRequest {
    #[serde(default)]
    pub project_path: Option<String>,
    pub task: String,
    #[serde(default = "default_agent")]
    pub agent_type: String,
    #[serde(default = "default_headless")]
    pub headless: bool,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub keep_pane: bool,
    #[serde(default)]
    pub worker_has_mcp_tools: bool,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_wait")]
    pub wait_ms: u64,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Option<Vec<String>>,
    /// State-based checks (file_exists / file_contains / file_absent) evaluated at completion.
    #[serde(default)]
    pub checks: Option<Vec<super::checks::Check>>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub allow_broad: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub agent_run_id: Option<String>,
    #[serde(default)]
    pub turn_index: u32,
    #[serde(default)]
    pub context_policy: ContextPolicy,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "reasoning_effort")]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub selected_history: Option<Vec<String>>,
    /// Resource locks held for the run (`type:name`, e.g. `file:src/a.rs`).
    #[serde(default)]
    pub locks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunView {
    pub handle: String,
    pub operation_id: String,
    pub turn_id: String,
    pub status: String,
    pub stage: Option<String>,
    pub result: Option<String>,
    pub result_capture: String,
    pub acceptance_status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<super::checks::CheckResult>,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub result_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<serde_json::Value>,
    pub prompt: Option<serde_json::Value>,
    pub error: Option<crate::operations::OperationError>,
    pub pane_id: Option<String>,
    pub duration_ms: Option<u64>,
    pub revision: u64,
    pub next_cursor: u64,
    pub name: Option<String>,
    pub context_policy: String,
    pub context_continuity: String,
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    pub requested_reasoning: Option<String>,
    pub resolved_reasoning: Option<String>,
    pub model_mismatch: bool,
    #[serde(default)]
    pub session_reset: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing)]
    pub capabilities: BackendCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_steer: Option<SteerView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_unchanged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerView {
    pub message_id: String,
    pub state: String,
    pub turn_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

impl From<OperationSnapshot> for AgentRunView {
    fn from(snapshot: OperationSnapshot) -> Self {
        let end = snapshot.finished_at_ms.unwrap_or(snapshot.updated_at_ms);
        let duration_ms = snapshot
            .started_at_ms
            .map(|start| end.saturating_sub(start));
        let capabilities = capabilities::for_snapshot(&snapshot);
        let operation_id = snapshot.operation_id.clone();
        let project_path = snapshot.project_path.clone();
        let handle = if snapshot.agent_run_id.is_empty() {
            operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        };
        let turn_id = format!("turn-{}", snapshot.turn_index);
        if snapshot.status.terminal() {
            messaging::expire_terminal_steers(&snapshot.project_path, &handle);
        }
        let last_steer = messaging::latest_steer(&snapshot.project_path, &handle).and_then(|receipt| {
            let same_turn = receipt.turn_id == turn_id;
            let open = matches!(
                receipt.disposition,
                Disposition::Accepted | Disposition::Queued | Disposition::Deferred
            );
            if !same_turn && !open {
                return None;
            }
            Some(SteerView {
                message_id: receipt.message_id,
                state: receipt.disposition.as_str().into(),
                turn_id: receipt.turn_id,
                result: if same_turn { receipt.result } else { None },
            })
        });
        Self {
            handle,
            turn_id,
            operation_id: operation_id.clone(),
            status: if snapshot
                .error
                .as_ref()
                .is_some_and(|error| error.code == "TIMEOUT")
            {
                "timeout".into()
            } else if snapshot.worker.closed || snapshot.stage.as_deref() == Some("closed") {
                "closed".into()
            } else if snapshot.stage.as_deref() == Some("interrupted") {
                "interrupted".into()
            } else {
                match snapshot.status {
                    OperationStatus::WaitingInput => "needs_input".into(),
                    OperationStatus::Queued
                    | OperationStatus::Starting
                    | OperationStatus::Running
                    | OperationStatus::Cancelling => "running".into(),
                    OperationStatus::Completed => "completed".into(),
                    OperationStatus::Failed => "failed".into(),
                    OperationStatus::Cancelled => "cancelled".into(),
                }
            },
            stage: snapshot.stage,
            result: snapshot.result,
            result_capture: if snapshot.status.terminal() {
                match snapshot.result_capture {
                    Some(ResultCapture::Authoritative) => "authoritative".into(),
                    Some(ResultCapture::Inferred) => "inferred".into(),
                    Some(ResultCapture::Missing) | None => "missing".into(),
                }
            } else {
                "pending".into()
            },
            acceptance_status: match snapshot.acceptance_status {
                AcceptanceStatus::NotChecked => "not_checked".into(),
                AcceptanceStatus::Passed => "passed".into(),
                AcceptanceStatus::Failed => "failed".into(),
                AcceptanceStatus::Blocked => "blocked".into(),
            },
            checks: snapshot.check_results.clone(),
            verified: snapshot.verified,
            result_verified: snapshot.verified,
            model_verified: match (
                snapshot.worker.requested_model.as_deref(),
                snapshot.worker.resolved_model.as_deref(),
            ) {
                (None, _) => None,
                (Some(requested), Some(resolved)) => Some(requested == resolved),
                (Some(_), None) => Some(false),
            },
            recovery: snapshot.required_action.as_ref().and_then(|value| {
                value
                    .get("kind")
                    .and_then(|kind| kind.as_str())
                    .filter(|kind| {
                        matches!(*kind, "session_replaced" | "process_restarted")
                    })
                    .map(|_| value.clone())
            }),
            prompt: snapshot
                .required_action
                .map(super::headless::compact_prompt),
            error: snapshot.error,
            pane_id: snapshot.pane_id,
            duration_ms,
            revision: snapshot.revision,
            next_cursor: snapshot.worker.event_cursor.max(snapshot.revision),
            name: snapshot.worker.name.clone(),
            context_policy: snapshot.worker.context_policy.as_str().into(),
            context_continuity: snapshot.worker.context_continuity.as_str().into(),
            requested_model: snapshot.worker.requested_model.clone(),
            resolved_model: snapshot.worker.resolved_model.clone(),
            requested_reasoning: snapshot.worker.requested_reasoning.clone(),
            resolved_reasoning: snapshot.worker.resolved_reasoning.clone(),
            model_mismatch: persist::visible_mismatch(
                snapshot.worker.requested_model.as_deref(),
                snapshot.worker.resolved_model.as_deref(),
                snapshot.worker.requested_reasoning.as_deref(),
                snapshot.worker.resolved_reasoning.as_deref(),
            ),
            session_reset: snapshot.worker.session_reset,
            provider_session_id: snapshot.worker.provider_session_id.clone(),
            capabilities,
            last_steer,
            result_unchanged: None,
            wake_reason: None,
            watch_command: Some(crate::watch_command::format_watch_command(
                operation_id.as_str(),
                Some(project_path.as_str()),
            )),
        }
    }
}

impl AgentRunView {
    pub fn present(
        snapshot: OperationSnapshot,
        registry: Option<&Arc<Mutex<crate::pty::PaneRegistry>>>,
    ) -> Self {
        let mut view = Self::from(snapshot.clone());
        let live = registry.and_then(|registry| persist::live_resolved_model(registry, &snapshot));
        let (requested, resolved) = persist::resolve_model_fields(
            snapshot.worker.requested_model.as_deref(),
            snapshot.worker.resolved_model.as_deref(),
            live,
            snapshot.agent_type == "opencode_native",
        );
        view.requested_model = requested;
        view.resolved_model = resolved;
        view
    }
}

/// `caller_cursor` is the revision the caller explicitly said it had already seen, if any. The
/// result is omitted as unchanged only then; with no cursor there is nothing to be "unchanged"
/// since, so a run that finished before the wait started must still return its result.
pub fn agent_view_after_wait(
    snapshot: OperationSnapshot,
    caller_cursor: Option<u64>,
    wake_reason: Option<String>,
) -> AgentRunView {
    let mut view = AgentRunView::from(snapshot.clone());
    view.wake_reason = wake_reason;
    if caller_cursor.is_some_and(|seen| snapshot.revision <= seen) && view.result.is_some() {
        view.result = None;
        view.result_unchanged = Some(true);
    }
    view
}

pub fn transcript_page(
    project: &str,
    operation_id: &str,
    after: u64,
) -> Result<transcript::TranscriptPage, OperationError> {
    transcript::read(project, operation_id, after)
        .map_err(|error| OperationError::new("TRANSCRIPT_READ_FAILED", error.to_string(), true))
}

pub fn transcript_for_handle(
    project: &str,
    handle: &str,
    after: u64,
) -> Result<transcript::TranscriptPage, OperationError> {
    let ids = operations::turns_for_handle(project, handle)?
        .into_iter()
        .map(|snapshot| snapshot.operation_id)
        .collect::<Vec<_>>();
    transcript::read_turns(project, &ids, after)
        .map_err(|error| OperationError::new("TRANSCRIPT_READ_FAILED", error.to_string(), true))
}

pub fn record_user_task(project: &str, operation_id: &str, task: &str) {
    transcript::append(project, operation_id, "user_task", task);
}

pub fn current_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    current_agent_with_access(project, handle, session_id, RunAccess::Mutate)
}

#[derive(Clone, Copy)]
enum RunAccess {
    Read,
    Wait,
    Mutate,
}

fn current_agent_with_access(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    access: RunAccess,
) -> Result<OperationSnapshot, OperationError> {
    let snapshot = operations::resolve_agent_run(project, handle)?;
    if let Some(session) = session_id {
        if crate::mcp_sessions::agent_is_registered(handle) {
            let check = match access {
                RunAccess::Mutate => crate::mcp_sessions::check_run_access(session, handle),
                RunAccess::Wait => crate::mcp_sessions::check_run_wait_access(session, handle),
                RunAccess::Read => crate::mcp_sessions::check_run_read_access(session, handle),
            };
            check?;
        } else if snapshot
            .owner_session_id
            .as_deref()
            .is_some_and(|owner| owner != session)
        {
            return Err(OperationError::new(
                "AUTHORIZATION_DENIED",
                "agent handle belongs to a different MCP session",
                false,
            ));
        }
    }
    Ok(snapshot)
}

pub fn current_agent_for_read(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    current_agent_with_access(project, handle, session_id, RunAccess::Read)
}

pub fn current_agent_for_wait(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    current_agent_with_access(project, handle, session_id, RunAccess::Wait)
}

pub fn cancel_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> Result<AgentRunView, OperationError> {
    interrupt_agent(project, handle, session_id, registry, app)
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

pub(super) fn interrupt_active_turns(
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
            Some("turn interrupted before a final response was recorded".into())
        });
        transcript::append(
            project,
            &snapshot.operation_id,
            "interrupted",
            partial.as_deref().unwrap_or("interrupted"),
        );
        let interrupted =
            operations::mark_turn_interrupted(project, &snapshot.operation_id, partial)?;
        publish(&interrupted)?;
    }
    let final_snapshot = current_agent(project, handle, session_id)?;
    Ok(final_snapshot.into())
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
                crate::opencode::client::abort_session(&base, &session, Some(&directory)).map_err(
                    |error| OperationError::new("INTERRUPT_FAILED", error, true),
                )?;
                let _ = crate::opencode::status::wait_until_native_accepts_prompt(
                    registry,
                    pane_id,
                    Duration::from_secs(5),
                );
            }
            Ok(())
        }
        Some(messaging::InterruptAction::StopHeadless) => {
            match super::headless::cancel_headless(snapshot) {
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
            crate::pty::registry_write_input(registry, app, pane_id, &sequence, false, false, None)
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
    let view = interrupt_agent(
        project,
        handle,
        session_id,
        registry.clone(),
        app.clone(),
    )?;
    if let Ok(latest) = operations::resolve_agent_run(project, handle) {
        super::pane_close::dispose_explicit_close(&latest, &registry)?;
    }
    if let Some(session) = session_id {
        let _ = crate::mcp_sessions::release_owned_agent(session, handle);
    }
    operations::mark_worker_closed(project, handle).map(AgentRunView::from).or(Ok(view))
}

pub(super) fn active_turns(
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

pub fn send_message(
    project: &str,
    handle: &str,
    message: String,
    session_id: Option<&str>,
    delivery: Option<&str>,
    idempotency_key: Option<&str>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, OperationError> {
    let snapshot = current_agent(project, handle, session_id)?;
    let mode = DeliveryMode::parse(delivery);
    let mut receipt = messaging::send_message(
        project,
        handle,
        &message,
        &snapshot,
        mode,
        idempotency_key,
    )?;
    if receipt.disposition == Disposition::Accepted {
        if let Err(error) = deliver_live(&snapshot, &message, &registry, &app) {
            if mode == DeliveryMode::Live {
                return Err(error);
            }
            receipt.disposition = Disposition::Queued;
            messaging::update_receipt(project, handle, &receipt)?;
        } else {
            rebase_native_steer(&snapshot, &registry)?;
        }
    }
    if matches!(
        receipt.disposition,
        Disposition::Accepted | Disposition::Queued | Disposition::Deferred
    ) {
        transcript::append(project, &snapshot.operation_id, "user_steer", &message);
    }
    let mut value = messaging::receipt_value(&receipt);
    if let Ok(current) = current_agent(project, handle, session_id) {
        if let Some(object) = value.as_object_mut() {
            object.insert("next_cursor".into(), json!(current.worker.event_cursor.max(current.revision)));
            object.insert("last_steer".into(), serde_json::to_value(SteerView {
                message_id: receipt.message_id.clone(),
                state: receipt.disposition.as_str().into(),
                turn_id: receipt.turn_id.clone(),
                result: receipt.result.clone(),
            }).unwrap_or(serde_json::Value::Null));
        }
    }
    Ok(value)
}

fn rebase_native_steer(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Result<(), OperationError> {
    let Some(pane_id) = snapshot.pane_id.as_deref() else {
        return Ok(());
    };
    let ids = crate::opencode::messages::read_pane_messages(registry.as_ref(), pane_id, 10_000, None)
        .ok()
        .map(|view| {
            view.messages
                .into_iter()
                .filter_map(|message| message.id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    operations::set_message_baseline(
        &snapshot.project_path,
        &snapshot.operation_id,
        ids,
    )?;
    Ok(())
}

fn deliver_live(
    snapshot: &OperationSnapshot,
    message: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) -> Result<(), OperationError> {
    if !capabilities::for_snapshot(snapshot).live_messages {
        return Err(OperationError::new(
            "DELIVERY_UNSUPPORTED",
            "live steering is only supported for backends that claim live_messages",
            false,
        ));
    }
    let pane_id = snapshot.pane_id.as_deref().ok_or_else(|| {
        OperationError::new(
            "DELIVERY_UNSUPPORTED",
            "native worker has no pane for live steering",
            true,
        )
    })?;
    crate::opencode::write_native_input(registry, app, pane_id, message, None).map_err(|error| {
        OperationError::new("DELIVERY_FAILED", error, true)
    })
}

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
    let blocked_on_trust = super::headless::blocked_on_trust(&previous);
    if !previous.status.terminal() && !blocked_on_trust {
        let queued = queue_followup(
            &project,
            handle,
            previous,
            task,
            session_id,
            registry,
            app,
            opts,
        );
        drop(serial);
        return queued;
    }
    if let Some(pane_id) = previous.pane_id.as_deref() {
        if !registry.lock().panes.contains_key(pane_id) {
            if let Some(error) = super::pane_close::closed_after_run_in(
                std::slice::from_ref(&previous),
                pane_id,
                false,
            ) {
                return Err(error);
            }
            return Err(super::pane_close::bound_pane_missing_error(pane_id));
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
        format!(
            "{task}\n\nSteering messages:\n- {}",
            pending.join("\n- ")
        )
    };
    let request = AgentRunRequest {
        project_path: Some(project.to_string()),
        task,
        agent_type: previous.agent_type.clone(),
        headless,
        background: opts.background,
        read_only: previous.read_only,
        keep_pane: previous.keep_pane,
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
        acceptance_criteria: None,
        checks: None,
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
        locks: Vec::new(),
    };
    drop(serial);
    run_agent(request, registry, app, session_id)
}

pub(super) fn enqueue_followup(
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
        policy,
        task: task.trim(),
        scope: opts
            .scope
            .as_deref()
            .or(previous.worker.scope.as_deref()),
        prior_result: previous.result.as_deref(),
        prior_user: conversation.as_deref(),
        selected_history: opts.selected_history.as_deref().unwrap_or(&[]),
        can_resume: persist::snapshot_can_resume(previous),
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
    operations::create_operation(operations::DelegateWorkRequest {
        project_path: project.to_string(),
        task,
        agent_type: previous.agent_type.clone(),
        pane_id: previous.pane_id.clone(),
        idempotency_key,
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: Vec::new(),
        timeout_ms: previous.timeout_ms,
        read_only: previous.read_only,
        keep_pane: previous.keep_pane,
        owner_session_id: session_id.or(previous.owner_session_id.clone()),
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.to_string()),
        turn_index: next_turn,
        worker,
        context_policy: Some(policy),
        checks: Vec::new(),
    })
}

fn queue_followup(
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
    let (queued, created) = enqueue_followup(
        project,
        handle,
        &previous,
        task,
        session_id.clone(),
        opts,
    )?;
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
                super::headless::execute_headless(
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
                super::headless::publish(
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
