//! Idempotency keys and WAIT_TIMEOUT recovery payloads for retried runs and steers.

use super::*;

pub(crate) fn ensure_idempotency_key(args: &mut Value) -> String {
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

pub(crate) fn lookup_operation_by_key(project: Option<&str>, key: &str) -> Result<Value, String> {
    let mut path = format!(
        "/operations/by-key?idempotency_key={}",
        encode_path_segment(key)
    );
    if let Some(project) = project.filter(|value| !value.is_empty()) {
        path.push_str(&format!("&project_path={}", encode_path_segment(project)));
    }
    bridge_request("GET", &path, None).and_then(|body| {
        serde_json::from_str(&body).map_err(|err| format!("invalid operation JSON: {err}"))
    })
}

pub(crate) fn lookup_steer_receipt(args: &Value) -> Result<Value, String> {
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

pub(crate) fn agent_handle_from_snapshot(snapshot: &Value) -> String {
    snapshot
        .get("agent_run_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| snapshot.get("operation_id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn turn_id_from_snapshot(snapshot: &Value) -> String {
    snapshot
        .get("turn_index")
        .and_then(Value::as_u64)
        .map(|index| format!("turn-{index}"))
        .unwrap_or_else(|| "turn-0".into())
}

pub(crate) fn wait_timeout_next_action(handle: &str) -> Value {
    json!({
        "tool": "wait_agents",
        "handles": [handle],
        "until": ["completed", "failed", "cancelled", "waiting_input"]
    })
}

pub(crate) fn format_operation_timeout_recovery(
    path: &str,
    args: &Value,
    snapshot: &Value,
) -> String {
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
    let shell = args
        .get("shell")
        .and_then(Value::as_str)
        .and_then(puppet_master_app_lib::watch_command::ShellHint::parse)
        .unwrap_or_else(puppet_master_app_lib::watch_command::ShellHint::from_env);
    // Prefer the command the app already put on the snapshot (it knows the stable CLI copy);
    // this shim process cannot resolve the bundled watcher itself.
    let watch_command = match snapshot.get("watch_command").and_then(Value::as_str) {
        Some(existing) => puppet_master_app_lib::watch_command::reformat_for_shell(
            existing,
            operation_id,
            project_path,
            shell,
        ),
        None => puppet_master_app_lib::watch_command::format_watch_command_for(
            operation_id,
            project_path,
            shell,
        ),
    };
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

pub(crate) fn wait_timeout_recovery(path: &str, args: &Value, error: String) -> String {
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

pub(crate) fn steer_timeout_recovery(args: &Value, error: String) -> String {
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

pub(crate) fn is_bridge_timeout(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("10060")
        || lower.contains("timed out")
        || lower.contains("timeout grace")
        || lower.contains("timedout")
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
