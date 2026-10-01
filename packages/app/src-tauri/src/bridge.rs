//! Embedded HTTP bridge for external MCP clients.
//!
//! This bridge intentionally shares the same Rust `PaneRegistry` used by the
//! Tauri commands. External MCP calls therefore operate on the real visible
//! terminal panes instead of a stub sidecar process.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::events::{ResourceId, SystemEvent, TaskId};
use crate::mobile_pairing::{self, PairRequestBody};
use crate::pty::agents::AgentType;
use crate::pty::{
    registry_kill_pane, registry_read_buffer, registry_read_raw_buffer, registry_read_snapshot,
    registry_set_project_path, registry_spawn_pane, registry_write_input, PaneRegistry,
    SpawnPaneArgs,
};
use crate::settings_store;

/// SSE client — each connected GET /events response gets a sender.
type SseSender = std::sync::mpsc::SyncSender<String>;
pub type SseClients = Arc<Mutex<Vec<SseSender>>>;

/// Global SSE client registry, shared between bridge thread and Tauri commands.
static SSE_CLIENTS: once_cell::sync::OnceCell<SseClients> = once_cell::sync::OnceCell::new();
static BRIDGE_APP: once_cell::sync::OnceCell<AppHandle> = once_cell::sync::OnceCell::new();
static BRIDGE_REGISTRY: once_cell::sync::OnceCell<Arc<Mutex<PaneRegistry>>> =
    once_cell::sync::OnceCell::new();
static RESOURCE_LOCK_SERIAL: once_cell::sync::Lazy<Mutex<()>> =
    once_cell::sync::Lazy::new(|| Mutex::new(()));
static PANE_ASSIGNMENT_SERIAL: once_cell::sync::Lazy<Mutex<()>> =
    once_cell::sync::Lazy::new(|| Mutex::new(()));

fn spawn_pane_with_timeout(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    args: SpawnPaneArgs,
    timeout: Duration,
) -> Result<String, crate::operations::OperationError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let registry = Arc::clone(registry);
    let app = app.clone();
    thread::Builder::new()
        .name("operation-pane-spawn".into())
        .spawn(move || {
            let _ = tx.send(registry_spawn_pane(&registry, &app, args));
        })
        .map_err(|err| {
            crate::operations::OperationError::new("PANE_SPAWN_FAILED", err.to_string(), true)
        })?;
    match rx.recv_timeout(timeout) {
        Ok(Ok(pane_id)) => Ok(pane_id),
        Ok(Err(err)) => Err(crate::operations::OperationError::new(
            "PANE_SPAWN_FAILED",
            err,
            true,
        )),
        Err(_) => Err(crate::operations::OperationError::new(
            "PANE_SPAWN_FAILED",
            format!(
                "timed out after {}s waiting for worker pane spawn",
                timeout.as_secs()
            ),
            true,
        )),
    }
}

/// Set in `start_embedded_bridge` for code paths that need AppHandle without an explicit parameter.
#[allow(dead_code)]
pub fn app_handle() -> Option<AppHandle> {
    BRIDGE_APP.get().cloned()
}

#[allow(dead_code)]
pub fn registry() -> Option<Arc<Mutex<PaneRegistry>>> {
    BRIDGE_REGISTRY.get().cloned()
}

pub fn get_sse_clients() -> SseClients {
    SSE_CLIENTS
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

/// Push a raw SSE payload (e.g. "event: chat\ndata: {...}\n\n") to all connected clients.
pub fn push_sse(payload: String) {
    let clients = get_sse_clients();
    let mut guard = clients.lock();
    guard.retain(|sender| sender.try_send(payload.clone()).is_ok());
}

/// Forward live PTY bytes to mobile xterm.js clients.
pub fn push_terminal_sse(pane_id: &str, data: &[u8]) {
    let payload = json!({ "pane_id": pane_id, "data": data });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: terminal\ndata: {json}\n\n"));
    }
    crate::pane_wait_notify::bump_waiters();
}

/// Forward pane status changes to mobile clients.
pub fn push_pane_status_sse(pane_id: &str, status: &str) {
    observe_operation_pane_status(pane_id, status);
    let payload = json!({ "pane_id": pane_id, "status": status });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-status\ndata: {json}\n\n"));
        crate::pane_wait_notify::bump_waiters();
    }
}

fn observe_operation_pane_status(pane_id: &str, status: &str) {
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
    let observed_state = if native_action.is_some() {
        "waiting_input"
    } else {
        status
    };
    let action = native_action.or_else(|| (status == "waiting_input").then(|| json!({
        "kind": "manual_input_required",
        "pane_id": pane_id,
        "detail": "Inspect the pane and resolve the prompt manually. No permission was approved automatically."
    })));
    if let Ok(snapshot) = crate::operations::mark_operation_observation(
        &project_text,
        &operation.operation_id,
        Some(observed_state.to_string()),
        action,
        if native_status.is_some() {
            crate::operations::StateSource::Native
        } else {
            crate::operations::StateSource::Inferred
        },
    ) {
        push_operation_sse(&snapshot);
    }
}

/// Forward PTY geometry changes so remote viewers can mirror without resizing the PTY.
pub fn push_pane_resize_sse(pane_id: &str, cols: u16, rows: u16) {
    let payload = json!({ "pane_id": pane_id, "cols": cols, "rows": rows });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-resize\ndata: {json}\n\n"));
    }
}

const HOST: &str = "127.0.0.1";
const PORT_MIN: u16 = 17321;
const PORT_MAX: u16 = 17399;

#[derive(Debug, Deserialize)]
struct WriteInputBody {
    text: String,
    #[serde(default = "default_true")]
    append_newline: bool,
    #[serde(default)]
    via_opencode_api: bool,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SwitchModelBody {
    #[serde(default)]
    model_provider: Option<String>,
    model_id: String,
}

#[derive(Debug, Deserialize)]
struct PressKeyBody {
    key: String,
}

#[derive(Debug, Deserialize)]
struct OpenCodePermissionReplyBody {
    reply: String,
}

#[derive(Debug, Deserialize)]
struct OpenCodeQuestionReplyBody {
    answer: String,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeKeyRotateBody {
    #[serde(default)]
    profile: Option<String>,
    pane_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OrchestratorMessageBody {
    pub text: String,
    pub message_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OrchestratorViewportBody {
    pub width: f64,
    pub height: f64,
    pub active: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ResizeBody {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
struct ProjectPathBody {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CreateTaskBody {
    title: String,
    #[serde(default)]
    exclusive: bool,
}

#[derive(Debug, Deserialize)]
struct ClaimTaskBody {
    agent_id: String,
    lease_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TaskStatusBody {
    status: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CompleteTaskBody {
    agent_id: String,
    evidence: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BlockTaskBody {
    agent_id: String,
    reason: String,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssignReviewerBody {
    reviewer_id: String,
}

#[derive(Debug, Deserialize)]
struct AcquireLockBody {
    resource_type: String,
    name: String,
    owner_id: String,
    lease_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReleaseLockBody {
    resource_type: String,
    name: String,
    owner_id: String,
}

#[derive(Debug, Deserialize)]
struct WaitOperationBody {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    after_revision: Option<u64>,
    #[serde(default)]
    until: Option<Vec<crate::operations::OperationStatus>>,
    #[serde(default = "default_operation_wait_ms")]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
struct CancelOperationBody {
    #[serde(default)]
    project_path: Option<String>,
}

fn default_operation_wait_ms() -> u64 {
    30_000
}

fn parse_operation_lock(
    value: &str,
) -> Result<(String, String), crate::operations::OperationError> {
    let Some((resource_type, name)) = value.split_once(':') else {
        return Err(crate::operations::OperationError::new(
            "INVALID_LOCK",
            "requested locks must use type:name format",
            false,
        ));
    };
    if resource_type.trim().is_empty() || name.trim().is_empty() {
        return Err(crate::operations::OperationError::new(
            "INVALID_LOCK",
            "requested lock type and name must be non-empty",
            false,
        ));
    }
    Ok((resource_type.trim().to_string(), name.trim().to_string()))
}

fn explicit_manual_approval_required(
    agent_type: &str,
    transcript: &str,
    pending_permission_ids: &[String],
) -> bool {
    if agent_type == "opencode_native" && !pending_permission_ids.is_empty() {
        return true;
    }
    let lower = transcript.to_ascii_lowercase();
    [
        "approval required",
        "requires your approval",
        "permission required",
        "permission request",
        "allow this command",
        "do you want to allow",
        "approve this command",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
}

fn startup_manual_approval_required(
    agent_type: &str,
    pane_status: Option<&str>,
    current_screen: &str,
    pending_permission_ids: &[String],
) -> bool {
    if agent_type == "opencode_native" && !pending_permission_ids.is_empty() {
        return true;
    }
    pane_status == Some("waiting_input")
        && explicit_manual_approval_required(agent_type, current_screen, pending_permission_ids)
}

fn native_status_unavailable(error: impl std::fmt::Display) -> crate::operations::OperationError {
    crate::operations::OperationError::new(
        "NATIVE_STATUS_UNAVAILABLE",
        format!("could not verify native worker permissions: {error}"),
        true,
    )
}

/// OpenCode native: trust the session API (healthy, attached, not generating).
/// Other backends still require a TUI-idle pane.
fn pane_ready_for_dispatch(
    pane: &crate::pty::PaneInfo,
    agent_type: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> bool {
    if pane.agent_type != agent_type || pane.status == "error" {
        return false;
    }
    if agent_type == "opencode_native" {
        return crate::opencode::status::pane_native_accepts_prompt(registry, &pane.id)
            .unwrap_or(pane.status == "idle");
    }
    pane.status == "idle"
}

fn wait_for_startup_permission_resolution(
    registry: &Arc<Mutex<PaneRegistry>>,
    project: &str,
    operation_id: &str,
    pane_id: &str,
    agent_type: &str,
) -> Result<String, crate::operations::OperationError> {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut wake_generation = 0_u64;
    loop {
        let operation = crate::operations::get_operation(project, operation_id)?;
        if operation.status == crate::operations::OperationStatus::Cancelling
            || operation.status == crate::operations::OperationStatus::Cancelled
        {
            return Err(crate::operations::OperationError::new(
                "OPERATION_CANCELLED",
                "operation was cancelled while waiting for manual permission resolution",
                false,
            ));
        }
        let pane = registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
            .ok_or_else(|| {
                crate::operations::OperationError::new(
                    "PANE_UNAVAILABLE",
                    "assigned pane disappeared while waiting for permission resolution",
                    true,
                )
            })?;
        if pane.status == "error" {
            return Err(crate::operations::OperationError::new(
                "PANE_READINESS_FAILED",
                "assigned pane entered an error state while waiting for permission resolution",
                true,
            ));
        }
        let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
        let permissions = if agent_type == "opencode_native" {
            crate::opencode::status::worker_status(registry, pane_id)
                .map_err(|error| {
                    crate::operations::OperationError::new("NATIVE_STATUS_UNAVAILABLE", error, true)
                })?
                .pending_permission_ids
        } else {
            Vec::new()
        };
        if !explicit_manual_approval_required(agent_type, &screen, &permissions) {
            return Ok(pane.status);
        }
        if Instant::now() >= deadline {
            return Err(crate::operations::OperationError::new(
                "APPROVAL_WAIT_FAILED",
                "permission prompt remained unresolved for five minutes",
                true,
            ));
        }
        crate::pane_wait_notify::wait_for_change(deadline, &mut wake_generation);
    }
}

#[derive(Debug, Deserialize)]
struct SessionContextPatchBody {
    current_goal: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetPaneRoleBody {
    role: crate::session_context::PaneRole,
}

#[derive(Debug, Deserialize)]
struct PaneDigestBody {
    summary: String,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrchestratorStatePatchBody {
    standby_poll_ms: Option<u64>,
    standby_max_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
    version: &'static str,
    catalog_version: String,
    tool_count: usize,
}

fn default_true() -> bool {
    true
}

pub struct BridgeHandle {
    pub url: String,
}

impl BridgeHandle {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

fn bind_listener() -> Result<(TcpListener, u16), String> {
    for port in PORT_MIN..=PORT_MAX {
        match TcpListener::bind((HOST, port)) {
            Ok(listener) => return Ok((listener, port)),
            Err(_) => continue,
        }
    }
    Err(format!("no free bridge port in {PORT_MIN}-{PORT_MAX}"))
}

pub fn start_embedded_bridge(
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
    port_file: PathBuf,
    pairing_file: PathBuf,
) -> Result<BridgeHandle, String> {
    let _ = BRIDGE_APP.set(app.clone());
    let _ = BRIDGE_REGISTRY.set(registry.clone());
    if let Ok(recovered) = crate::operations::recover_all_interrupted_operations() {
        for snapshot in recovered {
            push_operation_sse(&snapshot);
        }
    }
    mobile_pairing::init_pairing_store(pairing_file)?;
    let _ = crate::app_paths::ensure_app_data_dir();
    let (listener, port) = bind_listener()?;
    let url = format!("http://{HOST}:{port}");
    fs::write(&port_file, format!("{HOST}:{port}\n"))
        .map_err(|err| format!("write bridge port file {}: {err}", port_file.display()))?;

    thread::Builder::new()
        .name("puppet-master-http-bridge".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let registry = registry.clone();
                        let app = app.clone();
                        thread::spawn(move || {
                            if let Err(err) = handle_connection(stream, registry, app) {
                                tracing::debug!(%err, "bridge request failed");
                            }
                        });
                    }
                    Err(err) => tracing::debug!(%err, "bridge accept failed"),
                }
            }
        })
        .map_err(|err| format!("spawn bridge thread: {err}"))?;

    Ok(BridgeHandle::new(url))
}

fn handle_connection(
    mut stream: TcpStream,
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
) -> Result<(), String> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end;
    loop {
        let n = stream
            .read(&mut chunk)
            .map_err(|err| format!("read request: {err}"))?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buf) {
            header_end = pos;
            break;
        }
        if buf.len() > 1024 * 1024 {
            return Err("request headers too large".into());
        }
    }

    let headers_raw = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = headers_raw.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "missing request line".to_string())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or("GET");
    let target = request_parts.next().unwrap_or("/");
    let content_length = headers_raw
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let body_start = header_end + 4;
    while buf.len().saturating_sub(body_start) < content_length {
        let n = stream
            .read(&mut chunk)
            .map_err(|err| format!("read body: {err}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body =
        &buf[body_start..body_start + content_length.min(buf.len().saturating_sub(body_start))];

    let peer_loopback = stream
        .peer_addr()
        .map(|addr| addr.ip().is_loopback())
        .unwrap_or(false);

    if method == "OPTIONS" {
        return write_json(&mut stream, 204, &json!({}));
    }

    let (path, query) = split_target(target);
    let raw_segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let segments = mobile_pairing::normalize_bridge_segments(&raw_segments).to_vec();

    if segments == ["events"] && method == "GET" {
        if let Err((status, value)) =
            mobile_pairing::authorize_bridge_request(&headers_raw, peer_loopback, &segments, method)
        {
            return write_json(&mut stream, status, &value);
        }
        if let Err(error) = crate::mcp_sessions::authorize(
            request_session(&headers_raw),
            method,
            "/events",
            &json!({}),
        ) {
            let (status, value) = operation_http_error(error);
            return write_json(&mut stream, status, &value);
        }
        handle_sse(&mut stream, registry, &app);
        return Ok(());
    }

    let bridge_tool = bridge_tool_name(method, &segments);
    if let Some(tool) = &bridge_tool {
        crate::event_log::append_bridge_event(SystemEvent::McpToolCalled { tool: tool.clone() });
    }

    let result = route(
        &mut stream,
        method,
        &segments,
        query,
        body,
        registry,
        app,
        &headers_raw,
        peer_loopback,
    );
    if let Some(tool) = bridge_tool {
        let (status, ok) = match &result {
            Ok((status, _)) => (*status, *status < 400),
            Err((status, _)) => (*status, false),
        };
        crate::event_log::append_bridge_event(SystemEvent::McpToolCompleted { tool, ok, status });
    }
    match result {
        Ok((0, _)) => Ok(()), // SSE handled inline — stream already consumed
        Ok((status, value)) => write_json(&mut stream, status, &value),
        Err((status, value)) => write_json(&mut stream, status, &value),
    }
}

fn bridge_tool_name(method: &str, segments: &[&str]) -> Option<String> {
    match (method, segments) {
        ("GET", ["health"]) => Some("bridge_health".to_string()),
        ("GET", ["agent-contexts"]) => Some("list_agent_contexts".to_string()),
        ("GET", ["panes"]) => Some("list_panes".to_string()),
        ("POST", ["panes"]) => Some("spawn_agent".to_string()),
        ("DELETE", ["panes", _]) => Some("kill_pane_process".to_string()),
        ("GET", ["panes", _, "buffer"]) => Some("read_terminal_buffer".to_string()),
        ("GET", ["panes", _, "snapshot"]) => Some("read_terminal_snapshot".to_string()),
        ("POST", ["panes", _, "input"]) => Some("write_terminal_input".to_string()),
        ("POST", ["panes", _, "key"]) => Some("press_key".to_string()),
        ("POST", ["panes", _, "detach"]) => Some("detach_terminal_pane".to_string()),
        ("POST", ["panes", "wait", "model"]) => Some("wait_for_model".to_string()),
        ("POST", ["panes", "wait", "task"]) => Some("wait_for_task".to_string()),
        ("POST", ["panes", "wait", "worker"]) => Some("wait_for_worker".to_string()),
        ("POST", ["panes", "wait"]) => Some("wait_for_panes".to_string()),
        ("GET", ["events", "recent"]) => Some("read_recent_events".to_string()),
        ("GET", ["panes", _, "model"]) => Some("inspect_agent_model".to_string()),
        ("POST", ["panes", _, "model"]) => Some("switch_agent_model".to_string()),
        ("GET", ["panes", _, "agent-context"]) => Some("read_agent_context".to_string()),
        ("GET", ["events", "replay", "panes"]) => Some("replay_pane_timeline".to_string()),
        ("GET", ["workspace", "state"]) => Some("get_workspace_state".to_string()),
        ("GET", ["tasks"]) => Some("list_tasks".to_string()),
        ("GET", ["locks"]) => Some("list_locks".to_string()),
        ("GET", ["agents", _, "inbox"]) => Some("read_agent_inbox".to_string()),
        ("GET", ["audit"]) => Some("get_audit".to_string()),
        ("POST", ["context-packs"]) => Some("build_context_pack".to_string()),
        ("GET", ["project-ir", "status"]) => Some("read_project_ir_status".to_string()),
        ("GET", ["librarian", "prompt"]) => Some("read_librarian_prompt".to_string()),
        ("GET", ["session", "context"]) => Some("read_session_context".to_string()),
        ("PATCH", ["session", "context"]) => Some("update_session_context".to_string()),
        ("POST", ["panes", _, "role"]) => Some("set_pane_role".to_string()),
        ("GET", ["panes", _, "digest"]) => Some("read_pane_digest".to_string()),
        ("POST", ["panes", _, "digest"]) => Some("update_pane_digest".to_string()),
        ("POST", ["delegate-task"]) => Some("delegate_task".to_string()),
        ("POST", ["operations", "delegate"]) => Some("delegate_work".to_string()),
        ("GET", ["operations", _]) => Some("get_operation".to_string()),
        ("POST", ["operations", _, "wait"]) => Some("wait_for_operation".to_string()),
        ("POST", ["operations", _, "cancel"]) => Some("cancel_operation".to_string()),
        ("GET", ["orchestrator", "state"]) => Some("read_orchestrator_state".to_string()),
        ("PATCH", ["orchestrator", "state"]) => Some("update_orchestrator_state".to_string()),
        ("GET", ["opencode", "keys", "status"]) => Some("read_opencode_key_status".to_string()),
        ("PATCH", ["opencode", "keys", "settings"])
        | ("POST", ["opencode", "keys", "settings"]) => {
            Some("set_opencode_key_settings".to_string())
        }
        ("POST", ["opencode", "keys", "rotate"]) => Some("rotate_opencode_key".to_string()),
        ("GET", ["opencode", "worker-status"]) => Some("read_opencode_worker_status".to_string()),
        ("GET", ["panes", _, "opencode", "status"]) => {
            Some("read_opencode_worker_status".to_string())
        }
        ("GET", ["panes", _, "opencode", "messages"]) => Some("read_opencode_messages".to_string()),
        ("POST", ["panes", _, "opencode", "permissions", _, "reply"]) => {
            Some("reply_opencode_permission".to_string())
        }
        ("POST", ["panes", _, "opencode", "question", "reply"]) => {
            Some("reply_opencode_question".to_string())
        }
        ("POST", ["tasks"]) => Some("create_task".to_string()),
        ("POST", ["tasks", _, "claim"]) => Some("claim_task".to_string()),
        ("POST", ["tasks", _, "lease"]) => Some("renew_task_lease".to_string()),
        ("POST", ["tasks", _, "status"]) => Some("report_task_status".to_string()),
        ("POST", ["tasks", _, "complete"]) => Some("complete_task".to_string()),
        ("POST", ["tasks", _, "block"]) => Some("block_task".to_string()),
        ("POST", ["tasks", _, "reviewer"]) => Some("assign_reviewer".to_string()),
        ("POST", ["locks"]) => Some("acquire_resource_lock".to_string()),
        ("POST", ["locks", "release"]) => Some("release_resource_lock".to_string()),
        ("POST", ["locks", _, "expire"]) => Some("expire_resource_lock".to_string()),
        _ => None,
    }
}

fn mcp_registry_route(method: &str, segments: &[&str]) -> Option<(u16, serde_json::Value)> {
    match (method, segments) {
        ("GET", ["mcp", "tools"]) => Some((
            200,
            serde_json::to_value(crate::tool_registry::tools()).unwrap(),
        )),
        ("GET", ["mcp", "resources"]) => Some((
            200,
            serde_json::to_value(crate::tool_registry::resources()).unwrap(),
        )),
        ("GET", ["mcp", "prompts"]) => Some((
            200,
            serde_json::to_value(crate::tool_registry::prompts()).unwrap(),
        )),
        ("GET", ["mcp", "instructions"]) => Some((
            200,
            json!({"instructions": crate::tool_registry::mcp_instructions()}),
        )),
        _ => None,
    }
}

/// Return value: None means the response was already written (e.g. SSE).
fn route(
    stream: &mut TcpStream,
    method: &str,
    segments: &[&str],
    query: &str,
    body: &[u8],
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
    headers: &str,
    peer_loopback: bool,
) -> Result<(u16, serde_json::Value), (u16, serde_json::Value)> {
    if let Err(auth_err) =
        mobile_pairing::authorize_bridge_request(headers, peer_loopback, segments, method)
    {
        return Err(auth_err);
    }
    if segments.is_empty() && method == "GET" {
        return Ok((
            200,
            json!({
                "service": "puppet-master-bridge",
                "hint": "This is the API bridge, not the mobile UI. Tunnel the Vite app (port 1420 or 4173) and use /bridge on that origin.",
                "health": "/health",
            }),
        ));
    }

    if segments == ["health"] && method == "GET" {
        crate::mcp_sessions::authorize(request_session(headers), method, "/health", &json!({}))
            .map_err(operation_http_error)?;
        return Ok((
            200,
            serde_json::to_value(Health {
                ok: true,
                version: env!("CARGO_PKG_VERSION"),
                catalog_version: crate::tool_registry::catalog_version(),
                tool_count: crate::tool_registry::external_mcp_tool_count(),
            })
            .unwrap(),
        ));
    }

    let session = request_session(headers);
    let request_body: serde_json::Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(body).map_err(|error| {
            (
                400,
                json!({"code":"INVALID_ARGUMENT","error":error.to_string()}),
            )
        })?
    };
    let route_path = format!("/{}", segments.join("/"));
    crate::mcp_sessions::authorize(session, method, &route_path, &request_body)
        .map_err(|error| enrich_handle_error(error, &registry))
        .map_err(operation_http_error)?;
    if segments == ["mcp", "tools"] && method == "GET" {
        let tools = session
            .map(crate::mcp_sessions::tools_for_session)
            .unwrap_or_else(crate::tool_registry::tools);
        return Ok((
            200,
            json!({"tools": tools, "catalog_version": crate::tool_registry::catalog_version()}),
        ));
    }
    if segments == ["mcp", "session"] && method == "GET" {
        let session = session.ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "SESSION_REQUIRED",
                "an MCP session is required",
                false,
            ))
        })?;
        let identity = crate::mcp_sessions::session_identity(session);
        return Ok((
            200,
            json!({
                "connection_id": identity.connection_id,
                "coordinator_id": identity.coordinator_id,
                "attach_token": identity.attach_token,
            }),
        ));
    }
    if segments == ["mcp", "mode"] && method == "POST" {
        let session = session.ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "SESSION_REQUIRED",
                "an MCP session is required",
                false,
            ))
        })?;
        let mode = request_body
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let tools = crate::mcp_sessions::set_mode(session, mode).map_err(operation_http_error)?;
        return Ok((200, json!({"mode":mode,"tool_count":tools.len()})));
    }
    if let Some(response) =
        crate::agent_runs::handle_request(method, segments, query, body, &registry, &app, session)
    {
        return response
            .map_err(|error| enrich_handle_error(error, &registry))
            .map(|value| (200, value))
            .map_err(operation_http_error);
    }
    if segments == ["shell", "exec"] && method == "POST" {
        let command = request_body
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let timeout = request_body
            .get("timeout_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(30_000);
        let requested_pane = request_body
            .get("pane_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let requested_cwd = request_body
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty());

        let (pane_id, cwd, created_pane) = if let Some(pane_id) = requested_pane {
            let cwd = requested_cwd
                .map(|path| normalize_shell_cwd(Some(path), &registry))
                .transpose()
                .map_err(operation_http_error)?;
            (pane_id.to_string(), cwd, false)
        } else {
            let cwd =
                normalize_shell_cwd(requested_cwd, &registry).map_err(operation_http_error)?;
            let agent_type = if cfg!(windows) { "powershell" } else { "bash" };
            let pane_id = registry_spawn_pane(
                &registry,
                &app,
                SpawnPaneArgs {
                    agent_type: agent_type.to_string(),
                    cwd: Some(cwd.clone()),
                    cols: None,
                    rows: None,
                    extra_args: None,
                    pane_id: None,
                },
            )
            .map_err(|error| {
                operation_http_error(crate::operations::OperationError::new(
                    "PANE_SPAWN_FAILED",
                    error,
                    true,
                ))
            })?;
            if let Err(error) = register_spawned_pane(session, agent_type, &pane_id) {
                let _ = registry_kill_pane(&registry, &pane_id);
                return Err(operation_http_error(error));
            }
            (pane_id, Some(cwd), true)
        };

        let mut result =
            crate::shell_exec::execute_in(&registry, &pane_id, command, timeout, cwd.as_deref())
                .map_err(operation_http_error)?;
        if let Some(result) = result.as_object_mut() {
            result.insert("created_pane".into(), serde_json::Value::Bool(created_pane));
        }
        return Ok((200, result));
    }
    if segments == ["agents", "take-over"] && method == "POST" {
        return handoff_route(session, &request_body, &registry, false)
            .map_err(|error| enrich_handle_error(error, &registry))
            .map(|value| (200, value))
            .map_err(operation_http_error);
    }
    if segments == ["agents", "release"] && method == "POST" {
        return handoff_route(session, &request_body, &registry, true)
            .map_err(|error| enrich_handle_error(error, &registry))
            .map(|value| (200, value))
            .map_err(operation_http_error);
    }
    if segments == ["agents", "attach"] && method == "POST" {
        let session = session.ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "SESSION_REQUIRED",
                "attach requires an MCP connection",
                false,
            ))
        })?;
        return crate::mcp_sessions::attach_coordinator(
            session,
            request_body
                .get("attach_token")
                .and_then(serde_json::Value::as_str),
            request_body
                .get("coordinator_name")
                .and_then(serde_json::Value::as_str),
        )
        .map(|value| (200, value))
        .map_err(operation_http_error);
    }
    if segments == ["agents", "release-lease"] && method == "POST" {
        let session = session.ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "SESSION_REQUIRED",
                "release_lease requires an MCP connection",
                false,
            ))
        })?;
        let handle = request_body
            .get("handle")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_ARGUMENT",
                    "handle is required",
                    false,
                ))
            })?;
        return crate::mcp_sessions::release_agent_lease(session, handle)
            .map(|()| (200, json!({"handle": handle, "lease": "released"})))
            .map_err(operation_http_error);
    }
    if segments == ["agents", "transfer"] && method == "POST" {
        let session = session.ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "SESSION_REQUIRED",
                "transfer_agent requires an MCP connection",
                false,
            ))
        })?;
        let handle = request_body
            .get("handle")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_ARGUMENT",
                    "handle is required",
                    false,
                ))
            })?;
        return crate::mcp_sessions::transfer_agent(
            session,
            handle,
            request_body
                .get("to_session_id")
                .and_then(serde_json::Value::as_str),
            request_body
                .get("coordinator_name")
                .and_then(serde_json::Value::as_str),
        )
        .map(|value| (200, value))
        .map_err(operation_http_error);
    }
    if let Some(response) = mcp_registry_route(method, segments) {
        return Ok(response);
    }

    if segments == ["pair"] && method == "POST" {
        let req: PairRequestBody = parse_json(body)?;
        let store = mobile_pairing::pairing_store()
            .ok_or_else(|| (503, serde_json::json!({ "error": "pairing_unavailable" })))?;
        let response = {
            let mut guard = store.lock();
            guard
                .pair_device(req)
                .map_err(|err| (400, json!({ "error": err })))?
        };
        return Ok((200, serde_json::to_value(response).unwrap()));
    }

    if segments.len() == 3 && segments[0] == "pair" && segments[1] == "session" && method == "GET" {
        let code = segments[2];
        let store = mobile_pairing::pairing_store()
            .ok_or_else(|| (503, serde_json::json!({ "error": "pairing_unavailable" })))?;
        let info = {
            let guard = store.lock();
            guard
                .lookup_pairing_session(code)
                .map_err(|err| (404, json!({ "error": err })))?
        };
        return Ok((200, serde_json::to_value(info).unwrap()));
    }

    if segments == ["events"] && method == "GET" {
        handle_sse(stream, registry, &app);
        return Ok((0, serde_json::Value::Null));
    }

    if segments == ["events", "replay", "panes"] && method == "GET" {
        let timeline = crate::event_log::replay_global_pane_timeline()
            .map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(timeline).unwrap()));
    }

    if segments == ["workspace", "state"] && method == "GET" {
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(read_models.workspace).unwrap()));
    }

    if segments == ["operations", "delegate"] && method == "POST" {
        let mut req: crate::operations::DelegateWorkRequest = parse_json(body)?;
        req.owner_session_id = request_session(headers).map(str::to_string);
        return delegate_operation_http(req, registry, app);
    }

    if segments == ["operations", "by-key"] && method == "GET" {
        let key = query_param(query, "idempotency_key")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_ARGUMENT",
                    "idempotency_key is required",
                    false,
                ))
            })?;
        let project = query_param(query, "project_path");
        let snapshot =
            crate::operations::find_operation_by_idempotency_key(project.as_deref(), &key)
                .map_err(operation_http_error)?
                .ok_or_else(|| {
                    operation_http_error(crate::operations::OperationError::new(
                        "OPERATION_NOT_FOUND",
                        "no operation found for idempotency_key",
                        false,
                    ))
                })?;
        return Ok((200, operation_response_value(snapshot)));
    }

    if segments == ["operations", "by-handle"] && method == "GET" {
        let handle = query_param(query, "agent_run_id")
            .or_else(|| query_param(query, "handle"))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_ARGUMENT",
                    "agent_run_id or handle is required",
                    false,
                ))
            })?;
        let project = resolve_operation_project(query_param(query, "project_path").as_deref(), &registry)?;
        let snapshot = crate::operations::resolve_agent_run(
            &project.to_string_lossy(),
            &handle,
        )
        .map_err(operation_http_error)?;
        return Ok((200, operation_response_value(snapshot)));
    }

    if segments.len() == 2 && segments[0] == "operations" && method == "GET" {
        let requested = query_param(query, "project_path");
        let project = resolve_operation_project(requested.as_deref(), &registry)?;
        let snapshot = crate::operations::get_operation(&project.to_string_lossy(), segments[1])
            .map_err(operation_http_error)?;
        return Ok((200, operation_response_value(snapshot)));
    }

    if segments.len() == 3 && segments[0] == "operations" && method == "POST" {
        match segments[2] {
            "wait" => {
                let req: WaitOperationBody = parse_json(body)?;
                let project = resolve_operation_project(
                    requested_project_path(req.project_path.as_deref(), query).as_deref(),
                    &registry,
                )?;
                let after_revision = match req.after_revision {
                    Some(revision) => revision,
                    None => {
                        crate::operations::get_operation(&project.to_string_lossy(), segments[1])
                            .map_err(operation_http_error)?
                            .revision
                    }
                };
                let result = crate::operations::wait_for_operation(
                    &project.to_string_lossy(),
                    segments[1],
                    after_revision,
                    req.until,
                    req.timeout_ms,
                )
                .map_err(operation_http_error)?;
                let sanitized = crate::operations::OperationWaitResult {
                    snapshot: crate::operations::snapshot_for_api(result.snapshot),
                    reason: result.reason,
                };
                return Ok((200, serde_json::to_value(sanitized).unwrap()));
            }
            "cancel" => {
                let req: CancelOperationBody = if body.is_empty() {
                    CancelOperationBody { project_path: None }
                } else {
                    parse_json(body)?
                };
                let project = resolve_operation_project(
                    requested_project_path(req.project_path.as_deref(), query).as_deref(),
                    &registry,
                )?;
                let project_text = project.to_string_lossy().into_owned();
                return cancel_operation_http(project_text, segments[1], registry, app);
            }
            _ => {}
        }
    }

    if segments == ["tasks"] && method == "GET" {
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(read_models.tasks).unwrap()));
    }

    if segments == ["tasks"] && method == "POST" {
        let req: CreateTaskBody = parse_json(body)?;
        let task_id = TaskId::new();
        crate::event_log::append_bridge_event(SystemEvent::TaskCreated {
            task_id: task_id.clone(),
            title: req.title,
            exclusive: req.exclusive,
        });
        return Ok((201, json!({ "task_id": task_id.0 })));
    }

    if segments.len() == 3 && segments[0] == "tasks" && method == "POST" {
        let task_id = TaskId(segments[1].to_string());
        match segments[2] {
            "claim" => {
                let req: ClaimTaskBody = parse_json(body)?;
                let read_models =
                    rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
                let task = read_models
                    .tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .ok_or_else(|| (404, json!({ "error": "unknown task" })))?;
                if task.exclusive && task.claimed_by.is_some() && task.status == "claimed" {
                    return Err((409, json!({ "error": "task already claimed" })));
                }
                crate::event_log::append_bridge_event(SystemEvent::TaskClaimed {
                    task_id: task_id.clone(),
                    agent_id: req.agent_id,
                    lease_expires_at_ms: lease_expires_at(req.lease_ms),
                });
                return Ok((200, json!({ "task_id": task_id.0, "claimed": true })));
            }
            "lease" => {
                let req: ClaimTaskBody = parse_json(body)?;
                crate::event_log::append_bridge_event(SystemEvent::TaskLeaseRenewed {
                    task_id: task_id.clone(),
                    agent_id: req.agent_id,
                    lease_expires_at_ms: lease_expires_at(req.lease_ms),
                });
                return Ok((200, json!({ "task_id": task_id.0, "renewed": true })));
            }
            "status" => {
                let req: TaskStatusBody = parse_json(body)?;
                if req.status == "completed" {
                    return Err(operation_http_error(
                        crate::operations::OperationError::new(
                            "COMPLETION_EVIDENCE_REQUIRED",
                            "use complete_task with evidence to complete a task",
                            false,
                        ),
                    ));
                }
                let project =
                    resolve_task_project(req.project_path.as_deref(), &task_id.0, &registry)?;
                if req.status == "blocked" {
                    let agent_id = req.agent_id.as_deref().ok_or_else(|| (400, json!({"error":"agent_id is required when blocking a task","code":"AGENT_ID_REQUIRED","recoverable":false,"retry_after_ms":null,"context":null})))?;
                    validate_task_owner(&project.to_string_lossy(), &task_id.0, agent_id)?;
                }
                let event = if req.status == "blocked" {
                    SystemEvent::TaskBlocked {
                        task_id: task_id.clone(),
                        agent_id: req.agent_id.clone().unwrap_or_else(|| "agent".into()),
                        reason: req
                            .reason
                            .clone()
                            .unwrap_or_else(|| "task reported blocked".into()),
                    }
                } else {
                    SystemEvent::TaskStatusUpdated {
                        task_id: task_id.clone(),
                        status: req.status.clone(),
                    }
                };
                append_operation_event(&project.to_string_lossy(), event).map_err(|err| (500, json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true,"retry_after_ms":null,"context":null})))?;
                if req.status == "blocked" {
                    match crate::operations::operation_for_task(
                        &project.to_string_lossy(),
                        &task_id.0,
                    ) {
                        Ok(operation) => {
                            let error = crate::operations::OperationError::new(
                                "TASK_BLOCKED",
                                req.reason
                                    .clone()
                                    .unwrap_or_else(|| "task reported blocked".into()),
                                true,
                            );
                            let snapshot = crate::operations::mark_operation_state(
                                &project.to_string_lossy(),
                                &operation.operation_id,
                                crate::operations::OperationStatus::Failed,
                                crate::operations::StateSource::Native,
                                Some("task_blocked".into()),
                                None,
                                Some(error),
                            )
                            .map_err(operation_http_error)?;
                            let _ = release_operation_locks(&snapshot);
                            push_operation_sse(&snapshot);
                        }
                        Err(error) if error.code == "OPERATION_NOT_FOUND" => {}
                        Err(error) => return Err(operation_http_error(error)),
                    }
                }
                return Ok((200, json!({ "task_id": task_id.0, "ok": true })));
            }
            "complete" => {
                let req: CompleteTaskBody = parse_json(body)?;
                let evidence = req.evidence.unwrap_or_default();
                if evidence.trim().is_empty() {
                    return Err(operation_http_error(
                        crate::operations::OperationError::new(
                            "COMPLETION_EVIDENCE_REQUIRED",
                            "evidence is required for task completion",
                            false,
                        ),
                    ));
                }
                let project = resolve_task_project(
                    requested_project_path(req.project_path.as_deref(), query).as_deref(),
                    &task_id.0,
                    &registry,
                )?;
                validate_task_owner(&project.to_string_lossy(), &task_id.0, &req.agent_id)?;
                let snapshots = crate::operations::mark_task_completed(
                    &project.to_string_lossy(),
                    &task_id.0,
                    &evidence,
                )
                .map_err(operation_http_error)?;
                let task_event = SystemEvent::TaskCompleted {
                    task_id: task_id.clone(),
                    agent_id: req.agent_id,
                    evidence: evidence.clone(),
                };
                append_operation_event(&project.to_string_lossy(), task_event).map_err(|err| (500, json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true,"retry_after_ms":null,"context":null})))?;
                for snapshot in snapshots {
                    release_operation_locks(&snapshot).map_err(|err| {
                        operation_http_error(crate::operations::OperationError::new(
                            "LOCK_RELEASE_FAILED",
                            err,
                            true,
                        ))
                    })?;
                    push_operation_sse(&snapshot);
                }
                return Ok((200, json!({ "task_id": task_id.0, "completed": true })));
            }
            "block" => {
                let req: BlockTaskBody = parse_json(body)?;
                let reason = req.reason.clone();
                let project =
                    resolve_task_project(req.project_path.as_deref(), &task_id.0, &registry)?;
                validate_task_owner(&project.to_string_lossy(), &task_id.0, &req.agent_id)?;
                let event = SystemEvent::TaskBlocked {
                    task_id: task_id.clone(),
                    agent_id: req.agent_id,
                    reason: req.reason,
                };
                append_operation_event(&project.to_string_lossy(), event).map_err(|err| (500, json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true,"retry_after_ms":null,"context":null})))?;
                match crate::operations::operation_for_task(&project.to_string_lossy(), &task_id.0)
                {
                    Ok(operation) => {
                        let error =
                            crate::operations::OperationError::new("TASK_BLOCKED", reason, true);
                        let snapshot = crate::operations::mark_operation_state(
                            &project.to_string_lossy(),
                            &operation.operation_id,
                            crate::operations::OperationStatus::Failed,
                            crate::operations::StateSource::Native,
                            Some("task_blocked".into()),
                            None,
                            Some(error),
                        )
                        .map_err(operation_http_error)?;
                        release_operation_locks(&snapshot).map_err(|err| {
                            operation_http_error(crate::operations::OperationError::new(
                                "LOCK_RELEASE_FAILED",
                                err,
                                true,
                            ))
                        })?;
                        push_operation_sse(&snapshot);
                    }
                    Err(error) if error.code == "OPERATION_NOT_FOUND" => {}
                    Err(error) => return Err(operation_http_error(error)),
                }
                return Ok((200, json!({ "task_id": task_id.0, "blocked": true })));
            }
            "reviewer" => {
                let req: AssignReviewerBody = parse_json(body)?;
                crate::event_log::append_bridge_event(SystemEvent::ReviewerAssigned {
                    task_id: task_id.clone(),
                    reviewer_id: req.reviewer_id,
                });
                return Ok((200, json!({ "task_id": task_id.0, "ok": true })));
            }
            _ => {}
        }
    }

    if segments == ["locks"] && method == "GET" {
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(read_models.locks).unwrap()));
    }

    if segments == ["locks"] && method == "POST" {
        let req: AcquireLockBody = parse_json(body)?;
        let _lock_guard = RESOURCE_LOCK_SERIAL.lock();
        let resource_id = ResourceId::from_parts(&req.resource_type, &req.name);
        let project = resolve_operation_project(None, &registry)?;
        let read_models = rebuild_operation_read_models(&project.to_string_lossy())
            .map_err(|err| (500, json!({ "error": err })))?;
        if let Some(existing) = read_models
            .locks
            .iter()
            .find(|lock| lock.resource_id == resource_id)
        {
            append_operation_event(
                &project.to_string_lossy(),
                SystemEvent::ResourceLockConflict {
                    resource_id: resource_id.clone(),
                    requested_owner_id: req.owner_id,
                    existing_owner_id: existing.owner.clone(),
                },
            )
            .map_err(|err| {
                (
                    500,
                    json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true}),
                )
            })?;
            return Err((409, json!({ "error": "resource already locked" })));
        }
        append_operation_event(
            &project.to_string_lossy(),
            SystemEvent::ResourceLockAcquired {
                resource_id: resource_id.clone(),
                resource_type: req.resource_type,
                owner_id: req.owner_id,
                lease_expires_at_ms: req
                    .lease_ms
                    .map(|lease_ms| lease_expires_at(Some(lease_ms))),
            },
        )
        .map_err(|err| {
            (
                500,
                json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true}),
            )
        })?;
        return Ok((201, json!({ "resource_id": resource_id.0, "locked": true })));
    }

    if segments == ["locks", "release"] && method == "POST" {
        let req: ReleaseLockBody = parse_json(body)?;
        let resource_id = ResourceId::from_parts(&req.resource_type, &req.name);
        crate::event_log::append_bridge_event(SystemEvent::ResourceLockReleased {
            resource_id: resource_id.clone(),
            owner_id: req.owner_id,
        });
        return Ok((
            200,
            json!({ "resource_id": resource_id.0, "released": true }),
        ));
    }

    if segments.len() == 3 && segments[0] == "locks" && segments[2] == "expire" && method == "POST"
    {
        let resource_id = ResourceId(segments[1].to_string());
        crate::event_log::append_bridge_event(SystemEvent::ResourceLockExpired {
            resource_id: resource_id.clone(),
        });
        return Ok((
            200,
            json!({ "resource_id": resource_id.0, "expired": true }),
        ));
    }

    if segments.len() == 3 && segments[0] == "agents" && segments[2] == "inbox" && method == "GET" {
        let inbox = crate::projections::agent_inbox(segments[1].to_string());
        return Ok((200, serde_json::to_value(inbox).unwrap()));
    }

    if segments == ["audit"] && method == "GET" {
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(read_models.audit).unwrap()));
    }

    if segments == ["context-packs"] && method == "POST" {
        let req: crate::context_pack::ContextPackRequest = parse_json(body)?;
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        let project_root = resolve_project_root(&registry);
        let indexer_path = librarian_indexer_path(&app);
        let pack = crate::context_pack::build_context_pack(
            req,
            &read_models,
            project_root.as_deref(),
            indexer_path.as_deref(),
        );
        return Ok((200, serde_json::to_value(pack).unwrap()));
    }

    if segments == ["project-ir", "status"] && method == "GET" {
        let project_root = resolve_project_root(&registry).ok_or_else(|| {
            (
                400,
                json!({ "error": "no active project path — set project folder first" }),
            )
        })?;
        let indexer_path = librarian_indexer_path(&app);
        let status = crate::project_ir::status(&project_root, indexer_path.as_deref());
        return Ok((200, serde_json::to_value(status).unwrap()));
    }

    if segments == ["librarian", "prompt"] && method == "GET" {
        let project_root = resolve_project_root(&registry).ok_or_else(|| {
            (
                400,
                json!({ "error": "no active project path — set project folder first" }),
            )
        })?;
        let prompt = crate::project_ir::render_librarian_prompt(&project_root)
            .map_err(|err| (500, json!({ "error": err })))?;
        return Ok((
            200,
            json!({
                "prompt": prompt,
                "delegate_to": "opencode_native",
                "completion_marker": "LIBRARIAN_INDEX_COMPLETE",
            }),
        ));
    }

    if segments == ["session", "context"] && method == "GET" {
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(read_models.session).unwrap()));
    }

    if segments == ["session", "context"] && method == "PATCH" {
        let req: SessionContextPatchBody = parse_json(body)?;
        crate::event_log::append_bridge_event(SystemEvent::SessionGoalUpdated {
            current_goal: normalize_optional_string(req.current_goal),
        });
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(read_models.session).unwrap()));
    }

    if segments == ["delegate-task"] && method == "POST" {
        let req = parse_json::<crate::session_context::DelegateTaskRequest>(body)?
            .validated()
            .map_err(|err| (400, json!({ "error": err })))?;
        let prompt = crate::session_context::render_codex_delegation_prompt(&req);
        crate::event_log::append_bridge_event(SystemEvent::DelegationPrepared {
            task_id: req.task_id.clone().map(TaskId),
            target_pane_id: req.target_pane_id.clone().map(crate::events::PaneId),
            intent: req.intent.clone(),
        });
        let mut payload = json!({
            "task_id": req.task_id,
            "target_pane_id": req.target_pane_id,
            "prompt": prompt,
        });
        if let Some(pane_id) = req.target_pane_id.as_deref() {
            return Ok((
                200,
                crate::mcp_hints::mutate_ok(
                    payload,
                    crate::mcp_hints::suggested_wait_after_delegate(pane_id),
                ),
            ));
        }
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("ok".into(), json!(true));
        }
        return Ok((200, payload));
    }

    if segments == ["orchestrator", "state"] && method == "GET" {
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((
            200,
            serde_json::to_value(read_models.session.orchestrator).unwrap(),
        ));
    }

    if segments == ["orchestrator", "state"] && method == "PATCH" {
        let req: OrchestratorStatePatchBody = parse_json(body)?;
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        let current = read_models.session.orchestrator;
        let standby_poll_ms = req
            .standby_poll_ms
            .unwrap_or(current.standby_poll_ms)
            .max(1);
        let standby_max_ms = req.standby_max_ms.unwrap_or(current.standby_max_ms).max(1);
        crate::event_log::append_bridge_event(SystemEvent::OrchestratorStandbyPolicyUpdated {
            standby_poll_ms,
            standby_max_ms,
        });
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((
            200,
            serde_json::to_value(read_models.session.orchestrator).unwrap(),
        ));
    }

    if segments == ["panes", "wait"] && method == "POST" {
        let req: crate::pane_wait::WaitForPanesRequest = parse_json(body)?;
        let result = crate::pane_wait::wait_for_panes(&registry, req)
            .map_err(|err| (400, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(result).unwrap()));
    }

    if segments == ["panes", "wait", "model"] && method == "POST" {
        #[derive(Deserialize)]
        struct WaitModelBody {
            pane_id: String,
            #[serde(default)]
            provider_id: Option<String>,
            #[serde(default)]
            model_id: Option<String>,
            #[serde(default = "default_wait_timeout")]
            timeout_ms: u64,
        }
        fn default_wait_timeout() -> u64 {
            120_000
        }
        let req: WaitModelBody = parse_json(body)?;
        let result = crate::pane_wait::wait_for_model(
            &registry,
            &req.pane_id,
            req.provider_id.as_deref(),
            req.model_id.as_deref(),
            req.timeout_ms,
        )
        .map_err(|err| (400, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(result).unwrap()));
    }

    if segments == ["panes", "wait", "task"] && method == "POST" {
        #[derive(Deserialize)]
        struct WaitTaskBody {
            pane_id: String,
            task_id: String,
            #[serde(default)]
            until: Vec<String>,
            #[serde(default = "default_wait_timeout")]
            timeout_ms: u64,
        }
        fn default_wait_timeout() -> u64 {
            120_000
        }
        let req: WaitTaskBody = parse_json(body)?;
        let until = if req.until.is_empty() {
            vec!["task_completed", "task_blocked", "error"]
        } else {
            req.until.iter().map(String::as_str).collect::<Vec<_>>()
        };
        let result = crate::pane_wait::wait_for_task(
            &registry,
            &req.pane_id,
            &req.task_id,
            &until,
            req.timeout_ms,
        )
        .map_err(|err| (400, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(result).unwrap()));
    }

    if segments == ["panes", "wait", "worker"] && method == "POST" {
        #[derive(Deserialize)]
        struct WaitWorkerBody {
            pane_id: String,
            #[serde(default = "default_wait_timeout")]
            timeout_ms: u64,
        }
        fn default_wait_timeout() -> u64 {
            120_000
        }
        let req: WaitWorkerBody = parse_json(body)?;
        let result = crate::pane_wait::wait_for_worker(&registry, &req.pane_id, req.timeout_ms)
            .map_err(|err| (400, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(result).unwrap()));
    }

    if segments == ["events", "recent"] && method == "GET" {
        let limit = query_param(query, "limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50);
        let pane_id = query_param(query, "pane_id");
        let since_id = query_param(query, "since_id");
        let types = query_param(query, "types").map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
        let events = crate::event_log::read_recent_events(
            limit,
            pane_id.as_deref(),
            types.as_deref(),
            since_id.as_deref(),
        )
        .map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(events).unwrap()));
    }

    if segments == ["opencode", "keys", "status"] && method == "GET" {
        let status =
            crate::opencode::keys::status().map_err(|err| (500, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(status).unwrap()));
    }

    if segments == ["opencode", "worker-status"] && method == "GET" {
        let pane_id = query_param(query, "pane_id")
            .or_else(|| query_param(query, "worker_id"))
            .or_else(|| resolve_default_opencode_pane_id(&registry));
        let pane_id = pane_id.ok_or_else(|| {
            (
                400,
                json!({
                    "error": "pane_id is required when more than one opencode_native pane is open",
                    "code": "PANE_ID_REQUIRED",
                }),
            )
        })?;
        let status = crate::opencode::status::worker_status(&registry, &pane_id)
            .map_err(|err| (404, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(status).unwrap()));
    }

    if segments == ["opencode", "keys", "settings"] && (method == "PATCH" || method == "POST") {
        let patch: crate::opencode::keys::OpenCodeKeySettingsPatch = parse_json(body)?;
        let status = crate::opencode::keys::patch_settings(&patch)
            .map_err(|err| (400, json!({ "error": err })))?;
        return Ok((200, serde_json::to_value(status).unwrap()));
    }

    if segments == ["opencode", "keys", "rotate"] && method == "POST" {
        let req: OpenCodeKeyRotateBody = parse_json(body)?;
        let profile = req.profile.unwrap_or_else(|| "next".to_string());
        let target = crate::opencode::keys::RotateTarget::parse(&profile)
            .ok_or_else(|| (400, json!({ "error": "profile must be next, a, or b" })))?;
        let status =
            crate::opencode::keys::rotate(target).map_err(|err| (400, json!({ "error": err })))?;
        let restarted = if let Some(pane_id) = normalize_optional_string(req.pane_id) {
            vec![crate::opencode::native::restart_native_pane(
                Arc::clone(&registry),
                &app,
                &pane_id,
            )
            .map_err(|err| (404, json!({ "error": err })))?]
        } else {
            crate::opencode::native::restart_all_native_panes(Arc::clone(&registry), &app)
                .map_err(|err| (500, json!({ "error": err })))?
        };
        return Ok((
            200,
            json!({
                "ok": true,
                "active_profile": status.active_profile,
                "profiles": status.profiles,
                "restarted_panes": restarted,
            }),
        ));
    }

    if segments == ["agent-contexts"] && method == "GET" {
        return Ok((
            200,
            serde_json::to_value(crate::agent_contexts::list_agent_context_profiles()).unwrap(),
        ));
    }

    if segments == ["settings"] && method == "GET" {
        return Ok((200, settings_store::read_public_settings(&app)));
    }

    if segments == ["settings"] && (method == "PATCH" || method == "POST") {
        let patch: serde_json::Value = parse_json(body)?;
        let updated = settings_store::patch_public_settings(&app, patch);
        return Ok((200, updated));
    }

    if segments == ["project-path"] && method == "POST" {
        let req: ProjectPathBody = parse_json(body)?;
        let normalized =
            crate::project_path::normalize_project_path(std::path::Path::new(&req.path))
                .map_err(|err| (400, json!({ "error": err })))?;
        registry_set_project_path(&registry, normalized.to_string_lossy().into_owned());
        crate::event_log::set_active_project_path(Some(normalized));
        return Ok((200, json!({ "ok": true })));
    }

    if segments.len() == 1 && segments[0] == "panes" {
        match method {
            "GET" => {
                let panes = registry.lock().list();
                return Ok((200, serde_json::to_value(panes).unwrap()));
            }
            "POST" => {
                let req: SpawnPaneArgs = parse_json(body)?;
                let agent_type = req.agent_type.clone();
                let pane_id = registry_spawn_pane(&registry, &app, req)
                    .map_err(|err| (500, json!({ "error": err })))?;
                register_spawned_pane(session, &agent_type, &pane_id)
                    .map_err(operation_http_error)?;
                present_spawned_pane(&registry, &app, &pane_id);
                return Ok((
                    201,
                    crate::mcp_hints::mutate_ok(
                        json!({ "pane_id": pane_id }),
                        crate::mcp_hints::suggested_wait_after_spawn(&pane_id),
                    ),
                ));
            }
            _ => {}
        }
    }

    if segments.len() >= 2 && segments[0] == "panes" {
        let pane_id = segments[1];
        let tail = segments.get(2).copied();
        if tail.is_none() && method == "DELETE" {
            registry_kill_pane(&registry, pane_id).map_err(|err| (500, json!({ "error": err })))?;
            emit_panes_changed(&registry, &app);
            return Ok((200, json!({ "ok": true })));
        }
        if tail == Some("buffer") && method == "GET" {
            let lines = query_param(query, "lines")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(200);
            let requested_view = query_param(query, "view");
            let agent_type = registry
                .lock()
                .panes
                .get(pane_id)
                .map(|pane| pane.info.agent_type.clone())
                .ok_or_else(|| {
                    (
                        404,
                        json!({ "error": crate::pty::registry::unknown_pane_message(&pane_id) }),
                    )
                })?;
            let requested_view = requested_view.as_deref();
            let view = pane_buffer_view(requested_view, &agent_type).map_err(|message| {
                (
                    400,
                    json!({"error":message,"allowed":["screen","scrollback"]}),
                )
            })?;
            let content = if view == "screen" {
                registry_read_snapshot(&registry, pane_id)
            } else {
                registry_read_buffer(&registry, pane_id, lines)
            }
            .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "content": content, "view": view })));
        }
        if tail == Some("raw") && method == "GET" {
            let lines = query_param(query, "lines")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10_000);
            let raw = registry_read_raw_buffer(&registry, pane_id, lines)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "data": raw })));
        }
        if tail == Some("snapshot") && method == "GET" {
            let content = registry_read_snapshot(&registry, pane_id)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "content": content })));
        }
        if tail == Some("input") && method == "POST" {
            let req: WriteInputBody = parse_json(body)?;
            let settings = settings_store::read_public_settings(&app);
            let model = crate::opencode::resolve_model(
                req.model_provider.as_deref(),
                req.model_id.as_deref(),
                &settings,
            );
            registry_write_input(
                &registry,
                &app,
                pane_id,
                &req.text,
                req.append_newline,
                req.via_opencode_api,
                model,
            )
            .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((
                200,
                crate::mcp_hints::mutate_ok(
                    json!({ "pane_id": pane_id, "written": true }),
                    crate::mcp_hints::suggested_wait_after_write(pane_id),
                ),
            ));
        }
        if tail == Some("key") && method == "POST" {
            let req: PressKeyBody = parse_json(body)?;
            let seq = crate::pty::keys::sequence(&req.key)
                .map_err(|err| (400, json!({ "error": err })))?;
            let bytes = seq.len();
            registry_write_input(&registry, &app, pane_id, &seq, false, false, None)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "ok": true, "key": req.key, "bytes": bytes })));
        }
        if tail == Some("opencode")
            && segments.get(3) == Some(&"status")
            && method == "GET"
            && segments.len() == 4
        {
            let status = crate::opencode::status::worker_status(&registry, pane_id)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, serde_json::to_value(status).unwrap()));
        }
        if tail == Some("opencode")
            && segments.get(3) == Some(&"messages")
            && method == "GET"
            && segments.len() == 4
        {
            let limit = query_param(query, "limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(20)
                .clamp(1, 200);
            let role = query_param(query, "role");
            let view = crate::opencode::messages::read_pane_messages(
                &registry,
                pane_id,
                limit,
                role.as_deref(),
            )
            .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, serde_json::to_value(view).unwrap()));
        }
        if tail == Some("opencode") && method == "GET" && segments.len() == 3 {
            let link = crate::opencode::pane_link(&registry, pane_id).ok_or_else(|| {
                (
                    404,
                    json!({ "error": format!("pane {pane_id} has no opencode native session") }),
                )
            })?;
            return Ok((200, serde_json::to_value(link).unwrap()));
        }
        if tail == Some("opencode")
            && segments.get(3) == Some(&"permissions")
            && method == "GET"
            && segments.len() == 4
        {
            let permissions = crate::opencode::list_pane_permissions(&registry, pane_id)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, serde_json::to_value(permissions).unwrap()));
        }
        if tail == Some("opencode")
            && segments.get(3) == Some(&"permissions")
            && segments.get(5) == Some(&"reply")
            && method == "POST"
            && segments.len() == 6
        {
            let request_id = segments[4];
            let req: OpenCodePermissionReplyBody = parse_json(body)?;
            crate::opencode::reply_pane_permission(&registry, pane_id, request_id, &req.reply)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "ok": true })));
        }
        if tail == Some("opencode")
            && segments.get(3) == Some(&"question")
            && segments.get(4) == Some(&"reply")
            && method == "POST"
            && segments.len() == 5
        {
            let req: OpenCodeQuestionReplyBody = parse_json(body)?;
            let result = crate::opencode::reply_pane_question(
                &registry,
                pane_id,
                &req.answer,
                req.request_id.as_deref(),
            )
            .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, result));
        }
        if tail == Some("detach") && method == "POST" {
            let pane = registry
                .lock()
                .list()
                .into_iter()
                .find(|pane| pane.id == pane_id)
                .ok_or_else(|| {
                    (
                        404,
                        json!({ "error": crate::pty::registry::unknown_pane_message(&pane_id) }),
                    )
                })?;
            let title = format!(
                "{} · {}",
                pane.agent_type,
                pane.id.chars().take(8).collect::<String>()
            );
            let _ = app.emit(
                "pane://detach",
                json!({
                    "pane_id": pane.id,
                    "title": title,
                    "cols": pane.cols,
                    "rows": pane.rows,
                }),
            );
            return Ok((200, json!({ "ok": true })));
        }
        if tail == Some("role") && method == "POST" {
            let req: SetPaneRoleBody = parse_json(body)?;
            crate::event_log::append_bridge_event(SystemEvent::PaneRoleSet {
                pane_id: crate::events::PaneId(pane_id.to_string()),
                role: req.role,
            });
            let read_models =
                rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
            return Ok((200, serde_json::to_value(read_models.session).unwrap()));
        }
        if tail == Some("digest") && method == "GET" {
            let read_models =
                rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
            let digest = read_models
                .session
                .pane_digests
                .get(pane_id)
                .ok_or_else(|| (404, json!({ "error": "pane digest not found" })))?;
            return Ok((200, serde_json::to_value(digest).unwrap()));
        }
        if tail == Some("digest") && method == "POST" {
            let req: PaneDigestBody = parse_json(body)?;
            let summary = req.summary.trim();
            if summary.is_empty() {
                return Err((400, json!({ "error": "summary is required" })));
            }
            crate::event_log::append_bridge_event(SystemEvent::PaneDigestUpdated {
                pane_id: crate::events::PaneId(pane_id.to_string()),
                summary: summary.to_string(),
                source: normalize_optional_string(req.source)
                    .unwrap_or_else(|| "manual".to_string()),
            });
            let read_models =
                rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
            let digest = read_models
                .session
                .pane_digests
                .get(pane_id)
                .ok_or_else(|| (500, json!({ "error": "pane digest projection missing" })))?;
            return Ok((200, serde_json::to_value(digest).unwrap()));
        }
        if tail == Some("resize") && method == "POST" {
            // Remote bridge clients must not resize the shared PTY — only desktop does.
            let _ = parse_json::<ResizeBody>(body)?;
            return Ok((200, json!({ "ok": true, "ignored": true })));
        }
        if tail == Some("model") && method == "POST" {
            let req: SwitchModelBody = parse_json(body)?;
        if let Some(error) = crate::agent_runs::pane_close::closed_after_run_error(
            &crate::pty::registry::get_project_path(&registry),
            &registry,
            pane_id,
        ) {
            return Err(operation_http_error(error));
        }
            if let Some(session) = session {
                if !crate::mcp_sessions::session_controls_pane(session, pane_id) {
                    let project = crate::pty::registry::get_project_path(&registry);
                    if let Ok(handle) =
                        crate::agent_runs::latest_handle_for_pane(&project, pane_id)
                    {
                        if crate::mcp_sessions::check_run_access(session, &handle).is_ok() {
                            let _ = crate::mcp_sessions::take_over_pane(session, pane_id, true);
                        }
                    }
                }
            }
            let settings = settings_store::read_public_settings(&app);
            let model = crate::opencode::resolve_model(
                req.model_provider.as_deref(),
                Some(req.model_id.as_str()),
                &settings,
            )
            .ok_or_else(|| (400, json!({ "error": "model_id required" })))?;
            crate::opencode::switch_native_model(&registry, &app, pane_id, &model)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((
                200,
                crate::opencode::switch_model_response(&registry, pane_id, &model),
            ));
        }
        if tail == Some("model") && method == "GET" {
        if let Some(error) = crate::agent_runs::pane_close::closed_after_run_error(
            &crate::pty::registry::get_project_path(&registry),
            &registry,
            pane_id,
        ) {
            return Err(operation_http_error(error));
        }
            let lines = query_param(query, "lines")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(200);
            let pane = registry
                .lock()
                .list()
                .into_iter()
                .find(|pane| pane.id == pane_id)
                .ok_or_else(|| {
                    (
                        404,
                        json!({ "error": crate::pty::registry::unknown_pane_message(&pane_id) }),
                    )
                })?;
            let agent_type = AgentType::parse(&pane.agent_type).ok_or_else(|| {
                (
                    400,
                    json!({ "error": format!("unknown agent_type: {}", pane.agent_type) }),
                )
            })?;
            let buffer = registry_read_buffer(&registry, pane_id, lines)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((
                200,
                serde_json::to_value(crate::agent_contexts::inspect_agent_model_with_registry(
                    Some(&registry),
                    pane_id,
                    agent_type,
                    &buffer,
                ))
                .unwrap(),
            ));
        }
        if tail == Some("agent-context") && method == "GET" {
            let panes = registry.lock().list();
            let pane = panes.iter().find(|pane| pane.id == pane_id);
            if let Some(pane) = pane {
                let buffer = registry_read_buffer(&registry, pane_id, 200)
                    .map_err(|err| (404, json!({ "error": err })))?;
                let context = crate::agent_contexts::build_pane_agent_context(
                    Some(&registry),
                    pane.clone(),
                    &buffer,
                )
                .ok_or_else(|| (400, json!({ "error": "unknown pane agent_type" })))?;
                return Ok((200, serde_json::to_value(context).unwrap()));
            }
            return Err((
                404,
                json!({ "error": crate::pty::registry::unknown_pane_message(&pane_id) }),
            ));
        }
    }

    // POST /orchestrator/message — mobile PWA sends prompt to desktop orchestrator
    if segments == ["orchestrator", "message"] && method == "POST" {
        let req: OrchestratorMessageBody = parse_json(body)?;
        let user_event = json!({
            "type": "user",
            "message_id": req.message_id,
            "text": req.text,
        });
        push_sse(format!("event: chat\ndata: {user_event}\n\n"));
        let _ = app.emit("orchestrator://message", req);
        return Ok((200, json!({ "ok": true })));
    }

    // POST /orchestrator/viewport — mobile PWA reports visible viewport for PTY sizing
    if segments == ["orchestrator", "viewport"] && method == "POST" {
        let req: OrchestratorViewportBody = parse_json(body)?;
        if let Ok(json) = serde_json::to_string(&req) {
            push_sse(format!("event: orchestrator-viewport\ndata: {json}\n\n"));
        }
        return Ok((200, json!({ "ok": true })));
    }

    Err((
        404,
        json!({
            "error": "not found",
            "hint": "Unknown bridge route. Mobile UI is served by Vite on port 1420/4173; API lives under /health, /events, /panes, /orchestrator/message, /orchestrator/viewport.",
        }),
    ))
}

fn rebuild_read_models() -> Result<crate::projections::ReadModels, String> {
    let entries = crate::event_log::read_global_entries()?;
    Ok(crate::projections::build_read_models(&entries))
}

fn rebuild_operation_read_models(project: &str) -> Result<crate::projections::ReadModels, String> {
    let log = crate::event_log::EventLog::new(crate::event_log::project_event_log_path(
        std::path::Path::new(project),
    ))?;
    Ok(crate::projections::build_read_models(&log.read_all()?))
}

fn append_operation_event(project: &str, payload: SystemEvent) -> Result<(), String> {
    let log = crate::event_log::EventLog::new(crate::event_log::project_event_log_path(
        std::path::Path::new(project),
    ))?;
    log.append(&crate::events::EventEntry::new(
        crate::actors::ActorId::bridge(),
        crate::events::CommandId::new(),
        payload,
    ))?;
    crate::pane_wait_notify::bump_waiters();
    Ok(())
}

/// Acquire every lock requested by `operation`, owned by the operation id. Re-acquiring a lock
/// this operation already holds is a no-op; a lock held by anyone else fails with
/// RESOURCE_LOCKED (context: resource_id, owner_id). Callers release via `release_operation_locks`,
/// which only frees locks this operation owns, so a partial acquisition is safe to roll back.
pub(crate) fn acquire_operation_locks(
    operation: &crate::operations::OperationSnapshot,
) -> Result<(), crate::operations::OperationError> {
    if operation.locks.is_empty() {
        return Ok(());
    }
    let _lock_guard = RESOURCE_LOCK_SERIAL.lock();
    let mut requested = Vec::new();
    for lock_name in &operation.locks {
        requested.push(parse_operation_lock(lock_name)?);
    }
    for (resource_type, resource_name) in requested {
        let resource_id = ResourceId::from_parts(&resource_type, &resource_name);
        let models = rebuild_operation_read_models(&operation.project_path).map_err(|err| {
            crate::operations::OperationError::new("STATE_UNAVAILABLE", err, true)
        })?;
        if let Some(existing) = models
            .locks
            .iter()
            .find(|lock| lock.resource_id == resource_id)
        {
            if existing.owner == operation.operation_id {
                continue;
            }
            let mut error = crate::operations::OperationError::new(
                "RESOURCE_LOCKED",
                "a requested resource is already locked",
                true,
            );
            error.retry_after_ms = Some(1000);
            error.context = json!({"resource_id":resource_id.0,"owner_id":existing.owner});
            return Err(error);
        }
        append_operation_event(
            &operation.project_path,
            SystemEvent::ResourceLockAcquired {
                resource_id,
                resource_type,
                owner_id: operation.operation_id.clone(),
                lease_expires_at_ms: None,
            },
        )
        .map_err(|err| crate::operations::OperationError::new("EVENT_WRITE_FAILED", err, true))?;
    }
    Ok(())
}

/// Release the locks held by `operation`. Locks currently owned by a different operation are
/// left alone (the release event is not owner-checked by the projection).
pub(crate) fn release_operation_locks(
    operation: &crate::operations::OperationSnapshot,
) -> Result<(), String> {
    if operation.locks.is_empty() {
        return Ok(());
    }
    let _lock_guard = RESOURCE_LOCK_SERIAL.lock();
    let models = rebuild_operation_read_models(&operation.project_path)?;
    for lock in &operation.locks {
        if let Some((resource_type, name)) = lock.split_once(':') {
            let resource_id = ResourceId::from_parts(resource_type.trim(), name.trim());
            let owned = models
                .locks
                .iter()
                .any(|held| held.resource_id == resource_id && held.owner == operation.operation_id);
            if !owned {
                continue;
            }
            append_operation_event(
                &operation.project_path,
                SystemEvent::ResourceLockReleased {
                    resource_id,
                    owner_id: operation.operation_id.clone(),
                },
            )?;
        }
    }
    Ok(())
}

fn cleanup_failed_operation(
    operation: &crate::operations::OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) {
    let _ =
        crate::agent_runs::pane_close::maybe_dispose_on_dispatch_failure(operation, registry);
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

fn operation_http_error(error: crate::operations::OperationError) -> (u16, serde_json::Value) {
    let status = match error.code.as_str() {
        "OPERATION_NOT_FOUND" | "AGENT_NOT_FOUND" | "PANE_NOT_FOUND" => 404,
        "AUTHORIZATION_DENIED" => 403,
        "MODE_MISMATCH" => 409,
        "INVALID_ARGUMENT"
        | "INVALID_TIMEOUT"
        | "SESSION_REQUIRED"
        | "READ_ONLY_UNSUPPORTED"
        | "AGENT_NOT_DISPATCHABLE"
        | "WORKER_MCP_TOOLS_UNAVAILABLE" => 400,
        "RESOURCE_LOCKED"
        | "IDEMPOTENCY_KEY_CONFLICT"
        | "OPERATION_CANCELLED"
        | "DISPATCH_CANCELLED"
        | "PANE_BUSY"
        | "OPERATION_ALREADY_DISPATCHED"
        | "CANCELLATION_IN_PROGRESS"
        | "CANCELLATION_REQUIRES_WORKER_CONTROL"
        | "PANE_UNAVAILABLE"
        | "PANE_RESERVATION_LOST"
        | "INVALID_STATE_TRANSITION" => 409,
        "INVALID_TASK"
        | "INVALID_IDEMPOTENCY_KEY"
        | "INVALID_ACCEPTANCE_CRITERIA"
        | "INVALID_PROJECT_PATH"
        | "INVALID_OPERATION_ID"
        | "INVALID_AGENT_TYPE"
        | "INVALID_LOCK"
        | "COMPLETION_EVIDENCE_REQUIRED" => 400,
        "PROJECT_PATH_REQUIRED" => 400,
        _ => 503,
    };
    (
        status,
        json!({"error": error.message, "message": error.message, "code": error.code, "recoverable": error.recoverable, "retry_after_ms": error.retry_after_ms, "context": error.context}),
    )
}

fn push_operation_sse(snapshot: &crate::operations::OperationSnapshot) {
    if let Ok(payload) = serde_json::to_string(snapshot) {
        push_sse(format!("event: operation\ndata: {payload}\n\n"));
    }
}

fn lease_expires_at(lease_ms: Option<i64>) -> i64 {
    crate::event_log::now_ms() + lease_ms.unwrap_or(5 * 60 * 1000).max(1)
}

fn handle_sse(stream: &mut TcpStream, registry: Arc<Mutex<PaneRegistry>>, app: &AppHandle) {
    let headers = "HTTP/1.1 200 OK\r\n\
        Content-Type: text/event-stream\r\n\
        Cache-Control: no-cache\r\n\
        Connection: keep-alive\r\n\
        Access-Control-Allow-Origin: *\r\n\
        \r\n\
        : connected\n\n";
    if stream.write_all(headers.as_bytes()).is_err() {
        return;
    }

    let panes = registry.lock().list();
    if let Ok(json) = serde_json::to_string(&panes) {
        let snapshot = format!("event: panes\ndata: {json}\n\n");
        if stream.write_all(snapshot.as_bytes()).is_err() {
            return;
        }
    }

    // Push the current public settings snapshot so the mobile PWA is in sync
    // from the moment it connects (and doesn't have to race GET /settings).
    let settings = settings_store::read_public_settings(app);
    if let Ok(json) = serde_json::to_string(&settings) {
        let snapshot = format!("event: settings\ndata: {json}\n\n");
        if stream.write_all(snapshot.as_bytes()).is_err() {
            return;
        }
    }

    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(64);
    get_sse_clients().lock().push(tx);

    for payload in rx {
        if stream.write_all(payload.as_bytes()).is_err() {
            break;
        }
    }
}

pub fn push_settings_sse(settings: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(settings) {
        push_sse(format!("event: settings\ndata: {json}\n\n"));
    }
}

pub fn push_panes_sse(registry: &Arc<Mutex<PaneRegistry>>) {
    let panes = registry.lock().list();
    if let Ok(json) = serde_json::to_string(&panes) {
        push_sse(format!("event: panes\ndata: {json}\n\n"));
    }
}

fn resolve_project_root(registry: &Arc<Mutex<PaneRegistry>>) -> Option<PathBuf> {
    crate::event_log::active_project_path().or_else(|| {
        let path = crate::pty::registry::get_project_path(registry);
        let candidate = PathBuf::from(path);
        if crate::project_path::is_valid_project_path(&candidate) {
            Some(candidate)
        } else {
            None
        }
    })
}

fn resolve_operation_project(
    requested: Option<&str>,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<PathBuf, (u16, serde_json::Value)> {
    if let Some(path) = requested.filter(|path| !path.trim().is_empty()) {
        return crate::project_path::normalize_project_path(std::path::Path::new(path)).map_err(
            |err| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_PROJECT_PATH",
                    err,
                    false,
                ))
            },
        );
    }
    resolve_project_root(registry).ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "PROJECT_PATH_REQUIRED",
            "project_path is required when no project is selected",
            false,
        ))
    })
}

fn resolve_task_project(
    requested: Option<&str>,
    task_id: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<PathBuf, (u16, serde_json::Value)> {
    if let Some(path) = requested.filter(|path| !path.trim().is_empty()) {
        return crate::project_path::normalize_project_path(std::path::Path::new(path)).map_err(
            |err| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_PROJECT_PATH",
                    err,
                    false,
                ))
            },
        );
    }
    if let Ok(operation) = crate::operations::operation_for_task_any_project(task_id) {
        return Ok(PathBuf::from(operation.project_path));
    }
    resolve_project_root(registry).ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "PROJECT_PATH_REQUIRED",
            "project_path is required when no project is selected",
            false,
        ))
    })
}

fn validate_task_owner(
    project: &str,
    task_id: &str,
    agent_id: &str,
) -> Result<(), (u16, serde_json::Value)> {
    let models = rebuild_operation_read_models(project).map_err(|err| (500, json!({"error":err,"code":"STATE_UNAVAILABLE","recoverable":true,"retry_after_ms":null,"context":null})))?;
    let task = models.tasks.iter().find(|task| task.id == TaskId(task_id.to_string()))
        .ok_or_else(|| (404, json!({"error":"unknown task","code":"TASK_NOT_FOUND","recoverable":false,"retry_after_ms":null,"context":null})))?;
    if task.claimed_by.as_deref() != Some(agent_id) {
        return Err((
            409,
            json!({"error":"task is claimed by a different agent","code":"TASK_OWNER_MISMATCH","recoverable":false,"retry_after_ms":null,"context":{"task_id":task_id,"claimed_by":task.claimed_by}}),
        ));
    }
    Ok(())
}

fn librarian_indexer_path(app: &AppHandle) -> Option<String> {
    crate::settings_store::read_public_settings(app)
        .get("librarian_indexer_path")
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty())
}

fn emit_panes_changed(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) {
    push_panes_sse(registry);
    let _ = app.emit("pty://panes-changed", json!({ "changed": true }));
}

/// Pop out a worker pane so MCP/bridge spawns are visible even when the home screen is open.
fn present_spawned_pane(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
) {
    if pane_id.starts_with("puppet-master-orchestrator-") {
        emit_panes_changed(registry, app);
        return;
    }
    let Some(pane) = registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id)
    else {
        emit_panes_changed(registry, app);
        return;
    };
    let title = format!(
        "{} · {}",
        pane.agent_type,
        pane.id.chars().take(8).collect::<String>()
    );
    let _ = app.emit(
        "pane://detach",
        json!({
            "pane_id": pane.id,
            "title": title,
            "cols": pane.cols,
            "rows": pane.rows,
        }),
    );
    emit_panes_changed(registry, app);
}

fn operation_response_value(snapshot: crate::operations::OperationSnapshot) -> serde_json::Value {
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

fn resolve_default_opencode_pane_id(registry: &Arc<Mutex<PaneRegistry>>) -> Option<String> {
    let panes: Vec<_> = registry
        .lock()
        .list()
        .into_iter()
        .filter(|pane| pane.agent_type == "opencode_native" && pane.status != "error")
        .collect();
    if panes.len() == 1 {
        Some(panes[0].id.clone())
    } else {
        None
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, (u16, serde_json::Value)> {
    serde_json::from_slice(body)
        .map_err(|err| (400, json!({ "error": format!("invalid json: {err}") })))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

fn requested_project_path(body_path: Option<&str>, query: &str) -> Option<String> {
    body_path
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
        .or_else(|| query_param(query, "project_path"))
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if percent_decode(key).as_deref() == Some(name) {
            percent_decode(value)
        } else {
            None
        }
    })
}

fn pane_buffer_view(
    requested: Option<&str>,
    agent_type: &str,
) -> Result<&'static str, &'static str> {
    let default = if crate::pty::status::is_shell_agent(agent_type) {
        "scrollback"
    } else {
        "screen"
    };
    match requested {
        None => Ok(default),
        Some("screen") => Ok("screen"),
        Some("scrollback") => Ok("scrollback"),
        Some(_) => Err("view must be screen or scrollback"),
    }
}

fn register_spawned_pane(
    session: Option<&str>,
    agent_type: &str,
    pane_id: &str,
) -> Result<(), crate::operations::OperationError> {
    let Some(session) = session else {
        return Ok(());
    };
    if crate::pty::status::is_shell_agent(agent_type) {
        crate::mcp_sessions::register_shell_pane(session, pane_id)
    } else {
        crate::mcp_sessions::register_owned_pane(session, pane_id)
    }
}

fn normalize_shell_cwd(
    requested: Option<&str>,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<String, crate::operations::OperationError> {
    let raw_path = requested
        .map(str::to_string)
        .unwrap_or_else(|| crate::pty::registry_get_project_path(registry));
    let normalized =
        crate::project_path::normalize_project_path(std::path::Path::new(&raw_path))
            .map_err(|error| crate::operations::OperationError::new("INVALID_CWD", error, false))?;
    let raw = normalized.to_string_lossy();
    if raw.contains('\0') || raw.contains('\r') || raw.contains('\n') {
        return Err(crate::operations::OperationError::new(
            "INVALID_CWD",
            "cwd cannot contain NUL or newline characters",
            false,
        ));
    }
    #[cfg(windows)]
    if raw.contains('"') {
        return Err(crate::operations::OperationError::new(
            "INVALID_CWD",
            "cwd contains a character that cannot be represented safely by cmd.exe",
            false,
        ));
    }
    let canonical = std::fs::canonicalize(&normalized).map_err(|error| {
        crate::operations::OperationError::new(
            "INVALID_CWD",
            format!("cwd does not exist or cannot be accessed: {error}"),
            false,
        )
    })?;
    if !canonical.is_dir() {
        return Err(crate::operations::OperationError::new(
            "INVALID_CWD",
            "cwd must name an existing directory",
            false,
        ));
    }
    let mut path = canonical.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        if let Some(stripped) = path.strip_prefix(r"\\?\UNC\") {
            path = format!(r"\\{stripped}");
        } else if let Some(stripped) = path.strip_prefix(r"\\?\") {
            path = stripped.to_string();
        }
    }
    Ok(path)
}

fn percent_decode(value: &str) -> Option<String> {
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
                let high = (bytes[index + 1] as char).to_digit(16)? as u8;
                let low = (bytes[index + 2] as char).to_digit(16)? as u8;
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'%' => return None,
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    value: &serde_json::Value,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let body = if status == 204 {
        String::new()
    } else {
        serde_json::to_string(value).map_err(|err| format!("serialize response: {err}"))?
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, PATCH, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-PM-Proxied\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        body.as_bytes().len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| format!("write response: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_buffer_default_follows_the_pane_type_and_explicit_views_override_it() {
        assert_eq!(pane_buffer_view(None, "codex"), Ok("screen"));
        assert_eq!(pane_buffer_view(None, "bash"), Ok("scrollback"));
        assert_eq!(
            pane_buffer_view(Some("scrollback"), "codex"),
            Ok("scrollback")
        );
        assert_eq!(pane_buffer_view(Some("screen"), "powershell"), Ok("screen"));
        assert!(pane_buffer_view(Some("raw"), "codex").is_err());
    }

    #[test]
    fn spawned_shells_register_as_shell_panes_for_the_request_session() {
        let session = uuid::Uuid::new_v4().to_string();
        let pane_id = uuid::Uuid::new_v4().to_string();
        register_spawned_pane(Some(&session), "powershell", &pane_id).unwrap();
        crate::mcp_sessions::set_mode(&session, "shell").unwrap();
        assert!(crate::mcp_sessions::authorize(
            Some(&session),
            "POST",
            &format!("/panes/{pane_id}/input"),
            &json!({"text":"Get-Location"})
        )
        .is_ok());
    }

    #[test]
    fn spawned_agents_register_as_agent_panes_for_the_request_session() {
        let session = uuid::Uuid::new_v4().to_string();
        let pane_id = uuid::Uuid::new_v4().to_string();
        register_spawned_pane(Some(&session), "codex", &pane_id).unwrap();
        crate::mcp_sessions::set_mode(&session, "shell").unwrap();
        assert!(crate::mcp_sessions::authorize(
            Some(&session),
            "POST",
            &format!("/panes/{pane_id}/input"),
            &json!({"text":"unexpected takeover"})
        )
        .is_err());
    }

    #[test]
    fn mcp_resources_route_returns_session_resource() {
        let (status, value) = mcp_registry_route("GET", &["mcp", "resources"]).unwrap();
        assert_eq!(status, 200);
        let resources = value.as_array().unwrap();
        assert!(resources.iter().any(|resource| {
            resource.get("uri").and_then(serde_json::Value::as_str)
                == Some("puppet-master://session")
        }));
    }

    #[test]
    fn mcp_prompts_route_returns_status_check_prompt() {
        let (status, value) = mcp_registry_route("GET", &["mcp", "prompts"]).unwrap();
        assert_eq!(status, 200);
        let prompts = value.as_array().unwrap();
        assert!(prompts.iter().any(|prompt| {
            prompt.get("name").and_then(serde_json::Value::as_str) == Some("status_check")
        }));
    }

    #[test]
    fn bridge_tool_names_include_session_context_routes() {
        assert_eq!(
            bridge_tool_name("GET", &["session", "context"]).as_deref(),
            Some("read_session_context")
        );
        assert_eq!(
            bridge_tool_name("PATCH", &["session", "context"]).as_deref(),
            Some("update_session_context")
        );
        assert_eq!(
            bridge_tool_name("POST", &["panes", "pane-1", "role"]).as_deref(),
            Some("set_pane_role")
        );
        assert_eq!(
            bridge_tool_name("POST", &["delegate-task"]).as_deref(),
            Some("delegate_task")
        );
        assert_eq!(
            bridge_tool_name("POST", &["operations", "delegate"]).as_deref(),
            Some("delegate_work")
        );
        assert_eq!(
            bridge_tool_name("GET", &["operations", "op-1"]).as_deref(),
            Some("get_operation")
        );
        assert_eq!(
            bridge_tool_name("POST", &["operations", "op-1", "wait"]).as_deref(),
            Some("wait_for_operation")
        );
        assert_eq!(
            bridge_tool_name("POST", &["operations", "op-1", "cancel"]).as_deref(),
            Some("cancel_operation")
        );
    }

    #[test]
    fn operation_lock_contract_requires_typed_nonempty_resources() {
        assert_eq!(
            parse_operation_lock("file:src/main.rs").unwrap(),
            ("file".into(), "src/main.rs".into())
        );
        assert_eq!(
            parse_operation_lock(" :name ").unwrap_err().code,
            "INVALID_LOCK"
        );
        assert_eq!(
            parse_operation_lock("missing-type").unwrap_err().code,
            "INVALID_LOCK"
        );
    }

    #[test]
    fn readiness_distinguishes_normal_agent_prompt_from_real_permission() {
        assert!(!explicit_manual_approval_required(
            "codex",
            "How can I help? >",
            &[]
        ));
        assert!(explicit_manual_approval_required(
            "codex",
            "Allow this command to run? (y/n)",
            &[],
        ));
        assert!(explicit_manual_approval_required(
            "opencode_native",
            "Waiting for input",
            &["permission-123".into()],
        ));
    }

    #[test]
    fn native_pending_permission_blocks_startup_even_when_pane_looks_idle() {
        assert!(startup_manual_approval_required(
            "opencode_native",
            Some("idle"),
            "",
            &["permission-123".into()],
        ));
        assert!(!startup_manual_approval_required(
            "codex",
            Some("idle"),
            "Allow this command?",
            &[],
        ));
        assert!(startup_manual_approval_required(
            "codex",
            Some("waiting_input"),
            "Allow this command?",
            &[],
        ));
    }

    #[test]
    fn operation_errors_use_the_structured_retry_contract() {
        let mut error = crate::operations::OperationError::new("RESOURCE_LOCKED", "busy", true);
        error.retry_after_ms = Some(750);
        error.context = json!({"resource_id":"file:src/main.rs"});
        let (status, body) = operation_http_error(error);
        assert_eq!(status, 409);
        assert_eq!(body["code"], "RESOURCE_LOCKED");
        assert_eq!(body["message"], "busy");
        assert_eq!(body["error"], "busy");
        assert_eq!(body["recoverable"], true);
        assert_eq!(body["retry_after_ms"], 750);
        assert_eq!(body["context"]["resource_id"], "file:src/main.rs");
        for (code, expected_status) in [
            ("IDEMPOTENCY_KEY_CONFLICT", 409),
            ("PANE_UNAVAILABLE", 409),
            ("INVALID_AGENT_TYPE", 400),
        ] {
            let (status, body) = operation_http_error(crate::operations::OperationError::new(
                code, "invalid", false,
            ));
            assert_eq!(status, expected_status, "wrong mapping for {code}");
            assert_eq!(body["code"], code);
        }
    }

    #[test]
    fn native_permission_status_failure_is_recoverable_and_fails_closed() {
        let error = native_status_unavailable("worker API is unreachable");
        assert_eq!(error.code, "NATIVE_STATUS_UNAVAILABLE");
        assert!(error.recoverable);
        let (status, body) = operation_http_error(error);
        assert_eq!(status, 503);
        assert_eq!(body["code"], "NATIVE_STATUS_UNAVAILABLE");
        assert_eq!(body["recoverable"], true);
    }

    #[test]
    fn query_parameters_decode_url_encoded_paths_and_reject_malformed_values() {
        assert_eq!(
            query_param(
                "project_path=C%3A%2FUsers%2FDev%20Folder%2Fapp",
                "project_path"
            ),
            Some("C:/Users/Dev Folder/app".into())
        );
        assert_eq!(query_param("project_path=%ZZ", "project_path"), None);
        assert_eq!(query_param("other=value", "project_path"), None);
    }

    #[test]
    fn wait_project_path_prefers_body_then_query() {
        assert_eq!(
            requested_project_path(Some("C:/body"), "project_path=C%3A%2Fquery"),
            Some("C:/body".into())
        );
        assert_eq!(
            requested_project_path(None, "project_path=C%3A%2Fquery"),
            Some("C:/query".into())
        );
        assert_eq!(requested_project_path(Some("  "), ""), None);
    }
}

/// Fail fast: acquire the operation's locks before any worker starts. On conflict the queued
/// operation is marked failed (never left running) and the RESOURCE_LOCKED error is returned.
pub(crate) fn fail_operation_on_lock_conflict(
    snapshot: &crate::operations::OperationSnapshot,
) -> Result<(), crate::operations::OperationError> {
    let Err(mut error) = acquire_operation_locks(snapshot) else {
        return Ok(());
    };
    if let Some(context) = error.context.as_object_mut() {
        context.insert(
            "operation_id".into(),
            serde_json::Value::String(snapshot.operation_id.clone()),
        );
    }
    let _ = release_operation_locks(snapshot);
    let _ = crate::operations::mark_operation_state(
        &snapshot.project_path,
        &snapshot.operation_id,
        crate::operations::OperationStatus::Failed,
        crate::operations::StateSource::Native,
        Some("failed".into()),
        None,
        Some(error.clone()),
    );
    Err(error)
}

fn delegate_operation_http(
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
        crate::project_path::prepare_project_path(std::path::Path::new(&req.project_path))
            .map_err(|err| {
                operation_http_error(crate::operations::OperationError::new(
                    "INVALID_PROJECT_PATH",
                    err,
                    false,
                ))
            })?
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
    return Ok((202, operation_response_value(snapshot)));
}

fn cancel_operation_http(
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
    return Ok((200, operation_response_value(snapshot)));
}

fn request_session(headers: &str) -> Option<&str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.eq_ignore_ascii_case("x-puppet-master-session") && !value.trim().is_empty())
            .then_some(value.trim())
    })
}

fn operation_error_from_http(error: (u16, serde_json::Value)) -> crate::operations::OperationError {
    serde_json::from_value(error.1.clone()).unwrap_or_else(|_| {
        crate::operations::OperationError::new(
            "BRIDGE_ERROR",
            error
                .1
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("bridge operation failed"),
            error.0 >= 500,
        )
    })
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

pub fn cancel_operation_control(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    project: &str,
    id: &str,
) -> Result<crate::operations::OperationSnapshot, crate::operations::OperationError> {
    let (_, value) = cancel_operation_http(project.to_string(), id, registry.clone(), app.clone())
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
        if snapshot.pane_id.is_some() {
            crate::agent_runs::pane_close::maybe_dispose_after_terminal(snapshot, registry)?;
            emit_panes_changed(registry, app);
        }
    }
    push_operation_sse(snapshot);
    Ok(())
}

fn registry_pane_ids(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<String> {
    registry.lock().panes.keys().cloned().collect()
}

/// Upgrade a bare AGENT_NOT_FOUND into a kind-aware message when the id is really a pane id.
fn enrich_handle_error(
    error: crate::operations::OperationError,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> crate::operations::OperationError {
    if error.code != "AGENT_NOT_FOUND" {
        return error;
    }
    let (project, cwds) = {
        let guard = registry.lock();
        (
            guard.project_path.clone(),
            guard
                .panes
                .iter()
                .map(|(id, pane)| (id.clone(), pane.info.cwd.clone()))
                .collect::<std::collections::HashMap<_, _>>(),
        )
    };
    let ids: Vec<String> = cwds.keys().cloned().collect();
    crate::mcp_sessions::enrich_handle_error_with(error, &ids, |pane_id| {
        crate::agent_runs::latest_handle_for_pane(&project, pane_id)
            .ok()
            .or_else(|| {
                cwds.get(pane_id)
                    .and_then(|cwd| crate::agent_runs::latest_handle_for_pane(cwd, pane_id).ok())
            })
    })
}

fn handoff_route(
    session: Option<&str>,
    request: &serde_json::Value,
    registry: &Arc<Mutex<PaneRegistry>>,
    release: bool,
) -> Result<serde_json::Value, crate::operations::OperationError> {
    use crate::operations::OperationError;
    let session = session.ok_or_else(|| {
        OperationError::new(
            "SESSION_REQUIRED",
            "handoff requires an MCP connection",
            false,
        )
    })?;
    let requested_handle = request
        .get("handle")
        .and_then(serde_json::Value::as_str)
        .filter(|handle| !handle.is_empty());
    let mut prompt = serde_json::Value::Null;
    let mut run_handle: Option<String> = None;
    let pane_id = if let Some(handle) = requested_handle {
        let project = resolve_operation_project(
            request
                .get("project_path")
                .and_then(serde_json::Value::as_str),
            registry,
        )
        .map_err(operation_error_from_http)?;
        let project_str = project.to_string_lossy().into_owned();
        let mut resolved_name = None;
        let lookup = match crate::operations::resolve_agent_run(&project_str, handle) {
            Err(error) if error.code == "AGENT_NOT_FOUND" => {
                match crate::agent_runs::resolve_worker_name(&project_str, handle)? {
                    Some(named) => {
                        let found = crate::operations::resolve_agent_run(&project_str, &named);
                        resolved_name = Some(named);
                        found
                    }
                    None => Err(error),
                }
            }
            other => other,
        };
        let handle = resolved_name.as_deref().unwrap_or(handle);
        match lookup {
            Ok(snapshot) => {
                prompt = snapshot.required_action.unwrap_or(serde_json::Value::Null);
                run_handle = Some(handle.to_string());
                snapshot.pane_id.clone().unwrap_or_default()
            }
            Err(error) => {
                let ids = registry_pane_ids(registry);
                match crate::mcp_sessions::resolve_pane_id(handle, &ids) {
                    Ok(id) => id,
                    Err(_) => {
                        return Err(crate::mcp_sessions::enrich_handle_error(error, &ids))
                    }
                }
            }
        }
    } else {
        let requested = request
            .get("pane_id")
            .and_then(serde_json::Value::as_str)
            .filter(|pane| !pane.is_empty())
            .ok_or_else(|| {
                OperationError::new("PANE_REQUIRED", "specify a handle or pane_id", false)
            })?;
        let ids = registry_pane_ids(registry);
        match crate::mcp_sessions::resolve_pane_id(requested, &ids) {
            Ok(id) => id,
            Err(error) => {
                if error.code == "PANE_NOT_FOUND" {
                    if let Some(closed) = crate::agent_runs::pane_close::closed_after_run_error(
                        &crate::pty::registry::get_project_path(registry),
                        registry,
                        requested,
                    ) {
                        return Err(closed);
                    }
                    if let Ok(project) = resolve_operation_project(
                        request
                            .get("project_path")
                            .and_then(serde_json::Value::as_str),
                        registry,
                    ) {
                        if crate::operations::resolve_agent_run(
                            &project.to_string_lossy(),
                            requested,
                        )
                        .is_ok()
                        {
                            return Err(OperationError::new("PANE_NOT_FOUND",format!("{requested} is an agent run handle, not a pane id; pass it as `handle`"),false));
                        }
                    }
                }
                return Err(error);
            }
        }
    };
    if !pane_id.is_empty() && pane_id.starts_with("puppet-master-orchestrator-") {
        return Err(OperationError::new(
            "AUTHORIZATION_DENIED",
            "orchestrator panes cannot be taken over",
            false,
        ));
    }
    let pane_alive = !pane_id.is_empty() && registry.lock().panes.contains_key(&pane_id);
    if !pane_alive && !pane_id.is_empty() {
        if let Some(error) = crate::agent_runs::pane_close::closed_after_run_error(
            &crate::pty::registry::get_project_path(registry),
            registry,
            &pane_id,
        ) {
            return Err(error);
        }
    }
    let screen = if pane_alive {
        registry_read_snapshot(registry, &pane_id)
            .map_err(|error| OperationError::new("PANE_NOT_FOUND", error, false))?
    } else {
        String::new()
    };
    if release {
        if let Some(handle) = run_handle.as_deref() {
            crate::mcp_sessions::check_run_access(session, handle)?;
        }
    }
    if release {
        crate::mcp_sessions::release_owned_pane(session, &pane_id)?;
        if let Some(handle) = run_handle.as_deref() {
            crate::mcp_sessions::release_owned_agent(session, handle)?;
        }
    } else {
        let grant = request
            .get("grant")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if pane_alive {
            crate::mcp_sessions::take_over_pane(session, &pane_id, grant)?;
        } else if run_handle.is_none() {
            return Err(OperationError::new(
                "PANE_NOT_FOUND",
                "pane is not in the registry; pass `handle` with grant=true to reclaim the run lease",
                false,
            ));
        }
        if let Some(handle) = run_handle.as_deref() {
            crate::mcp_sessions::take_over_agent(session, handle, grant)?;
        }
    }
    Ok(json!({
        "handle": run_handle.as_deref().or(requested_handle).unwrap_or(pane_id.as_str()),
        "pane_id": if pane_id.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(pane_id.clone())
        },
        "screen": screen,
        "prompt": prompt,
        "control": if release { "agent" } else { "shell" },
    }))
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

pub fn dispatch_existing_operation(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    project: &str,
    id: &str,
) -> Result<(), crate::operations::OperationError> {
    let snapshot = crate::operations::get_operation(project, id)?;
    if snapshot.status != crate::operations::OperationStatus::Queued {
        return Err(crate::operations::OperationError::new(
            "OPERATION_ALREADY_DISPATCHED",
            "operation must be queued",
            false,
        ));
    }
    let registry_for_dispatch = registry.clone();
    let app_for_dispatch = app.clone();
    let operation_project = snapshot.project_path.clone();
    let operation_id = snapshot.operation_id.clone();
    let registry_for_cleanup = registry.clone();
    let app_for_cleanup = app.clone();
    thread::spawn(move || {
        tracing::info!(operation_id = %operation_id, "operation dispatch started");
        let result = crate::operations::run_operation_dispatch(
            &operation_project,
            &operation_id,
            move |operation| {
                let mut _pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
                let state = crate::operations::get_operation(
                    &operation.project_path,
                    &operation.operation_id,
                )?;
                if state.status != crate::operations::OperationStatus::Starting {
                    return Err(crate::operations::OperationError::new(
                        "OPERATION_CANCELLED",
                        "operation was cancelled before pane assignment",
                        false,
                    ));
                }
                let task_id = operation.task_id.clone().unwrap_or_else(|| TaskId::new().0);
                operation.task_id = Some(task_id.clone());
                append_operation_event(
                    &operation.project_path,
                    SystemEvent::TaskCreated {
                        task_id: TaskId(task_id.clone()),
                        title: operation.task.clone(),
                        exclusive: operation.exclusive,
                    },
                )
                .map_err(|err| {
                    crate::operations::OperationError::new("EVENT_WRITE_FAILED", err, true)
                })?;
                *operation = crate::operations::checkpoint_operation(operation)?;
                acquire_operation_locks(operation)?;
                let agent_type = operation.agent_type.clone();
                if AgentType::parse(&agent_type).is_none() {
                    return Err(crate::operations::OperationError::new(
                        "INVALID_AGENT_TYPE",
                        format!("unsupported agent_type: {agent_type}"),
                        false,
                    ));
                }
                if agent_type == "cursor" {
                    return Err(crate::operations::OperationError::new(
                        "AGENT_NOT_DISPATCHABLE",
                        "cursor opens the IDE and cannot run delegated work; use claude, codex, opencode, or opencode_native",
                        false,
                    ));
                }
                if operation.read_only
                    && !crate::agent_runs::read_only_supported(
                        &agent_type,
                        operation.pane_id.is_some(),
                    )
                {
                    return Err(crate::operations::OperationError::new(
                        "READ_ONLY_UNSUPPORTED",
                        crate::agent_runs::read_only_unsupported_message(
                            &agent_type,
                            operation.pane_id.is_some(),
                        ),
                        false,
                    ));
                }
                let project_path = std::path::PathBuf::from(&operation.project_path);
                if let Some(requested_id) = operation.pane_id.clone() {
                    if agent_type == "opencode_native" {
                        drop(_pane_assignment_guard);
                        let _ = crate::opencode::status::wait_until_native_accepts_prompt(
                            &registry_for_dispatch,
                            &requested_id,
                            Duration::from_secs(8),
                        );
                        _pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
                    }
                }
                let existing_panes = registry_for_dispatch.lock().list();
                let eligible = |pane: &&crate::pty::PaneInfo| {
                    (operation
                        .owner_session_id
                        .as_deref()
                        .map_or(true, |session| {
                            crate::mcp_sessions::session_controls_pane(session, &pane.id)
                        }))
                        && pane_ready_for_dispatch(pane, &agent_type, &registry_for_dispatch)
                        && crate::project_path::normalize_project_path(std::path::Path::new(
                            &pane.cwd,
                        ))
                        .ok()
                        .as_deref()
                            == Some(project_path.as_path())
                        && crate::operations::conflicting_pane_operation(
                            &operation.project_path,
                            &pane.id,
                            &operation.operation_id,
                        )
                        .ok()
                        .flatten()
                        .is_none()
                };
                let (pane_id, pane_created) = if let Some(requested_id) = operation.pane_id.clone()
                {
                    let pane = existing_panes
                        .iter()
                        .find(|pane| pane.id == requested_id)
                        .ok_or_else(|| {
                            crate::agent_runs::pane_close::bound_pane_missing_error(&requested_id)
                        })?;
                    if !pane_ready_for_dispatch(pane, &agent_type, &registry_for_dispatch)
                    {
                        let mut error = crate::operations::OperationError::new(
                            "PANE_UNAVAILABLE",
                            "requested pane is not ready to accept work for this agent type",
                            true,
                        );
                        error.context = json!({
                            "pane_id": pane.id,
                            "pane_status": pane.status,
                            "agent_type": pane.agent_type,
                        });
                        return Err(error);
                    }
                    let pane_cwd = std::path::PathBuf::from(&pane.cwd);
                    if !crate::project_path::workspace_covers(&project_path, &pane_cwd)
                        && !crate::project_path::workspace_covers(&pane_cwd, &project_path)
                    {
                        let mut error = crate::operations::OperationError::new(
                            "WORKSPACE_MISMATCH",
                            format!(
                                "worker workspace is {}; supplied {}",
                                pane.cwd, operation.project_path
                            ),
                            false,
                        );
                        error.context = json!({
                            "expected": pane.cwd,
                            "supplied": operation.project_path,
                            "pane_id": pane.id,
                        });
                        return Err(error);
                    }
                    if let Some(active) = crate::operations::conflicting_pane_operation(
                        &operation.project_path,
                        &pane.id,
                        &operation.operation_id,
                    )? {
                        let mut error = crate::operations::OperationError::new(
                            "PANE_BUSY",
                            "requested pane already has an active operation",
                            true,
                        );
                        error.context =
                            json!({"pane_id": pane.id, "operation_id": active.operation_id});
                        return Err(error);
                    }
                    (requested_id, false)
                } else if let Some(pane) = existing_panes.iter().find(eligible) {
                    (pane.id.clone(), false)
                } else {
                    drop(_pane_assignment_guard);
                    operation.stage = Some("spawning_pane".into());
                    *operation = crate::operations::save_operation_snapshot(operation)?;
                    tracing::info!(
                        operation_id = %operation.operation_id,
                        agent = %agent_type,
                        "dispatch spawning worker pane"
                    );
                    let pane_id = spawn_pane_with_timeout(
                        &registry_for_dispatch,
                        &app_for_dispatch,
                        SpawnPaneArgs {
                            agent_type: agent_type.clone(),
                            cwd: Some(operation.project_path.clone()),
                            cols: None,
                            rows: None,
                            extra_args: None,
                            pane_id: None,
                        },
                        Duration::from_secs(45),
                    )?;
                    _pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
                    (pane_id, true)
                };
                if pane_created {
                    present_spawned_pane(&registry_for_dispatch, &app_for_dispatch, &pane_id);
                }
                operation.pane_created = pane_created;
                operation.pane_id = Some(pane_id.clone());
                if let Some(session) = operation.owner_session_id.as_deref() {
                    if !pane_created
                        && !crate::mcp_sessions::session_controls_pane(session, &pane_id)
                    {
                        return Err(crate::operations::OperationError::new(
                            "AUTHORIZATION_DENIED",
                            "take over this pane explicitly before assigning work",
                            false,
                        ));
                    }
                    crate::mcp_sessions::register_owned_pane(session, &pane_id)?;
                }
                *operation = crate::operations::reserve_pane(
                    &operation.project_path,
                    &operation.operation_id,
                    &pane_id,
                    pane_created,
                )?;
                drop(_pane_assignment_guard);
                if pane_created {
                    operation.stage = Some("waiting_for_pane_readiness".into());
                    *operation = crate::operations::save_operation_snapshot(operation)?;
                    let readiness = crate::pane_wait::wait_for_dispatch_ready(
                        &registry_for_dispatch,
                        &pane_id,
                        120_000,
                    )
                    .map_err(|err| {
                        crate::operations::OperationError::new("PANE_READINESS_FAILED", err, true)
                    })?;
                    if readiness.reason == "timeout"
                        || readiness.status.as_deref() == Some("error")
                        || readiness.reason == "gone"
                    {
                        let mut error = crate::operations::OperationError::new(
                            "PANE_NOT_READY",
                            format!(
                                "worker pane did not reach a safe ready state ({})",
                                readiness.reason
                            ),
                            true,
                        );
                        error.context = json!({"pane_id":pane_id,"status":readiness.status,"reason":readiness.reason});
                        return Err(error);
                    }
                    let screen = registry_read_snapshot(&registry_for_dispatch, &pane_id)
                        .unwrap_or_default();
                    let native_permissions = if agent_type == "opencode_native" {
                        let status = crate::opencode::status::worker_status(
                            &registry_for_dispatch,
                            &pane_id,
                        )
                        .map_err(native_status_unavailable)?;
                        if let Some(model) = status
                            .session_model
                            .or(status.last_user_model)
                        {
                            operation.worker.resolved_model =
                                Some(format!("{}/{}", model.provider_id, model.model_id));
                        }
                        status.pending_permission_ids
                    } else {
                        Vec::new()
                    };
                    if startup_manual_approval_required(
                        &agent_type,
                        readiness.status.as_deref(),
                        &screen,
                        &native_permissions,
                    ) {
                        let required_action = if !native_permissions.is_empty() {
                            json!({"kind":"permission_required","pane_id":pane_id,"permission_ids":native_permissions})
                        } else {
                            json!({"kind":"manual_approval_required","pane_id":pane_id,"detail":"Resolve the displayed permission prompt manually; the bridge will not approve it."})
                        };
                        *operation = crate::operations::mark_operation_startup_wait(
                            &operation.project_path,
                            &operation.operation_id,
                            Some("waiting_input".into()),
                            Some(required_action),
                            if agent_type == "opencode_native" {
                                crate::operations::StateSource::Native
                            } else {
                                crate::operations::StateSource::Inferred
                            },
                        )?;
                        push_operation_sse(operation);
                        let ready_state = wait_for_startup_permission_resolution(
                            &registry_for_dispatch,
                            &operation.project_path,
                            &operation.operation_id,
                            &pane_id,
                            &agent_type,
                        )?;
                        *operation = crate::operations::resume_operation_dispatch_when_ready(
                            &operation.project_path,
                            &operation.operation_id,
                            &ready_state,
                            if agent_type == "opencode_native" {
                                crate::operations::StateSource::Native
                            } else {
                                crate::operations::StateSource::Inferred
                            },
                        )?;
                        push_operation_sse(operation);
                    }
                }
                operation.stage = Some("dispatching".into());
                *operation = crate::operations::save_operation_snapshot(operation)?;
                append_operation_event(
                    &operation.project_path,
                    SystemEvent::TaskClaimed {
                        task_id: TaskId(task_id.clone()),
                        agent_id: pane_id.clone(),
                        lease_expires_at_ms: lease_expires_at(None),
                    },
                )
                .map_err(|err| {
                    crate::operations::OperationError::new("EVENT_WRITE_FAILED", err, true)
                })?;
                *operation = crate::operations::save_operation_snapshot(operation)?;
                let mut prompt = operation.task.clone();
                if operation.worker_has_mcp_tools {
                    prompt.push_str(&format!("\nWhen complete call complete_task with task_id={}, agent_id={}, project_path={} and evidence. If blocked call report_task_status with status=blocked and reason.",task_id,pane_id,operation.project_path));
                }
                let _dispatch_guard = PANE_ASSIGNMENT_SERIAL.lock();
                let current = crate::operations::get_operation(
                    &operation.project_path,
                    &operation.operation_id,
                )?;
                if current.status != crate::operations::OperationStatus::Starting {
                    return Err(crate::operations::OperationError::new(
                        "OPERATION_CANCELLED",
                        "operation was cancelled before prompt dispatch",
                        false,
                    ));
                }
                if let Some(session) = operation.owner_session_id.as_deref() {
                    crate::mcp_sessions::register_owned_pane(session, &pane_id)?;
                }
                if agent_type == "opencode_native" {
                    let baseline = crate::opencode::messages::read_pane_messages(
                        &registry_for_dispatch,
                        &pane_id,
                        10_000,
                        None,
                    )
                    .map_err(|error| {
                        crate::operations::OperationError::new(
                            "NATIVE_STATUS_UNAVAILABLE",
                            error,
                            true,
                        )
                    })?;
                    if !baseline.session_id.is_empty() {
                        operation.worker.provider_session_id = Some(baseline.session_id.clone());
                    }
                    operation.message_baseline_ids = baseline
                        .messages
                        .into_iter()
                        .filter_map(|message| message.id)
                        .collect();
                }
                if let Ok(screen) =
                    registry_read_snapshot(&registry_for_dispatch, &pane_id)
                {
                    operation.output_baseline = crate::operations::compact_output_baseline(&screen);
                }
                crate::agent_runs::record_user_task(
                    &operation.project_path,
                    &operation.operation_id,
                    &prompt,
                );
                operation.started_at_ms = Some(crate::event_log::now_ms().max(0) as u64);
                *operation = crate::operations::save_operation_snapshot(operation)?;
                crate::operations::dispatch_input_if_active(
                    &operation.project_path,
                    &operation.operation_id,
                    |active| {
                        if active.pane_id.as_deref() != Some(pane_id.as_str()) {
                            return Err(crate::operations::OperationError::new(
                                "PANE_RESERVATION_LOST",
                                "operation no longer owns its dispatch pane",
                                false,
                            ));
                        }
                        registry_write_input(
                            &registry_for_dispatch,
                            &app_for_dispatch,
                            &pane_id,
                            &prompt,
                            true,
                            agent_type == "opencode_native",
                            None,
                        )
                        .map_err(|err| {
                            crate::operations::OperationError::new("DISPATCH_FAILED", err, true)
                        })
                    },
                )?;
                operation.stage = Some("dispatched_waiting_for_task_completion".into());
                emit_panes_changed(&registry_for_dispatch, &app_for_dispatch);
                Ok(())
            },
        );
        match result {
            Ok(updated) => push_operation_sse(&updated),
            Err(_) => {
                if let Ok(updated) =
                    crate::operations::get_operation(&operation_project, &operation_id)
                {
                    cleanup_failed_operation(&updated, &registry_for_cleanup, &app_for_cleanup);
                    push_operation_sse(&updated);
                }
            }
        }
    });
    crate::agent_runs::supervise_existing(snapshot, registry.clone(), app.clone());
    Ok(())
}
