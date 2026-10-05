//! Catalog entries: Operations and delegation.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "delegate_task",
            description: "Prepare and validate a structured delegation request, then render a worker prompt. This does not launch or dispatch work; use delegate_work to create an asynchronous operation.",
            input_schema: object_schema(
                json!({
                    "task_id": { "type": "string" },
                    "target_pane_id": { "type": "string" },
                    "intent": { "type": "string" },
                    "acceptance_criteria": { "type": "array", "items": { "type": "string" } },
                    "locked_resources": { "type": "array", "items": { "type": "string" } },
                    "evidence_required": { "type": "array", "items": { "type": "string" } },
                    "token_budget_hint": { "type": "number" },
                    "timeout_ms": { "type": "number" }
                }),
                vec!["intent", "acceptance_criteria"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/delegate-task",
        },
        ToolDefinition {
            name: "delegate_work",
            description: "Create an asynchronous agent operation. Returns an operation_id; use wait_for_operation to wait for revision-based progress, get_operation to inspect state, or cancel_operation to request cancellation. This call creates work and is not automatically retried.",
            input_schema: object_schema(json!({
                "project_path": { "type": "string" },
                "task": { "type": "string", "description": "Work to delegate" },
                "idempotency_key": { "type": "string", "description": "Caller-supplied key to make retries safe" },
                "pane_id": { "type": "string" },
                "agent_type": { "type": "string", "enum": ["claude", "codex", "cursor", "opencode", "opencode_native"] },
                "acceptance_criteria": { "type": "array", "items": { "type": "string", "minLength": 1 }, "description": "Optional free-text guidance for the worker; NOT machine-checked (a text mismatch never fails a run and acceptance_status stays not_checked). For verified acceptance pass `checks` (file_exists / file_contains / file_absent evaluated against disk, result_contains against the reply text, command_exits runs an argv and compares its exit code, all at completion); acceptance_status then reflects the checks." },
                "context_policy": { "type": "string", "enum": ["fresh", "packet", "parent_summary", "resume", "selected_history"] },
                "task_id": { "type": "string" },
                "exclusive": { "type": "boolean" },
                "locks": { "type": "array", "items": { "type": "string", "minLength": 1 } }
            }), vec!["project_path", "task", "idempotency_key"]),
            output_schema: Some(operation_snapshot_schema()),
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/operations/delegate",
        },
        ToolDefinition {
            name: "get_operation",
            description: "Read the current state and revision of an asynchronous operation.",
            input_schema: object_schema(json!({ "operation_id": { "type": "string" }, "project_path": { "type": "string", "description": "Optional project directory; defaults to the active project" } }), vec!["operation_id"]),
            output_schema: Some(operation_snapshot_schema()),
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/operations/{operation_id}",
        },
        ToolDefinition {
            name: "wait_for_operation",
            description: "Wait until an operation revision changes or it reaches a terminal state. Prefer this over polling get_operation.",
            input_schema: object_schema(json!({
                "operation_id": { "type": "string" },
                "project_path": { "type": "string", "description": "Optional project directory; defaults to the active project" },
                "after_revision": { "type": "integer" },
                "until": { "type": "array", "items": { "type": "string", "enum": ["queued", "starting", "running", "waiting_input", "cancelling", "completed", "failed", "cancelled"] } },
                "timeout_ms": { "type": "integer", "default": 120000 }
            }), vec!["operation_id"]),
            output_schema: Some(json!({"type":"object","properties":{"snapshot":operation_snapshot_schema(),"reason":{"type":"string","enum":["matched_state","revision_changed","terminal","timeout"]}},"required":["snapshot","reason"]})),
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/operations/{operation_id}/wait",
        },
        ToolDefinition {
            name: "cancel_operation",
            description: "Request cancellation of an asynchronous operation. Queued operations can be cancelled; cancellation during a running operation may be rejected until worker interruption is supported.",
            input_schema: object_schema(json!({ "operation_id": { "type": "string" }, "project_path": { "type": "string", "description": "Optional project directory; defaults to the active project" } }), vec!["operation_id"]),
            output_schema: Some(operation_snapshot_schema()),
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/operations/{operation_id}/cancel",
        },
    ]
}
