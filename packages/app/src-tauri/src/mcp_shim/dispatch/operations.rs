//! Operation and delegation tools: maps each tool call to its bridge request.

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
        "delegate_task" => bridge_request("POST", "/delegate-task", Some(args.clone())),
        "delegate_work" => {
            let mut body = args.clone();
            (|| {
                required_string(args, "project_path")?;
                required_string(args, "task")?;
                required_string(args, "idempotency_key")?;
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
        "get_operation" => required_string(args, "operation_id").and_then(|id| {
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
            let id = required_string(args, "operation_id")?;
            let timeout = optional_number(args, "timeout_ms").unwrap_or(120_000);
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
            bridge_request_wait(&id, &query, body, timeout, cancel_rx)
        })(),
        "cancel_operation" => required_string(args, "operation_id").and_then(|id| {
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
        _ => {
            *known = false;
            Ok(String::new())
        }
    }
}
