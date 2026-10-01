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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMode {
    Agent,
    Shell,
    Both,
}

impl McpMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "shell" => Some(Self::Shell),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
            Self::Both => "both",
        }
    }
}

pub fn tools_for_mode(mode: McpMode) -> Vec<ToolDefinition> {
    tools()
        .into_iter()
        .filter(|tool| tool.visibility.external_mcp && tool_visible_in_mode(tool.name, mode))
        .collect()
}

pub fn tool_visible_in_mode(name: &str, mode: McpMode) -> bool {
    if name == "set_mode" {
        return true;
    }
    if mode == McpMode::Both {
        return true;
    }
    const AGENT_TOOLS: &[&str] = &[
        "run_agent",
        "wait_agents",
        "send_agent",
        "send_message",
        "followup_task",
        "answer_prompt",
        "cancel_agent",
        "interrupt_agent",
        "list_agents",
        "list_workers",
        "list_panes",
        "inspect_agent",
        "agent_transcript",
        "close_agent",
        "take_over",
        "release",
        "attach_agents",
        "release_lease",
        "transfer_agent",
        "session_identity",
        "reply_opencode_question",
        "reply_opencode_permission",
        "read_opencode_messages",
        "read_opencode_worker_status",
        "bridge_health",
        "delegate_work",
        "get_operation",
        "wait_for_operation",
        "cancel_operation",
    ];
    const SHELL_TOOLS: &[&str] = &[
        "list_panes",
        "bridge_health",
        "read_agent_context",
        "inspect_agent_model",
        "switch_agent_model",
        "read_terminal_buffer",
        "write_terminal_input",
        "press_key",
        "spawn_agent",
        "kill_pane_process",
        "wait_for_panes",
        "wait_for_model",
        "wait_for_worker",
        "read_recent_events",
        "shell_exec",
        "release",
        "take_over",
    ];
    match mode {
        McpMode::Agent => AGENT_TOOLS.contains(&name),
        McpMode::Shell => SHELL_TOOLS.contains(&name),
        McpMode::Both => true,
    }
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

fn operation_snapshot_schema() -> Value {
    json!({"type":"object","properties":{
        "operation_id":{"type":"string"},"project_path":{"type":"string"},"task":{"type":"string"},
        "pane_id":{"type":["string","null"]},"idempotency_key":{"type":"string"},
        "status":{"type":"string","enum":["queued","starting","running","waiting_input","cancelling","completed","failed","cancelled"]},
        "revision":{"type":"integer","minimum":0},"created_at_ms":{"type":"integer"},"updated_at_ms":{"type":"integer"},
        "source":{"type":"string","enum":["native","inferred"]},"stage":{"type":["string","null"]},
        "progress_pct":{"type":["integer","null"],"minimum":0,"maximum":100},
        "result":{"type":["string","null"]},"error":{"type":["object","null"],"properties":{
            "code":{"type":"string"},"message":{"type":"string"},"recoverable":{"type":"boolean"},
            "retry_after_ms":{"type":"integer"},"context":{}
        },"required":["code","message","recoverable"]},"required_action":{},"pane_state":{}
    },"required":["operation_id","project_path","task","idempotency_key","status","revision","created_at_ms","updated_at_ms","source"]})
}

fn agent_run_schema() -> Value {
    let mut schema = json!({"type":"object","properties":{
        "handle":{"type":"string"},"operation_id":{"type":"string"},"turn_id":{"type":"string"},
        "status":{"type":"string","enum":["running","needs_input","completed","failed","cancelled","timeout","interrupted","closed"]},
        "result":{"type":["string","null"]},"result_capture":{"type":"string","enum":["authoritative","inferred","missing","pending"]},
        "watch_command":{"type":"string"},
        "acceptance_status":{"type":"string","enum":["not_checked","passed","failed","blocked"]},
        "verified":{"type":"boolean"},
        "prompt":{"type":["object","null"]},"error":{"type":["object","null"]},
        "pane_id":{"type":["string","null"]},"duration_ms":{"type":["integer","null"]},"revision":{"type":"integer"},
        "next_cursor":{"type":"integer","minimum":0},
        "name":{"type":["string","null"]},
        "context_policy":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"]},
        "context_continuity":{"type":"string","enum":["resume","reconstructed_summary","none"]},
        "requested_model":{"type":["string","null"]},"resolved_model":{"type":["string","null"]},
        "requested_reasoning":{"type":["string","null"]},"resolved_reasoning":{"type":["string","null"]},
        "model_mismatch":{"type":"boolean"},
        "capabilities":{"type":"object","properties":{
            "session_resume":{"type":"boolean"},"live_messages":{"type":"boolean"},
            "queued_followups":{"type":"boolean"},"structured_results":{"type":"boolean"},
            "model_selection":{"type":"boolean"},"reasoning_selection":{"type":"boolean"},
            "graceful_interrupt":{"type":"boolean"},"visible_terminal":{"type":"boolean"},
            "interface":{"type":"string","enum":["native_protocol","structured_headless","tui_observation"]},
            "notes":{"type":"array","items":{"type":"string"}}
        }}
    },"required":["handle","operation_id","status","verified","revision"]});
    if let Some(properties) = schema.pointer_mut("/properties").and_then(Value::as_object_mut) {
        properties.insert("result_unchanged".into(), json!({"type":"boolean"}));
        properties.insert("wake_reason".into(), json!({"type":"string"}));
        properties.insert("result_verified".into(), json!({"type":"boolean"}));
        properties.insert("model_verified".into(), json!({"type":["boolean","null"]}));
        properties.insert("recovery".into(), json!({"type":["object","null"]}));
    }
    schema
}

/// Coordinator-facing MCP initialize text (stdio binary and HTTP catalog consumers).
pub fn mcp_instructions() -> &'static str {
    "Puppet Master MCP (agent mode). Sequence: list_agents → run_agent(worker_id or pane_id, task) → handle; send_message(handle) steers the current turn (receipt ≠ result); followup_task(handle, task) starts the next turn on the same conversation; wait_agents(handles) collects answers (default until: completed, failed, cancelled, needs_input). After run_agent/followup_task/delegate_work or WAIT_TIMEOUT, run the returned watch_command (plain `npx puppet-master watch …`) via your agent harness shell tool's background mode — do not use detached Start-Process; the harness must own stdout so it can wake when the turn finishes. run_agent/followup_task mint idempotency_key if omitted — reuse it after WAIT_TIMEOUT. Do not close the user's pane as cleanup. OpenCode yes/no: answer_prompt, not press_key."
}

pub fn catalog_version() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for tool in tools() {
        tool.name.hash(&mut hasher);
        tool.description.hash(&mut hasher);
        tool.method.hash(&mut hasher);
        tool.path.hash(&mut hasher);
        tool.input_schema.to_string().hash(&mut hasher);
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
            name: "set_mode",
            description: "Choose which MCP tool catalog this connection can use: agent, shell, or both. The selection is isolated to this MCP connection.",
            input_schema: object_schema(json!({"mode":{"type":"string","enum":["agent","shell","both"]}}), vec!["mode"]),
            output_schema: Some(json!({"type":"object","properties":{"mode":{"type":"string","enum":["agent","shell","both"]},"tool_count":{"type":"integer","minimum":0}},"required":["mode","tool_count"]})),
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/mcp/mode",
        },
        ToolDefinition {
            name: "run_agent",
            description: "Start or adopt a worker with a task and return a stable handle. Happy path: list_agents → run_agent(worker_id or pane_id, task) → handle. Waits up to wait_ms (120s default). On WAIT_TIMEOUT, retry with the same idempotency_key and call wait_agents — do not start a duplicate turn. Steering: send_message; next turn: followup_task(handle). Closing the user's pane is not cleanup. keep_pane defaults to false: a pane this run created is closed automatically when the run completes, after which take_over, switch_agent_model and live pane inspection fail with PANE_CLOSED_AFTER_RUN (agent_transcript keeps working from the stored transcript). Pass keep_pane=true to keep the pane until close_agent is called. Adopted UI panes are never closed.",
            input_schema: object_schema(json!({"project_path":{"type":"string"},"task":{"type":"string"},"name":{"type":"string"},"handle":{"type":"string"},"worker_id":{"type":"string"},"agent_run_id":{"type":"string"},"agent_type":{"type":"string","enum":["claude","codex","cursor_agent","opencode","opencode_native"]},"headless":{"type":"boolean","default":true},"background":{"type":"boolean","default":false},"wait_ms":{"type":"integer","minimum":0,"maximum":300000,"default":120000},"read_only":{"type":"boolean","default":false},"keep_pane":{"type":"boolean","default":false,"description":"Default false: a pane created for this run is closed when the run completes (take_over/switch_agent_model then fail with PANE_CLOSED_AFTER_RUN). true keeps the pane until close_agent."},"timeout_ms":{"type":"integer","minimum":1,"maximum":86400000,"default":900000},"idempotency_key":{"type":"string"},"acceptance_criteria":{"type":"array","items":{"type":"string","minLength":1},"description":"Optional free-text guidance for the worker; NOT machine-checked. For verified acceptance use `checks`."},"checks":{"type":"array","maxItems":20,"description":"State-based acceptance checks evaluated on disk at completion (no commands run). Paths are relative to project_path. A failing check fails the run with ACCEPTANCE_FAILED.","items":{"type":"object","properties":{"type":{"type":"string","enum":["file_exists","file_contains","file_absent"]},"path":{"type":"string","minLength":1},"text":{"type":"string","minLength":1}},"required":["type","path"]}},"pane_id":{"type":"string"},"allow_broad":{"type":"boolean","default":false},"context_policy":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"]},"context_mode":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"],"description":"alias for context_policy"},"model":{"type":"string"},"reasoning":{"type":"string"},"role":{"type":"string"},"scope":{"type":"string"},"selected_history":{"type":"array","items":{"type":"string"}},"locks":{"type":"array","items":{"type":"string","minLength":1},"description":"Optional resource locks (type:name, e.g. file:src/a.rs) acquired before the run starts and released when it ends. Fails fast with RESOURCE_LOCKED (context resource_id, owner_id) if another run holds one."}}), vec!["task"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/run",
        },
        ToolDefinition {
            name: "wait_agents",
            description: "Wait for worker handles. Default until is completed, failed, cancelled, and needs_input (progress revisions do not wake). Pass wake_on_progress=true to wake on any revision. Omits result when unchanged since after_cursor (result_unchanged). Default and maximum timeout_ms is 120000: MCP hosts commonly abandon a call after 2-3 minutes (Request timed out) and a timeout never drops ownership, so loop wait_agents for longer runs.",
            input_schema: object_schema(json!({"project_path":{"type":"string"},"handles":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}},"after_cursor":{"type":"integer","minimum":0},"after_revisions":{"type":"object","additionalProperties":{"type":"integer","minimum":0}},"until":{"type":"array","items":{"type":"string","enum":["queued","starting","running","waiting_input","needs_input","cancelling","completed","failed","cancelled"]}},"wake_on_progress":{"type":"boolean","default":false},"timeout_ms":{"type":"integer","minimum":0,"maximum":120000,"default":120000},"mode":{"type":"string","enum":["any","all"],"default":"any"}}), vec!["handles"]),
            output_schema: Some(json!({"type":"object","properties":{"agents":{"type":"array","items":agent_run_schema()},"reason":{"type":"string"},"mode":{"type":"string","enum":["any","all"]}},"required":["agents","reason","mode"]})), safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(), method: "POST", path: "/agents/wait",
        },
        ToolDefinition {
            name: "send_message",
            description: "Deliver a correction or steering message to the current turn. Returns a receipt with state accepted|queued|processed|deferred|unsupported|expired|rejected. accepted means the native session took the message; processed is set when the next assistant reply is recorded. Steering a finished turn returns rejected and suggests followup_task. Never injects arbitrary text into a busy TUI. delivery=live_or_queue (default).",
            input_schema: object_schema(json!({"handle":{"type":"string"},"message":{"type":"string","minLength":1},"task":{"type":"string","minLength":1},"delivery":{"type":"string","enum":["live_or_queue","live","queue"],"default":"live_or_queue"},"idempotency_key":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object","properties":{"message_id":{"type":"string"},"idempotency_key":{"type":"string"},"disposition":{"type":"string","enum":["accepted","queued","processed","deferred","unsupported","expired","rejected"]},"state":{"type":"string","enum":["accepted","queued","processed","deferred","unsupported","expired","rejected"]},"handle":{"type":"string"},"turn_id":{"type":"string"},"result":{"type":["string","null"]},"suggestion":{"type":"string"},"next_cursor":{"type":"integer"},"last_steer":{"type":"object","properties":{"message_id":{"type":"string"},"state":{"type":"string"},"turn_id":{"type":"string"}}}},"required":["message_id","disposition"]})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/send",
        },
        ToolDefinition {
            name: "followup_task",
            description: "Give the same worker a new task as the next turn. Like run_agent, this waits up to 120 seconds and returns the settled turn (status/result). Call wait_agents only if status is still queued or running; a transport timeout still keeps this handle — wait_agents recovers the answer. The handle stays stable; a finished task does not dispose the worker.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"task":{"type":"string","minLength":1},"project_path":{"type":"string"},"idempotency_key":{"type":"string"},"context_policy":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"]},"context_mode":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"],"description":"alias for context_policy"},"selected_history":{"type":"array","items":{"type":"string"}},"model":{"type":"string"},"reasoning":{"type":"string"}}), vec!["handle","task"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/followup",
        },
        ToolDefinition {
            name: "send_agent",
            description: "Compatibility wrapper for followup_task. Prefer followup_task for a new turn or send_message for steering the current turn.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"task":{"type":"string","minLength":1},"message":{"type":"string","minLength":1},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/followup",
        },
        ToolDefinition {
            name: "answer_prompt",
            description: "Answer the approval prompt blocking a worker. Pass handle (agent run) or pane_id (pane this connection controls). Get prompt_id from wait_agents or inspect_agent.",
            input_schema: json!({
                "type":"object",
                "properties":{
                    "handle":{"type":"string"},
                    "pane_id":{"type":"string"},
                    "project_path":{"type":"string"},
                    "prompt_id":{"type":"string"},
                    "choice":{"type":"string"},
                    "allow_broad":{"type":"boolean","default":false}
                },
                "required":["prompt_id","choice"],
                "anyOf":[{"required":["handle"]},{"required":["pane_id"]}]
            }),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/answer",
        },
        ToolDefinition {
            name: "cancel_agent",
            description: "Compatibility wrapper for interrupt_agent. Stops the current turn and keeps the worker handle.",
            input_schema: object_schema(json!({"handle":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/cancel",
        },
        ToolDefinition {
            name: "interrupt_agent",
            description: "Stop the current turn without disposing the worker. The handle stays valid for followup_task. Resolves the worker by handle even when project_path is omitted.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/cancel",
        },
        ToolDefinition {
            name: "list_agents",
            description: "List workers this connection can adopt: managed runs and live UI panes. Each entry has worker_id, backend, workspace, status, model when known, grant_required, and read_only_supported. Pass worker_id to run_agent to adopt. Alias: list_workers.",
            input_schema: object_schema(json!({"project_path":{"type":"string"}}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"workers":{"type":"array","items":{"type":"object","properties":{"worker_id":{"type":"string"},"handle":{"type":"string"},"pane_id":{"type":["string","null"]},"backend":{"type":"string"},"workspace":{"type":"string"},"status":{"type":"string"},"model":{"type":["string","null"]},"owned":{"type":"boolean"},"grant_required":{"type":"boolean"},"read_only_supported":{"type":"boolean"},"adoptable":{"type":"boolean"}}}}},"required":["workers"]})),
            safety: ToolSafety::ReadOnly, visibility: visible_everywhere(), method: "GET", path: "/agents",
        },
        ToolDefinition {
            name: "list_workers",
            description: "Same as list_agents: discover UI panes and managed runs in agent mode, then adopt with run_agent(worker_id, task).",
            input_schema: object_schema(json!({"project_path":{"type":"string"}}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"workers":{"type":"array"}},"required":["workers"]})),
            safety: ToolSafety::ReadOnly, visibility: visible_everywhere(), method: "GET", path: "/agents",
        },
        ToolDefinition {
            name: "inspect_agent",
            description: "Inspect a worker: status, current task, context_policy, requested vs resolved model, capabilities, and turn ids. Pass the handle from run_agent, not a pane id.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(), method: "GET", path: "/agents/{handle}",
        },
        ToolDefinition {
            name: "agent_transcript",
            description: "Read typed transcript events for a worker (user task/steering, assistant commentary and final response, tool activity, prompts, interruption, failure). Truncation never drops the stored turn result — use wait_agents or inspect_agent for that.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"},"after":{"type":"integer","minimum":0},"operation_id":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(), method: "GET", path: "/agents/{handle}/transcript",
        },
        ToolDefinition {
            name: "close_agent",
            description: "Explicitly end a worker session. Cancels the current turn, releases this connection's control, and returns status=closed.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Destructive,
            visibility: visible_everywhere(), method: "POST", path: "/agents/close",
        },
        ToolDefinition {
            name: "take_over",
            description: "Take temporary pane or worker control and return the rendered screen. Pass `handle` (run handle or a list_agents worker_id/pane id) or `pane_id`. A pane not created by this connection requires grant=true. Prefer run_agent(worker_id) to adopt. This does not transfer ownership; use transfer_agent for that.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"},"pane_id":{"type":"string"},"grant":{"type":"boolean","default":false}}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"handle":{"type":"string"},"pane_id":{"type":"string"},"screen":{"type":"string"},"prompt":{},"control":{"type":"string","enum":["agent","shell"]}},"required":["handle","pane_id","screen","control"]})),
            safety: ToolSafety::Mutating, visibility: visible_everywhere(), method: "POST", path: "/agents/take-over",
        },
        ToolDefinition {
            name: "shell_exec",
            description: "Run a shell command in the selected project directory.",
            input_schema: object_schema(json!({"command":{"type":"string","minLength":1},"cwd":{"type":"string"},"pane_id":{"type":"string"},"timeout_ms":{"type":"integer","minimum":1}}), vec!["command"]),
            output_schema: Some(json!({"type":"object","properties":{"stdout":{"type":"string"},"stderr":{"type":"string"},"exit_code":{"type":"integer"}}})),
            safety: ToolSafety::Mutating, visibility: visible_everywhere(), method: "POST", path: "/shell/exec",
        },
        ToolDefinition {
            name: "release",
            description: "Return temporary manual pane control to the worker's owner. Does not drop the worker lease and does not transfer ownership. Use release_lease to drop the lease, or transfer_agent to move it.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"pane_id":{"type":"string"}}), vec![]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/release",
        },
        ToolDefinition {
            name: "session_identity",
            description: "Read this connection's coordinator identity. The connection UUID is a transport id; coordinator_id plus attach_token are the reconnect credentials. A caller-supplied name is not an identity.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"connection_id":{"type":"string"},"coordinator_id":{"type":"string"},"attach_token":{"type":"string"}},"required":["connection_id","coordinator_id","attach_token"]})),
            safety: ToolSafety::ReadOnly, visibility: visible_everywhere(), method: "GET", path: "/mcp/session",
        },
        ToolDefinition {
            name: "attach_agents",
            description: "Re-attach this connection to a coordinator's workers after reconnect. Requires the server-issued attach_token from session_identity. coordinator_name is ignored and never grants authority.",
            input_schema: object_schema(json!({"attach_token":{"type":"string"},"coordinator_name":{"type":"string"}}), vec!["attach_token"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/attach",
        },
        ToolDefinition {
            name: "release_lease",
            description: "Drop this connection's worker control lease. Distinct from release (return pane control) and transfer_agent (move ownership).",
            input_schema: object_schema(json!({"handle":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/release-lease",
        },
        ToolDefinition {
            name: "transfer_agent",
            description: "Transfer worker ownership to another live MCP connection (to_session_id). A coordinator name does not grant authority and is not a valid target.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"to_session_id":{"type":"string"},"coordinator_name":{"type":"string"}}), vec!["handle","to_session_id"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/transfer",
        },
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
            description: "Spawn a worker pane (OpenCode API or PowerShell). Reuse an existing pane of the same agent_type when possible; pass force_new to create another. Never reuse orchestrator panes.",
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
                "acceptance_criteria": { "type": "array", "items": { "type": "string", "minLength": 1 }, "description": "Optional free-text guidance for the worker; NOT machine-checked (a text mismatch never fails a run and acceptance_status stays not_checked). For verified acceptance pass `checks` (file_exists / file_contains / file_absent, evaluated against disk at completion); acceptance_status then reflects the checks." },
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
    fn spawn_agent_accepts_powershell_and_force_new() {
        let spawn = tools()
            .into_iter()
            .find(|tool| tool.name == "spawn_agent")
            .unwrap();
        let agent_enum = spawn
            .input_schema
            .pointer("/properties/agent_type/enum")
            .and_then(|value| value.as_array())
            .expect("agent_type enum");
        let types: Vec<&str> = agent_enum
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        assert!(types.contains(&"opencode_native"));
        assert!(types.contains(&"powershell"));
        assert!(spawn
            .input_schema
            .pointer("/properties/force_new")
            .is_some());
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
    fn operation_tools_expose_contract_fields_and_structured_output_schemas() {
        let registry = tools();
        let delegate = registry
            .iter()
            .find(|tool| tool.name == "delegate_work")
            .unwrap();
        assert!(delegate
            .input_schema
            .pointer("/properties/idempotency_key")
            .is_some());
        assert!(delegate
            .input_schema
            .pointer("/properties/project_path")
            .is_some());
        assert!(delegate.output_schema.is_some());
        let wait = registry
            .iter()
            .find(|tool| tool.name == "wait_for_operation")
            .unwrap();
        assert!(wait
            .input_schema
            .pointer("/properties/after_revision")
            .is_some());
        assert!(wait.output_schema.is_some());
        for name in ["get_operation", "cancel_operation"] {
            assert!(registry
                .iter()
                .find(|tool| tool.name == name)
                .unwrap()
                .output_schema
                .is_some());
        }
        let prepared = registry
            .iter()
            .find(|tool| tool.name == "delegate_task")
            .unwrap();
        assert!(prepared.description.contains("does not launch"));
    }

    #[test]
    fn registry_contains_primary_agent_controls_and_legacy_wrappers() {
        let names = tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        for name in [
            "run_agent",
            "wait_agents",
            "send_message",
            "followup_task",
            "interrupt_agent",
            "inspect_agent",
            "agent_transcript",
            "close_agent",
            "send_agent",
            "cancel_agent",
            "spawn_agent",
        ] {
            assert!(names.contains(&name), "missing {name}");
        }
        let spawn = tools()
            .into_iter()
            .find(|tool| tool.name == "spawn_agent")
            .unwrap();
        assert_eq!(spawn.path, "/panes");
        let send = tools()
            .into_iter()
            .find(|tool| tool.name == "send_message")
            .unwrap();
        let followup = tools()
            .into_iter()
            .find(|tool| tool.name == "followup_task")
            .unwrap();
        let send_agent = tools()
            .into_iter()
            .find(|tool| tool.name == "send_agent")
            .unwrap();
        assert_eq!(send.path, "/agents/send");
        assert_eq!(followup.path, "/agents/followup");
        assert_eq!(send_agent.path, "/agents/followup");
        assert_ne!(send.path, followup.path);
        assert!(followup.description.contains("wait_agents"));
        assert!(followup.description.contains("120"));
        let wait = tools()
            .into_iter()
            .find(|tool| tool.name == "wait_agents")
            .unwrap();
        assert_eq!(
            wait.input_schema["properties"]["timeout_ms"]["default"],
            120000
        );
        assert!(wait.description.contains("until"));
    }

    #[test]
    fn agent_run_output_schema_includes_wait_and_verification_fields() {
        let schema = agent_run_schema();
        let props = schema["properties"]
            .as_object()
            .expect("properties object");
        for key in [
            "result_unchanged",
            "wake_reason",
            "result_verified",
            "model_verified",
            "recovery",
        ] {
            assert!(props.contains_key(key), "missing output field {key}");
        }
    }

    #[test]
    fn agent_run_view_json_includes_required_catalog_fields() {
        use crate::agent_runs::AgentRunView;
        use crate::operations::{OperationSnapshot, OperationStatus, StateSource, WorkerPersist};
        let snapshot = OperationSnapshot {
            operation_id: "op-1".into(),
            runtime_id: "rt".into(),
            request_fingerprint: String::new(),
            project_path: "/tmp".into(),
            task: "task".into(),
            agent_type: "codex".into(),
            pane_id: None,
            pane_created: false,
            idempotency_key: "key".into(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            status: OperationStatus::Completed,
            revision: 2,
            created_at_ms: 0,
            updated_at_ms: 0,
            observed_at_ms: None,
            pane_state: None,
            required_action: None,
            source: StateSource::Native,
            stage: None,
            progress_pct: None,
            result: Some("ok".into()),
            error: None,
            timeout_ms: 900_000,
            read_only: false,
            keep_pane: false,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: "handle".into(),
            turn_index: 0,
            verified: true,
            started_at_ms: None,
            finished_at_ms: Some(1),
            message_baseline_ids: vec![],
            output_baseline: None,
            result_capture: None,
            acceptance_status: Default::default(),
            worker: WorkerPersist {
                requested_model: Some("gpt".into()),
                resolved_model: Some("gpt".into()),
                ..Default::default()
            },
            checks: Vec::new(),
            check_results: Vec::new(),
        };
        let value = serde_json::to_value(AgentRunView::from(snapshot)).unwrap();
        for key in ["handle", "operation_id", "status", "result_verified", "model_verified"] {
            assert!(value.get(key).is_some(), "missing view field {key}");
        }
    }

    #[test]
    fn agent_mode_advertises_worker_controls_without_terminal_tools() {
        let names: Vec<_> = tools_for_mode(McpMode::Agent)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        for name in [
            "run_agent",
            "wait_agents",
            "send_message",
            "followup_task",
            "interrupt_agent",
            "inspect_agent",
            "agent_transcript",
            "close_agent",
            "set_mode",
        ] {
            assert!(names.contains(&name), "agent mode missing {name}");
        }
        assert!(names.contains(&"send_agent"));
        assert!(names.contains(&"cancel_agent"));
        assert!(names.contains(&"list_agents"));
        assert!(names.contains(&"list_workers"));
        assert!(names.contains(&"list_panes"));
        assert!(names.contains(&"reply_opencode_question"));
        assert!(!names.contains(&"spawn_agent"));
        assert!(!names.contains(&"write_terminal_input"));
        assert!(!names.contains(&"press_key"));
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
