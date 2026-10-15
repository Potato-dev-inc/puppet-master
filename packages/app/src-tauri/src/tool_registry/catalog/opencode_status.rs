//! Catalog entries: OpenCode key, worker and message status.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_opencode_key_status",
            description: "Read which OpenCode API key profile is active (a or b). Does not return key material.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/opencode/keys/status",
        },
        ToolDefinition {
            name: "rotate_opencode_key",
            description: "Switch the active OpenCode API key profile (a/b/next) and restart opencode_native worker panes so the new key takes effect. Never returns key material.",
            input_schema: object_schema(
                json!({
                    "profile": {
                        "type": "string",
                        "enum": ["next", "a", "b"],
                        "description": "Profile to activate. Default next toggles a↔b."
                    },
                    "pane_id": {
                        "type": "string",
                        "description": "Optional opencode_native pane to restart. Omit to restart all native workers."
                    }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/opencode/keys/rotate",
        },
        ToolDefinition {
            name: "read_opencode_worker_status",
            description: "Compact opencode_native worker status: pane status, serve health, pending permission ids, session model. pane_id is optional when exactly one opencode_native pane is open.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "worker_id": { "type": "string", "description": "Alias for pane_id on native workers." }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/opencode/worker-status",
        },
        ToolDefinition {
            name: "read_opencode_messages",
            description: "Read structured OpenCode session messages for opencode_native (assistant text, tool/question parts). Prefer this over read_terminal_buffer for model output — no TUI chrome.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "limit": {
                        "type": "number",
                        "description": "Max messages to return (default 20, max 200)"
                    },
                    "role": {
                        "type": "string",
                        "enum": ["all", "user", "assistant"],
                        "description": "Filter by role (default all)"
                    }
                }),
                vec!["pane_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes/{pane_id}/opencode/messages",
        },
    ]
}
