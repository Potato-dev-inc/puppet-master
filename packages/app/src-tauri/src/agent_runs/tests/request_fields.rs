//! Run request parsing: defaults, context-mode aliasing, model fields, shell/allow options.

use crate::agent_runs::runtime::AgentRunRequest;
use crate::operations::ContextPolicy;
use serde_json::json;

#[test]
fn run_defaults_to_bounded_foreground_with_safe_worker_tools() {
    let request: AgentRunRequest = serde_json::from_value(json!({"task":"inspect"})).unwrap();
    assert_eq!(request.timeout_ms, 900_000);
    assert_eq!(request.wait_ms, 120_000);
    assert!(!request.headless, "visible-first: a run opens a pane unless headless=true is passed");
    assert!(!request.background);
    assert!(!request.worker_has_mcp_tools);
}

fn parse_run_request(value: serde_json::Value) -> AgentRunRequest {
    let mut value = value;
    crate::agent_runs::runtime::normalize_context_policy_fields(&mut value).unwrap();
    serde_json::from_value(value).unwrap()
}

#[test]
fn run_agent_request_maps_context_mode_alias() {
    let request = parse_run_request(json!({
        "task":"inspect",
        "context_mode":"fresh"
    }));
    assert_eq!(request.context_policy, ContextPolicy::Fresh);
}

#[test]
fn run_agent_request_accepts_equal_context_fields() {
    let request = parse_run_request(json!({
        "task":"inspect",
        "context_mode":"packet",
        "context_policy":"packet"
    }));
    assert_eq!(request.context_policy, ContextPolicy::Packet);
}

#[test]
fn run_agent_request_rejects_differing_context_fields() {
    let mut value = json!({
        "task":"inspect",
        "context_mode":"fresh",
        "context_policy":"resume"
    });
    let err = crate::agent_runs::runtime::normalize_context_policy_fields(&mut value).unwrap_err();
    assert!(err.message.contains("disagree"));
}

#[test]
fn run_agent_request_treats_the_packet_schema_default_as_unset() {
    // Regression: setting one alias while the other carried the old `packet` default failed with
    // "context_mode and context_policy disagree (resume vs packet)".
    for (mode, policy, expected) in [
        ("resume", "packet", ContextPolicy::Resume),
        ("packet", "resume", ContextPolicy::Resume),
        ("fresh", "packet", ContextPolicy::Fresh),
    ] {
        let request = parse_run_request(json!({
            "task":"inspect",
            "context_mode":mode,
            "context_policy":policy
        }));
        assert_eq!(request.context_policy, expected, "{mode}/{policy}");
    }
}

#[test]
fn context_fields_every_combination() {
    let values = [
        None,
        Some("fresh"),
        Some("packet"),
        Some("parent_summary"),
        Some("resume"),
        Some("selected_history"),
    ];
    for mode in values {
        for policy in values {
            let mut body = json!({"task":"inspect"});
            if let Some(m) = mode {
                body["context_mode"] = json!(m);
            }
            if let Some(p) = policy {
                body["context_policy"] = json!(p);
            }
            let result = crate::agent_runs::runtime::normalize_context_policy_fields(&mut body);
            match (mode, policy) {
                (Some(m), Some(p)) if m != p && m != "packet" && p != "packet" => {
                    assert!(result.is_err(), "{m} vs {p} must error");
                }
                (Some(m), Some(p)) if m != p => {
                    // One side is the legacy `packet` default: the explicit side wins.
                    result.unwrap_or_else(|e| panic!("{m}/{p}: {e:?}"));
                    let request: AgentRunRequest = serde_json::from_value(body).unwrap();
                    let explicit = if m == "packet" { p } else { m };
                    let expected: ContextPolicy = serde_json::from_value(json!(explicit)).unwrap();
                    assert_eq!(request.context_policy, expected, "{m}/{p}");
                }
                _ => {
                    result.unwrap_or_else(|e| panic!("{mode:?}/{policy:?}: {e:?}"));
                    let request: AgentRunRequest = serde_json::from_value(body).unwrap();
                    let expected = mode.or(policy).map_or(ContextPolicy::Packet, |v| {
                        serde_json::from_value(json!(v)).unwrap()
                    });
                    assert_eq!(request.context_policy, expected, "{mode:?}/{policy:?}");
                }
            }
        }
    }
}

#[test]
fn model_fields_resolve_from_request_and_live_pane() {
    use crate::agent_runs::persist::resolve_model_fields as r;
    assert_eq!(
        r(Some("deepseek-v4.1-flash"), None, None, true),
        (Some("deepseek-v4.1-flash".into()), None)
    );
    assert_eq!(
        r(
            None,
            Some("old/model"),
            Some("deepseek/deepseek-v4.1-flash".into()),
            true
        ),
        (None, Some("deepseek/deepseek-v4.1-flash".into()))
    );
    assert_eq!(
        r(None, Some("old/model"), None, true).1.as_deref(),
        Some("old/model")
    );
    assert_eq!(
        r(Some(" "), Some("persisted"), Some("profile".into()), false),
        (None, Some("persisted".into()))
    );
}

#[test]
fn run_agent_deserializes_name_handle_and_defaults_to_packet() {
    let request: AgentRunRequest = serde_json::from_value(json!({
        "task": "review the diff",
        "name": "reviewer",
        "handle": "reviewer",
        "agent_run_id": "reviewer",
        "model": "luna-low",
        "reasoning": "low"
    }))
    .unwrap();
    assert_eq!(request.name.as_deref(), Some("reviewer"));
    assert_eq!(request.handle.as_deref(), Some("reviewer"));
    assert_eq!(request.agent_run_id.as_deref(), Some("reviewer"));
    assert_eq!(request.context_policy, ContextPolicy::Packet);
    assert_eq!(request.model.as_deref(), Some("luna-low"));
    assert_eq!(request.reasoning.as_deref(), Some("low"));
}

#[test]
fn allow_shell_adds_force_only_when_asked_and_never_with_read_only() {
    let launch = |read_only, allow_shell| {
        crate::tui::headless_launch(
            "cursor_agent",
            &crate::tui::HeadlessLaunch {
                read_only,
                allow_shell,
                ..crate::tui::HeadlessLaunch::new(".", "t")
            },
        )
    };
    assert!(!launch(false, false)
        .unwrap()
        .args
        .iter()
        .any(|arg| arg == "--force"));
    assert!(launch(false, true)
        .unwrap()
        .args
        .iter()
        .any(|arg| arg == "--force"));
    assert!(launch(true, true).is_err());
}

#[test]
fn a_retried_run_agent_call_gets_the_same_handle_but_different_calls_do_not() {
    let a = crate::agent_runs::persist::stable_handle("C:/proj", "key-1");
    assert_eq!(
        a,
        crate::agent_runs::persist::stable_handle("C:/proj", "key-1")
    );
    assert_eq!(
        a,
        crate::agent_runs::persist::stable_handle("C:/proj", " key-1 "),
        "keys are trimmed"
    );
    assert_ne!(
        a,
        crate::agent_runs::persist::stable_handle("C:/proj", "key-2")
    );
    assert_ne!(
        a,
        crate::agent_runs::persist::stable_handle("C:/other", "key-1")
    );
    // Uuid-shaped like every other handle.
    assert_eq!(a.len(), 36);
    assert!(
        a.chars()
            .enumerate()
            .all(|(i, c)| if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }),
        "{a}"
    );
}

#[test]
fn allow_shell_is_a_noop_for_opencode_native_and_rejected_for_read_only_and_unsupported() {
    use crate::agent_runs::headless::allow_shell_applies;
    let native = crate::tui::profile_for("opencode_native").run_options();
    // Shell is already on for the native worker: accepted, nothing to apply, even in a pane.
    assert_eq!(allow_shell_applies(&native, "opencode_native", false, false).unwrap(), false);
    assert!(allow_shell_applies(&native, "opencode_native", false, true).is_err());
    let cursor = crate::tui::profile_for("cursor_agent").run_options();
    assert_eq!(allow_shell_applies(&cursor, "cursor_agent", true, false).unwrap(), true);
    assert!(allow_shell_applies(&cursor, "cursor_agent", false, false).is_err());
    assert!(allow_shell_applies(&cursor, "cursor_agent", true, true).is_err());
    let claude = crate::tui::profile_for("claude").run_options();
    let error = allow_shell_applies(&claude, "claude", true, false).unwrap_err();
    assert!(error.message.contains("not supported by claude"), "{}", error.message);
}

#[test]
fn explicit_headless_true_stays_headless_and_placement_defaults_to_unset() {
    let request: AgentRunRequest =
        serde_json::from_value(json!({"task":"inspect","headless":true})).unwrap();
    assert!(request.headless);
    assert_eq!(request.placement, None, "unset placement means the pop-out window");
}
