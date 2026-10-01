use crate::*;
pub(crate) fn run() {
    if env::args().any(|arg| arg == "--version") {
        println!("{SERVER_NAME} {SERVER_VERSION}");
        return;
    }

    log("starting");
    let session_id = uuid::Uuid::new_v4().to_string();
    let launch_args: Vec<String> = env::args().skip(1).collect();
    match initial_mode_from(&launch_args, env::var(MODE_ENV).ok()) {
        Ok(Some(mode)) => match mcp_sessions::set_mode(&session_id, mode.as_str()) {
            Ok(_) => {
                let _ = INITIAL_MODE.set(mode);
                log(format!("starting in {} mode", mode.as_str()));
            }
            Err(err) => log(format!("ignoring starting mode: {}", err.message)),
        },
        Ok(None) => {}
        Err(message) => log(format!("ignoring starting mode: {message}")),
    }
    let stdin = io::stdin();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let writer = std::thread::spawn(move || {
        let mut stdout = io::stdout();
        while let Ok(response) = rx.recv() {
            if writeln!(stdout, "{response}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let (slots_tx, slots_rx) = std::sync::mpsc::sync_channel::<()>(16);
    for _ in 0..16 {
        let _ = slots_tx.send(());
    }
    let active = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
        String,
        std::sync::mpsc::Sender<()>,
    >::new()));
    let mut workers = Vec::new();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                log(format!("stdin read failed: {err}"));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                let _ = tx.send(
                    json_rpc_error(Value::Null, -32700, "invalid JSON-RPC request".into())
                        .to_string(),
                );
                continue;
            }
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if method == "notifications/cancelled" {
            if let Some(id) = request.pointer("/params/requestId").map(json_id_key) {
                if let Some(cancel) = active.lock().ok().and_then(|map| map.get(&id).cloned()) {
                    let _ = cancel.send(());
                }
            }
            continue;
        }
        if method == "notifications/initialized" || request.get("id").is_none() {
            continue;
        }
        let id = request.get("id").map(json_id_key).unwrap_or_default();
        if method == "tools/call" {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(name, "delegate_work" | "cancel_operation") {
                // Mutation semantics require caller-controlled retries; never cancel implicitly.
            }
        }
        if slots_rx.try_recv().is_err() {
            let _ = tx.send(
                json_rpc_error(
                    request.get("id").cloned().unwrap_or(Value::Null),
                    -32000,
                    "server busy: request concurrency limit reached".into(),
                )
                .to_string(),
            );
            continue;
        }
        let tx = tx.clone();
        let release = slots_tx.clone();
        let active_ref = active.clone();
        let session_id = session_id.clone();
        let cancel_rx = if id.is_empty() {
            None
        } else {
            let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
            if let Ok(mut map) = active.lock() {
                map.insert(id.clone(), cancel_tx);
            }
            Some(cancel_rx)
        };
        let is_tool_call = method == "tools/call";
        let worker = std::thread::spawn(move || {
            set_mcp_session_id(session_id);
            if is_tool_call {
                ensure_initial_mode_synced();
            }
            if let Some(response) =
                handle_json_rpc_line_cancellable(&line, cancel_rx, Some(tx.clone()))
            {
                let _ = tx.send(response);
            }
            if let Ok(mut map) = active_ref.lock() {
                map.remove(&id);
            }
            let _ = release.send(());
        });
        workers.push(worker);
        workers.retain(|worker| !worker.is_finished());
    }
    drop(tx);
    for worker in workers {
        let _ = worker.join();
    }
    let _ = writer.join();
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

fn json_id_key(id: &Value) -> String {
    id.to_string()
}

fn handle_json_rpc_line(line: &str) -> Option<String> {
    handle_json_rpc_line_cancellable(line, None, None)
}

fn handle_json_rpc_line_cancellable(
    line: &str,
    cancel_rx: Option<std::sync::mpsc::Receiver<()>>,
    notifications: Option<std::sync::mpsc::Sender<String>>,
) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            return Some(
                json_rpc_error(
                    Value::Null,
                    -32700,
                    format!("invalid JSON-RPC request: {err}"),
                )
                .to_string(),
            );
        }
    };
    if request.get("id").is_none() {
        return None;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": negotiate_protocol_version(request.pointer("/params/protocolVersion").and_then(Value::as_str)),
            "capabilities": { "tools": { "listChanged": true }, "resources": {}, "prompts": {} },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
                "catalog_version": tool_registry::catalog_version(),
            },
            "instructions": tool_registry::mcp_instructions()
        })),
        "tools/list" => Ok(json!({ "tools": mcp_tools() })),
        "resources/list" => Ok(json!({ "resources": tool_registry::resources() })),
        "prompts/list" => Ok(json!({ "prompts": tool_registry::prompts() })),
        "tools/call" => {
            let token = request.pointer("/params/_meta/progressToken").cloned();
            match call_tool_with_progress(
                request.get("params").cloned().unwrap_or_else(|| json!({})),
                cancel_rx,
                token,
                notifications.clone(),
            ) {
                Ok(value) => Ok(value),
                Err(message) => {
                    let detail = tool_error_detail(&message);
                    Ok(
                        json!({"content":[{"type":"text","text":detail.get("message").and_then(Value::as_str).unwrap_or("tool call failed")}],"isError":true,"structuredContent":detail}),
                    )
                }
            }
        }
        _ => Err(format!("unknown method: {method}")),
    };

    let mut result = result;
    if method == "tools/call"
        && request.pointer("/params/name").and_then(Value::as_str) == Some("set_mode")
    {
        if let Ok(value) = result.as_mut() {
            if value.get("isError") != Some(&Value::Bool(true)) {
                let sent = notifications.as_ref().is_some_and(|sender| {
                    sender
                        .send(
                            json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})
                                .to_string(),
                        )
                        .is_ok()
                });
                annotate_set_mode(value, sent);
            }
        }
    }

    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
        Err(message) => json_rpc_error(id, -32603, message).to_string(),
    })
}

/// Tell the caller whether `notifications/tools/list_changed` was sent, and that hosts which
/// read `tools/list` only once at startup will keep showing the old catalog regardless.
fn annotate_set_mode(result: &mut Value, notification_sent: bool) {
    let note = if notification_sent {
        "Sent notifications/tools/list_changed. Hosts that read tools/list only once at startup ignore it and keep the old tool list; if tool_count differs from what you can see, restart the MCP server with --mode <mode> or reconnect."
    } else {
        "This transport could not send notifications/tools/list_changed. The host will keep the old tool list until it re-reads tools/list; restart the MCP server with --mode <mode> or reconnect."
    };
    if let Some(structured) = result
        .get_mut("structuredContent")
        .and_then(Value::as_object_mut)
    {
        structured.insert(
            "tools_list_changed_sent".into(),
            Value::Bool(notification_sent),
        );
        structured.insert("note".into(), Value::String(note.into()));
    }
    if let Some(text) = result
        .pointer_mut("/content/0/text")
        .filter(|text| text.is_string())
    {
        let body = text.as_str().unwrap_or_default().to_owned();
        *text = Value::String(format!(
            "{body}

Note: {note}"
        ));
    }
}

fn tool_error_detail(message: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(message) {
        if value.get("code").is_some() {
            return ensure_error_message(value);
        }
    }
    let invalid = message.starts_with("missing required")
        || message.starts_with("refusing to target")
        || message.starts_with("unknown tool:");
    json!({"code":if invalid {"INVALID_ARGUMENT"} else {"bridge_error"},"message":message,"recoverable":!invalid,"context":{}})
}

fn json_rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn mcp_tools() -> Vec<Value> {
    let tools = current_mcp_session_id()
        .map(|id| mcp_sessions::tools_for_session(&id))
        .unwrap_or_else(|| tool_registry::tools());
    tools
        .into_iter()
        .filter(|tool| tool.visibility.external_mcp)
        .map(|tool| {
            let mut value = json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            });
            if let Some(schema) = tool.output_schema {
                value["outputSchema"] = schema;
            }
            value
        })
        .collect()
}
