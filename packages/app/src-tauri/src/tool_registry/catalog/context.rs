//! Catalog entries: Librarian, project IR, context packs, session context, pane roles/digests.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_librarian_prompt",
            description: "Render the OpenCode librarian prompt (DeepWiki-style index task). Send the returned prompt to opencode_native via write_terminal_input; worker writes .puppet-master/project-ir.json.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/librarian/prompt",
        },
        ToolDefinition {
            name: "read_project_ir_status",
            description: "Check whether the librarian project index exists, is stale vs current git HEAD, and which indexer command to run.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/project-ir/status",
        },
        ToolDefinition {
            name: "build_context_pack",
            description: "Build a compact Rust-generated context pack for an assigned task.",
            input_schema: object_schema(
                json!({
                    "task_id": { "type": "string" },
                    "agent_id": { "type": "string" },
                    "user_constraints": { "type": "array", "items": { "type": "string" } },
                    "manager_instructions": { "type": "string" },
                    "raw_scrollback": { "type": "string" }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/context-packs",
        },
        ToolDefinition {
            name: "read_session_context",
            description: "Read the current Rust session context, including current goal, pane roles, pane digests, timeline, lock conflicts, and orchestrator policy.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "current_goal": { "type": ["string", "null"] },
                    "pane_roles": { "type": "object" },
                    "pane_digests": { "type": "object" },
                    "timeline": { "type": "array" },
                    "lock_conflicts": { "type": "array" },
                    "orchestrator": { "type": "object" }
                }
            })),
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/session/context",
        },
        ToolDefinition {
            name: "update_session_context",
            description: "Update the current Rust session context. The first supported field is current_goal.",
            input_schema: object_schema(
                json!({
                    "current_goal": {
                        "type": ["string", "null"],
                        "description": "Current user goal; null clears it."
                    }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "PATCH",
            path: "/session/context",
        },
        ToolDefinition {
            name: "set_pane_role",
            description: "Assign a coordination role to a pane. Allowed roles are implementer, reviewer, shell, orchestrator, and observer.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "role": {
                        "type": "string",
                        "enum": ["implementer", "reviewer", "shell", "orchestrator", "observer"]
                    }
                }),
                vec!["pane_id", "role"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/role",
        },
        ToolDefinition {
            name: "read_pane_digest",
            description: "Read the latest manually supplied digest for a pane.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" }
                }),
                vec!["pane_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes/{pane_id}/digest",
        },
        ToolDefinition {
            name: "update_pane_digest",
            description: "Store a concise manually supplied digest for a pane in the Rust event log.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "summary": { "type": "string" },
                    "source": { "type": "string", "default": "manual" }
                }),
                vec!["pane_id", "summary"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/digest",
        },
    ]
}
