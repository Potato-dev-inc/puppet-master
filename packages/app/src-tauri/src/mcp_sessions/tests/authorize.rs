//! Session tests: authorize.

use super::*;

#[test]
fn wrong_mode_calls_are_rejected_even_if_client_guesses_route() {
    let session = id();
    let error = authorize(
        Some(&session),
        "POST",
        "/panes/p1/input",
        &serde_json::json!({"text":"x"}),
    )
    .unwrap_err();
    assert_eq!(error.code, "MODE_MISMATCH");
    assert_eq!(
        error
            .context
            .pointer("/switch_with/mode")
            .and_then(Value::as_str),
        Some("shell")
    );
    set_mode(&session, "shell").unwrap();
    assert!(authorize(Some(&session), "POST", "/agents/run", &Value::Null).is_err());
    assert!(authorize(Some(&session), "POST", "/agents/quick", &Value::Null).is_err());
    let shell_id = format!("shell-{}", Uuid::new_v4());
    register_shell_pane(&session, &shell_id).unwrap();
    assert!(authorize(
        Some(&session),
        "POST",
        "/shell/exec",
        &serde_json::json!({"command":"ls","pane_id":shell_id})
    )
    .is_ok());
    assert!(authorize(
        Some(&session),
        "POST",
        "/shell/exec",
        &serde_json::json!({"command":"ls","cwd":"C:/project"})
    )
    .is_ok());
}

#[test]
fn managed_agent_pane_needs_explicit_shell_takeover_and_other_sessions_need_grant() {
    let owner = id();
    let other = id();
    register_owned_pane(&owner, "agent-pane-1").unwrap();
    set_mode(&owner, "shell").unwrap();
    let write = || {
        authorize(
            Some(&owner),
            "POST",
            "/panes/agent-pane-1/input",
            &serde_json::json!({"text":"x"}),
        )
    };
    assert!(write().is_err());
    take_over_pane(&owner, "agent-pane-1", true).unwrap();
    assert!(write().is_ok());
    assert!(take_over_pane(&other, "agent-pane-1", false).is_err());
    take_over_pane(&other, "agent-pane-1", true).unwrap();
    assert!(session_controls_pane(&other, "agent-pane-1"));
    assert!(!session_controls_pane(&owner, "agent-pane-1"));
}

#[test]
fn shell_panes_are_not_mistaken_for_read_only_agent_panes() {
    let shell = id();
    register_shell_pane(&shell, "shell-pane").unwrap();
    set_mode(&shell, "shell").unwrap();
    assert!(authorize(
        Some(&shell),
        "POST",
        "/panes/shell-pane/input",
        &serde_json::json!({"text":"ls"})
    )
    .is_ok());
}

#[test]
fn shell_exec_targeting_agent_pane_requires_takeover() {
    let owner = id();
    register_owned_pane(&owner, "agent-pane-shell-exec").unwrap();
    set_mode(&owner, "shell").unwrap();
    let args = serde_json::json!({"command":"pytest","pane_id":"agent-pane-shell-exec"});
    assert!(authorize(Some(&owner), "POST", "/shell/exec", &args).is_err());
    take_over_pane(&owner, "agent-pane-shell-exec", true).unwrap();
    assert!(authorize(Some(&owner), "POST", "/shell/exec", &args).is_ok());
}

#[test]
fn missing_session_cannot_mutate_a_managed_pane_through_key_or_shell_routes() {
    let owner = id();
    register_owned_pane(&owner, "agent-pane-no-header").unwrap();
    assert!(authorize(
        None,
        "POST",
        "/panes/agent-pane-no-header/key",
        &serde_json::json!({"key":"enter"})
    )
    .is_err());
    assert!(authorize(
        None,
        "POST",
        "/shell/exec",
        &serde_json::json!({"command":"ls","pane_id":"agent-pane-no-header"})
    )
    .is_err());
}

#[test]
fn missing_session_cannot_use_agent_apis_even_for_unowned_handles() {
    for (method, path, body) in [
        ("GET", "/agents", Value::Null),
        ("POST", "/agents/run", serde_json::json!({"task":"work"})),
        ("POST", "/agents/quick", serde_json::json!({"task":"work"})),
        (
            "POST",
            "/agents/cancel",
            serde_json::json!({"handle":"unknown-run"}),
        ),
    ] {
        let error = authorize(None, method, path, &body).unwrap_err();
        assert_eq!(error.code, "SESSION_REQUIRED");
    }
}

#[test]
fn missing_session_may_read_the_agent_type_list_only() {
    assert!(authorize(None, "GET", "/agents/types", &Value::Null).is_ok());
    assert!(authorize(None, "GET", "/agents/types?x=1", &Value::Null).is_ok());
    let error = authorize(None, "POST", "/agents/types", &Value::Null).unwrap_err();
    assert_eq!(error.code, "SESSION_REQUIRED");
}

#[test]
fn readonly_operation_wait_allowed_without_session_for_managed_runs() {
    let owner = id();
    register_owned_agent(&owner, "managed-op").unwrap();
    assert!(authorize(
        None,
        "POST",
        "/operations/managed-op/wait",
        &serde_json::json!({"timeout_ms": 1000}),
    )
    .is_ok());
    assert!(authorize(
        None,
        "GET",
        "/operations/managed-op",
        &Value::Null,
    )
    .is_ok());
}

#[test]
fn wait_agents_checks_every_handle_in_the_batch() {
    let owner = id();
    let other = id();
    register_owned_agent(&owner, "owner-run").unwrap();
    register_owned_agent(&other, "other-run").unwrap();
    let own = serde_json::json!({"handles":["owner-run"],"timeout_ms":1});
    let mixed = serde_json::json!({"handles":["owner-run","other-run"],"timeout_ms":1});
    assert!(authorize(Some(&owner), "POST", "/agents/wait", &own).is_ok());
    assert!(authorize(Some(&owner), "POST", "/agents/wait", &mixed).is_err());
}

#[test]
fn run_agent_with_unadopted_pane_id_passes_authorize() {
    let session = id();
    set_mode(&session, "agent").unwrap();
    let body = serde_json::json!({"task": "work", "pane_id": "fresh-ui-pane"});
    assert!(authorize(Some(&session), "POST", "/agents/run", &body).is_ok());
}

#[test]
fn run_agent_with_pane_owned_by_another_session_is_denied() {
    let owner = id();
    let other = id();
    register_owned_pane(&owner, "adopted-pane").unwrap();
    take_over_pane(&owner, "adopted-pane", true).unwrap();
    set_mode(&other, "agent").unwrap();
    let body = serde_json::json!({"task": "work", "pane_id": "adopted-pane"});
    let err = authorize(Some(&other), "POST", "/agents/run", &body).unwrap_err();
    assert_eq!(err.code, "AUTHORIZATION_DENIED");
    assert!(err.message.contains("another MCP coordinator"));
}

#[test]
fn resolving_a_prefix_never_bypasses_grant_for_unmanaged_panes() {
    let ids = panes();
    let session = id();
    let full = resolve_pane_id("57c03490", &ids).unwrap();
    assert!(take_over_pane(&session, &full, false).is_err());
    take_over_pane(&session, &full, true).unwrap();
    assert!(session_controls_pane(&session, &full));
}

#[test]
fn answer_prompt_by_pane_needs_control_like_any_pane_write() {
    let owner = id();
    let other = id();
    register_owned_pane(&owner, "prompt-pane").unwrap();
    take_over_pane(&owner, "prompt-pane", true).unwrap();
    set_mode(&owner, "both").unwrap();
    set_mode(&other, "both").unwrap();
    let body =
        serde_json::json!({"pane_id":"prompt-pane","prompt_id":"p","choice":"allow_once"});
    assert!(authorize(Some(&owner), "POST", "/agents/answer", &body).is_ok());
    let denied = authorize(Some(&other), "POST", "/agents/answer", &body).unwrap_err();
    assert_eq!(denied.code, "AUTHORIZATION_DENIED");
    assert!(denied.message.contains("another MCP coordinator"));
    release_owned_pane(&owner, "prompt-pane").unwrap();
}
