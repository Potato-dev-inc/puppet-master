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
    registry_kill_pane, registry_read_buffer, registry_resize, registry_set_project_path,
    registry_spawn_pane, registry_write_input, PaneRegistry, SpawnPaneArgs,
};

const HOST: &str = "127.0.0.1";
const PORT_MIN: u16 = 17321;
const PORT_MAX: u16 = 17399;

#[derive(Debug, Deserialize)]
struct WriteInputBody {
    text: String,
    #[serde(default = "default_true")]
    append_newline: bool,
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
) -> Result<BridgeHandle, String> {
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

    if method == "OPTIONS" {
        return write_json(&mut stream, 204, &json!({}));
    }

    let (path, query) = split_target(target);
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let result = route(method, &segments, query, body, registry, app);
    match result {
        Ok((status, value)) => write_json(&mut stream, status, &value),
        Err((status, value)) => write_json(&mut stream, status, &value),
    }
}

fn route(
    method: &str,
    segments: &[&str],
    query: &str,
    body: &[u8],
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
) -> Result<(u16, serde_json::Value), (u16, serde_json::Value)> {
    if segments == ["health"] && method == "GET" {
        return Ok((
            200,
            serde_json::to_value(Health {
                ok: true,
                version: "0.1.0",
            })
            .unwrap(),
        ));
    }

    if segments == ["events"] && method == "GET" {
        return Ok((
            200,
            json!({ "ok": true, "note": "SSE is not implemented by the embedded bridge yet" }),
        ));
    }

    if segments == ["agent-contexts"] && method == "GET" {
        return Ok((200, json!([])));
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
                emit_panes_changed(&app);
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
            emit_panes_changed(&app);
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
        if tail == Some("input") && method == "POST" {
            let req: WriteInputBody = parse_json(body)?;
            registry_write_input(&registry, pane_id, &req.text, req.append_newline)
                .map_err(|err| (404, json!({ "error": err })))?;
            return Ok((200, json!({ "ok": true })));
        }
        if tail == Some("resize") && method == "POST" {
            let req: ResizeBody = parse_json(body)?;
            registry_resize(&registry, pane_id, req.cols, req.rows)
                .map_err(|err| (404, json!({ "error": err })))?;
            emit_panes_changed(&app);
            return Ok((200, json!({ "ok": true })));
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

    Err((404, json!({ "error": "not found" })))
}

fn emit_panes_changed(app: &AppHandle) {
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
         Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
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
