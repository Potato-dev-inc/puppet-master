//! Minimal HTTP plumbing: connection parsing, query-string helpers and JSON responses.

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use tauri::AppHandle;

use super::errors::operation_http_error;
use super::router::route;
use super::sse::handle_sse;
use super::tool_names::bridge_tool_name;
use crate::events::SystemEvent;
use crate::mobile_pairing;
use crate::pty::PaneRegistry;

pub(super) fn handle_connection(
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
        method,
        &segments,
        query,
        body,
        &registry,
        &app,
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
    let (Ok((status, value)) | Err((status, value))) = result;
    write_json(&mut stream, status, &value)
}

pub(super) fn parse_json<T: for<'de> Deserialize<'de>>(
    body: &[u8],
) -> Result<T, (u16, serde_json::Value)> {
    serde_json::from_slice(body)
        .map_err(|err| (400, json!({ "error": format!("invalid json: {err}") })))
}

pub(super) fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(super) fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

pub(super) fn requested_project_path(body_path: Option<&str>, query: &str) -> Option<String> {
    body_path
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
        .or_else(|| query_param(query, "project_path"))
}

pub(super) fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if percent_decode(key).as_deref() == Some(name) {
            percent_decode(value)
        } else {
            None
        }
    })
}

pub(super) fn percent_decode(value: &str) -> Option<String> {
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

pub(super) fn write_json(
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
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| format!("write response: {err}"))
}

pub(super) fn request_session(headers: &str) -> Option<&str> {
    let session = headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.eq_ignore_ascii_case("x-puppet-master-session") && !value.trim().is_empty())
            .then_some(value.trim())
    })?;
    // The MCP process also names itself so a dead one can be told apart from a live one.
    let pid = headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case("x-puppet-master-pid")
            .then(|| value.trim().parse::<u32>().ok())
            .flatten()
    });
    if let Some(pid) = pid {
        crate::mcp_sessions::note_session_pid(session, pid);
    }
    Some(session)
}
