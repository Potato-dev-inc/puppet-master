//! Long-poll waits that stream `notifications/progress` while a tool call is blocked.

use super::*;

pub(crate) fn call_tool_with_progress(
    params: Value,
    cancel: Option<std::sync::mpsc::Receiver<()>>,
    token: Option<Value>,
    notifications: Option<std::sync::mpsc::Sender<String>>,
) -> Result<Value, String> {
    if let Some(name) = params.get("name").and_then(Value::as_str) {
        ensure_tool_mode(name)?;
    }
    let tool_name = params.get("name").and_then(Value::as_str);
    if tool_name == Some("ask_user") {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        return ask_user::run(&args, cancel.as_ref(), token.as_ref(), notifications.as_ref())
            .map(ask_user::wrap);
    }
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

pub(crate) fn call_wait_agents_with_progress(
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
        body["timeout_ms"] = json!(remaining.clamp(1, 4_000));
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
        let reason = value
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // A per-chunk "timeout" only means this 4s slice elapsed; the overall deadline decides.
        let terminal = matches!(reason, "matched_state" | "terminal");
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
            // The bridge only saw the last <=4s slice; report the caller's whole wait instead.
            let mut value = value;
            if let Some(object) = value.as_object_mut() {
                object.insert("requested_wait_ms".into(), json!(timeout_ms));
                object.insert(
                    "waited_ms".into(),
                    json!(timeout_ms.saturating_sub(
                        deadline
                            .saturating_duration_since(std::time::Instant::now())
                            .as_millis() as u64
                    )),
                );
            }
            return Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap_or(response)}],"structuredContent":value}),
            );
        }
    }
}
