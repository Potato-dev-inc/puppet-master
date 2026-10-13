//! Catalog tests.

use super::schema::{agent_run_schema, agent_type_schema};
use super::*;
use serde_json::json;

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
    let pattern = spawn
        .input_schema
        .pointer("/properties/agent_type/pattern")
        .and_then(|value| value.as_str())
        .expect("agent_type pattern");
    let re = regex::Regex::new(pattern).unwrap();
    assert!(re.is_match("opencode_native"));
    assert!(re.is_match("powershell"));
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
    let absent = tools()
        .into_iter()
        .find(|tool| tool.output_schema.is_none())
        .expect("some tool has no output schema");
    let value = serde_json::to_value(absent).unwrap();
    assert!(value.get("outputSchema").is_none());
}

#[test]
fn bridge_health_documents_the_env_section() {
    let health = tools()
        .into_iter()
        .find(|tool| tool.name == "bridge_health")
        .unwrap();
    let schema = health.output_schema.expect("bridge_health output schema");
    for key in ["python", "pytest", "node", "git", "cargo"] {
        assert!(
            schema["properties"]["env"]["properties"].get(key).is_some(),
            "{key}"
        );
    }
    assert!(health.description.contains("env:"));
}

#[test]
fn run_agent_documents_cursor_steering_limits_and_the_shell_hint() {
    let tools = tools();
    let run = tools.iter().find(|tool| tool.name == "run_agent").unwrap();
    assert!(run
        .description
        .contains("Cursor headless steering works only by restart from turn 2"));
    assert!(run.description.contains("queue-only"));
    for name in ["run_agent", "wait_agents", "followup_task"] {
        let tool = tools.iter().find(|tool| tool.name == name).unwrap();
        assert_eq!(
            tool.input_schema["properties"]["shell"]["enum"],
            json!(["bash", "powershell", "cmd"]),
            "{name}"
        );
    }
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
        "quick_ask",
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
    let props = schema["properties"].as_object().expect("properties object");
    for key in [
        "result_unchanged",
        "wake_reason",
        "result_verified",
        "model_verified",
        "recovery",
        "receipt",
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
        placement: None,
        keep_pane_requested: None,
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
        warnings: Vec::new(),
    };
    let value = serde_json::to_value(AgentRunView::from(snapshot)).unwrap();
    for key in [
        "handle",
        "operation_id",
        "status",
        "result_verified",
        "model_verified",
    ] {
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
        "quick_ask",
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
fn check_item_schema_has_no_defaults_that_clients_would_fill_in() {
    // Schema-driven clients inject `default`s into every array item; a filled-in
    // expect_exit/timeout_ms would be rejected by result_contains (deny_unknown_fields).
    let run = tools()
        .into_iter()
        .find(|tool| tool.name == "run_agent")
        .unwrap();
    let items = &run.input_schema["properties"]["checks"]["items"]["properties"];
    for field in ["expect_exit", "timeout_ms", "command", "path", "text"] {
        assert!(
            items.get(field).is_some(),
            "{field} missing from checks schema"
        );
        assert!(
            items[field].get("default").is_none(),
            "{field} must not declare a default"
        );
    }
    let plain: crate::agent_runs::checks::Check =
        serde_json::from_value(serde_json::json!({"type":"result_contains","text":"x"})).unwrap();
    assert!(matches!(
        plain,
        crate::agent_runs::checks::Check::ResultContains { .. }
    ));
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

/// Pins the catalog hash (tool order, names, descriptions, routes, input schemas). Update the
/// constant deliberately when a tool is added or changed; a pure refactor must not move it.
#[test]
fn catalog_version_is_pinned() {
    assert_eq!(catalog_version(), "68-e4687e50f1687299");
}

/// Every tool input that names an agent (`agent_type`, or `backend` on quick_ask) uses the one
/// `agent_type_schema()` output: a pattern-validated string, never an enum of ids.
#[test]
fn agent_type_schema_has_pattern_not_enum() {
    let schema = agent_type_schema();
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["pattern"], "^[a-z][a-z0-9_]{1,31}$");
    assert!(schema.get("enum").is_none());
    assert!(schema["description"]
        .as_str()
        .unwrap()
        .contains("GET /agents/types"));
    let re = regex::Regex::new(schema["pattern"].as_str().unwrap()).unwrap();
    for ok in ["claude", "codex", "cursor", "cursor_agent", "opencode_native", "cmd", "kilo"] {
        assert!(re.is_match(ok), "{ok} should match");
    }
    for bad in ["", "a", "Claude", "1abc", "has-dash", "a_very_long_agent_identifier_over_32"] {
        assert!(!re.is_match(bad), "{bad} should not match");
    }
    let mut seen = Vec::new();
    for tool in tools() {
        for key in ["agent_type", "backend"] {
            if let Some(prop) = tool.input_schema.pointer(&format!("/properties/{key}")) {
                assert_eq!(prop, &schema, "{}.{key} must use agent_type_schema()", tool.name);
                seen.push(tool.name);
            }
        }
    }
    for name in ["quick_ask", "run_agent", "delegate_work", "read_agent_context", "spawn_agent"] {
        assert!(seen.contains(&name), "{name} lost its agent_type/backend property");
    }
}

#[test]
fn visible_run_tools_advertise_placement() {
    for name in ["run_agent", "delegate_work", "spawn_agent"] {
        let tool = tools().into_iter().find(|t| t.name == name).expect(name);
        let schema = tool.input_schema.to_string();
        assert!(schema.contains("\"placement\""), "{name} lacks placement");
        assert!(schema.contains("\"grid\"") && schema.contains("\"window\""), "{name} enum");
        let placement = &tool.input_schema["properties"]["placement"];
        assert_eq!(placement["default"], "window", "{name}: pop-out window is the default placement");
    }
}

#[test]
fn run_agent_is_visible_first_and_describes_the_headless_backup() {
    let tool = tools().into_iter().find(|t| t.name == "run_agent").expect("run_agent");
    let headless = &tool.input_schema["properties"]["headless"];
    assert_eq!(headless["default"], false, "visible is the default");
    assert!(headless["description"].as_str().unwrap().contains("fallback"));
    assert!(tool.description.contains("visible by default") && tool.description.contains("no_visible_mode"));
    let schema = tool.output_schema.as_ref().expect("output schema").to_string();
    assert!(schema.contains("\"fallback\"") && schema.contains("visible_failed"), "{schema}");
}

/// `run_agent` takes one optional `account` (a user account id); it is never required.
#[test]
fn run_agent_advertises_an_optional_account() {
    let tool = tools().into_iter().find(|tool| tool.name == "run_agent").expect("run_agent");
    assert_eq!(tool.input_schema["properties"]["account"]["type"], "string");
    let required = tool.input_schema["required"].as_array().cloned().unwrap_or_default();
    assert!(!required.iter().any(|name| name == "account"));
}

#[test]
fn run_agent_advertises_isolation_and_review_worktree_is_exposed() {
    let run = tools().into_iter().find(|t| t.name == "run_agent").expect("run_agent");
    let isolation = &run.input_schema["properties"]["isolation"];
    assert_eq!(isolation["enum"], json!(["none", "worktree"]));
    assert_eq!(isolation["default"], "none");
    assert!(run.output_schema.as_ref().unwrap().to_string().contains("worktree"), "receipt schema lacks worktree");
    let review = tools().into_iter().find(|t| t.name == "review_worktree").expect("review_worktree");
    assert_eq!(review.input_schema["properties"]["action"]["enum"], json!(["diff", "merge", "discard"]));
    assert_eq!(review.input_schema["required"], json!(["handle"]));
    assert_eq!((review.method, review.path), ("POST", "/worktrees/review"));
    assert!(tool_visible_in_mode("review_worktree", McpMode::Agent));
    assert!(!tool_visible_in_mode("review_worktree", McpMode::Shell));
}
