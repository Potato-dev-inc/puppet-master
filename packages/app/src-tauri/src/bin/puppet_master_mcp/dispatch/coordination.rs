//! Task board, resource lock, context pack, session context and pane digest tools: maps each tool call to its bridge request.

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
        "create_task" => {
            let body = json!({
                "title": required_string(args, "title")?,
                "exclusive": args.get("exclusive").and_then(Value::as_bool).unwrap_or(true),
            });
            Ok(bridge_request("POST", "/tasks", Some(body))?)
        }
        "claim_task" => Ok(bridge_request(
            "POST",
            &format!(
                "/tasks/{}/claim",
                encode_path_segment(&required_string(args, "task_id")?)
            ),
            Some(json!({
                "agent_id": required_string(args, "agent_id")?,
                "lease_ms": args.get("lease_ms").cloned().unwrap_or(Value::Null),
            })),
        )?),
        "report_task_status" => Ok(bridge_request(
            "POST",
            &format!(
                "/tasks/{}/status",
                encode_path_segment(&required_string(args, "task_id")?)
            ),
            Some(json!({
                "status": required_string(args, "status")?,
                "agent_id": args.get("agent_id").cloned().unwrap_or(Value::Null),
                "reason": args.get("reason").cloned().unwrap_or(Value::Null),
                "project_path": args.get("project_path").cloned().unwrap_or(Value::Null),
            })),
        )?),
        "complete_task" => Ok(bridge_request(
            "POST",
            &format!(
                "/tasks/{}/complete{}",
                encode_path_segment(&required_string(args, "task_id")?),
                args.get("project_path")
                    .and_then(Value::as_str)
                    .map(|p| format!("?project_path={}", encode_path_segment(p)))
                    .unwrap_or_default()
            ),
            Some(json!({
                "agent_id": required_string(args, "agent_id")?,
                "evidence": args.get("evidence").and_then(Value::as_str).unwrap_or_default(),
            })),
        )?),
        "list_tasks" => bridge_request("GET", "/tasks", None),
        "acquire_resource_lock" => bridge_request("POST", "/locks", Some(args.clone())),
        "release_resource_lock" => bridge_request("POST", "/locks/release", Some(args.clone())),
        "build_context_pack" => bridge_request("POST", "/context-packs", Some(args.clone())),
        "read_project_ir_status" => bridge_request("GET", "/project-ir/status", None),
        "read_librarian_prompt" => bridge_request("GET", "/librarian/prompt", None),
        "read_session_context" => bridge_request("GET", "/session/context", None),
        "update_session_context" => bridge_request("PATCH", "/session/context", Some(args.clone())),
        "set_pane_role" => {
            let pane_id = required_string(args, "pane_id")?;
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/role", encode_path_segment(&pane_id)),
                Some(args.clone()),
            )?)
        }
        "read_pane_digest" => {
            let pane_id = required_string(args, "pane_id")?;
            Ok(bridge_request(
                "GET",
                &format!("/panes/{}/digest", encode_path_segment(&pane_id)),
                None,
            )?)
        }
        "update_pane_digest" => {
            let pane_id = required_string(args, "pane_id")?;
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/digest", encode_path_segment(&pane_id)),
                Some(args.clone()),
            )?)
        }
        _ => {
            *known = false;
            Ok(String::new())
        }
    }
}
