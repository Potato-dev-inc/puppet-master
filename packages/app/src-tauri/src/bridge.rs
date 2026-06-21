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
use tauri::{AppHandle, Emitter};

use crate::events::{ResourceId, SystemEvent, TaskId};
use crate::mobile_pairing::{self, PairRequestBody};
use crate::pty::agents::AgentType;
use crate::pty::{
    registry_kill_pane, registry_read_buffer, registry_read_raw_buffer, registry_set_project_path,
    registry_spawn_pane, registry_write_input, PaneRegistry, SpawnPaneArgs,
};
use crate::settings_store;

/// SSE client — each connected GET /events response gets a sender.
type SseSender = std::sync::mpsc::SyncSender<String>;
pub type SseClients = Arc<Mutex<Vec<SseSender>>>;

/// Global SSE client registry, shared between bridge thread and Tauri commands.
static SSE_CLIENTS: once_cell::sync::OnceCell<SseClients> = once_cell::sync::OnceCell::new();

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
}

/// Forward pane status changes to mobile clients.
pub fn push_pane_status_sse(pane_id: &str, status: &str) {
    let payload = json!({ "pane_id": pane_id, "status": status });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-status\ndata: {json}\n\n"));
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
}

#[derive(Debug, Deserialize)]
struct CompleteTaskBody {
    agent_id: String,
    evidence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BlockTaskBody {
    agent_id: String,
    reason: String,
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
        ("POST", ["panes", _, "input"]) => Some("write_terminal_input".to_string()),
        ("GET", ["panes", _, "model"]) => Some("inspect_agent_model".to_string()),
        ("GET", ["panes", _, "agent-context"]) => Some("read_agent_context".to_string()),
        ("GET", ["events", "replay", "panes"]) => Some("replay_pane_timeline".to_string()),
        ("GET", ["workspace", "state"]) => Some("get_workspace_state".to_string()),
        ("GET", ["tasks"]) => Some("list_tasks".to_string()),
        ("GET", ["locks"]) => Some("list_locks".to_string()),
        ("GET", ["agents", _, "inbox"]) => Some("read_agent_inbox".to_string()),
        ("GET", ["audit"]) => Some("get_audit".to_string()),
        ("POST", ["context-packs"]) => Some("build_context_pack".to_string()),
        ("GET", ["session", "context"]) => Some("read_session_context".to_string()),
        ("PATCH", ["session", "context"]) => Some("update_session_context".to_string()),
        ("POST", ["panes", _, "role"]) => Some("set_pane_role".to_string()),
        ("GET", ["panes", _, "digest"]) => Some("read_pane_digest".to_string()),
        ("POST", ["panes", _, "digest"]) => Some("update_pane_digest".to_string()),
        ("POST", ["delegate-task"]) => Some("delegate_task".to_string()),
        ("GET", ["orchestrator", "state"]) => Some("read_orchestrator_state".to_string()),
        ("PATCH", ["orchestrator", "state"]) => Some("update_orchestrator_state".to_string()),
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
        return Ok((
            200,
            serde_json::to_value(Health {
                ok: true,
                version: "0.1.2",
            })
            .unwrap(),
        ));
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
                crate::event_log::append_bridge_event(SystemEvent::TaskStatusUpdated {
                    task_id: task_id.clone(),
                    status: req.status,
                });
                return Ok((200, json!({ "task_id": task_id.0, "ok": true })));
            }
            "complete" => {
                let req: CompleteTaskBody = parse_json(body)?;
                crate::event_log::append_bridge_event(SystemEvent::TaskCompleted {
                    task_id: task_id.clone(),
                    agent_id: req.agent_id,
                    evidence: req.evidence.unwrap_or_default(),
                });
                return Ok((200, json!({ "task_id": task_id.0, "completed": true })));
            }
            "block" => {
                let req: BlockTaskBody = parse_json(body)?;
                crate::event_log::append_bridge_event(SystemEvent::TaskBlocked {
                    task_id: task_id.clone(),
                    agent_id: req.agent_id,
                    reason: req.reason,
                });
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
        let resource_id = ResourceId::from_parts(&req.resource_type, &req.name);
        let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
        if let Some(existing) = read_models
            .locks
            .iter()
            .find(|lock| lock.resource_id == resource_id)
        {
            crate::event_log::append_bridge_event(SystemEvent::ResourceLockConflict {
                resource_id: resource_id.clone(),
                requested_owner_id: req.owner_id,
                existing_owner_id: existing.owner.clone(),
            });
            return Err((409, json!({ "error": "resource already locked" })));
        }
        crate::event_log::append_bridge_event(SystemEvent::ResourceLockAcquired {
            resource_id: resource_id.clone(),
            resource_type: req.resource_type,
            owner_id: req.owner_id,
            lease_expires_at_ms: req
                .lease_ms
                .map(|lease_ms| lease_expires_at(Some(lease_ms))),
        });
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
        let pack = crate::context_pack::build_context_pack(req, &read_models);
        return Ok((200, serde_json::to_value(pack).unwrap()));
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
            target_pane_id: req
                .target_pane_id
                .clone()
                .map(crate::events::PaneId),
            intent: req.intent.clone(),
        });
        return Ok((
            200,
            json!({
                "ok": true,
                "task_id": req.task_id,
                "target_pane_id": req.target_pane_id,
                "prompt": prompt,
            }),
        ));
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
        let standby_poll_ms = req.standby_poll_ms.unwrap_or(current.standby_poll_ms).max(1);
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
                let pane_id = registry_spawn_pane(&registry, &app, req)
                    .map_err(|err| (500, json!({ "error": err })))?;
                emit_panes_changed(&registry, &app);
                return Ok((201, json!({ "pane_id": pane_id })));
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
            let content = registry_read_buffer(&registry, pane_id, lines)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "content": content })));
        }
        if tail == Some("raw") && method == "GET" {
            let lines = query_param(query, "lines")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10_000);
            let raw = registry_read_raw_buffer(&registry, pane_id, lines)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "data": raw })));
        }
        if tail == Some("input") && method == "POST" {
            let req: WriteInputBody = parse_json(body)?;
            registry_write_input(&registry, pane_id, &req.text, req.append_newline)
                .map_err(|err| (404, json!({ "error": err })))?;
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
        if tail == Some("model") && method == "GET" {
            let lines = query_param(query, "lines")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(200);
            let pane = registry
                .lock()
                .list()
                .into_iter()
                .find(|pane| pane.id == pane_id)
                .ok_or_else(|| (404, json!({ "error": format!("unknown pane: {pane_id}") })))?;
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
                serde_json::to_value(crate::agent_contexts::inspect_agent_model(
                    pane_id, agent_type, &buffer,
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
                let context =
                    crate::agent_contexts::build_pane_agent_context(pane.clone(), &buffer)
                        .ok_or_else(|| (400, json!({ "error": "unknown pane agent_type" })))?;
                return Ok((200, serde_json::to_value(context).unwrap()));
            }
            return Err((404, json!({ "error": format!("unknown pane: {pane_id}") })));
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

fn emit_panes_changed(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) {
    push_panes_sse(registry);
    let _ = app.emit("pty://panes-changed", json!({ "changed": true }));
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

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if key == name {
            Some(value.to_string())
        } else {
            None
        }
    })
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
    }
}
