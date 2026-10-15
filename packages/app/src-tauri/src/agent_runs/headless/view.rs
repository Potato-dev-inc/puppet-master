//! Run views handed back to callers: presenting a snapshot, waiting on a run for a bounded time,
//! and compacting a required action into the prompt shape callers answer with `answer_prompt`.

use crate::agent_runs::runtime::AgentRunView;
use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;

pub(super) fn present_view(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> AgentRunView {
    AgentRunView::present(snapshot.clone(), Some(registry))
}

pub(super) fn wait_for_run(
    snapshot: &OperationSnapshot,
    wait_ms: u64,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<AgentRunView, OperationError> {
    if wait_ms == 0 {
        return Ok(present_view(snapshot, registry));
    }
    let result = operations::wait_for_operation_with_stage(
        &snapshot.project_path,
        &snapshot.operation_id,
        snapshot.revision,
        Some(vec![
            OperationStatus::WaitingInput,
            OperationStatus::Completed,
            OperationStatus::Failed,
            OperationStatus::Cancelled,
        ]),
        Some("settled_unverified"),
        wait_ms.min(300_000),
    )?;
    Ok(present_view(&result.snapshot, registry))
}

pub(in crate::agent_runs) fn compact_prompt(action: serde_json::Value) -> serde_json::Value {
    let id = ["prompt_id", "request_id", "message_id", "id"]
        .iter()
        .find_map(|key| action.get(*key).cloned())
        .or_else(|| {
            action
                .get("permission_ids")
                .and_then(serde_json::Value::as_array)
                .and_then(|ids| ids.first().cloned())
        })
        .unwrap_or(serde_json::Value::Null);
    let kind = action
        .get("kind")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let summary = action
        .get("question")
        .or_else(|| action.get("detail"))
        .or_else(|| action.get("text"))
        .or_else(|| action.get("summary"))
        .cloned()
        .unwrap_or_else(|| kind.clone());
    let options = action
        .get("options")
        .and_then(serde_json::Value::as_array)
        .map(|options| {
            serde_json::Value::Array(
                options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        if let Some(label) = option.as_str() {
                            serde_json::json!({"id": label, "label": label})
                        } else if let Some(object) = option.as_object() {
                            let label = object
                                .get("label")
                                .or_else(|| object.get("text"))
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            let id = object.get("id").cloned().unwrap_or_else(|| label.clone());
                            let mut normalized = serde_json::Map::new();
                            normalized.insert("id".into(), id);
                            normalized.insert("label".into(), label);
                            if let Some(description) =
                                object.get("description").or_else(|| object.get("detail"))
                            {
                                normalized.insert("description".into(), description.clone());
                            }
                            serde_json::Value::Object(normalized)
                        } else {
                            serde_json::json!({"id": index.to_string(), "label": option})
                        }
                    })
                    .collect(),
            )
        })
        .or_else(
            || match action.get("kind").and_then(serde_json::Value::as_str) {
                Some("permission_required") => Some(serde_json::json!([
                    {"id":"allow_once","label":"Allow once"},
                    {"id":"deny","label":"Deny"},
                    {"id":"allow_broad","label":"Always allow"}
                ])),
                Some("manual_approval_required") => Some(serde_json::json!([])),
                _ => None,
            },
        )
        .unwrap_or_else(|| serde_json::json!([]));
    let mut prompt = serde_json::json!({"id":id,"kind":kind,"summary":summary,"options":options});
    if let Some(object) = prompt.as_object_mut() {
        for key in [
            "action",
            "resource",
            "scope",
            "path",
            "permission",
            "previous_session",
            "session_id",
            "pane_id",
            "command",
            "cwd",
            // A usage limit (`handoff`): what ran out and when it can continue.
            "window",
            "reset_at_ms",
            "reset_text",
            "self_resume_armed",
        ] {
            if let Some(value) = action.get(key).cloned().filter(|value| !value.is_null()) {
                object.insert(key.to_string(), value);
            }
        }
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_view_prompt_carries_id_summary_command_and_cwd() {
        let screen = " $ cd \"C:\\x\" && python todo.py add \"buy milk\"\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n";
        let prompt = crate::pane_prompt::detect_in_screen("cursor_agent", screen, false).unwrap();
        let action = crate::pane_prompt::required_action_from_prompt(prompt.clone());
        let view = compact_prompt(action);
        assert_eq!(view["id"], prompt["prompt_id"]);
        assert!(view["id"].is_string());
        assert_eq!(view["kind"], "command_approval");
        assert_eq!(view["command"], prompt["command"]);
        assert_eq!(view["cwd"], "C:\\x");
        assert!(view["summary"]
            .as_str()
            .unwrap()
            .contains("python todo.py add"));
    }
}
