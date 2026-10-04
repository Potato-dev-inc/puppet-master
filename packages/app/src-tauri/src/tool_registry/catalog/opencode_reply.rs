//! Catalog entries: OpenCode permission and question replies.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "reply_opencode_permission",
            description: "Reply to an OpenCode API permission prompt for an opencode_native pane (e.g. reply once/always/deny).",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "request_id": { "type": "string" },
                    "reply": { "type": "string", "description": "e.g. once, always, deny" }
                }),
                vec!["pane_id", "request_id", "reply"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/opencode/permissions/{request_id}/reply",
        },
        ToolDefinition {
            name: "reply_opencode_question",
            description: "Answer an OpenCode yes/no question for opencode_native via session API. Prefer this over press_key — API prompts do not reach the TUI compose box.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "answer": { "type": "string", "description": "Option label, e.g. Yes or No" },
                    "request_id": { "type": "string", "description": "Optional; omit to answer the pane's current pending question" }
                }),
                vec!["pane_id", "answer"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/opencode/question/reply",
        },
    ]
}
