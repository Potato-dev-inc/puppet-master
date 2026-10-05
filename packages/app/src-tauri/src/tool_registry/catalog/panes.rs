//! Catalog entries: Panes, terminals, agent models and spawning.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_panes",
            description: "List live PTY panes (id, agent_type, status, cwd). Use this to find an existing worker window. Agent runs also appear in list_agents with pane_id. Panes with id puppet-master-orchestrator-* are dedicated orchestrators.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes",
        },
        ToolDefinition {
            name: "bridge_health",
            description: "Check whether the Puppet Master HTTP bridge is reachable and return its version metadata. Also returns env: {python, pytest, node, git, cargo} each {present, version, path} from PATH lookups on the MCP host (cached ~60s), so you need not install or probe tools yourself.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"ok":{"type":"boolean"},"version":{"type":"string"},"build_id":{"type":"string"},"catalog_version":{"type":"string"},"tool_count":{"type":"integer"},"shim_catalog_version":{"type":"string"},"schema_drift":{"type":"boolean"},"env":{"type":"object","properties":{"python":env_tool_schema(),"pytest":env_tool_schema(),"node":env_tool_schema(),"git":env_tool_schema(),"cargo":env_tool_schema()}}}})),
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/health",
        },
        ToolDefinition {
            name: "list_agent_contexts",
            description: "List static context profiles for supported agents, including strengths, smartness score, and orchestration actions.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/agent-contexts",
        },
        ToolDefinition {
            name: "read_agent_context",
            description: "Read context for an agent type or a live pane. If pane_id is provided, includes pane metadata, model inspection, and a recent buffer preview.",
            input_schema: object_schema(
                json!({
                    "agent_type": { "type": "string", "enum": ["claude", "codex", "opencode", "opencode_native", "cmd", "powershell", "bash", "cursor"] },
                    "pane_id": { "type": "string" }
                }),
                vec![],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes/{pane_id}/agent-context",
        },
        ToolDefinition {
            name: "inspect_agent_model",
            description: "Report the active model for a pane. For opencode_native, reads OpenCode session API (session_model + last_user_model); buffer text is fallback only. Do not poll in a loop — use wait_for_panes after mutations.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "lines": { "type": "number", "description": "Recent buffer lines to scan for model hints (default 200)" }
                }),
                vec!["pane_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes/{pane_id}/model",
        },
        ToolDefinition {
            name: "switch_agent_model",
            description: "Switch an opencode_native pane's session model and sync the TUI footer (no scratch sessions). Prefer OpenCode Go: pass bare model_id like glm-5.2 / deepseek-v4-pro, or model_provider=opencode-go. Use write_terminal_input with model_id when also sending a prompt.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "model_id": {
                        "type": "string",
                        "description": "Model id (bare id defaults to opencode-go), or provider/model like openrouter/xiaomi/mimo-v2.5"
                    },
                    "model_provider": {
                        "type": "string",
                        "description": "Optional provider (default opencode-go for bare model_id)"
                    }
                }),
                vec!["pane_id", "model_id"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/model",
        },
        ToolDefinition {
            name: "spawn_agent",
            description: "Advanced: only create an empty worker pane (OpenCode API or PowerShell) without running anything. To run a task, call run_agent directly: it creates its own pane in project_path and needs no spawn_agent. Pass cwd to pick the workspace (default: the app's selected project); run_agent against the pane must use the same workspace. Reuse an existing pane of the same agent_type when possible; pass force_new to create another. Never reuse orchestrator panes.",
            input_schema: object_schema(
                json!({
                    "agent_type": { "type": "string", "enum": ["opencode_native", "powershell"] },
                    "cwd": { "type": "string", "description": "Working directory; defaults to current project root" },
                    "cols": { "type": "number", "description": "Terminal columns (default 120)" },
                    "rows": { "type": "number", "description": "Terminal rows (default 30)" },
                    "pane_id": { "type": "string", "description": "Optional caller-supplied stable id" },
                    "force_new": { "type": "boolean", "description": "If true, always create a new pane even when a reusable pane of this agent_type exists" }
                }),
                vec!["agent_type"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes",
        },
        ToolDefinition {
            name: "read_terminal_buffer",
            description: "Read recent pane scrollback for evidence/debugging. Do NOT poll this for status — use wait_for_panes or read_opencode_worker_status instead.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "lines": { "type": "number", "description": "How many trailing lines to return (default 200)" },
                    "view": { "type": "string", "enum": ["screen", "scrollback"], "description": "Current rendered screen or historical scrollback. If omitted, agent/TUI panes use screen and shell panes use scrollback." }
                }),
                vec!["pane_id"],
            ),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes/{pane_id}/buffer",
        },
        ToolDefinition {
            name: "write_terminal_input",
            description: "Send keystrokes to a worker pane. Cannot target puppet-master-orchestrator-* panes.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "text": { "type": "string" },
                    "append_newline": { "type": "boolean", "default": true },
                    "model_provider": {
                        "type": "string",
                        "description": "OpenCode provider (optional; bare model_id defaults to opencode-go; else settings default)"
                    },
                    "model_id": {
                        "type": "string",
                        "description": "OpenCode model id or provider/model; prefer OpenCode Go bare ids like glm-5.2"
                    }
                }),
                vec!["pane_id", "text"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/input",
        },
        ToolDefinition {
            name: "press_key",
            description: "Send a named key to a worker pane PTY (menus, yes/no, navigation). Cannot target puppet-master-orchestrator-* panes.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "key": {
                        "type": "string",
                        "description": "enter, escape, tab, space, up, down, left, right, home, end, pageup, pagedown, y, n, yes, no, ctrl+c, ctrl+d, ctrl+z"
                    }
                }),
                vec!["pane_id", "key"],
            ),
            output_schema: None,
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/panes/{pane_id}/key",
        },
        ToolDefinition {
            name: "kill_pane_process",
            description: "Terminate a worker pane. Cannot kill puppet-master-orchestrator-* panes.",
            input_schema: object_schema(json!({ "pane_id": { "type": "string" } }), vec!["pane_id"]),
            output_schema: None,
            safety: ToolSafety::Destructive,
            visibility: visible_everywhere(),
            method: "DELETE",
            path: "/panes/{pane_id}",
        },
    ]
}
