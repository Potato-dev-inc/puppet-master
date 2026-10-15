//! Catalog entries: Task board and resource locks.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_task",
            description: "Create a coordination task in the Rust task board.",
            input_schema: object_schema(
                json!({
                    "title": { "type": "string" },
                    "exclusive": { "type": "boolean", "default": true }
                }),
                vec!["title"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/tasks",
        },
        ToolDefinition {
            name: "claim_task",
            description: "Claim an exclusive task lease for an agent.",
            input_schema: object_schema(
                json!({
                    "task_id": { "type": "string" },
                    "agent_id": { "type": "string" },
                    "lease_ms": { "type": "number" }
                }),
                vec!["task_id", "agent_id"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/tasks/{task_id}/claim",
        },
        ToolDefinition {
            name: "report_task_status",
            description: "Update task status in the Rust task board.",
            input_schema: object_schema(
                json!({
                    "task_id": { "type": "string" },
                    "status": { "type": "string", "enum": ["pending", "claimed", "in_progress", "blocked", "completed"] },
                    "agent_id": { "type": "string" },
                    "reason": { "type": "string" },
                    "project_path": { "type": "string" }
                }),
                vec!["task_id", "status"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/tasks/{task_id}/status",
        },
        ToolDefinition {
            name: "complete_task",
            description: "Complete a task with evidence.",
            input_schema: object_schema(
                json!({
                    "task_id": { "type": "string" },
                    "agent_id": { "type": "string" },
                    "evidence": { "type": "string", "minLength": 1 },
                    "project_path": { "type": "string", "description": "Optional project containing the task" }
                }),
                vec!["task_id", "agent_id", "evidence"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/tasks/{task_id}/complete",
        },
        ToolDefinition {
            name: "list_tasks",
            description: "List rebuildable task board state from the Rust event log.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/tasks",
        },
        ToolDefinition {
            name: "acquire_resource_lock",
            description: "Acquire an exclusive resource lock.",
            input_schema: object_schema(
                json!({
                    "resource_type": { "type": "string", "enum": ["file", "directory", "command", "port", "git branch", "pane ownership"] },
                    "name": { "type": "string" },
                    "owner_id": { "type": "string" },
                    "lease_ms": { "type": "number" }
                }),
                vec!["resource_type", "name", "owner_id"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/locks",
        },
        ToolDefinition {
            name: "release_resource_lock",
            description: "Release a resource lock owned by an agent or pane.",
            input_schema: object_schema(
                json!({
                    "resource_type": { "type": "string" },
                    "name": { "type": "string" },
                    "owner_id": { "type": "string" }
                }),
                vec!["resource_type", "name", "owner_id"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/locks/release",
        },
    ]
}
