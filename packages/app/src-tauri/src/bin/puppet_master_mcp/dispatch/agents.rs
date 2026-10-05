//! Agent-run, worker and ownership tools: maps each tool call to its bridge request.

use super::super::*;
use std::sync::mpsc::Receiver;

/// Runs `name` when this family owns it; `None` lets the next family try.
pub(super) fn dispatch(
    name: &str,
    args: &Value,
    cancel_rx: Option<&Receiver<()>>,
) -> Option<Result<String, String>> {
    let mut known = true;
    let result = run(name, args, cancel_rx, &mut known);
    known.then_some(result)
}

fn run(
    name: &str,
    args: &Value,
    cancel_rx: Option<&Receiver<()>>,
    known: &mut bool,
) -> Result<String, String> {
    match name {
        "set_mode" => {
            let mode = required_string(args, "mode")?;
            let response = bridge_request("POST", "/mcp/mode", Some(json!({"mode":mode})))?;
            if let Some(session_id) = current_mcp_session_id() {
                mcp_sessions::set_mode(&session_id, &mode)
                    .map_err(|error| serde_json::to_string(&error).unwrap_or(error.message))?;
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
        "quick_ask" => {
            let mut body = args.clone();
            ensure_idempotency_key(&mut body);
            match bridge_request("POST", "/agents/quick", Some(body.clone())) {
                Ok(response) => Ok(response),
                Err(error) => Err(wait_timeout_recovery("/agents/quick", &body, error)),
            }
        }
        "wait_agents" => bridge_request_wait_agents(args, cancel_rx),
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
        "answer_prompt" => bridge_request("POST", "/agents/answer", Some(args.clone())),
        "interrupt_agent" | "cancel_agent" => {
            bridge_request("POST", "/agents/cancel", Some(args.clone()))
        }
        "inspect_agent" => {
            let handle = required_string(args, "handle")?;
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
        "close_agent" => bridge_request("POST", "/agents/close", Some(args.clone())),
        "list_agents" | "list_workers" => {
            let mut query = Vec::new();
            if let Some(path) = args.get("project_path").and_then(Value::as_str) {
                query.push(format!("project_path={}", encode_path_segment(path)));
            }
            for key in ["mine_only", "active_only"] {
                if args.get(key).and_then(Value::as_bool) == Some(true) {
                    query.push(format!("{key}=true"));
                }
            }
            if let Some(ms) = args.get("ended_within_ms").and_then(Value::as_u64) {
                query.push(format!("ended_within_ms={ms}"));
            }
            let suffix = if query.is_empty() {
                String::new()
            } else {
                format!("?{}", query.join("&"))
            };
            bridge_request("GET", &format!("/agents{suffix}"), None)
        }
        "agent_transcript" => {
            let handle = required_string(args, "handle")?;
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
        "take_over" => bridge_request("POST", "/agents/take-over", Some(args.clone())),
        "shell_exec" => bridge_request("POST", "/shell/exec", Some(args.clone())),
        "release" => bridge_request("POST", "/agents/release", Some(args.clone())),
        "session_identity" => bridge_request("GET", "/mcp/session", None),
        "attach_agents" => bridge_request("POST", "/agents/attach", Some(args.clone())),
        "release_lease" => bridge_request("POST", "/agents/release-lease", Some(args.clone())),
        "transfer_agent" => bridge_request("POST", "/agents/transfer", Some(args.clone())),
        _ => {
            *known = false;
            Ok(String::new())
        }
    }
}
