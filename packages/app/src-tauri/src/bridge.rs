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

use crate::pty::{
    registry_kill_pane, registry_read_buffer, registry_read_raw_buffer,
    registry_set_project_path, registry_spawn_pane, registry_write_input, PaneRegistry,
    SpawnPaneArgs,
};
use crate::settings_store;
use crate::mobile_pairing::{self, PairRequestBody};

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

#[derive(Debug, Deserialize)]
struct ResizeBody {
    cols: u16,
    rows: u16,
}


#[derive(Debug, Deserialize)]
struct ProjectPathBody {
    path: String,
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
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();

    if segments == ["events"] && method == "GET" {
        if let Err((status, value)) =
            mobile_pairing::authorize_bridge_request(&headers_raw, peer_loopback, &segments, method)
        {
            return write_json(&mut stream, status, &value);
        }
        handle_sse(&mut stream, registry);
        return Ok(());
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
    match result {
        Ok((0, _)) => Ok(()), // SSE handled inline — stream already consumed
        Ok((status, value)) => write_json(&mut stream, status, &value),
        Err((status, value)) => write_json(&mut stream, status, &value),
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
                version: "0.1.1",
            })
            .unwrap(),
        ));
    }

    if segments == ["pair"] && method == "POST" {
        let req: PairRequestBody = parse_json(body)?;
        let store = mobile_pairing::pairing_store().ok_or_else(|| {
            (
                503,
                serde_json::json!({ "error": "pairing_unavailable" }),
            )
        })?;
        let response = {
            let mut guard = store.lock();
            guard.pair_device(req).map_err(|err| (400, json!({ "error": err })))?
        };
        return Ok((200, serde_json::to_value(response).unwrap()));
    }

    if segments.len() == 3 && segments[0] == "pair" && segments[1] == "session" && method == "GET" {
        let code = segments[2];
        let store = mobile_pairing::pairing_store().ok_or_else(|| {
            (
                503,
                serde_json::json!({ "error": "pairing_unavailable" }),
            )
        })?;
        let info = {
            let guard = store.lock();
            guard
                .lookup_pairing_session(code)
                .map_err(|err| (404, json!({ "error": err })))?
        };
        return Ok((200, serde_json::to_value(info).unwrap()));
    }

    if segments == ["events"] && method == "GET" {
        handle_sse(stream, registry);
        return Ok((0, serde_json::Value::Null));
    }

    if segments == ["agent-contexts"] && method == "GET" {
        return Ok((200, json!([])));
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
        registry_set_project_path(&registry, req.path);
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
        if tail == Some("resize") && method == "POST" {
            // Remote bridge clients must not resize the shared PTY — only desktop does.
            let _ = parse_json::<ResizeBody>(body)?;
            return Ok((200, json!({ "ok": true, "ignored": true })));
        }
        if tail == Some("model") && method == "GET" {
            return Ok((
                200,
                json!({
                    "pane_id": pane_id,
                    "detected_model": null,
                    "source": "unknown",
                    "confidence": "low",
                    "notes": ["Model detection is handled by the MCP fallback using read_terminal_buffer."]
                }),
            ));
        }
        if tail == Some("agent-context") && method == "GET" {
            let panes = registry.lock().list();
            let pane = panes.iter().find(|pane| pane.id == pane_id);
            if let Some(pane) = pane {
                return Ok((200, json!({ "pane": pane })));
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

    Err((
        404,
        json!({
            "error": "not found",
            "hint": "Unknown bridge route. Mobile UI is served by Vite on port 1420/4173; API lives under /health, /events, /panes, /orchestrator/message.",
        }),
    ))
}

fn handle_sse(stream: &mut TcpStream, registry: Arc<Mutex<PaneRegistry>>) {
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
