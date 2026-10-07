use serde_json::json;

use super::approvals::{
    explicit_manual_approval_required, native_status_unavailable, startup_manual_approval_required,
};
use super::errors::operation_http_error;
use super::http::{query_param, requested_project_path};
use super::locks::parse_operation_lock;
use super::pane_util::{pane_buffer_view, register_spawned_pane};
use super::routes::mcp::registry_route as mcp_registry_route;
use super::tool_names::bridge_tool_name;

#[test]
fn a_trust_dialog_on_screen_blocks_startup_dispatch_whatever_the_status_says() {
    // Real captures of a visible cursor_agent pane. Right after the dialog is drawn the pane
    // status still reads `running`; typing the task then hits the dialog's hotkeys.
    let pending = include_str!("../tui/cursor/fixtures/workspace_trust_visible_pending.txt");
    let resolved = include_str!("../tui/cursor/fixtures/workspace_trust_visible_resolved.txt");
    assert!(startup_manual_approval_required(
        "cursor_agent",
        Some("running"),
        pending,
        &[]
    ));
    assert!(!startup_manual_approval_required(
        "cursor_agent",
        Some("running"),
        resolved,
        &[]
    ));
    assert!(!startup_manual_approval_required(
        "cursor_agent",
        Some("idle"),
        "Cursor Agent
→ Plan, search, build anything
",
        &[]
    ));
}

#[test]
fn pane_buffer_default_follows_the_pane_type_and_explicit_views_override_it() {
    assert_eq!(pane_buffer_view(None, "codex"), Ok("screen"));
    assert_eq!(pane_buffer_view(None, "bash"), Ok("scrollback"));
    assert_eq!(
        pane_buffer_view(Some("scrollback"), "codex"),
        Ok("scrollback")
    );
    assert_eq!(pane_buffer_view(Some("screen"), "powershell"), Ok("screen"));
    assert!(pane_buffer_view(Some("raw"), "codex").is_err());
}

#[test]
fn spawned_shells_register_as_shell_panes_for_the_request_session() {
    let session = uuid::Uuid::new_v4().to_string();
    let pane_id = uuid::Uuid::new_v4().to_string();
    register_spawned_pane(Some(&session), "powershell", &pane_id).unwrap();
    crate::mcp_sessions::set_mode(&session, "shell").unwrap();
    assert!(crate::mcp_sessions::authorize(
        Some(&session),
        "POST",
        &format!("/panes/{pane_id}/input"),
        &json!({"text":"Get-Location"})
    )
    .is_ok());
}

#[test]
fn spawned_agents_register_as_agent_panes_for_the_request_session() {
    let session = uuid::Uuid::new_v4().to_string();
    let pane_id = uuid::Uuid::new_v4().to_string();
    register_spawned_pane(Some(&session), "codex", &pane_id).unwrap();
    crate::mcp_sessions::set_mode(&session, "shell").unwrap();
    assert!(crate::mcp_sessions::authorize(
        Some(&session),
        "POST",
        &format!("/panes/{pane_id}/input"),
        &json!({"text":"unexpected takeover"})
    )
    .is_err());
}

#[test]
fn mcp_resources_route_returns_session_resource() {
    let (status, value) = mcp_registry_route("GET", &["mcp", "resources"]).unwrap();
    assert_eq!(status, 200);
    let resources = value.as_array().unwrap();
    assert!(resources.iter().any(|resource| {
        resource.get("uri").and_then(serde_json::Value::as_str) == Some("puppet-master://session")
    }));
}

#[test]
fn mcp_prompts_route_returns_status_check_prompt() {
    let (status, value) = mcp_registry_route("GET", &["mcp", "prompts"]).unwrap();
    assert_eq!(status, 200);
    let prompts = value.as_array().unwrap();
    assert!(prompts.iter().any(|prompt| {
        prompt.get("name").and_then(serde_json::Value::as_str) == Some("status_check")
    }));
}

#[test]
fn bridge_tool_names_include_session_context_routes() {
    assert_eq!(
        bridge_tool_name("GET", &["session", "context"]).as_deref(),
        Some("read_session_context")
    );
    assert_eq!(
        bridge_tool_name("PATCH", &["session", "context"]).as_deref(),
        Some("update_session_context")
    );
    assert_eq!(
        bridge_tool_name("POST", &["panes", "pane-1", "role"]).as_deref(),
        Some("set_pane_role")
    );
    assert_eq!(
        bridge_tool_name("POST", &["delegate-task"]).as_deref(),
        Some("delegate_task")
    );
    assert_eq!(
        bridge_tool_name("POST", &["operations", "delegate"]).as_deref(),
        Some("delegate_work")
    );
    assert_eq!(
        bridge_tool_name("GET", &["operations", "op-1"]).as_deref(),
        Some("get_operation")
    );
    assert_eq!(
        bridge_tool_name("POST", &["operations", "op-1", "wait"]).as_deref(),
        Some("wait_for_operation")
    );
    assert_eq!(
        bridge_tool_name("POST", &["operations", "op-1", "cancel"]).as_deref(),
        Some("cancel_operation")
    );
}

#[test]
fn operation_lock_contract_requires_typed_nonempty_resources() {
    assert_eq!(
        parse_operation_lock("file:src/main.rs").unwrap(),
        ("file".into(), "src/main.rs".into())
    );
    assert_eq!(
        parse_operation_lock(" :name ").unwrap_err().code,
        "INVALID_LOCK"
    );
    assert_eq!(
        parse_operation_lock("missing-type").unwrap_err().code,
        "INVALID_LOCK"
    );
}

#[test]
fn readiness_distinguishes_normal_agent_prompt_from_real_permission() {
    assert!(!explicit_manual_approval_required(
        "codex",
        "How can I help? >",
        &[]
    ));
    assert!(explicit_manual_approval_required(
        "codex",
        "Allow this command to run? (y/n)",
        &[],
    ));
    assert!(explicit_manual_approval_required(
        "opencode_native",
        "Waiting for input",
        &["permission-123".into()],
    ));
}

#[test]
fn native_pending_permission_blocks_startup_even_when_pane_looks_idle() {
    assert!(startup_manual_approval_required(
        "opencode_native",
        Some("idle"),
        "",
        &["permission-123".into()],
    ));
    assert!(!startup_manual_approval_required(
        "codex",
        Some("idle"),
        "Allow this command?",
        &[],
    ));
    assert!(startup_manual_approval_required(
        "codex",
        Some("waiting_input"),
        "Allow this command?",
        &[],
    ));
}

#[test]
fn operation_errors_use_the_structured_retry_contract() {
    let mut error = crate::operations::OperationError::new("RESOURCE_LOCKED", "busy", true);
    error.retry_after_ms = Some(750);
    error.context = json!({"resource_id":"file:src/main.rs"});
    let (status, body) = operation_http_error(error);
    assert_eq!(status, 409);
    assert_eq!(body["code"], "RESOURCE_LOCKED");
    assert_eq!(body["message"], "busy");
    assert_eq!(body["error"], "busy");
    assert_eq!(body["recoverable"], true);
    assert_eq!(body["retry_after_ms"], 750);
    assert_eq!(body["context"]["resource_id"], "file:src/main.rs");
    for (code, expected_status) in [
        ("IDEMPOTENCY_KEY_CONFLICT", 409),
        ("PANE_UNAVAILABLE", 409),
        ("INVALID_AGENT_TYPE", 400),
        ("UNKNOWN_AGENT_TYPE", 400),
    ] {
        let (status, body) = operation_http_error(crate::operations::OperationError::new(
            code, "invalid", false,
        ));
        assert_eq!(status, expected_status, "wrong mapping for {code}");
        assert_eq!(body["code"], code);
    }
}

#[test]
fn native_permission_status_failure_is_recoverable_and_fails_closed() {
    let error = native_status_unavailable("worker API is unreachable");
    assert_eq!(error.code, "NATIVE_STATUS_UNAVAILABLE");
    assert!(error.recoverable);
    let (status, body) = operation_http_error(error);
    assert_eq!(status, 503);
    assert_eq!(body["code"], "NATIVE_STATUS_UNAVAILABLE");
    assert_eq!(body["recoverable"], true);
}

#[test]
fn query_parameters_decode_url_encoded_paths_and_reject_malformed_values() {
    assert_eq!(
        query_param(
            "project_path=C%3A%2FUsers%2FDev%20Folder%2Fapp",
            "project_path"
        ),
        Some("C:/Users/Dev Folder/app".into())
    );
    assert_eq!(query_param("project_path=%ZZ", "project_path"), None);
    assert_eq!(query_param("other=value", "project_path"), None);
}

#[test]
fn wait_project_path_prefers_body_then_query() {
    assert_eq!(
        requested_project_path(Some("C:/body"), "project_path=C%3A%2Fquery"),
        Some("C:/body".into())
    );
    assert_eq!(
        requested_project_path(None, "project_path=C%3A%2Fquery"),
        Some("C:/query".into())
    );
    assert_eq!(requested_project_path(Some("  "), ""), None);
}

fn assert_unknown_agent_type_response(status: u16, body: &serde_json::Value) {
    assert_eq!(status, 400);
    assert_eq!(body["code"], "UNKNOWN_AGENT_TYPE");
    assert_eq!(body["recoverable"], false);
    assert_eq!(body["context"]["hint"], "GET /agents/types");
    let valid: Vec<String> = body["context"]["valid"]
        .as_array()
        .expect("valid ids")
        .iter()
        .map(|value| value.as_str().expect("id").to_string())
        .collect();
    assert_eq!(valid.len(), 9);
    assert_eq!(valid, crate::tui::registered_agent_ids());
}

#[test]
fn run_agent_unknown_type_returns_unknown_agent_type() {
    let snapshot = crate::operations::OperationSnapshot {
        operation_id: uuid::Uuid::new_v4().to_string(),
        runtime_id: String::new(),
        request_fingerprint: String::new(),
        project_path: "C:/proj".into(),
        task: "work".into(),
        agent_type: "not-a-real-agent".into(),
        pane_id: None,
        pane_created: false,
        idempotency_key: "key".into(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        status: crate::operations::OperationStatus::Queued,
        revision: 1,
        created_at_ms: 0,
        updated_at_ms: 0,
        observed_at_ms: None,
        pane_state: None,
        required_action: None,
        source: crate::operations::StateSource::Native,
        stage: None,
        progress_pct: None,
        result: None,
        error: None,
        timeout_ms: 60_000,
        read_only: false,
        keep_pane: false,
        keep_pane_requested: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: "run".into(),
        turn_index: 0,
        verified: false,
        started_at_ms: None,
        finished_at_ms: None,
        message_baseline_ids: vec![],
        output_baseline: None,
        result_capture: None,
        acceptance_status: crate::operations::AcceptanceStatus::NotChecked,
        worker: Default::default(),
        checks: Vec::new(),
        check_results: Vec::new(),
        warnings: Vec::new(),
    };
    let error =
        super::dispatch::check_agent_dispatchable(&snapshot, "not-a-real-agent").unwrap_err();
    let (status, body) = operation_http_error(error);
    assert_unknown_agent_type_response(status, &body);
}

#[test]
fn spawn_unknown_type_returns_unknown_agent_type() {
    let unknown = match crate::tui::require_registered("bogus-agent") {
        Err(err) => err,
        Ok(_) => panic!("expected unknown agent type"),
    };
    let (status, body) = operation_http_error(
        crate::operations::OperationError::unknown_agent_type(unknown.id, unknown.valid),
    );
    assert_unknown_agent_type_response(status, &body);
}

#[test]
fn delegate_work_unknown_type_keeps_invalid_agent_type() {
    let dir = std::env::temp_dir().join(format!("pm-delegate-unknown-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let req = crate::operations::DelegateWorkRequest {
        project_path: dir.to_string_lossy().into_owned(),
        task: "do work".into(),
        agent_type: "not-a-real-agent".into(),
        pane_id: None,
        idempotency_key: "delegate-unknown".into(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let error = crate::operations::create_operation(req).unwrap_err();
    assert_eq!(error.code, "INVALID_AGENT_TYPE");
    assert_eq!(error.message, "agent_type is not supported");
    let (status, body) = operation_http_error(error);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "INVALID_AGENT_TYPE");
    assert_eq!(body["message"], "agent_type is not supported");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn agents_types_lists_every_code_profile() {
    let (status, body) = mcp_registry_route("GET", &["agents", "types"]).expect("route exists");
    assert_eq!(status, 200);
    let agents = body["agents"].as_array().expect("agents array");
    let ids: Vec<&str> = agents
        .iter()
        .map(|agent| agent["id"].as_str().expect("id"))
        .collect();
    let expected: Vec<&str> = crate::tui::profiles().iter().map(|p| p.id()).collect();
    assert_eq!(ids, expected, "registry order");
    for agent in agents {
        assert!(agent["installed"].is_boolean(), "{agent}");
        assert!(agent["modes"]["visible"].is_boolean(), "{agent}");
        assert!(agent["modes"]["headless"].is_boolean(), "{agent}");
        assert!(agent["features"]["read_only_headless"].is_boolean(), "{agent}");
        assert!(agent["run_options"]["allow_shell"].is_boolean(), "{agent}");
        assert!(agent["standby"]["visible"].is_boolean(), "{agent}");
        assert!(agent["label"].is_string(), "{agent}");
    }
    let dispatchable: Vec<&str> = body["dispatchable_ids"]
        .as_array()
        .expect("dispatchable_ids")
        .iter()
        .map(|id| id.as_str().expect("id"))
        .collect();
    assert_eq!(dispatchable, vec!["claude", "codex", "opencode", "opencode_native", "cursor_agent"]);
}
