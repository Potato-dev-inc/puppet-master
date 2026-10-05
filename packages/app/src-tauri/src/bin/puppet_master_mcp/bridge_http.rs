//! Minimal HTTP client for the loopback bridge, plus bridge error normalisation.

use super::*;

/// Compares this MCP binary's tool catalog with the running app's (from a `/health` body). A
/// mismatch means the client was given an older or newer schema than the bridge enforces, which
/// shows up as confusing argument rejections. The call is still served; this only explains.
pub(crate) fn schema_drift_from_health(health: &Value) -> Option<Value> {
    let bridge = health.get("catalog_version").and_then(Value::as_str)?;
    let shim = tool_registry::catalog_version();
    if bridge == shim {
        return None;
    }
    Some(json!({
        "code": "SCHEMA_DRIFT",
        "shim_catalog_version": shim,
        "bridge_catalog_version": bridge,
        "bridge_build_id": health.get("build_id"),
        "hint": format!("this MCP binary's tool schema ({shim}) differs from the running app's ({bridge}); restart the app or reconnect/reinstall the MCP binary (/mcp) so both match."),
    }))
}

pub(crate) fn detect_schema_drift() -> Option<Value> {
    let body = bridge_request("GET", "/health", None).ok()?;
    schema_drift_from_health(&serde_json::from_str(&body).ok()?)
}

pub(crate) fn bridge_request(
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<String, String> {
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
        .map(|id| {
            format!(
                "X-Puppet-Master-Session: {id}\r\nX-Puppet-Master-Pid: {}\r\n",
                std::process::id()
            )
        })
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
        body_text.len(),
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

pub(crate) fn bridge_request_wait(
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
        .map(|id| {
            format!(
                "X-Puppet-Master-Session: {id}\r\nX-Puppet-Master-Pid: {}\r\n",
                std::process::id()
            )
        })
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

pub(crate) fn bridge_request_wait_agents(
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
        .map(|id| {
            format!(
                "X-Puppet-Master-Session: {id}\r\nX-Puppet-Master-Pid: {}\r\n",
                std::process::id()
            )
        })
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

pub(crate) fn bridge_read_timeout_secs(method: &str, path: &str, body: Option<&Value>) -> u64 {
    if method == "POST"
        && (path.contains("/panes/wait")
            || path.contains("/operations/") && path.ends_with("/wait")
            || path == "/agents/wait"
            || path == "/agents/run"
            || path == "/agents/quick"
            || path == "/agents/followup")
    {
        if path == "/agents/quick" {
            // quick_ask waits wait_ms (default 90s, clamped to 120s) like the bridge does.
            let wait_ms = body
                .and_then(|value| value.get("wait_ms"))
                .and_then(Value::as_u64)
                .unwrap_or(90_000)
                .min(120_000);
            return (wait_ms / 1000).saturating_add(15);
        }
        if path == "/agents/run"
            && body
                .and_then(|value| value.get("background"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return 30;
        }
        let default_timeout_ms = DEFAULT_AGENT_WAIT_MS;
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

pub(crate) fn parse_http_response(response: &[u8]) -> Result<String, String> {
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

pub(crate) fn ensure_error_message(mut value: Value) -> Value {
    if value.get("message").and_then(Value::as_str).is_none() {
        if let Some(error) = value.get("error").cloned() {
            if let Some(object) = value.as_object_mut() {
                object.insert("message".into(), error);
            }
        }
    }
    value
}

pub(crate) fn legacy_http_error(status: u16, body: &str) -> Value {
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
