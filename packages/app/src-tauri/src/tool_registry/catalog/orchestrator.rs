//! Catalog entries: Orchestrator state.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_orchestrator_state",
            description: "Read Rust-owned durable orchestration runtime state, starting with standby polling policy.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/orchestrator/state",
        },
        ToolDefinition {
            name: "update_orchestrator_state",
            description: "Update Rust-owned durable orchestration runtime state. Currently supports standby_poll_ms and standby_max_ms.",
            input_schema: object_schema(
                json!({
                    "standby_poll_ms": { "type": "number" },
                    "standby_max_ms": { "type": "number" }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "PATCH",
            path: "/orchestrator/state",
        },
    ]
}
