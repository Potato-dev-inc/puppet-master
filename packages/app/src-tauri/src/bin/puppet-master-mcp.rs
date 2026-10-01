use puppet_master_app_lib::{mcp_sessions, tool_registry};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const SERVER_NAME: &str = "puppet-master";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const BRIDGE_PORT_FILE_ENV: &str = "PUPPET_MASTER_BRIDGE_PORT_FILE";
const DEFAULT_BRIDGE_PORT_FILE: &str = "puppet-master.bridge.port";
const APP_ID: &str = "com.puppetmaster.app";
const MODE_ENV: &str = "PUPPET_MASTER_MODE";
/// Default foreground wait for run_agent, followup_task, and wait_agents (matches bridge routes).
const DEFAULT_AGENT_WAIT_MS: u64 = 120_000;

/// Starting tool mode requested through `--mode` or `PUPPET_MASTER_MODE`.
static INITIAL_MODE: std::sync::OnceLock<tool_registry::McpMode> = std::sync::OnceLock::new();
/// True once the bridge knows this connection's mode (either the starting mode or a `set_mode` call).
static INITIAL_MODE_SYNCED: AtomicBool = AtomicBool::new(false);

thread_local! { static MCP_SESSION_ID: RefCell<Option<String>> = const { RefCell::new(None) }; }
fn set_mcp_session_id(id: String) {
    MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = Some(id));
}
fn current_mcp_session_id() -> Option<String> {
    MCP_SESSION_ID.with(|slot| slot.borrow().clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgeEndpoint {
    host: String,
    port: u16,
}

impl BridgeEndpoint {
    fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

fn log(message: impl AsRef<str>) {
    let _ = writeln!(io::stderr(), "[puppet-master-mcp-rs] {}", message.as_ref());
}

mod puppet_master_mcp {
    pub(super) mod stdio;
}
fn main() {
    puppet_master_mcp::stdio::run();
}
fn call_tool(params: Value) -> Result<Value, String> {
    call_tool_cancellable(params, None)
}

/// Reads the starting tool mode from `--mode <mode>` / `--mode=<mode>` (wins) or the
/// `PUPPET_MASTER_MODE` value. Hosts that read the tool list once and ignore
/// `notifications/tools/list_changed` use this to launch in the catalog they need.
/// Returns `Ok(None)` when nothing was requested.
fn initial_mode_from(
    args: &[String],
    env_value: Option<String>,
) -> Result<Option<tool_registry::McpMode>, String> {
    let mut requested: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--mode=") {
            requested = Some(value.to_string());
        } else if arg == "--mode" {
            let value = iter
                .next()
                .ok_or_else(|| "--mode requires agent, shell, or both".to_string())?;
            requested = Some(value.clone());
        }
    }
    let raw = match requested.or(env_value) {
        Some(value) => value,
        None => return Ok(None),
    };
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(None);
    }
    tool_registry::McpMode::parse(&value)
        .map(Some)
        .ok_or_else(|| format!("invalid mode {raw:?}: expected agent, shell, or both"))
}

/// Tells the bridge which mode this connection started in. The local catalog is set at
/// startup; the bridge enforces the mode per session too, so it must learn it before the
/// first tool call. Retried on later calls while the bridge is unreachable.
fn ensure_initial_mode_synced() {
    let Some(mode) = INITIAL_MODE.get().copied() else {
        return;
    };
    if INITIAL_MODE_SYNCED.load(Ordering::Acquire) {
        return;
    }
    match bridge_request("POST", "/mcp/mode", Some(json!({ "mode": mode.as_str() }))) {
        Ok(_) => INITIAL_MODE_SYNCED.store(true, Ordering::Release),
        Err(err) => log(format!(
            "could not apply starting mode {} to the bridge yet: {err}",
            mode.as_str()
        )),
    }
}

fn ensure_tool_mode(name: &str) -> Result<(), String> {
    if let Some(session) = current_mcp_session_id() {
        let mode = mcp_sessions::mode_for(&session);
        if !tool_registry::tool_visible_in_mode(name, mode) {
            let requested =
                if tool_registry::tool_visible_in_mode(name, tool_registry::McpMode::Agent) {
                    "agent"
                } else {
                    "shell"
                };
            return Err(json!({"code":"MODE_MISMATCH","message":format!("{name} is unavailable in {} mode",mode.as_str()),"recoverable":false,"context":{"current_mode":mode.as_str(),"switch_with":{"mode":requested}}}).to_string());
        }
    }
    Ok(())
}

fn call_tool_with_progress(
    params: Value,
    cancel: Option<std::sync::mpsc::Receiver<()>>,
    token: Option<Value>,
    notifications: Option<std::sync::mpsc::Sender<String>>,
) -> Result<Value, String> {
    if let Some(name) = params.get("name").and_then(Value::as_str) {
        ensure_tool_mode(name)?;
    }
    let tool_name = params.get("name").and_then(Value::as_str);
    if !matches!(tool_name, Some("wait_for_operation") | Some("wait_agents")) {
        return call_tool_cancellable(params, cancel);
    }
    if tool_name == Some("wait_agents") {
        return call_wait_agents_with_progress(params, cancel, token, notifications);
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let id = required_string(&args, "operation_id")?;
    let timeout_ms = optional_number(&args, "timeout_ms").unwrap_or(120_000);
    let mut revision = optional_number(&args, "after_revision").unwrap_or(0);
    let until = args
        .get("until")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let project = args
        .get("project_path")
        .and_then(Value::as_str)
        .map(|p| format!("?project_path={}", encode_path_segment(p)))
        .unwrap_or_default();
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if cancel.as_ref().is_some_and(|rx| rx.try_recv().is_ok()) {
            return Err(r#"{"code":"REQUEST_CANCELLED","message":"wait request was cancelled","recoverable":true,"context":{}}"#.into());
        }
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let mut body = json!({ "after_revision": revision, "timeout_ms": remaining.min(4_000) });
        if let Some(path) = args.get("project_path").cloned() {
            body["project_path"] = path;
        }
        let response = bridge_request(
            "POST",
            &format!("/operations/{}/wait{}", encode_path_segment(&id), project),
            Some(body),
        )?;
        let value: Value = serde_json::from_str(&response)
            .map_err(|e| format!("invalid operation wait response: {e}"))?;
        let snapshot = value.get("snapshot").unwrap_or(&value);
        let next = snapshot
            .get("revision")
            .and_then(Value::as_u64)
            .unwrap_or(revision);
        let status = snapshot
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if next > revision || matches!(status, "completed" | "failed" | "cancelled") {
            let notification = json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":token,"progress":next,"message":snapshot}}).to_string();
            if let (Some(sender), Some(_progress_token)) = (&notifications, &token) {
                let _ = sender.send(notification);
            }
        }
        revision = next;
        let matched_state = until
            .iter()
            .any(|candidate| candidate.as_str() == Some(status));
        if matches!(status, "completed" | "failed" | "cancelled")
            || matched_state
            || std::time::Instant::now() >= deadline
        {
            let mut result = value.clone();
            if matched_state {
                result["reason"] = json!("matched_state");
            }
            return Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&result).unwrap_or(response)}],"structuredContent":result}),
            );
        }
    }
}

fn call_wait_agents_with_progress(
    params: Value,
    cancel: Option<std::sync::mpsc::Receiver<()>>,
    token: Option<Value>,
    notifications: Option<std::sync::mpsc::Sender<String>>,
) -> Result<Value, String> {
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let timeout_ms = optional_number(&args, "timeout_ms").unwrap_or(DEFAULT_AGENT_WAIT_MS);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    let mut body = args.clone();
    let mut chunk = 0_u64;
    loop {
        if cancel.as_ref().is_some_and(|rx| rx.try_recv().is_ok()) {
            return Err(r#"{"code":"REQUEST_CANCELLED","message":"agent wait request was cancelled","recoverable":true,"context":{}}"#.into());
        }
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        body["timeout_ms"] = json!(remaining.min(4_000).max(1));
        let response = bridge_request("POST", "/agents/wait", Some(body.clone()))?;
        let value: Value = serde_json::from_str(&response)
            .map_err(|e| format!("invalid wait_agents response: {e}"))?;
        chunk += 1;
        if let (Some(sender), Some(_progress_token)) = (&notifications, &token) {
            let notification = json!({
                "jsonrpc":"2.0",
                "method":"notifications/progress",
                "params":{"progressToken":token,"progress":chunk,"message":value}
            })
            .to_string();
            let _ = sender.send(notification);
        }
        let reason = value.get("reason").and_then(Value::as_str).unwrap_or_default();
        let terminal = matches!(reason, "matched_state" | "terminal" | "timeout");
        let agents = value.get("agents").and_then(Value::as_array);
        let any_needs_input = agents.is_some_and(|list| {
            list.iter().any(|agent| {
                agent
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s == "needs_input" || s == "waiting_input")
            })
        });
        if terminal || any_needs_input || std::time::Instant::now() >= deadline {
            return Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap_or(response)}],"structuredContent":value}),
            );
        }
    }
}

fn call_tool_cancellable(
    params: Value,
    cancel_rx: Option<std::sync::mpsc::Receiver<()>>,
) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call missing params.name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    ensure_tool_mode(name)?;
    let call = match name {
        "set_mode" => {
            let mode = required_string(&args, "mode")?;
            let response = bridge_request("POST", "/mcp/mode", Some(json!({"mode":mode})))?;
            if let Some(session_id) = current_mcp_session_id() {
                mcp_sessions::set_mode(&session_id, &mode).map_err(|error| {
                    serde_json::to_string(&error).unwrap_or_else(|_| error.message)
                })?;
            }
            // An explicit mode choice replaces the starting mode; never re-apply the old one.
            INITIAL_MODE_SYNCED.store(true, Ordering::Release);
            Ok(response)
        }
        "run_agent" => {
            let mut body = args.clone();
            ensure_idempotency_key(&mut body);
            match bridge_request("POST", "/agents/run", Some(body.clone())) {
                Ok(response) => Ok(response),
                Err(error) => Err(wait_timeout_recovery("/agents/run", &body, error)),
            }
        }
        "wait_agents" => bridge_request_wait_agents(&args, cancel_rx.as_ref()),
        "send_message" => {
            let mut body = args.clone();
            if body.get("message").and_then(Value::as_str).is_none() {
                if let Some(task) = body.get("task").cloned() {
                    body["message"] = task;
                }
            }
            ensure_idempotency_key(&mut body);
            match bridge_request("POST", "/agents/send", Some(body.clone())) {
                Ok(response) => Ok(response),
                Err(error) => Err(steer_timeout_recovery(&body, error)),
            }
        }
        "send_agent" | "followup_task" => {
            let mut body = args.clone();
            if body.get("task").and_then(Value::as_str).is_none() {
                if let Some(message) = body.get("message").cloned() {
                    body["task"] = message;
                }
            }
            if body
                .get("task")
                .and_then(Value::as_str)
                .map_or(true, str::is_empty)
            {
                return Err("missing required argument: message or task".into());
            }
            ensure_idempotency_key(&mut body);
            match bridge_request("POST", "/agents/followup", Some(body.clone())) {
                Ok(response) => Ok(response),
                Err(error) => Err(wait_timeout_recovery("/agents/followup", &body, error)),
            }
        }
        "answer_prompt" => bridge_request("POST", "/agents/answer", Some(args)),
        "interrupt_agent" | "cancel_agent" => {
            bridge_request("POST", "/agents/cancel", Some(args))
        }
        "inspect_agent" => {
            let handle = required_string(&args, "handle")?;
            let suffix = args
                .get("project_path")
                .and_then(Value::as_str)
                .map(|path| format!("?project_path={}", encode_path_segment(path)))
                .unwrap_or_default();
            bridge_request(
                "GET",
                &format!("/agents/{}{suffix}", encode_path_segment(&handle)),
                None,
            )
        }
        "close_agent" => bridge_request("POST", "/agents/close", Some(args)),
        "list_agents" | "list_workers" => {
            let suffix = args
                .get("project_path")
                .and_then(Value::as_str)
                .map(|path| format!("?project_path={}", encode_path_segment(path)))
                .unwrap_or_default();
            bridge_request("GET", &format!("/agents{suffix}"), None)
        }
        "agent_transcript" => {
            let handle = required_string(&args, "handle")?;
            let mut query = Vec::new();
            for key in ["project_path", "operation_id", "after"] {
                if let Some(value) = args.get(key).and_then(Value::as_str) {
                    query.push(format!("{key}={}", encode_path_segment(value)));
                } else if let Some(value) = args.get(key).and_then(Value::as_u64) {
                    query.push(format!("{key}={value}"));
                }
            }
            let suffix = if query.is_empty() {
                String::new()
            } else {
                format!("?{}", query.join("&"))
            };
            bridge_request(
                "GET",
                &format!(
                    "/agents/{}/transcript{suffix}",
                    encode_path_segment(&handle)
                ),
                None,
            )
        }
        "take_over" => bridge_request("POST", "/agents/take-over", Some(args)),
        "shell_exec" => bridge_request("POST", "/shell/exec", Some(args)),
        "release" => bridge_request("POST", "/agents/release", Some(args)),
        "session_identity" => bridge_request("GET", "/mcp/session", None),
        "attach_agents" => bridge_request("POST", "/agents/attach", Some(args)),
        "release_lease" => bridge_request("POST", "/agents/release-lease", Some(args)),
        "transfer_agent" => bridge_request("POST", "/agents/transfer", Some(args)),
        "list_panes" => bridge_request("GET", "/panes", None),
        "bridge_health" => bridge_request("GET", "/health", None),
        "list_agent_contexts" => bridge_request("GET", "/agent-contexts", None),
        "read_agent_context" => read_agent_context(&args),
        "inspect_agent_model" => {
            let pane_id = required_string(&args, "pane_id")?;
            let lines = optional_number(&args, "lines").unwrap_or(200);
            Ok(bridge_request(
                "GET",
                &format!(
                    "/panes/{}/model?lines={lines}",
                    encode_path_segment(&pane_id)
                ),
                None,
            )?)
        }
        "switch_agent_model" => {
            let model_id = required_string(&args, "model_id")?;
            let mut body = json!({ "model_id": model_id });
            if let Some(provider) = args.get("model_provider").and_then(Value::as_str) {
                body["model_provider"] = json!(provider);
            }
            let pane_id = if let Some(pane) = args.get("pane_id").and_then(Value::as_str) {
                assert_worker_pane(pane)?
            } else if let Some(handle) = args.get("handle").and_then(Value::as_str) {
                let project = args
                    .get("project_path")
                    .and_then(Value::as_str)
                    .map(|p| format!("?project_path={}", encode_path_segment(p)))
                    .unwrap_or_default();
                let inspect = bridge_request(
                    "GET",
                    &format!("/agents/{}{}", encode_path_segment(handle), project),
                    None,
                )?;
                let value: Value = serde_json::from_str(&inspect)
                    .map_err(|e| format!("invalid inspect_agent response: {e}"))?;
                let pane = value
                    .get("pane_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        format!(
                            "handle {handle} has no live pane; set keep_pane:true on run_agent to retain the pane after completion"
                        )
                    })?;
                body["handle"] = json!(handle);
                assert_worker_pane(pane)?
            } else {
                return Err("switch_agent_model requires handle or pane_id".into());
            };
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/model", encode_path_segment(&pane_id)),
                Some(body),
            )?)
        }
        "spawn_agent" => bridge_request("POST", "/panes", Some(args)),
        "read_terminal_buffer" => {
            let pane_id = required_string(&args, "pane_id")?;
            let lines = optional_number(&args, "lines").unwrap_or(200);
            let view = args
                .get("view")
                .and_then(Value::as_str)
                .map(|view| format!("&view={}", encode_path_segment(view)))
                .unwrap_or_default();
            let response = bridge_request(
                "GET",
                &format!(
                    "/panes/{}/buffer?lines={lines}{view}",
                    encode_path_segment(&pane_id)
                ),
                None,
            )?;
            Ok(serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|value| {
                    value
                        .get("content")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or(response))
        }
        "write_terminal_input" => {
            let pane_id = assert_worker_pane(&required_string(&args, "pane_id")?)?;
            let mut body = json!({
                "text": required_string(&args, "text")?,
                "append_newline": args.get("append_newline").and_then(Value::as_bool).unwrap_or(true),
                "via_opencode_api": true,
            });
            if let Some(provider) = args.get("model_provider").and_then(Value::as_str) {
                body["model_provider"] = json!(provider);
            }
            if let Some(model_id) = args.get("model_id").and_then(Value::as_str) {
                body["model_id"] = json!(model_id);
            }
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/input", encode_path_segment(&pane_id)),
                Some(body),
            )?)
        }
        "press_key" => {
            let pane_id = assert_worker_pane(&required_string(&args, "pane_id")?)?;
            let key = required_string(&args, "key")?;
            let response = bridge_request(
                "POST",
                &format!("/panes/{}/key", encode_path_segment(&pane_id)),
                Some(json!({ "key": key })),
            )?;
            Ok(serde_json::from_str::<Value>(&response)
                .ok()
                .map(|value| {
                    let pressed = value.get("key").and_then(Value::as_str).unwrap_or(&key);
                    let bytes = value.get("bytes").and_then(Value::as_u64).unwrap_or(0);
                    format!(
                        "pressed {pressed} ({bytes} byte{})",
                        if bytes == 1 { "" } else { "s" }
                    )
                })
                .unwrap_or_else(|| format!("pressed {key}")))
        }
        "kill_pane_process" => {
            let pane_id = assert_worker_pane(&required_string(&args, "pane_id")?)?;
            bridge_request(
                "DELETE",
                &format!("/panes/{}", encode_path_segment(&pane_id)),
                None,
            )?;
            Ok("killed".to_string())
        }
        "create_task" => {
            let body = json!({
                "title": required_string(&args, "title")?,
                "exclusive": args.get("exclusive").and_then(Value::as_bool).unwrap_or(true),
            });
            Ok(bridge_request("POST", "/tasks", Some(body))?)
        }
        "claim_task" => Ok(bridge_request(
            "POST",
            &format!(
                "/tasks/{}/claim",
                encode_path_segment(&required_string(&args, "task_id")?)
            ),
            Some(json!({
                "agent_id": required_string(&args, "agent_id")?,
                "lease_ms": args.get("lease_ms").cloned().unwrap_or(Value::Null),
            })),
        )?),
        "report_task_status" => Ok(bridge_request(
            "POST",
            &format!(
                "/tasks/{}/status",
                encode_path_segment(&required_string(&args, "task_id")?)
            ),
            Some(json!({
                "status": required_string(&args, "status")?,
                "agent_id": args.get("agent_id").cloned().unwrap_or(Value::Null),
                "reason": args.get("reason").cloned().unwrap_or(Value::Null),
                "project_path": args.get("project_path").cloned().unwrap_or(Value::Null),
            })),
        )?),
        "complete_task" => Ok(bridge_request(
            "POST",
            &format!(
                "/tasks/{}/complete{}",
                encode_path_segment(&required_string(&args, "task_id")?),
                args.get("project_path")
                    .and_then(Value::as_str)
                    .map(|p| format!("?project_path={}", encode_path_segment(p)))
                    .unwrap_or_default()
            ),
            Some(json!({
                "agent_id": required_string(&args, "agent_id")?,
                "evidence": args.get("evidence").and_then(Value::as_str).unwrap_or_default(),
            })),
        )?),
        "list_tasks" => bridge_request("GET", "/tasks", None),
        "acquire_resource_lock" => bridge_request("POST", "/locks", Some(args)),
        "release_resource_lock" => bridge_request("POST", "/locks/release", Some(args)),
        "build_context_pack" => bridge_request("POST", "/context-packs", Some(args)),
        "read_project_ir_status" => bridge_request("GET", "/project-ir/status", None),
        "read_librarian_prompt" => bridge_request("GET", "/librarian/prompt", None),
        "read_session_context" => bridge_request("GET", "/session/context", None),
        "update_session_context" => bridge_request("PATCH", "/session/context", Some(args)),
        "set_pane_role" => {
            let pane_id = required_string(&args, "pane_id")?;
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/role", encode_path_segment(&pane_id)),
                Some(args),
            )?)
        }
        "read_pane_digest" => {
            let pane_id = required_string(&args, "pane_id")?;
            Ok(bridge_request(
                "GET",
                &format!("/panes/{}/digest", encode_path_segment(&pane_id)),
                None,
            )?)
        }
        "update_pane_digest" => {
            let pane_id = required_string(&args, "pane_id")?;
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/digest", encode_path_segment(&pane_id)),
                Some(args),
            )?)
        }
        "delegate_task" => bridge_request("POST", "/delegate-task", Some(args)),
        "delegate_work" => {
            let mut body = args.clone();
            (|| {
                required_string(&args, "project_path")?;
                required_string(&args, "task")?;
                required_string(&args, "idempotency_key")?;
                let object = body
                    .as_object_mut()
                    .ok_or_else(|| "arguments must be an object".to_string())?;
                bridge_request(
                    "POST",
                    "/operations/delegate",
                    Some(Value::Object(object.clone())),
                )
            })()
        }
        "get_operation" => required_string(&args, "operation_id").and_then(|id| {
            let project = args
                .get("project_path")
                .and_then(Value::as_str)
                .map(|p| format!("?project_path={}", encode_path_segment(p)))
                .unwrap_or_default();
            bridge_request(
                "GET",
                &format!("/operations/{}{}", encode_path_segment(&id), project),
                None,
            )
        }),
        "wait_for_operation" => (|| {
            let id = required_string(&args, "operation_id")?;
            let timeout = optional_number(&args, "timeout_ms").unwrap_or(120_000);
            let mut body = args.clone();
            let object = body
                .as_object_mut()
                .ok_or_else(|| "arguments must be an object".to_string())?;
            object.remove("operation_id");
            let query = object
                .get("project_path")
                .and_then(Value::as_str)
                .map(|p| format!("?project_path={}", encode_path_segment(p)))
                .unwrap_or_default();
            bridge_request_wait(&id, &query, body, timeout, cancel_rx.as_ref())
        })(),
        "cancel_operation" => required_string(&args, "operation_id").and_then(|id| {
            let project = args
                .get("project_path")
                .and_then(Value::as_str)
                .map(|p| format!("?project_path={}", encode_path_segment(p)))
                .unwrap_or_default();
            bridge_request(
                "POST",
                &format!("/operations/{}/cancel{}", encode_path_segment(&id), project),
                Some(json!({})),
            )
        }),
        "read_orchestrator_state" => bridge_request("GET", "/orchestrator/state", None),
        "update_orchestrator_state" => {
            Ok(bridge_request("PATCH", "/orchestrator/state", Some(args))?)
        }
        "read_opencode_key_status" => bridge_request("GET", "/opencode/keys/status", None),
        "rotate_opencode_key" => bridge_request("POST", "/opencode/keys/rotate", Some(args)),
        "read_opencode_worker_status" => {
            let mut query = Vec::new();
            if let Some(pane_id) = args
                .get("pane_id")
                .or_else(|| args.get("worker_id"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                query.push(format!("pane_id={}", encode_path_segment(pane_id)));
            }
            let suffix = if query.is_empty() {
                String::new()
            } else {
                format!("?{}", query.join("&"))
            };
            Ok(bridge_request(
                "GET",
                &format!("/opencode/worker-status{suffix}"),
                None,
            )?)
        }
        "read_opencode_messages" => {
            let pane_id = required_string(&args, "pane_id")?;
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 200);
            let mut path = format!(
                "/panes/{}/opencode/messages?limit={limit}",
                encode_path_segment(&pane_id)
            );
            if let Some(role) = args.get("role").and_then(Value::as_str) {
                if !role.trim().is_empty() {
                    path.push_str(&format!("&role={}", encode_path_segment(role)));
                }
            }
            Ok(bridge_request("GET", &path, None)?)
        }
        "wait_for_panes" => bridge_request("POST", "/panes/wait", Some(args)),
        "wait_for_model" => bridge_request("POST", "/panes/wait/model", Some(args)),
        "wait_for_task" => bridge_request("POST", "/panes/wait/task", Some(args)),
        "wait_for_worker" => bridge_request("POST", "/panes/wait/worker", Some(args)),
        "read_recent_events" => {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50);
            let pane_id = args.get("pane_id").and_then(Value::as_str);
            let since_id = args.get("since_id").and_then(Value::as_str);
            let types = args
                .get("types")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(",")
                });
            let mut path = format!("/events/recent?limit={limit}");
            if let Some(pane_id) = pane_id {
                path.push_str(&format!("&pane_id={}", encode_path_segment(pane_id)));
            }
            if let Some(since_id) = since_id {
                path.push_str(&format!("&since_id={}", encode_path_segment(since_id)));
            }
            if let Some(types) = types.filter(|value| !value.is_empty()) {
                path.push_str(&format!("&types={}", encode_path_segment(&types)));
            }
            Ok(bridge_request("GET", &path, None)?)
        }
        "reply_opencode_permission" => {
            let pane_id = required_string(&args, "pane_id")?;
            let request_id = required_string(&args, "request_id")?;
            let reply = required_string(&args, "reply")?;
            bridge_request(
                "POST",
                &format!(
                    "/panes/{}/opencode/permissions/{}/reply",
                    encode_path_segment(&pane_id),
                    encode_path_segment(&request_id)
                ),
                Some(json!({ "reply": reply })),
            )?;
            Ok("ok".to_string())
        }
        "reply_opencode_question" => {
            let pane_id = required_string(&args, "pane_id")?;
            Ok(bridge_request(
                "POST",
                &format!(
                    "/panes/{}/opencode/question/reply",
                    encode_path_segment(&pane_id)
                ),
                Some(args),
            )?)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };

    let text = call.map_err(|error| format!("{error}"))?;

    let structured = serde_json::from_str::<Value>(&text).ok();
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if let Some(value) = structured {
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "structuredContent".into(),
                if value.is_object() {
                    value
                } else {
                    json!({"data": value})
                },
            );
        }
    }
    Ok(result)
}

fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    match requested {
        Some("2025-11-25") => "2025-11-25",
        Some("2025-06-18") => "2025-06-18",
        Some("2025-03-26") => "2025-03-26",
        _ => "2024-11-05",
    }
}

fn read_agent_context(args: &Value) -> Result<String, String> {
    if let Some(pane_id) = args.get("pane_id").and_then(Value::as_str) {
        return bridge_request(
            "GET",
            &format!("/panes/{}/agent-context", encode_path_segment(pane_id)),
            None,
        );
    }
    let agent_type = required_string(args, "agent_type")?;
    let contexts = bridge_request("GET", "/agent-contexts", None)?;
    let value: Value = serde_json::from_str(&contexts)
        .map_err(|err| format!("bridge returned invalid agent contexts JSON: {err}"))?;
    let context = value
        .as_array()
        .and_then(|contexts| {
            contexts.iter().find(|context| {
                context.get("agent_type").and_then(Value::as_str) == Some(&agent_type)
            })
        })
        .ok_or_else(|| format!("unknown agent_type: {agent_type}"))?;
    serde_json::to_string_pretty(context).map_err(|err| format!("serialize agent context: {err}"))
}

fn bridge_request(method: &str, path: &str, body: Option<Value>) -> Result<String, String> {
    let endpoint = read_bridge_endpoint()?;
    let body_text = body
        .as_ref()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let read_timeout_secs = bridge_read_timeout_secs(method, path, body.as_ref());
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .map_err(|err| format!("bridge_down: {} ({err})", endpoint.base_url()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(read_timeout_secs)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let session_header = current_mcp_session_id()
        .map(|id| format!("X-Puppet-Master-Session: {id}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         {session_header}\
         Connection: close\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        endpoint.host,
        endpoint.port,
        body_text.as_bytes().len(),
        body_text
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("bridge write failed: {err}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|err| format!("bridge read failed: {err}"))?;
    parse_http_response(&response)
}

fn bridge_request_wait(
    id: &str,
    query: &str,
    body: Value,
    timeout_ms: u64,
    cancel: Option<&std::sync::mpsc::Receiver<()>>,
) -> Result<String, String> {
    let endpoint = read_bridge_endpoint()?;
    let body_text = body.to_string();
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .map_err(|err| format!("bridge_down: {} ({err})", endpoint.base_url()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let session_header = current_mcp_session_id()
        .map(|id| format!("X-Puppet-Master-Session: {id}\r\n"))
        .unwrap_or_default();
    let request = format!("POST /operations/{}/wait{} HTTP/1.1\r\nHost: {}:{}\r\n{}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", encode_path_segment(id), query, endpoint.host, endpoint.port, session_header, body_text.len(), body_text);
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("bridge write failed: {err}"))?;
    let deadline =
        std::time::Instant::now() + Duration::from_millis(timeout_ms.saturating_add(20_000));
    loop {
        if cancel.is_some_and(|rx| rx.try_recv().is_ok()) {
            return Err(r#"{"code":"REQUEST_CANCELLED","message":"wait request was cancelled","recoverable":true,"context":{}}"#.into());
        }
        let mut chunk = [0_u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if std::time::Instant::now() >= deadline {
                    return Err("bridge wait timed out".into());
                }
            }
            Err(err) => return Err(format!("bridge read failed: {err}")),
        }
    }
    // Fallback response path uses ordinary parsing; the bridge's wait endpoint still
    // returns a final snapshot if notifications were unavailable.
    Err("bridge closed operation wait without a response".into())
}

fn bridge_request_wait_agents(
    body: &Value,
    cancel: Option<&std::sync::mpsc::Receiver<()>>,
) -> Result<String, String> {
    let endpoint = read_bridge_endpoint()?;
    let body_text = body.to_string();
    let timeout_ms = body
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_AGENT_WAIT_MS);
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .map_err(|err| format!("bridge_down: {} ({err})", endpoint.base_url()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let session_header = current_mcp_session_id()
        .map(|id| format!("X-Puppet-Master-Session: {id}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST /agents/wait HTTP/1.1\r\nHost: {}:{}\r\n{}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        endpoint.host, endpoint.port, session_header, body_text.len(), body_text
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("bridge write failed: {err}"))?;
    let deadline =
        std::time::Instant::now() + Duration::from_millis(timeout_ms.saturating_add(20_000));
    let mut response = Vec::new();
    loop {
        if cancel.is_some_and(|rx| rx.try_recv().is_ok()) {
            return Err(r#"{"code":"REQUEST_CANCELLED","message":"agent wait request was cancelled","recoverable":true,"context":{}}"#.into());
        }
        let mut chunk = [0_u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => response.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if std::time::Instant::now() >= deadline {
                    return Err("agent wait exceeded its timeout grace period".into());
                }
            }
            Err(error) => return Err(format!("bridge read failed: {error}")),
        }
    }
    parse_http_response(&response)
}

fn bridge_read_timeout_secs(method: &str, path: &str, body: Option<&Value>) -> u64 {
    if method == "POST"
        && (path.contains("/panes/wait")
            || path.contains("/operations/") && path.ends_with("/wait")
            || path == "/agents/wait"
            || path == "/agents/run"
            || path == "/agents/followup")
    {
        if path == "/agents/run"
            && body
                .and_then(|value| value.get("background"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return 30;
        }
        let default_timeout_ms = if path == "/agents/run" || path == "/agents/followup" {
            DEFAULT_AGENT_WAIT_MS
        } else {
            DEFAULT_AGENT_WAIT_MS
        };
        let timeout_key = if path == "/agents/run" || path == "/agents/followup" {
            "wait_ms"
        } else {
            "timeout_ms"
        };
        let timeout_ms = body
            .and_then(|value| value.get(timeout_key))
            .and_then(Value::as_u64)
            .unwrap_or(default_timeout_ms);
        return (timeout_ms / 1000).saturating_add(15).min(330);
    }
    30
}

fn ensure_idempotency_key(args: &mut Value) -> String {
    if let Some(key) = args
        .get("idempotency_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return key.to_string();
    }
    let key = uuid::Uuid::new_v4().to_string();
    if let Some(object) = args.as_object_mut() {
        object.insert("idempotency_key".into(), Value::String(key.clone()));
    }
    key
}

fn lookup_operation_by_key(project: Option<&str>, key: &str) -> Result<Value, String> {
    let mut path = format!(
        "/operations/by-key?idempotency_key={}",
        encode_path_segment(key)
    );
    if let Some(project) = project.filter(|value| !value.is_empty()) {
        path.push_str(&format!(
            "&project_path={}",
            encode_path_segment(project)
        ));
    }
    bridge_request("GET", &path, None).and_then(|body| {
        serde_json::from_str(&body).map_err(|err| format!("invalid operation JSON: {err}"))
    })
}

fn lookup_steer_receipt(args: &Value) -> Result<Value, String> {
    let handle = args
        .get("handle")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "steer recovery requires handle".to_string())?;
    let key = args
        .get("idempotency_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "steer recovery requires idempotency_key".to_string())?;
    let mut path = format!(
        "/agents/steer-receipt?handle={}&idempotency_key={}",
        encode_path_segment(handle),
        encode_path_segment(key)
    );
    if let Some(project) = args
        .get("project_path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        path.push_str(&format!("&project_path={}", encode_path_segment(project)));
    }
    bridge_request("GET", &path, None).and_then(|body| {
        serde_json::from_str(&body).map_err(|err| format!("invalid steer receipt JSON: {err}"))
    })
}

pub(crate) fn operation_timeout_outcome(snapshot: &Value) -> &'static str {
    if snapshot
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .is_some_and(|code| code == "DISPATCH_FAILED")
    {
        return "not_dispatched";
    }
    match snapshot.get("status").and_then(Value::as_str) {
        Some("queued" | "starting" | "running" | "waiting_input") => "pending",
        Some("completed" | "failed" | "cancelled") => "completed_delivery_failed",
        _ => "not_dispatched",
    }
}

fn agent_handle_from_snapshot(snapshot: &Value) -> String {
    snapshot
        .get("agent_run_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| snapshot.get("operation_id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn turn_id_from_snapshot(snapshot: &Value) -> String {
    snapshot
        .get("turn_index")
        .and_then(Value::as_u64)
        .map(|index| format!("turn-{index}"))
        .unwrap_or_else(|| "turn-0".into())
}

fn wait_timeout_next_action(handle: &str) -> Value {
    json!({
        "tool": "wait_agents",
        "handles": [handle],
        "until": ["completed", "failed", "cancelled", "waiting_input"]
    })
}

fn format_operation_timeout_recovery(path: &str, args: &Value, snapshot: &Value) -> String {
    let outcome = operation_timeout_outcome(snapshot);
    let handle = agent_handle_from_snapshot(snapshot);
    let operation_id = snapshot
        .get("operation_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let turn_id = turn_id_from_snapshot(snapshot);
    let project_path = snapshot
        .get("project_path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let watch_command = puppet_master_app_lib::watch_command::format_watch_command(
        operation_id,
        project_path,
    );
    let mut payload = json!({
        "code": "WAIT_TIMEOUT",
        "message": "the worker may still be running; use next_action instead of retrying with a new idempotency key",
        "outcome": outcome,
        "recoverable": true,
        "handle": handle,
        "operation_id": operation_id,
        "turn_id": turn_id,
        "watch_command": watch_command,
        "idempotency_key": args.get("idempotency_key").cloned().unwrap_or(Value::Null),
        "next_action": wait_timeout_next_action(&handle),
        "context": {
            "path": path,
            "suggestion": "wait_agents"
        }
    });
    if outcome == "completed_delivery_failed" {
        if let Some(result) = snapshot.get("result") {
            payload["result"] = result.clone();
        }
    }
    payload.to_string()
}

fn wait_timeout_recovery(path: &str, args: &Value, error: String) -> String {
    if !is_bridge_timeout(&error) {
        return error;
    }
    if let Some(key) = args
        .get("idempotency_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let project = args.get("project_path").and_then(Value::as_str);
        match lookup_operation_by_key(project, key) {
            Ok(snapshot) => return format_operation_timeout_recovery(path, args, &snapshot),
            Err(lookup_error) if lookup_error.contains("bridge_down") => {
                return json!({
                    "code": "WAIT_TIMEOUT",
                    "outcome": "unknown",
                    "message": "bridge unreachable; retry with the same idempotency_key",
                    "recoverable": true,
                    "idempotency_key": key,
                    "next_action": wait_timeout_next_action(
                        args.get("handle")
                            .or_else(|| args.get("worker_id"))
                            .or_else(|| args.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    ),
                    "context": { "path": path, "lookup_error": lookup_error }
                })
                .to_string();
            }
            Err(_) => {
                return json!({
                    "code": "WAIT_TIMEOUT",
                    "outcome": "not_dispatched",
                    "message": "no operation was recorded for this idempotency_key",
                    "recoverable": true,
                    "idempotency_key": key,
                    "context": { "path": path }
                })
                .to_string();
            }
        }
    }
    let identity = ["handle", "worker_id", "name"]
        .iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .filter(|value| !value.is_empty());
    let Some(handle) = identity else {
        return error;
    };
    json!({
        "code": "WAIT_TIMEOUT",
        "outcome": "unknown",
        "message": format!(
            "transport timed out before identity was resolved; call wait_agents with handle {handle} or retry with idempotency_key"
        ),
        "recoverable": true,
        "handle": handle,
        "next_action": wait_timeout_next_action(handle),
        "context": {
            "handle": handle,
            "path": path,
            "suggestion": "wait_agents"
        }
    })
    .to_string()
}

fn steer_timeout_recovery(args: &Value, error: String) -> String {
    if !is_bridge_timeout(&error) {
        return error;
    }
    match lookup_steer_receipt(args) {
        Ok(receipt) => {
            let handle = receipt
                .get("handle")
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({
                "code": "WAIT_TIMEOUT",
                "outcome": "completed_delivery_failed",
                "message": "steer was recorded before the HTTP response returned",
                "recoverable": true,
                "handle": handle,
                "receipt": receipt,
                "next_action": wait_timeout_next_action(handle),
            })
            .to_string()
        }
        Err(lookup_error) if lookup_error.contains("bridge_down") => json!({
            "code": "WAIT_TIMEOUT",
            "outcome": "unknown",
            "message": "bridge unreachable; retry send_message with the same idempotency_key",
            "recoverable": true,
            "idempotency_key": args.get("idempotency_key").cloned().unwrap_or(Value::Null),
            "context": { "lookup_error": lookup_error }
        })
        .to_string(),
        Err(_) => json!({
            "code": "WAIT_TIMEOUT",
            "outcome": "not_dispatched",
            "message": "steer was not recorded before the transport timed out",
            "recoverable": true,
            "idempotency_key": args.get("idempotency_key").cloned().unwrap_or(Value::Null),
        })
        .to_string(),
    }
}

fn is_bridge_timeout(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("10060")
        || lower.contains("timed out")
        || lower.contains("timeout grace")
        || lower.contains("timedout")
}

fn parse_http_response(response: &[u8]) -> Result<String, String> {
    let marker = b"\r\n\r\n";
    let header_end = response
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| "bridge returned invalid HTTP response".to_string())?;
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| "bridge returned invalid HTTP status".to_string())?;
    let body = String::from_utf8_lossy(&response[header_end + marker.len()..]).to_string();
    if status >= 400 {
        return Err(legacy_http_error(status, &body).to_string());
    }
    Ok(body)
}

fn ensure_error_message(mut value: Value) -> Value {
    if value.get("message").and_then(Value::as_str).is_none() {
        if let Some(error) = value.get("error").cloned() {
            if let Some(object) = value.as_object_mut() {
                object.insert("message".into(), error);
            }
        }
    }
    value
}

fn legacy_http_error(status: u16, body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if value.get("code").is_some() && value.get("recoverable").is_some() {
            return ensure_error_message(value);
        }
    }
    let (code, recoverable, retry_after_ms) = match status {
        400 => ("INVALID_ARGUMENT", false, Value::Null),
        401 | 403 => ("AUTHORIZATION_DENIED", false, Value::Null),
        404 => ("NOT_FOUND", false, Value::Null),
        409 => ("CONFLICT", true, json!(1000)),
        429 => ("RATE_LIMITED", true, json!(1000)),
        500..=u16::MAX => ("BRIDGE_UNAVAILABLE", true, Value::Null),
        _ => ("BRIDGE_ERROR", true, Value::Null),
    };
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| body.to_owned());
    json!({"code":code,"message":message,"recoverable":recoverable,"retry_after_ms":retry_after_ms,"context":{"http_status":status}})
}

fn read_bridge_endpoint() -> Result<BridgeEndpoint, String> {
    let candidates = bridge_port_candidates();
    read_bridge_endpoint_from_candidates(&candidates)
}

fn read_bridge_endpoint_from_candidates(candidates: &[PathBuf]) -> Result<BridgeEndpoint, String> {
    let mut last_error = String::new();
    for candidate in candidates {
        match fs::read_to_string(candidate) {
            Ok(raw) => return parse_bridge_endpoint(&raw),
            Err(err) => last_error = format!("{}: {err}", candidate.display()),
        }
    }
    Err(format!(
        "bridge_down: Puppet Master bridge port file not found (tried: {}). Last error: {last_error}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn bridge_port_candidates() -> Vec<PathBuf> {
    if let Ok(path) = env::var(BRIDGE_PORT_FILE_ENV) {
        return vec![PathBuf::from(path)];
    }
    vec![
        PathBuf::from(DEFAULT_BRIDGE_PORT_FILE),
        default_app_data_bridge_port_file(),
    ]
}

fn default_app_data_bridge_port_file() -> PathBuf {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    } else if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    } else {
        home.join(".local")
            .join("share")
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    }
}

fn parse_bridge_endpoint(raw: &str) -> Result<BridgeEndpoint, String> {
    let trimmed = raw.trim();
    let (host, port_text) = trimmed
        .split_once(':')
        .map(|(host, port)| (if host.is_empty() { "127.0.0.1" } else { host }, port))
        .unwrap_or(("127.0.0.1", trimmed));
    let port = port_text
        .parse::<u16>()
        .map_err(|err| format!("invalid bridge port file value {trimmed:?}: {err}"))?;
    Ok(BridgeEndpoint {
        host: host.to_string(),
        port,
    })
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing required string argument: {key}"))
}

fn optional_number(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn assert_worker_pane(pane_id: &str) -> Result<String, String> {
    if pane_id.starts_with("puppet-master-orchestrator-") {
        return Err(format!("refusing to target orchestrator pane: {pane_id}"));
    }
    Ok(pane_id.to_string())
}

fn encode_path_segment(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
