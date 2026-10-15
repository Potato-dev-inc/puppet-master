//! Shim tests for the listed surface (aliases hidden but accepted) and the version-mismatch
//! warning. Tool calls run against the in-process mock bridge from `tests.rs`; the `/health` body
//! the drift check sees is set per test through `TEST_HEALTH`, so no test touches a real app.

use super::*;

const OLD_CATALOG: &str = "68-e4687e50f1687299";
const FIX: &str = "Rebuild or reinstall the Puppet Master MCP, then reconnect";

fn start_session(mode: &str) {
    let session = uuid::Uuid::new_v4().to_string();
    mcp_sessions::set_mode(&session, mode).unwrap();
    set_mcp_session_id(session);
    TEST_HEALTH.with(|health| *health.borrow_mut() = None);
}

fn end_session() {
    MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = None);
    TEST_HEALTH.with(|health| *health.borrow_mut() = None);
}

fn set_health(catalog_version: &str) {
    let body = json!({"ok": true, "build_id": "build-under-test", "catalog_version": catalog_version});
    TEST_HEALTH.with(|health| *health.borrow_mut() = Some(body));
}

fn rpc(method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    serde_json::from_str(&handle_json_rpc_line(&line).expect("a response")).unwrap()
}

fn listed(mode: &str) -> Vec<String> {
    start_session(mode);
    let response = rpc("tools/list", json!({}));
    end_session();
    response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

fn call(name: &str, arguments: Value) -> Value {
    rpc("tools/call", json!({"name": name, "arguments": arguments}))["result"].clone()
}

fn error_code(result: &Value) -> &str {
    result["structuredContent"]["code"].as_str().unwrap_or_default()
}

// ---- hidden aliases are still accepted ----

#[test]
fn tools_list_size_per_mode_over_json_rpc() {
    assert_eq!(listed("agent").len(), 22);
    assert_eq!(listed("shell").len(), 16);
    assert_eq!(listed("both").len(), 32);
    assert_eq!(listed("all").len(), 68);
}

#[test]
fn tools_list_never_shows_an_alias_except_in_all_mode() {
    for mode in ["agent", "shell", "both"] {
        let names = listed(mode);
        for (alias, _) in tool_registry::ALIASES {
            assert!(!names.iter().any(|name| name == alias), "{alias} listed in {mode}");
        }
    }
    let all = listed("all");
    for (alias, _) in tool_registry::ALIASES {
        assert!(all.iter().any(|name| name == alias), "{alias} missing from all");
    }
}

#[test]
fn a_hidden_alias_is_still_accepted_and_marked_as_an_alias() {
    ensure_questions_bridge();
    start_session("agent");
    assert!(!listed("agent").iter().any(|name| name == "list_workers"));
    start_session("agent");
    let workers = call("list_workers", json!({}));
    assert_ne!(workers["isError"], json!(true), "{workers}");
    assert_eq!(workers["structuredContent"]["alias_of"], "list_agents");
    let agents = call("list_agents", json!({}));
    assert_ne!(agents["isError"], json!(true), "{agents}");
    assert!(agents["structuredContent"].get("alias_of").is_none());
    end_session();
}

#[test]
fn hidden_aliases_pass_the_mode_gate_exactly_where_they_always_did() {
    ensure_questions_bridge();
    // Agent mode: the three wrappers get past the gate (the mock bridge then has no such route).
    start_session("agent");
    for alias in ["list_workers", "send_agent", "cancel_agent"] {
        let result = call(alias, json!({"handle":"h","task":"t"}));
        assert_ne!(error_code(&result), "MODE_MISMATCH", "{alias}: {result}");
    }
    // Agent mode still refuses a shell-only alias, as before.
    let refused = call("wait_for_model", json!({}));
    assert_eq!(error_code(&refused), "MODE_MISMATCH");
    end_session();
    // Shell mode: the two wait presets get past the gate.
    start_session("shell");
    for alias in ["wait_for_model", "wait_for_worker"] {
        let result = call(alias, json!({}));
        assert_ne!(error_code(&result), "MODE_MISMATCH", "{alias}: {result}");
    }
    end_session();
    // Both and all can call every alias.
    for mode in ["both", "all"] {
        start_session(mode);
        for (alias, _) in tool_registry::ALIASES {
            assert!(ensure_tool_mode(alias).is_ok(), "{alias} in {mode}");
        }
        end_session();
    }
}

#[test]
fn set_mode_all_makes_the_full_list_appear_for_the_same_connection() {
    start_session("agent");
    assert!(rpc("tools/list", json!({}))["result"]["tools"].as_array().unwrap().len() < 40);
    let session = current_mcp_session_id().unwrap();
    mcp_sessions::set_mode(&session, "all").unwrap();
    assert_eq!(rpc("tools/list", json!({}))["result"]["tools"].as_array().unwrap().len(), 68);
    end_session();
}

#[test]
fn starting_mode_accepts_all() {
    assert_eq!(
        initial_mode_from(&["--mode".to_string(), "all".to_string()], None).unwrap(),
        Some(tool_registry::McpMode::All)
    );
}

// ---- the version-mismatch warning ----

#[test]
fn drift_message_names_both_catalogs_the_build_and_the_fix() {
    let health = json!({"catalog_version": OLD_CATALOG, "build_id": "old-build"});
    let drift = schema_drift_from_health(&health).expect("versions differ");
    let message = drift["message"].as_str().unwrap();
    assert_eq!(drift["code"], "SCHEMA_DRIFT");
    assert!(message.contains(OLD_CATALOG));
    assert!(message.contains(&tool_registry::catalog_version()));
    assert!(message.contains("old-build"));
    assert!(message.contains(FIX), "{message}");
    assert_eq!(drift["bridge_catalog_version"], OLD_CATALOG);
    assert_eq!(drift["shim_catalog_version"], tool_registry::catalog_version());
    assert!(schema_drift_from_health(&json!({"catalog_version": tool_registry::catalog_version()})).is_none());
    assert!(schema_drift_from_health(&json!({"ok": true})).is_none());
}

#[test]
fn every_tool_result_carries_the_warning_while_the_versions_differ() {
    ensure_questions_bridge();
    start_session("agent");
    set_health(OLD_CATALOG);
    let result = call("list_agents", json!({}));
    assert_ne!(result["isError"], json!(true), "{result}");
    let blocks = result["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2, "{result}");
    // The tool's own JSON stays first and parseable; the warning is its own last block.
    assert!(serde_json::from_str::<Value>(blocks[0]["text"].as_str().unwrap()).is_ok());
    let warning = blocks[1]["text"].as_str().unwrap();
    assert!(warning.starts_with("WARNING [SCHEMA_DRIFT]"), "{warning}");
    assert!(warning.contains(FIX), "{warning}");
    let structured = &result["structuredContent"]["puppet_master_warnings"][0];
    assert_eq!(structured["code"], "SCHEMA_DRIFT");
    assert_eq!(structured["fix"], "Rebuild or reinstall the Puppet Master MCP, then reconnect (Claude Code: /mcp).");
    assert_eq!(structured["bridge_catalog_version"], OLD_CATALOG);
    assert_eq!(structured["shim_catalog_version"], tool_registry::catalog_version());
    // A second, different tool carries it too (not only the first call).
    let again = call("list_panes", json!({}));
    assert!(again["content"].as_array().unwrap().last().unwrap()["text"]
        .as_str()
        .unwrap()
        .contains("SCHEMA_DRIFT"));
    end_session();
}

#[test]
fn error_results_carry_the_warning_too() {
    start_session("agent");
    set_health(OLD_CATALOG);
    let refused = call("shell_exec", json!({"command":"echo hi"}));
    assert_eq!(refused["isError"], json!(true));
    assert_eq!(error_code(&refused), "MODE_MISMATCH");
    let last = refused["content"].as_array().unwrap().last().unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(last.contains("SCHEMA_DRIFT") && last.contains(FIX), "{last}");
    assert_eq!(refused["structuredContent"]["puppet_master_warnings"][0]["code"], "SCHEMA_DRIFT");
    end_session();
}

#[test]
fn matching_versions_and_an_unreachable_bridge_add_no_warning() {
    ensure_questions_bridge();
    start_session("agent");
    set_health(&tool_registry::catalog_version());
    let result = call("list_agents", json!({}));
    assert_eq!(result["content"].as_array().unwrap().len(), 1, "{result}");
    assert!(result["structuredContent"].get("puppet_master_warnings").is_none());
    // No health answer at all (bridge down): say nothing rather than guess.
    TEST_HEALTH.with(|health| *health.borrow_mut() = None);
    let result = call("list_agents", json!({}));
    assert_eq!(result["content"].as_array().unwrap().len(), 1, "{result}");
    end_session();
}

#[test]
fn the_first_thing_a_client_reads_leads_with_the_warning() {
    start_session("agent");
    set_health(OLD_CATALOG);
    let init = rpc("initialize", json!({"protocolVersion":"2025-06-18"}));
    let text = init["result"]["instructions"].as_str().unwrap();
    assert!(text.starts_with("WARNING [SCHEMA_DRIFT]"), "{text}");
    assert!(text.contains(FIX));
    assert!(text.contains("quick_ask("), "the happy path must still follow");
    set_health(&tool_registry::catalog_version());
    let init = rpc("initialize", json!({"protocolVersion":"2025-06-18"}));
    assert!(init["result"]["instructions"].as_str().unwrap().starts_with("Puppet Master MCP"));
    end_session();
}

#[test]
fn drift_check_asks_the_bridge_once_per_minute_and_never_caches_silence() {
    let mut cache = DriftCache::new();
    let t0 = std::time::Instant::now();
    let drift = json!({"code": "SCHEMA_DRIFT"});
    let mut asked = 0;
    let mut ask = |verdict: Option<Option<Value>>, at: std::time::Instant, cache: &mut DriftCache| {
        cache.lookup(at, || {
            asked += 1;
            verdict
        })
    };
    // First call asks and remembers the drift.
    assert_eq!(ask(Some(Some(drift.clone())), t0, &mut cache), Some(drift.clone()));
    // Within the minute the answer is served from memory (the closure is not even consulted).
    assert_eq!(ask(Some(None), t0 + Duration::from_secs(30), &mut cache), Some(drift.clone()));
    // After the minute it asks again and picks up a fix.
    assert_eq!(ask(Some(None), t0 + Duration::from_secs(61), &mut cache), None);
    assert_eq!(asked, 2);
    // A bridge that cannot answer is not remembered as "fine": the next call asks again.
    let mut cache = DriftCache::new();
    assert_eq!(cache.lookup(t0, || None), None);
    assert_eq!(cache.lookup(t0, || Some(Some(drift.clone()))), Some(drift));
}
