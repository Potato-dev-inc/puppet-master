//! Catalog entries: Blocking wait tools and recent events.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "wait_for_panes",
            description: "Long-poll until any worker pane reaches a target state. Prefer this over polling list_panes or read_terminal_buffer. Use until=[\"settled\"] after write_terminal_input (idle, permission, TUI yes/no). Returns agent_hint with next action.",
            input_schema: object_schema(
                json!({
                    "pane_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1
                    },
                    "until": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": [
                                "idle",
                                "waiting_input",
                                "error",
                                "gone",
                                "permission",
                                "unhealthy",
                                "key_swap_required",
                                "key_swap",
                                "key_rotated",
                                "rate_limited",
                                "model_ready",
                                "tui_ready",
                                "task_completed",
                                "task_blocked",
                                "output_match",
                                "settled",
                                "tui_prompt"
                            ]
                        },
                        "description": "Wake triggers. Defaults to standard worker set."
                    },
                    "match": {
                        "type": "object",
                        "properties": {
                            "provider_id": { "type": "string" },
                            "model_id": { "type": "string" }
                        },
                        "description": "Required for model_ready when checking a specific model"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Task id for task_completed / task_blocked waits"
                    },
                    "output_regex": {
                        "type": "string",
                        "description": "Regex for output_match wait (use with until=[\"output_match\"])"
                    },
                    "timeout_ms": {
                        "type": "number",
                        "description": "Max wait (default 120000, max 300000)"
                    }
                }),
                vec!["pane_ids"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/wait",
        },
        ToolDefinition {
            name: "wait_for_model",
            description: "Long-poll until an opencode_native pane's footer model matches (alias of wait_for_panes with model_ready + tui_ready; unlisted by default, prefer wait_for_panes).",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "provider_id": { "type": "string" },
                    "model_id": { "type": "string" },
                    "timeout_ms": { "type": "number", "description": "Max wait ms (default 120000)" }
                }),
                vec!["pane_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/wait/model",
        },
        ToolDefinition {
            name: "wait_for_task",
            description: "Long-poll until a task reaches completed or blocked (alias of wait_for_panes task predicates; unlisted by default, prefer wait_for_panes).",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "until": {
                        "type": "array",
                        "items": { "type": "string", "enum": ["task_completed", "task_blocked", "error"] }
                    },
                    "timeout_ms": { "type": "number" }
                }),
                vec!["pane_id", "task_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/wait/task",
        },
        ToolDefinition {
            name: "wait_for_worker",
            description: "Long-poll until a worker pane settles (idle, waiting_input, permission, OpenCode TUI yes/no, or error). Alias of wait_for_panes with until=[\"settled\",\"error\"]; unlisted by default, prefer wait_for_panes. Returns agent_hint for the next step.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "timeout_ms": { "type": "number", "description": "Max wait ms (default 120000)" }
                }),
                vec!["pane_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/wait/worker",
        },
        ToolDefinition {
            name: "read_recent_events",
            description: "Read recent system events for debugging (not for hot-loop polling).",
            input_schema: object_schema(
                json!({
                    "limit": { "type": "number", "description": "Max events (1-200, default 50)" },
                    "pane_id": { "type": "string" },
                    "types": { "type": "array", "items": { "type": "string" } },
                    "since_id": { "type": "string" }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/events/recent",
        },
    ]
}
