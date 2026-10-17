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
fn a_run_pane_the_connection_started_is_its_own_and_other_sessions_need_grant() {
    let owner = id();
    let other = id();
    register_owned_pane(&owner, "agent-pane-1").unwrap();
    set_mode(&owner, "shell").unwrap();
    let write = |session: &str| {
        authorize(
            Some(session),
            "POST",
            "/panes/agent-pane-1/input",
            &serde_json::json!({"text":"x"}),
        )
    };
    // Its own run's pane: no take_over needed.
    assert!(write(&owner).is_ok());
    assert!(take_over_pane(&other, "agent-pane-1", false).is_err());
    set_mode(&other, "shell").unwrap();
    assert!(write(&other).is_err(), "someone else's pane is still refused");
    take_over_pane(&other, "agent-pane-1", true).unwrap();
    assert!(session_controls_pane(&other, "agent-pane-1"));
    assert!(!session_controls_pane(&owner, "agent-pane-1"));
    assert!(write(&owner).is_err(), "the lease moved: the first connection is refused now");
}

// ---- a pane the connection's own run_agent / followup_task opened ------------------------------
//
// A lead called run_agent (visible pane), the agent waited on an approval nobody recognised as a
// card, and press_key on that pane was refused until the lead called take_over. The pane is the
// lead's own; only panes it did not start keep the take_over rule.

fn terminal_calls(pane: &str) -> Vec<(&'static str, String, Value)> {
    vec![
        ("press_key", format!("/panes/{pane}/key"), serde_json::json!({"key":"enter"})),
        ("write_terminal_input", format!("/panes/{pane}/input"), serde_json::json!({"text":"y"})),
    ]
}

#[test]
fn press_key_write_input_and_answer_prompt_work_on_the_pane_my_own_run_opened() {
    let lead = id();
    let pane = format!("own-run-pane-{}", Uuid::new_v4());
    set_mode(&lead, "both").unwrap();
    // What dispatch does when run_agent spawns the pane (twice: reserve and bind).
    register_owned_pane(&lead, &pane).unwrap();
    register_owned_pane(&lead, &pane).unwrap();
    for (tool, path, body) in terminal_calls(&pane) {
        assert!(authorize(Some(&lead), "POST", &path, &body).is_ok(), "{tool} on my own run's pane");
    }
    let answer = serde_json::json!({"pane_id": pane, "prompt_id":"p", "choice":"allow_once"});
    assert!(authorize(Some(&lead), "POST", "/agents/answer", &answer).is_ok());
}

#[test]
fn the_own_pane_rule_is_only_for_keys_text_and_answers() {
    let lead = id();
    let pane = format!("own-run-pane-{}", Uuid::new_v4());
    set_mode(&lead, "both").unwrap();
    register_owned_pane(&lead, &pane).unwrap();
    let args = serde_json::json!({"command":"pytest","pane_id": pane});
    assert!(
        authorize(Some(&lead), "POST", "/shell/exec", &args).is_err(),
        "a shell command into an agent's pane still needs take_over"
    );
}

#[test]
fn another_agents_run_pane_is_still_refused_for_keys_text_and_answers() {
    let lead = id();
    let rival = id();
    let pane = format!("rival-run-pane-{}", Uuid::new_v4());
    set_mode(&lead, "both").unwrap();
    set_mode(&rival, "both").unwrap();
    register_owned_pane(&rival, &pane).unwrap();
    for (tool, path, body) in terminal_calls(&pane) {
        let error = authorize(Some(&lead), "POST", &path, &body).unwrap_err();
        assert_eq!(error.code, "AUTHORIZATION_DENIED", "{tool}");
    }
    let answer = serde_json::json!({"pane_id": pane, "prompt_id":"p", "choice":"allow_once"});
    assert_eq!(authorize(Some(&lead), "POST", "/agents/answer", &answer).unwrap_err().code, "AUTHORIZATION_DENIED");
    // Only an explicit take_over (which the consent gate judges) opens it.
    take_over_pane(&lead, &pane, true).unwrap();
    for (tool, path, body) in terminal_calls(&pane) {
        assert!(authorize(Some(&lead), "POST", &path, &body).is_ok(), "{tool} after take_over");
    }
}

#[test]
fn a_pane_the_person_opened_is_still_refused_until_take_over() {
    let lead = id();
    set_mode(&lead, "both").unwrap();
    // Not in the table at all: opened by the person.
    let ui_pane = format!("ui-pane-{}", Uuid::new_v4());
    for (tool, path, body) in terminal_calls(&ui_pane) {
        let error = authorize(Some(&lead), "POST", &path, &body).unwrap_err();
        assert_eq!(error.code, "AUTHORIZATION_DENIED", "{tool}");
        assert!(error.message.contains("not adopted"), "{tool}: {}", error.message);
    }
    // Adopted for a run of the lead's (bind_agent_pane): the lead still did not start it.
    let handle = Uuid::new_v4().to_string();
    register_owned_agent(&lead, &handle).unwrap();
    take_over_pane(&lead, &ui_pane, true).unwrap();
    bind_agent_pane(&lead, &handle, &ui_pane, true).unwrap();
    for (tool, path, body) in terminal_calls(&ui_pane) {
        let error = authorize(Some(&lead), "POST", &path, &body).unwrap_err();
        assert!(error.message.contains("read-only to terminal controls"), "{tool}: {}", error.message);
    }
    take_over_pane(&lead, &ui_pane, true).unwrap();
    for (tool, path, body) in terminal_calls(&ui_pane) {
        assert!(authorize(Some(&lead), "POST", &path, &body).is_ok(), "{tool} after take_over");
    }
}

#[test]
fn an_own_pane_handed_back_after_a_take_over_is_still_its_own() {
    let lead = id();
    let pane = format!("own-released-{}", Uuid::new_v4());
    set_mode(&lead, "both").unwrap();
    register_owned_pane(&lead, &pane).unwrap();
    take_over_pane(&lead, &pane, true).unwrap();
    release_owned_pane(&lead, &pane).unwrap();
    // Control is back with the owner, and it started the pane: still its own.
    assert!(authorize(Some(&lead), "POST", &format!("/panes/{pane}/key"), &serde_json::json!({"key":"enter"})).is_ok());
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

// ---- closing a pane that a run created (VERIFY-R7 finding E) -----------------------------------
//
// `DELETE /panes/<id>` is refused for a run-created pane by design (the pane belongs to the run's
// MCP session and its worker handle; mode rules keep `kill_pane_process` out of agent mode). What was
// wrong is that the refusal did not say how to close it.

#[test]
fn a_run_pane_cannot_be_deleted_over_bare_http_and_the_error_says_how_to_close_it() {
    let session = id();
    let pane = format!("run-pane-{}", Uuid::new_v4());
    register_owned_pane(&session, &pane).unwrap();
    let error = authorize(None, "DELETE", &format!("/panes/{pane}"), &Value::Null).unwrap_err();
    assert_eq!(error.code, "AUTHORIZATION_DENIED");
    assert!(error.message.contains("close_agent"), "{}", error.message);
    assert!(error.message.contains("/operations/<operation_id>/cancel"), "{}", error.message);
    assert!(error.message.contains("Close on the pane"), "{}", error.message);
    assert_eq!(error.context["pane_id"], pane.as_str());
    assert_eq!(error.context["close_with"][0]["route"], "POST /agents/close");
    // Other mutations keep the short refusal.
    let input = authorize(None, "POST", &format!("/panes/{pane}/input"), &serde_json::json!({"text":"x"})).unwrap_err();
    assert_eq!(input.message, "managed pane mutations require an MCP session id");
    // A pane nobody manages is still closable over bare HTTP (the legacy local behaviour).
    assert!(authorize(None, "DELETE", "/panes/not-managed-by-anyone", &Value::Null).is_ok());
}

#[test]
fn kill_pane_process_in_agent_mode_points_at_close_agent() {
    let session = id();
    let error = authorize(Some(&session), "DELETE", "/panes/p1", &Value::Null).unwrap_err();
    assert_eq!(error.code, "MODE_MISMATCH");
    assert!(error.message.starts_with("kill_pane_process is unavailable in agent mode"), "{}", error.message);
    assert!(error.message.contains("close_agent"), "{}", error.message);
    assert_eq!(error.context["use_instead"]["tool"], "close_agent");
    assert_eq!(error.context.pointer("/switch_with/mode").and_then(Value::as_str), Some("shell"));
}

#[test]
fn the_owning_session_can_close_its_own_run_with_close_agent() {
    let session = id();
    let handle = Uuid::new_v4().to_string();
    register_owned_agent(&session, &handle).unwrap();
    assert!(authorize(Some(&session), "POST", "/agents/close", &serde_json::json!({"handle": handle})).is_ok());
    // Another connection cannot close it.
    let other = id();
    assert!(authorize(Some(&other), "POST", "/agents/close", &serde_json::json!({"handle": handle})).is_err());
}
