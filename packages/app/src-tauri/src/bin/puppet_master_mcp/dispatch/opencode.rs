//! Orchestrator state, OpenCode and wait/event tools: maps each tool call to its bridge request.

use super::super::*;
use std::sync::mpsc::Receiver;

/// Runs `name` when this family owns it; `None` lets the next family try.
pub(super) fn dispatch(
    name: &str,
    args: &Value,
    _cancel_rx: Option<&Receiver<()>>,
) -> Option<Result<String, String>> {
    let mut known = true;
    let result = run(name, args, &mut known);
    known.then_some(result)
}

fn run(name: &str, args: &Value, known: &mut bool) -> Result<String, String> {
    match name {
        "read_orchestrator_state" => bridge_request("GET", "/orchestrator/state", None),
        "update_orchestrator_state" => Ok(bridge_request(
            "PATCH",
            "/orchestrator/state",
            Some(args.clone()),
        )?),
        "read_opencode_key_status" => bridge_request("GET", "/opencode/keys/status", None),
        "rotate_opencode_key" => {
            bridge_request("POST", "/opencode/keys/rotate", Some(args.clone()))
        }
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
            let pane_id = required_string(args, "pane_id")?;
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
        "wait_for_panes" => bridge_request("POST", "/panes/wait", Some(args.clone())),
        "wait_for_model" => bridge_request("POST", "/panes/wait/model", Some(args.clone())),
        "wait_for_task" => bridge_request("POST", "/panes/wait/task", Some(args.clone())),
        "wait_for_worker" => bridge_request("POST", "/panes/wait/worker", Some(args.clone())),
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
            let pane_id = required_string(args, "pane_id")?;
            let request_id = required_string(args, "request_id")?;
            let reply = required_string(args, "reply")?;
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
            let pane_id = required_string(args, "pane_id")?;
            Ok(bridge_request(
                "POST",
                &format!(
                    "/panes/{}/opencode/question/reply",
                    encode_path_segment(&pane_id)
                ),
                Some(args.clone()),
            )?)
        }
        _ => {
            *known = false;
            Ok(String::new())
        }
    }
}
