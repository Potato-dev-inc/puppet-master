use super::*;

#[test]
fn shell_hint_is_stripped_from_the_request_and_rewrites_watch_commands() {
    let mut args = json!({"task":"t","shell":"bash","project_path":"C:/a b"});
    let hint = take_shell_hint("run_agent", &mut args)
        .unwrap()
        .expect("hint");
    assert!(
        args.get("shell").is_none(),
        "the bridge must never see the hint"
    );
    let response = json!({
        "operation_id": "op-1",
        "watch_command": "npx puppet-master watch op-1 --project-path 'C:/a b'"
    })
    .to_string();
    let rewritten: Value =
        serde_json::from_str(&apply_shell_hint(&response, hint, Some("C:/a b"))).unwrap();
    let command = rewritten["watch_command"].as_str().unwrap();
    assert!(command.ends_with("--project-path 'C:/a b'"), "{command}");

    let mut other = json!({"shell":"fish"});
    assert!(take_shell_hint("wait_agents", &mut other).is_err());
    let mut untouched = json!({"shell":"bash"});
    assert!(take_shell_hint("list_agents", &mut untouched)
        .unwrap()
        .is_none());
    assert!(untouched.get("shell").is_some());
}

#[test]
fn hidden_tools_and_progress_waits_fail_before_reaching_the_bridge() {
    let session = uuid::Uuid::new_v4().to_string();
    set_mcp_session_id(session);
    let hidden =
        call_tool(json!({"name":"read_terminal_buffer","arguments":{"pane_id":"p1"}})).unwrap_err();
    assert_eq!(
        serde_json::from_str::<Value>(&hidden).unwrap()["code"],
        "MODE_MISMATCH"
    );
    let shell =
        call_tool(json!({"name":"shell_exec","arguments":{"command":"echo hi"}})).unwrap_err();
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
    assert!(instructions.contains("quick_ask"));
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
    assert_eq!(bridge_read_timeout_secs("POST", "/agents/run", None), 135);
    assert_eq!(
        bridge_read_timeout_secs(
            "POST",
            "/agents/run",
            Some(&json!({"background": true, "wait_ms": 120_000}))
        ),
        30
    );
    assert_eq!(bridge_read_timeout_secs("POST", "/agents/quick", None), 105);
    assert_eq!(
        bridge_read_timeout_secs("POST", "/agents/quick", Some(&json!({"wait_ms": 900_000}))),
        135
    );
    assert_eq!(bridge_read_timeout_secs("POST", "/agents/wait", None), 135);
    assert_eq!(
        bridge_read_timeout_secs("POST", "/agents/followup", None),
        135
    );
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
    let missing = super::legacy_http_error(404, r#"{"error":"unknown pane"}"#);
    assert_eq!(
        missing.get("code").and_then(Value::as_str),
        Some("NOT_FOUND")
    );
    assert_eq!(
        missing.get("recoverable").and_then(Value::as_bool),
        Some(false)
    );
    let busy = super::legacy_http_error(409, "conflict");
    assert_eq!(busy.get("code").and_then(Value::as_str), Some("CONFLICT"));
    assert_eq!(
        busy.get("retry_after_ms").and_then(Value::as_u64),
        Some(1000)
    );
    let typed = super::legacy_http_error(
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
    let legacy_message = super::legacy_http_error(
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
        "quick_ask",
        "wait_agents",
        "send_message",
        "followup_task",
        "interrupt_agent",
        "inspect_agent",
        "agent_transcript",
        "close_agent",
        "list_agents",
        "list_panes",
    ] {
        assert!(names.iter().any(|item| item == name), "missing {name}");
    }
    // Aliases are hidden from tools/list (and stay callable; see notices_tests).
    for hidden in ["send_agent", "cancel_agent", "list_workers"] {
        assert!(!names.iter().any(|item| item == hidden), "{hidden} must be unlisted");
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
    assert_eq!(
        operation_timeout_outcome(&done),
        "completed_delivery_failed"
    );
    let failed_dispatch = json!({"status":"failed", "error":{"code":"DISPATCH_FAILED"}});
    assert_eq!(
        operation_timeout_outcome(&failed_dispatch),
        "not_dispatched"
    );
}

#[test]
fn stable_client_id_gives_a_stable_session_id_and_default_is_unique() {
    assert_eq!(
        session_id_from(Some(" my agent/1 ".into())),
        "client-myagent1"
    );
    assert_eq!(
        session_id_from(Some("a".into())),
        session_id_from(Some("a".into()))
    );
    assert_ne!(session_id_from(None), session_id_from(None));
    assert_ne!(
        session_id_from(Some("  ".into())),
        session_id_from(Some("".into()))
    );
}

#[test]
fn stale_port_file_falls_back_to_a_live_bridge_port() {
    let live = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let live_port = live.local_addr().unwrap().port();
    let ok = live_endpoint_or_probe(BridgeEndpoint { host: "127.0.0.1".into(), port: live_port });
    assert_eq!(ok.port, live_port, "a live port is kept as is");
    // A closed port with nothing in 17321..17330 listening returns the original endpoint.
    let dead = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let dead_port = dead.local_addr().unwrap().port();
    drop(dead);
    let got = live_endpoint_or_probe(BridgeEndpoint { host: "127.0.0.1".into(), port: dead_port });
    assert!(got.port == dead_port || (17321..=17330).contains(&got.port));
}

// ---- ask_user: the real shim against an in-process bridge that serves the questions routes ----

/// A loopback "bridge" that answers `/questions/*` with the app's real route handlers and the
/// process-wide question store. Started once; the port file env var points the shim at it.
fn ensure_questions_bridge() {
    use std::io::{Read, Write};
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let file = std::env::temp_dir().join(format!("pm-ask-user-test-{}.port", uuid::Uuid::new_v4()));
        fs::write(&file, format!("127.0.0.1:{port}\n")).unwrap();
        std::env::set_var(BRIDGE_PORT_FILE_ENV, &file);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                std::thread::spawn(move || {
                    let mut raw = Vec::new();
                    let mut chunk = [0_u8; 4096];
                    let (header_end, length) = loop {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => return, // endpoint probe, nothing to serve
                            Ok(n) => raw.extend_from_slice(&chunk[..n]),
                        }
                        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&raw[..pos]).to_ascii_lowercase();
                            let length = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (pos + 4, length);
                        }
                    };
                    while raw.len() < header_end + length {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => raw.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
                    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
                    let (method, path) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
                    let body: Value =
                        serde_json::from_slice(&raw[header_end..]).unwrap_or_else(|_| json!({}));
                    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
                    // The notices tests also need a bridge that answers the worker listing.
                    let reply = if path.starts_with("/team/agent/") {
                        // The Team tools' routes: scripted by `team_tests.rs`.
                        Ok(team_tests::mock(path, &head, &body))
                    } else if method == "GET" && path.starts_with("/agents") {
                        Ok((200, json!({"agents": [], "workers": []})))
                    } else {
                        puppet_master_app_lib::questions::routes::handle(
                            method, &segments, &body, None, false, &|| {},
                        )
                        .unwrap_or_else(|| Err((404, json!({"error":"not found"}))))
                    };
                    let (status, value) = match reply {
                        Ok(r) | Err(r) => r,
                    };
                    let text = value.to_string();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                        text.len()
                    );
                });
            }
        });
    });
}

fn pending_question_id(text: &str) -> Option<String> {
    puppet_master_app_lib::questions::store()
        .list_pending(puppet_master_app_lib::questions::now_ms())
        .into_iter()
        .find(|q| q.question == text)
        .map(|q| q.id)
}

fn wait_for_pending(text: &str) -> String {
    for _ in 0..200 {
        if let Some(id) = pending_question_id(text) {
            return id;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("question {text:?} never reached the store");
}

#[test]
fn ask_user_blocks_until_answered_and_returns_the_choice() {
    ensure_questions_bridge();
    let call = std::thread::spawn(|| {
        call_tool(json!({"name":"ask_user","arguments":{
            "question":"mcp-test: ship it?",
            "choices":[{"id":"yes","label":"Yes"},{"id":"no","label":"No"}],
            "timeout_s":60
        }}))
    });
    let id = wait_for_pending("mcp-test: ship it?");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!call.is_finished(), "the tool call must stay blocked while the question is open");
    puppet_master_app_lib::questions::store()
        .answer(&id, Some("yes"), None, "phone", puppet_master_app_lib::questions::now_ms())
        .unwrap();
    let result = call.join().unwrap().unwrap();
    let structured = &result["structuredContent"];
    assert_eq!(structured["answered"], true);
    assert_eq!(structured["choice_id"], "yes");
    assert_eq!(structured["answered_by"], "phone");
}

#[test]
fn ask_user_returns_free_text_and_reports_dismissal() {
    ensure_questions_bridge();
    let free = std::thread::spawn(|| {
        call_tool(json!({"name":"ask_user","arguments":{"question":"mcp-test: which branch?"}}))
    });
    let id = wait_for_pending("mcp-test: which branch?");
    puppet_master_app_lib::questions::store()
        .answer(&id, None, Some("release/1.2"), "desktop", puppet_master_app_lib::questions::now_ms())
        .unwrap();
    let result = free.join().unwrap().unwrap();
    assert_eq!(result["structuredContent"]["text"], "release/1.2");
    assert_eq!(result["structuredContent"]["answered_by"], "desktop");

    let dismissed = std::thread::spawn(|| {
        call_tool(json!({"name":"ask_user","arguments":{"question":"mcp-test: dismiss me"}}))
    });
    let id = wait_for_pending("mcp-test: dismiss me");
    puppet_master_app_lib::questions::store()
        .dismiss(&id, puppet_master_app_lib::questions::now_ms())
        .unwrap();
    let result = dismissed.join().unwrap().unwrap();
    assert_eq!(result["structuredContent"]["answered"], false);
    assert_eq!(result["structuredContent"]["reason"], "cancelled");
}

#[test]
fn ask_user_times_out_and_cancelling_the_call_withdraws_the_question() {
    ensure_questions_bridge();
    let timed = call_tool(json!({"name":"ask_user","arguments":{"question":"mcp-test: timeout","timeout_s":1}}))
        .unwrap();
    assert_eq!(timed["structuredContent"]["answered"], false);
    assert_eq!(timed["structuredContent"]["reason"], "timeout");

    let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
    let call = std::thread::spawn(move || {
        call_tool_cancellable(
            json!({"name":"ask_user","arguments":{"question":"mcp-test: cancel me","timeout_s":60}}),
            Some(cancel_rx),
        )
    });
    let id = wait_for_pending("mcp-test: cancel me");
    cancel_tx.send(()).unwrap();
    let error = call.join().unwrap().unwrap_err();
    assert!(error.contains("REQUEST_CANCELLED"), "{error}");
    // Cancelling the call removed the question from the Inbox.
    assert!(pending_question_id("mcp-test: cancel me").is_none(), "{id} should be withdrawn");
}

#[test]
fn the_local_secret_beside_the_port_file_is_sent_with_every_request() {
    let dir = std::env::temp_dir().join(format!("pm-shim-secret-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let port_file = dir.join("puppet-master.bridge.port");
    fs::write(&port_file, "127.0.0.1:17321\n").unwrap();
    // An older app writes no secret file: no header (the bridge only logs that, for now).
    assert_eq!(local_secret_header_from_candidates(&[port_file.clone()]), "");
    fs::write(dir.join("puppet-master.bridge.secret"), "ab12cd34\n").unwrap();
    assert_eq!(
        local_secret_header_from_candidates(&[dir.join("missing.port"), port_file.clone()]),
        "X-PM-Local-Secret: ab12cd34\r\n"
    );
    // Anything that is not hex is not sent (it would be a header injection).
    fs::write(dir.join("puppet-master.bridge.secret"), "ab\r\nX-Evil: 1").unwrap();
    assert_eq!(local_secret_header_from_candidates(&[port_file]), "");
    let _ = fs::remove_dir_all(dir);
}

#[path = "notices_tests.rs"]
mod notices_tests;

#[path = "team_tests.rs"]
mod team_tests;

#[test]
fn a_run_under_a_budget_names_itself_as_the_parent_of_the_helpers_it_starts() {
    let mut body = json!({"task": "t"});
    inject_budget_parent(&mut body, Some(" run-7 ".into()));
    assert_eq!(body["budget_parent"], "run-7");
    // No budget around the agent: the body is untouched (today's behaviour).
    let mut plain = json!({"task": "t"});
    inject_budget_parent(&mut plain, None);
    inject_budget_parent(&mut plain, Some("  ".into()));
    assert!(plain.get("budget_parent").is_none());
    // What the caller wrote is kept.
    let mut explicit = json!({"task": "t", "budget_parent": "mine"});
    inject_budget_parent(&mut explicit, Some("run-7".into()));
    assert_eq!(explicit["budget_parent"], "mine");
}
