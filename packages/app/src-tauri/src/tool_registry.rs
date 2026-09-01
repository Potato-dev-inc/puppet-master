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

pub fn catalog_version() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for tool in tools() {
        tool.name.hash(&mut hasher);
        tool.method.hash(&mut hasher);
        tool.path.hash(&mut hasher);
    }
    format!("{}-{:016x}", tools().len(), hasher.finish())
}

pub fn external_mcp_tool_count() -> usize {
    tools()
        .into_iter()
        .filter(|tool| tool.visibility.external_mcp)
        .count()
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
            description: "Spawn an OpenCode native worker pane (API + attach TUI). Reuse an existing opencode_native pane when possible; never reuse orchestrator panes.",
            input_schema: object_schema(
                json!({
                    "agent_type": { "type": "string", "enum": ["opencode_native"] },
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
            description: "Read recent pane scrollback for evidence/debugging. Do NOT poll this for status — use wait_for_panes or read_opencode_worker_status instead.",
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
            description: "Compact opencode_native worker status: pane status, serve health, pending permission ids, active key profile. Prefer this over read_terminal_buffer for status checks.",
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
            path: "/panes/{pane_id}/opencode/status",
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
            description: "Long-poll until an opencode_native pane's footer model matches (alias for wait_for_panes with model_ready + tui_ready).",
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
            description: "Long-poll until a task reaches completed or blocked (alias for wait_for_panes task predicates).",
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
            description: "Long-poll until a worker pane settles (idle, waiting_input, permission, OpenCode TUI yes/no, or error). Alias for wait_for_panes with until=[\"settled\",\"error\"]. Returns agent_hint for the next step.",
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
    fn registry_contains_wait_tools() {
        let names = tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"wait_for_panes"));
        assert!(names.contains(&"wait_for_model"));
        assert!(names.contains(&"wait_for_task"));
        assert!(names.contains(&"read_recent_events"));
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
