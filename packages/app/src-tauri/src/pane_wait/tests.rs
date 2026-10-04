use super::*;
use crate::opencode::quota::KeySwapEvent;

fn native_status(pane_status: &str) -> crate::opencode::status::OpenCodeWorkerStatus {
    crate::opencode::status::OpenCodeWorkerStatus {
        pane_id: "p".into(),
        pane_status: pane_status.into(),
        serve_healthy: true,
        session_id: "s".into(),
        pending_permission_count: 0,
        pending_permission_ids: vec![],
        pending_permissions: vec![],
        active_key_profile: None,
        key_swap_pending: false,
        key_swap_kind: None,
        from_profile: None,
        to_profile: None,
        session_model: None,
        last_user_model: None,
        footer_model_source: None,
        tui_attached: true,
        reattaching: false,
        last_event: None,
    }
}

#[test]
fn native_dispatch_skips_tui_paint_wait_only_when_healthy_and_attached() {
    assert!(native_status_ready_for_dispatch(&native_status("running")));
    assert!(native_status_ready_for_dispatch(&native_status("idle")));
    assert!(!native_status_ready_for_dispatch(&native_status("error")));
    assert!(!native_status_ready_for_dispatch(&native_status(
        "waiting_input"
    )));
    let mut s = native_status("running");
    s.serve_healthy = false;
    assert!(!native_status_ready_for_dispatch(&s));
    let mut s = native_status("running");
    s.tui_attached = false;
    assert!(!native_status_ready_for_dispatch(&s));
    let mut s = native_status("running");
    s.reattaching = true;
    assert!(!native_status_ready_for_dispatch(&s));
    let mut s = native_status("running");
    s.key_swap_pending = true;
    assert!(!native_status_ready_for_dispatch(&s));
}

#[test]
fn parses_wait_triggers() {
    assert_eq!(
        WaitUntil::parse("permission").unwrap(),
        WaitUntil::Permission
    );
    assert_eq!(
        WaitUntil::parse("waiting").unwrap(),
        WaitUntil::WaitingInput
    );
    assert_eq!(
        WaitUntil::parse("key_swap").unwrap(),
        WaitUntil::KeySwapRequired
    );
    assert_eq!(
        WaitUntil::parse("rate_limited").unwrap(),
        WaitUntil::KeyRotated
    );
    assert_eq!(
        WaitUntil::parse("model_ready").unwrap(),
        WaitUntil::ModelReady
    );
    assert_eq!(WaitUntil::parse("tui_ready").unwrap(), WaitUntil::TuiReady);
    assert_eq!(
        WaitUntil::parse("task_completed").unwrap(),
        WaitUntil::TaskCompleted
    );
    assert_eq!(WaitUntil::parse("settled").unwrap(), WaitUntil::Settled);
    assert_eq!(
        WaitUntil::parse("tui_prompt").unwrap(),
        WaitUntil::TuiPrompt
    );
    assert_eq!(WaitUntil::parse("running").unwrap(), WaitUntil::Running);
}

#[test]
fn live_running_pane_is_dispatchable_after_readiness_timeout() {
    assert!(live_enough_for_dispatch(Some("running")));
    assert!(live_enough_for_dispatch(Some("idle")));
    assert!(live_enough_for_dispatch(Some("waiting_input")));
    assert!(!live_enough_for_dispatch(Some("error")));
    assert!(!live_enough_for_dispatch(None));
}

#[test]
fn dispatch_ready_accepts_running_claude_pane_with_output() {
    let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::new()));
    let pane = crate::pty::PaneRegistry::test_pane_stub("claude-1");
    pane.scrollback
        .lock()
        .push_chunk(b"Claude Code loaded\nLogin expired\n");
    registry.lock().panes.insert("claude-1".into(), pane);
    let result = wait_for_dispatch_ready(&registry, "claude-1", 1_000).expect("wait");
    assert_eq!(result.reason, "running");
    assert_eq!(result.status.as_deref(), Some("running"));
}

#[test]
fn pane_info_uses_live_status_lock_not_spawn_snapshot() {
    let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::new()));
    let pane = crate::pty::PaneRegistry::test_pane_stub("live-status");
    *pane.status.lock() = crate::pty::status::PaneStatus::Idle;
    registry.lock().panes.insert("live-status".into(), pane);
    let info = status::pane_info(&registry, "live-status").expect("pane");
    assert_eq!(info.status, "idle");
    assert_ne!(
        info.status,
        registry.lock().panes["live-status"].info.status
    );
}

#[test]
fn opencode_reply_hint_requires_native_state() {
    assert!(agent_hint_for_reason("tui_prompt", None).is_none());
}

#[test]
fn default_until_includes_permission_and_key_swap() {
    let triggers = default_until();
    assert!(triggers.contains(&WaitUntil::Permission));
    assert!(triggers.contains(&WaitUntil::Idle));
    assert!(triggers.contains(&WaitUntil::KeySwapRequired));
    assert!(triggers.contains(&WaitUntil::KeyRotated));
}

#[test]
fn match_key_event_maps_kind_to_reason() {
    let rotated = KeySwapEvent {
        kind: KEY_ROTATED.into(),
        pane_id: "p1".into(),
        from_profile: "a".into(),
        to_profile: Some("b".into()),
        reason: "rate_limited".into(),
        auto_rotated: true,
        at_ms: 0,
    };
    assert_eq!(
        match_key_event_to_reason(&rotated, true, true),
        Some("key_rotated")
    );
    assert_eq!(match_key_event_to_reason(&rotated, true, false), None);

    let swap = KeySwapEvent {
        kind: KEY_SWAP_REQUIRED.into(),
        pane_id: "p1".into(),
        from_profile: "a".into(),
        to_profile: None,
        reason: "rate_limited".into(),
        auto_rotated: false,
        at_ms: 0,
    };
    assert_eq!(
        match_key_event_to_reason(&swap, true, false),
        Some("key_swap_required")
    );
}

#[test]
fn hints_follow_the_agent_type_not_opencode_presence() {
    let cursor = tui_hint_for("cursor_agent", "waiting_input", Some("waiting_input")).unwrap();
    assert!(cursor["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "press_key"));
    assert!(!cursor.to_string().contains("opencode"));
    let shell = tui_hint_for("powershell", "idle", Some("idle")).unwrap();
    assert_eq!(shell["action"], "shell_exec");
    assert!(tui_hint_for("powershell", "waiting_input", Some("waiting_input")).is_none());
    assert!(tui_hint_for("opencode_native", "idle", Some("idle")).is_none());
}
