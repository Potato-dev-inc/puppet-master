use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolSafety {
    ReadOnly,
    Mutating,
    Destructive,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolVisibility {
    pub sidebar: bool,
    pub external_mcp: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub safety: ToolSafety,
    pub visibility: ToolVisibility,
    pub method: &'static str,
    pub path: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResourceDefinition {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "mimeType")]
    pub mime_type: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PromptDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: Vec<PromptArgument>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PromptArgument {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

fn visible_everywhere() -> ToolVisibility {
    ToolVisibility {
        sidebar: true,
        external_mcp: true,
    }
}

fn object_schema(properties: Value, required: Vec<&'static str>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

pub fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_panes",
            description: "List all live PTY panes. Panes with id puppet-master-orchestrator-* are dedicated orchestrators; delegate only to worker panes.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
            safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(),
            method: "GET",
            path: "/panes",
        },
        ToolDefinition {
            name: "bridge_health",
            description: "Check whether the Puppet Master HTTP bridge is reachable and return its version metadata.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: None,
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
                    "agent_type": { "type": "string", "enum": ["claude", "codex", "opencode", "cmd", "powershell", "bash", "cursor"] },
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
            description: "Inspect a live terminal pane and report the best-known model signal plus an advisory smartness score.",
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
            name: "spawn_agent",
            description: "Spawn a worker PTY pane. Reuse existing worker panes of the same agent_type when possible; never reuse orchestrator panes.",
            input_schema: object_schema(
                json!({
                    "agent_type": { "type": "string", "enum": ["claude", "codex", "opencode", "cmd", "powershell", "bash", "cursor"] },
                    "cwd": { "type": "string", "description": "Working directory; defaults to current project root" },
                    "cols": { "type": "number", "description": "Terminal columns (default 120)" },
                    "rows": { "type": "number", "description": "Terminal rows (default 30)" },
                    "pane_id": { "type": "string", "description": "Optional caller-supplied stable id" }
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
            description: "Read the recent scrollback of a pane as text.",
            input_schema: object_schema(
                json!({
                    "pane_id": { "type": "string" },
                    "lines": { "type": "number", "description": "How many trailing lines to return (default 200)" }
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
                    "append_newline": { "type": "boolean", "default": true }
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
            name: "kill_pane_process",
            description: "Terminate a worker pane. Cannot kill puppet-master-orchestrator-* panes.",
            input_schema: object_schema(json!({ "pane_id": { "type": "string" } }), vec!["pane_id"]),
            output_schema: None,
            safety: ToolSafety::Destructive,
            visibility: visible_everywhere(),
            method: "DELETE",
            path: "/panes/{pane_id}",
        },
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
                    "status": { "type": "string" }
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
                    "evidence": { "type": "string" }
                }),
                vec!["task_id", "agent_id"],
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
        ToolDefinition {
            name: "delegate_task",
            description: "Validate a structured delegation request and render a Codex-style worker prompt without launching a worker.",
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

pub fn resources() -> Vec<ResourceDefinition> {
    vec![
        ResourceDefinition {
            uri: "puppet-master://session",
            name: "Current session",
            description: "Current Puppet Master session state and orchestration context.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://panes",
            name: "Live panes",
            description: "Live terminal panes known to the Rust bridge.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://panes/{id}/digest",
            name: "Pane digest",
            description: "Latest pane digest supplied through the Rust session context event log.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://tasks",
            name: "Tasks",
            description: "Task board projection rebuilt from the Rust event log.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://locks",
            name: "Locks",
            description: "Resource lock projection rebuilt from the Rust event log.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://audit",
            name: "Audit",
            description: "Recent coordination and MCP audit entries.",
            mime_type: "application/json",
        },
    ]
}

pub fn prompts() -> Vec<PromptDefinition> {
    vec![
        PromptDefinition {
            name: "status_check",
            description: "Inspect bridge health, panes, tasks, and locks before choosing next action.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "summarize_session",
            description: "Summarize current session state, active work, blockers, and recommended next steps.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "handoff_to_worker",
            description: "Prepare a concise handoff prompt for a worker pane.",
            arguments: vec![PromptArgument {
                name: "pane_id",
                description: "Target worker pane id.",
                required: true,
            }],
        },
        PromptDefinition {
            name: "delegate_refactor",
            description: "Render a structured refactor delegation prompt with acceptance criteria and evidence requirements.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "implement_with_review",
            description: "Render a two-step implementation prompt that asks for verification and reviewer evidence.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "fix_ci",
            description: "Render a CI-fix prompt focused on reproducing failures and reporting command output.",
            arguments: vec![],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_bridge_health() {
        assert!(tools().iter().any(|tool| tool.name == "bridge_health"));
    }

    #[test]
    fn bridge_health_is_visible_everywhere_and_read_only() {
        let health = tools()
            .into_iter()
            .find(|tool| tool.name == "bridge_health")
            .unwrap();
        assert!(health.visibility.sidebar);
        assert!(health.visibility.external_mcp);
        assert_eq!(health.safety, ToolSafety::ReadOnly);
    }

    #[test]
    fn serializes_mcp_input_schema_name() {
        let value = serde_json::to_value(&tools()[0]).unwrap();
        assert!(value.get("inputSchema").is_some());
        assert!(value.get("input_schema").is_none());
    }

    #[test]
    fn mutating_tools_are_annotated() {
        let spawn = tools()
            .into_iter()
            .find(|tool| tool.name == "spawn_agent")
            .unwrap();
        assert_eq!(spawn.safety, ToolSafety::Mutating);
    }

    #[test]
    fn destructive_tools_are_annotated() {
        let kill = tools()
            .into_iter()
            .find(|tool| tool.name == "kill_pane_process")
            .unwrap();
        assert_eq!(kill.safety, ToolSafety::Destructive);
    }

    #[test]
    fn omits_output_schema_when_absent() {
        let value = serde_json::to_value(
            tools()
                .into_iter()
                .find(|tool| tool.name == "bridge_health")
                .unwrap(),
        )
        .unwrap();
        assert!(value.get("outputSchema").is_none());
    }

    #[test]
    fn includes_output_schema_when_present() {
        let value = serde_json::to_value(
            tools()
                .into_iter()
                .find(|tool| tool.name == "read_session_context")
                .unwrap(),
        )
        .unwrap();
        assert!(value.get("outputSchema").is_some());
    }

    #[test]
    fn registry_contains_session_context_tools() {
        let names = tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"read_session_context"));
        assert!(names.contains(&"update_session_context"));
        assert!(names.contains(&"set_pane_role"));
        assert!(names.contains(&"read_pane_digest"));
        assert!(names.contains(&"delegate_task"));
    }

    #[test]
    fn resources_include_session() {
        assert!(resources()
            .iter()
            .any(|resource| resource.uri == "puppet-master://session"));
    }

    #[test]
    fn prompts_include_status_check() {
        assert!(prompts().iter().any(|prompt| prompt.name == "status_check"));
    }
}
