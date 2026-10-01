use super::*;

#[test]
fn hidden_tools_and_progress_waits_fail_before_reaching_the_bridge() {
    let session = uuid::Uuid::new_v4().to_string();
    set_mcp_session_id(session);
    let hidden = call_tool(json!({"name":"read_terminal_buffer","arguments":{"pane_id":"p1"}}))
        .unwrap_err();
    assert_eq!(
        serde_json::from_str::<Value>(&hidden).unwrap()["code"],
        "MODE_MISMATCH"
    );
    let shell = call_tool(json!({"name":"shell_exec","arguments":{"command":"echo hi"}}))
        .unwrap_err();
    assert_eq!(
        serde_json::from_str::<Value>(&shell).unwrap()["code"],
        "MODE_MISMATCH"
    );
    MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = None);
}

#[test]
fn parses_host_and_port() {
    assert_eq!(
        parse_bridge_endpoint("127.0.0.1:17321\n").unwrap(),
        BridgeEndpoint {
            host: "127.0.0.1".to_string(),
            port: 17321,
        }
    );
}

#[test]
fn parses_port_only() {
    assert_eq!(
        parse_bridge_endpoint("17321").unwrap(),
        BridgeEndpoint {
            host: "127.0.0.1".to_string(),
            port: 17321,
        }
    );
}

#[test]
fn missing_port_file_returns_bridge_down_error() {
    let missing = PathBuf::from("definitely-missing-puppet-master-port-file");
    let err = read_bridge_endpoint_from_candidates(&[missing]).unwrap_err();
    assert!(err.contains("bridge_down"));
}

#[test]
fn initialize_returns_server_info() {
    let response = handle_json_rpc_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        )
        .unwrap();
    let value: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        value
            .pointer("/result/serverInfo/name")
            .and_then(Value::as_str),
        Some(SERVER_NAME)
    );
    let instructions = value
        .pointer("/result/instructions")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(instructions.contains("list_agents"));
    assert!(instructions.contains("followup_task"));
    assert!(instructions.contains("wait_agents"));
}

#[test]
fn protocol_negotiation_never_echoes_unknown_versions() {
    assert_eq!(
        negotiate_protocol_version(Some("future-version")),
        "2024-11-05"
    );
    assert_eq!(negotiate_protocol_version(Some("2025-06-18")), "2025-06-18");
}

#[test]
fn tools_list_contains_bridge_health() {
    let response =
        handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
            .unwrap();
    let value: Value = serde_json::from_str(&response).unwrap();
    let tools = value
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap();
    assert!(tools
        .iter()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some("bridge_health")));
}

#[test]
fn operation_tools_are_discoverable_with_output_schemas() {
    let list = mcp_tools();
    for name in [
        "delegate_work",
        "get_operation",
        "wait_for_operation",
        "cancel_operation",
    ] {
        let tool = list
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
            .unwrap();
        assert!(tool.get("inputSchema").is_some());
        assert_eq!(
            tool.pointer("/outputSchema/type").and_then(Value::as_str),
            Some("object")
        );
    }
}

#[test]
fn tools_list_contains_session_context_tools() {
    let response =
        handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
            .unwrap();
    let value: Value = serde_json::from_str(&response).unwrap();
    let tools = value
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap();
    assert!(tools
        .iter()
        .any(|tool| { tool.get("name").and_then(Value::as_str) == Some("read_session_context") }));
    assert!(tools
        .iter()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some("delegate_task")));
}

#[test]
fn resources_list_contains_session() {
    let response =
        handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#)
            .unwrap();
    let value: Value = serde_json::from_str(&response).unwrap();
    let resources = value
        .pointer("/result/resources")
        .and_then(Value::as_array)
        .unwrap();
    assert!(resources.iter().any(|resource| {
        resource.get("uri").and_then(Value::as_str) == Some("puppet-master://session")
    }));
}

#[test]
fn prompts_list_contains_status_check() {
    let response =
        handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list","params":{}}"#)
            .unwrap();
    let value: Value = serde_json::from_str(&response).unwrap();
    let prompts = value
        .pointer("/result/prompts")
        .and_then(Value::as_array)
        .unwrap();
    assert!(prompts
        .iter()
        .any(|prompt| prompt.get("name").and_then(Value::as_str) == Some("status_check")));
}

#[test]
fn notification_has_no_response() {
    assert!(handle_json_rpc_line(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#
    )
    .is_none());
}

#[test]
fn failed_set_mode_does_not_publish_tool_catalog_change() {
    let (tx, rx) = std::sync::mpsc::channel();
    let response = handle_json_rpc_line_cancellable(
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"set_mode","arguments":{"mode":"invalid"}}}"#,
        None,
        Some(tx),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(value.pointer("/result/isError"), Some(&Value::Bool(true)));
    assert!(rx.try_recv().is_err());
}

#[test]
fn wait_tools_use_extended_bridge_timeout() {
    assert_eq!(bridge_read_timeout_secs("POST", "/panes/wait", None), 135);
    assert_eq!(
        bridge_read_timeout_secs(
            "POST",
            "/panes/wait/model",
            Some(&json!({ "timeout_ms": 300_000 }))
        ),
        315
    );
    assert_eq!(bridge_read_timeout_secs("GET", "/panes", None), 30);
    assert_eq!(
        bridge_read_timeout_secs("POST", "/agents/run", Some(&json!({"wait_ms":120_000}))),
        135
    );
    assert_eq!(
        bridge_read_timeout_secs("POST", "/agents/run", None),
        135
    );
    assert_eq!(
        bridge_read_timeout_secs(
            "POST",
            "/agents/run",
            Some(&json!({"background": true, "wait_ms": 120_000}))
        ),
        30
    );
    assert_eq!(bridge_read_timeout_secs("POST", "/agents/wait", None), 135);
    assert_eq!(bridge_read_timeout_secs("POST", "/agents/followup", None), 135);
    assert_eq!(
        bridge_read_timeout_secs(
            "POST",
            "/agents/followup",
            Some(&json!({"wait_ms": 30_000}))
        ),
        45
    );
}

#[test]
fn wait_timeout_keeps_the_followup_handle() {
    let recovered: Value = serde_json::from_str(&wait_timeout_recovery(
        "/agents/followup",
        &json!({"handle": "investigator", "task": "next"}),
        "bridge read failed: A connection attempt failed (os error 10060)".into(),
    ))
    .unwrap();
    assert_eq!(recovered["code"], "WAIT_TIMEOUT");
    assert_eq!(recovered["handle"], "investigator");
    assert_eq!(recovered["outcome"], "unknown");
    assert_eq!(recovered["context"]["suggestion"], "wait_agents");
    let unrelated = wait_timeout_recovery(
        "/agents/followup",
        &json!({"handle": "investigator"}),
        "bridge_down: connection refused".into(),
    );
    assert!(unrelated.contains("connection refused"));
}

#[test]
fn maps_legacy_http_errors_to_nonrecoverable_or_retryable_codes() {
    let missing = crate::legacy_http_error(404, r#"{"error":"unknown pane"}"#);
    assert_eq!(
        missing.get("code").and_then(Value::as_str),
        Some("NOT_FOUND")
    );
    assert_eq!(
        missing.get("recoverable").and_then(Value::as_bool),
        Some(false)
    );
    let busy = crate::legacy_http_error(409, "conflict");
    assert_eq!(busy.get("code").and_then(Value::as_str), Some("CONFLICT"));
    assert_eq!(
        busy.get("retry_after_ms").and_then(Value::as_u64),
        Some(1000)
    );
    let typed = crate::legacy_http_error(
        500,
        r#"{"code":"OPERATION_NOT_FOUND","message":"gone","recoverable":false}"#,
    );
    assert_eq!(
        typed.get("code").and_then(Value::as_str),
        Some("OPERATION_NOT_FOUND")
    );
    assert_eq!(
        typed.get("recoverable").and_then(Value::as_bool),
        Some(false)
    );
    let legacy_message = crate::legacy_http_error(
        400,
        r#"{"code":"PROJECT_PATH_REQUIRED","error":"project_path is required when no project is selected","recoverable":false}"#,
    );
    assert_eq!(
        legacy_message.get("message").and_then(Value::as_str),
        Some("project_path is required when no project is selected")
    );
}

#[test]
fn typed_tool_errors_copy_error_field_into_message() {
    let detail = tool_error_detail(
        r#"{"code":"PROJECT_PATH_REQUIRED","error":"project_path is required when no project is selected","recoverable":false}"#,
    );
    assert_eq!(
        detail.get("code").and_then(Value::as_str),
        Some("PROJECT_PATH_REQUIRED")
    );
    assert_eq!(
        detail.get("message").and_then(Value::as_str),
        Some("project_path is required when no project is selected")
    );
}

#[test]
fn rejects_orchestrator_pane_targets() {
    assert!(assert_worker_pane("puppet-master-orchestrator-123").is_err());
    assert!(assert_worker_pane("codex-123").is_ok());
}

#[test]
fn starting_mode_argument_wins_over_environment() {
    let args = vec!["--mode".to_string(), "shell".to_string()];
    assert_eq!(
        initial_mode_from(&args, Some("both".to_string())).unwrap(),
        Some(tool_registry::McpMode::Shell)
    );
    let args = vec!["--mode=both".to_string()];
    assert_eq!(
        initial_mode_from(&args, Some("agent".to_string())).unwrap(),
        Some(tool_registry::McpMode::Both)
    );
}

#[test]
fn starting_mode_reads_environment_and_defaults_to_none() {
    assert_eq!(
        initial_mode_from(&[], Some(" Shell ".to_string())).unwrap(),
        Some(tool_registry::McpMode::Shell)
    );
    assert_eq!(initial_mode_from(&[], None).unwrap(), None);
    assert_eq!(initial_mode_from(&[], Some(String::new())).unwrap(), None);
}

#[test]
fn starting_mode_rejects_unknown_values_and_missing_argument() {
    assert!(initial_mode_from(&[], Some("admin".to_string())).is_err());
    assert!(initial_mode_from(&["--mode".to_string()], None).is_err());
    assert!(initial_mode_from(&["--mode=nope".to_string()], None).is_err());
}

#[test]
fn starting_mode_selects_the_local_tool_catalog() {
    let session = uuid::Uuid::new_v4().to_string();
    mcp_sessions::set_mode(&session, "shell").unwrap();
    set_mcp_session_id(session);
    let names: Vec<String> = mcp_tools()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    assert!(names.iter().any(|name| name == "list_panes"));
    assert!(names.iter().any(|name| name == "shell_exec"));
    assert!(names.iter().any(|name| name == "set_mode"));
    assert!(!names.iter().any(|name| name == "run_agent"));
    MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = None);
}

#[test]
fn agent_mode_tools_list_is_local_and_includes_primary_controls() {
    let session = uuid::Uuid::new_v4().to_string();
    mcp_sessions::set_mode(&session, "agent").unwrap();
    set_mcp_session_id(session);
    let names: Vec<String> = mcp_tools()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
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
        "send_agent",
        "cancel_agent",
        "list_agents",
        "list_workers",
        "list_panes",
    ] {
        assert!(names.iter().any(|item| item == name), "missing {name}");
    }
    assert!(!names.iter().any(|name| name == "spawn_agent"));
    assert!(!names.iter().any(|name| name == "press_key"));
    MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = None);
}

#[test]
fn set_mode_reports_whether_hosts_were_notified() {
    let base = || json!({"content":[{"type":"text","text":"{\"mode\":\"both\",\"tool_count\":55}"}],"structuredContent":{"mode":"both","tool_count":55}});
    let mut sent = base();
    annotate_set_mode(&mut sent, true);
    assert_eq!(
        sent.pointer("/structuredContent/tools_list_changed_sent"),
        Some(&Value::Bool(true))
    );
    let note = sent
        .pointer("/structuredContent/note")
        .and_then(Value::as_str)
        .unwrap();
    assert!(note.contains("read tools/list only once"));
    assert!(sent
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap()
        .contains("Note:"));

    let mut unsent = base();
    annotate_set_mode(&mut unsent, false);
    assert_eq!(
        unsent.pointer("/structuredContent/tools_list_changed_sent"),
        Some(&Value::Bool(false))
    );
    assert!(unsent
        .pointer("/structuredContent/note")
        .and_then(Value::as_str)
        .unwrap()
        .contains("could not send"));
}

#[test]
fn operation_timeout_outcome_classifies_dispatch_and_terminal_states() {
    let pending = json!({"status":"running"});
    assert_eq!(operation_timeout_outcome(&pending), "pending");
    let done = json!({"status":"completed", "result":"ok"});
    assert_eq!(operation_timeout_outcome(&done), "completed_delivery_failed");
    let failed_dispatch = json!({"status":"failed", "error":{"code":"DISPATCH_FAILED"}});
    assert_eq!(operation_timeout_outcome(&failed_dispatch), "not_dispatched");
}
