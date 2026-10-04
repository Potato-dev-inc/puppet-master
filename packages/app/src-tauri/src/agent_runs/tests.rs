use super::completion::{
    clean_extracted_result, observe_tui_turn, tui_turn_evidence, TuiTurnDecision, TuiTurnWatch,
    NOT_SUBMITTED_TICKS, RECONCILE_IDLE_TICKS,
};
use super::native::native_completion_evidence;
use super::runtime::AgentRunRequest;
use super::{headless, transcript};
use crate::opencode::messages::{MessagePartView, MessageView, SessionMessagesView};
use crate::operations::{
    self, ContextContinuity, ContextPolicy, DelegateWorkRequest, OperationError, OperationStatus,
    ResultCapture, StateSource, WorkerPersist,
};
use serde_json::json;
use std::fs;
use uuid::Uuid;

fn assistant(id: &str, finish: &str, text: &str, error: Option<&str>) -> MessageView {
    MessageView {
        id: Some(id.into()),
        role: "assistant".into(),
        model: None,
        finish: Some(finish.into()),
        error: error.map(str::to_string),
        parts: vec![MessagePartView::Text { text: text.into() }],
    }
}

fn view(messages: Vec<MessageView>) -> SessionMessagesView {
    SessionMessagesView {
        pane_id: "pane-native".into(),
        session_id: "session-native".into(),
        messages,
        pending_question: None,
        last_assistant_text: None,
    }
}

#[test]
fn run_defaults_to_bounded_foreground_with_safe_worker_tools() {
    let request: AgentRunRequest = serde_json::from_value(json!({"task":"inspect"})).unwrap();
    assert_eq!(request.timeout_ms, 900_000);
    assert_eq!(request.wait_ms, 120_000);
    assert!(request.headless);
    assert!(!request.background);
    assert!(!request.worker_has_mcp_tools);
}

fn parse_run_request(value: serde_json::Value) -> AgentRunRequest {
    let mut value = value;
    super::runtime::normalize_context_policy_fields(&mut value).unwrap();
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
    let err = super::runtime::normalize_context_policy_fields(&mut value).unwrap_err();
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
            let result = super::runtime::normalize_context_policy_fields(&mut body);
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
    use super::persist::resolve_model_fields as r;
    assert_eq!(
        r(Some("deepseek-v4.1-flash"), None, None, true),
        (Some("deepseek-v4.1-flash".into()), None)
    );
    assert_eq!(
        r(None, Some("old/model"), Some("deepseek/deepseek-v4.1-flash".into()), true),
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
fn native_completion_ignores_assistant_messages_from_the_previous_turn() {
    let messages = view(vec![assistant("previous", "stop", "stale answer", None)]);
    assert!(native_completion_evidence(&messages, &["previous".into()], false).is_none());
}

#[test]
fn native_completion_rejects_missing_message_ids() {
    let mut message = assistant("temporary", "stop", "answer", None);
    message.id = None;
    assert!(native_completion_evidence(&view(vec![message]), &[], false).is_none());
}

#[test]
fn native_completion_requires_fresh_stop_and_no_pending_prompt() {
    let running = view(vec![assistant(
        "fresh-running",
        "tool-calls",
        "not final",
        None,
    )]);
    assert!(native_completion_evidence(&running, &["previous".into()], false).is_none());

    let stopped = view(vec![assistant(
        "fresh-stop",
        "stop",
        "new verified answer",
        None,
    )]);
    assert!(native_completion_evidence(&stopped, &["previous".into()], true).is_none());
    assert_eq!(
        native_completion_evidence(&stopped, &["previous".into()], false),
        Some(Ok("new verified answer".into()))
    );
}

#[test]
fn native_assistant_error_is_not_reported_as_verified_success() {
    let messages = view(vec![assistant(
        "fresh-error",
        "error",
        "",
        Some("provider failed"),
    )]);
    assert_eq!(
        native_completion_evidence(&messages, &[], false),
        Some(Err("provider failed".into()))
    );
}

#[test]
fn agent_view_uses_compact_statuses_and_exposes_timeout() {
    let project = std::env::temp_dir().join(format!("pm-agent-view-{}", Uuid::new_v4()));
    let request = DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "inspect".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["report findings".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("stable-handle".into()),
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    let queued_view = super::runtime::AgentRunView::from(queued.clone());
    assert_eq!(queued_view.status, "running");
    assert!(!queued_view.verified);

    let mut timed_out = queued;
    timed_out.status = OperationStatus::Failed;
    timed_out.error = Some(OperationError::new("TIMEOUT", "deadline exceeded", true));
    assert_eq!(
        super::runtime::AgentRunView::from(timed_out).status,
        "timeout"
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_transcript_preserves_utf8_across_reader_chunks() {
    let project = std::env::temp_dir().join(format!("pm-agent-utf8-{}", Uuid::new_v4()));
    let project = project.to_string_lossy().into_owned();
    let operation = Uuid::new_v4().to_string();
    let bytes = "漢".as_bytes();
    let mut pending = Vec::new();
    pending.extend_from_slice(&bytes[..2]);
    headless::append_valid_utf8(&project, &operation, "stdout", &mut pending, false);
    pending.extend_from_slice(&bytes[2..]);
    headless::append_valid_utf8(&project, &operation, "stdout", &mut pending, false);
    headless::append_valid_utf8(&project, &operation, "stdout", &mut pending, true);
    let page = transcript::read(&project, &operation, 0).unwrap();
    let combined = page
        .chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect::<String>();
    assert_eq!(combined, "漢");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn cancellation_selection_orders_all_active_turns_newest_first() {
    let project = std::env::temp_dir().join(format!("pm-agent-cancel-order-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    for turn_index in 0..3 {
        let request = DelegateWorkRequest {
            project_path: project_path.clone(),
            task: format!("turn {turn_index}"),
            agent_type: "codex".into(),
            pane_id: None,
            idempotency_key: format!("turn-key-{turn_index}"),
            acceptance_criteria: Some(vec!["return a result".into()]),
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 900_000,
            read_only: false,
            keep_pane: false,
            owner_session_id: Some("cancel-test-session".into()),
            worker_has_mcp_tools: false,
            agent_run_id: Some("cancel-test-handle".into()),
            turn_index,
            worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
        };
        operations::create_operation(request).unwrap();
    }
    let ordered = super::runtime::active_turns(
        operations::list_operations(&project_path).unwrap(),
        "cancel-test-handle",
    );
    assert_eq!(
        ordered
            .iter()
            .map(|snapshot| snapshot.turn_index)
            .collect::<Vec<_>>(),
        vec![2, 1, 0]
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_failure_message_includes_last_stderr_line_and_trust_hint() {
    let message = headless::failure_message(1, "warming up\nWorkspace not trusted\n\n", None);
    assert!(message.contains("Workspace not trusted"));
    assert!(message.contains("trust the workspace folder"));
    let plain = headless::failure_message(1, "boom\n", None);
    assert!(plain.ends_with(": boom"));
    assert_eq!(
        headless::failure_message(1, "", None),
        "agent process exited with a failure status"
    );
    let buried = headless::failure_message(
        1,
        "error: unexpected argument '--ask-for-approval' found\n\nFor more information, try '--help'.\n",
        None,
    );
    assert!(buried.contains("unexpected argument '--ask-for-approval'"));
    assert!(!buried.contains("try '--help'"));
}

fn parked_snapshot(status: OperationStatus, kind: &str) -> operations::OperationSnapshot {
    let project = std::env::temp_dir().join(format!("pm-agent-trust-{}", Uuid::new_v4()));
    let request = DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "inspect".into(),
        agent_type: "cursor_agent".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["report findings".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: true,
        keep_pane: false,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let mut snapshot = operations::create_operation(request).unwrap().0;
    snapshot.status = status;
    snapshot.required_action = Some(json!({"kind": kind}));
    snapshot
}

#[test]
fn run_parked_on_workspace_trust_can_be_cancelled_and_retried_without_a_worker() {
    // The cancel hook sees `Cancelling`, because cancel_operation_with flips status first.
    let cancelling = parked_snapshot(OperationStatus::Cancelling, "workspace_trust");
    assert!(headless::cancel_headless(&cancelling).is_ok());
    let waiting = parked_snapshot(OperationStatus::WaitingInput, "workspace_trust");
    assert!(headless::blocked_on_trust(&waiting));
    assert!(
        !headless::blocked_on_trust(&cancelling),
        "a cancelling run must not be retried"
    );
}

#[test]
fn other_prompts_and_running_runs_are_not_treated_as_trust_parked() {
    let other = parked_snapshot(OperationStatus::WaitingInput, "command_approval");
    assert!(!headless::blocked_on_trust(&other));
    assert_eq!(
        headless::cancel_headless(&other).unwrap_err().code,
        "WORKER_NOT_FOUND"
    );
    let running = parked_snapshot(OperationStatus::Running, "workspace_trust");
    assert!(!headless::blocked_on_trust(&running));
}

#[test]
fn tui_turn_evidence_ignores_unchanged_idle_chrome() {
    let idle = "Add follow-up >";
    assert!(tui_turn_evidence(idle, idle).is_none());
}

#[test]
fn tui_turn_evidence_returns_new_reply_after_idle_baseline() {
    let before = "previous chatter\nAdd follow-up >";
    let after = "previous chatter\nPUPPET_MASTER_AGENT_MODE_OK\n\nAdd follow-up >";
    assert_eq!(
        tui_turn_evidence(after, before).as_deref(),
        Some("PUPPET_MASTER_AGENT_MODE_OK")
    );
}

#[test]
fn tui_turn_evidence_ignores_chrome_only_delta() {
    let before = "hello\nAdd follow-up >";
    let after = "hello\n→ Add follow-up";
    assert!(tui_turn_evidence(after, before).is_none());
}

fn tick_until_idle(
    watch: &mut TuiTurnWatch,
    status: &str,
    screen: &str,
    has_prompt: bool,
    times: u8,
) -> TuiTurnDecision {
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..times {
        last = observe_tui_turn(watch, status, screen, has_prompt);
    }
    last
}

#[test]
fn fast_reply_never_observed_busy_completes_with_result_not_timeout() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    let screen = "The connection fails because the token expired.\nAdd follow-up >";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    match decision {
        TuiTurnDecision::Complete { result, capture } => {
            assert!(result.contains("token expired"));
            assert_eq!(capture, ResultCapture::Inferred);
        }
        other => panic!("expected complete, got {other:?}"),
    }
}

#[test]
fn blank_baseline_banner_text_with_no_work_is_not_a_completed_turn() {
    // The baseline snapshot raced the ConPTY screen and came back blank, so the banner, model and
    // working directory that are already on screen look like "new output".
    let mut watch = TuiTurnWatch::from_baseline("").with_task("do X");
    let screen = "Cursor Agent v2026.10.01-abc\n~/proj\nAdd a follow-up";
    for tick in 0..6 {
        let decision = observe_tui_turn(&mut watch, "idle", screen, false);
        assert!(
            !matches!(decision, TuiTurnDecision::Complete { .. }),
            "tick {tick}: banner text must not complete a turn that did nothing: {decision:?}"
        );
    }
}

#[test]
fn blank_baseline_startup_furniture_beyond_the_known_chrome_is_not_a_completed_turn() {
    // Same race, but the idle screen carries a bare non-Composer model footer, a trust-folder
    // line and an update notice: none are in the chrome list, so they read as "new output".
    let mut watch = TuiTurnWatch::from_baseline("").with_task("do X");
    let screen =
        "Cursor Agent v2026.10.01-abc\n~/proj\nGPT-5\nUpdate available: run agent update\nAdd a follow-up";
    for tick in 0..6 {
        let decision = observe_tui_turn(&mut watch, "idle", screen, false);
        assert!(
            !matches!(decision, TuiTurnDecision::Complete { .. }),
            "tick {tick}: startup furniture must not complete a turn that did nothing: {decision:?}"
        );
    }
}

#[test]
fn blank_baseline_turn_completes_once_activity_was_observed() {
    let mut watch = TuiTurnWatch::from_baseline("").with_task("do X");
    let _ = observe_tui_turn(&mut watch, "running", "Cursor Agent v1\nworking", false);
    let screen = "Cursor Agent v1\nThe answer is 42.\nAdd a follow-up";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    assert!(matches!(decision, TuiTurnDecision::Complete { .. }), "{decision:?}");
}

#[test]
fn blank_baseline_turn_completes_once_the_task_echo_is_on_screen() {
    let mut watch = TuiTurnWatch::from_baseline("").with_task("summarize the readme file");
    let echoing = "Cursor Agent v1\n> summarize the readme file\nAdd a follow-up";
    // Echo seen on a busy-looking-idle tick, then the reply lands.
    let _ = observe_tui_turn(&mut watch, "idle", echoing, false);
    let screen = "Cursor Agent v1\n> summarize the readme file\nIt documents the CLI.\nAdd a follow-up";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    assert!(matches!(decision, TuiTurnDecision::Complete { .. }), "{decision:?}");
}

#[test]
fn repeated_identical_answers_still_count_as_a_new_turn_result() {
    let previous = "PUPPET_MASTER_AGENT_MODE_OK\nAdd follow-up >";
    let mut watch = TuiTurnWatch::from_baseline(previous);
    let screen = "PUPPET_MASTER_AGENT_MODE_OK\nPUPPET_MASTER_AGENT_MODE_OK\nAdd follow-up >";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    match decision {
        TuiTurnDecision::Complete { result, capture } => {
            assert_eq!(result, "PUPPET_MASTER_AGENT_MODE_OK");
            assert_eq!(capture, ResultCapture::Inferred);
        }
        other => panic!("expected complete, got {other:?}"),
    }
}

#[test]
fn stale_pre_dispatch_output_is_not_the_new_result() {
    let stale = "old answer still on screen\nAdd follow-up >";
    let mut watch = TuiTurnWatch::from_baseline(stale);
    let decision = tick_until_idle(
        &mut watch,
        "idle",
        stale,
        false,
        NOT_SUBMITTED_TICKS,
    );
    assert_eq!(decision, TuiTurnDecision::NotSubmitted);
    assert!(tui_turn_evidence(stale, stale).is_none());
}

#[test]
fn permission_prompt_is_not_completion() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    let screen = "Allow once  Don't ask again\nThe connection fails because...\nAdd follow-up >";
    let decision = tick_until_idle(&mut watch, "idle", screen, true, 4);
    assert_eq!(decision, TuiTurnDecision::WaitingInput);
}

#[test]
fn wait_agents_view_separates_result_from_capture_and_acceptance() {
    let project = std::env::temp_dir().join(format!("pm-agent-wait-view-{}", Uuid::new_v4()));
    let request = DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "explain the failure".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["report findings".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("worker-123".into()),
        turn_index: 4,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(&queued.project_path, &queued.operation_id, |_| Ok(()))
        .unwrap();
    let finished = operations::mark_operation_finished(
        &queued.project_path,
        &queued.operation_id,
        Some("The connection fails because...".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let view = super::runtime::AgentRunView::from(finished);
    assert_eq!(view.handle, "worker-123");
    assert_eq!(view.turn_id, "turn-4");
    assert_eq!(view.status, "completed");
    assert_eq!(
        view.result.as_deref(),
        Some("The connection fails because...")
    );
    assert_eq!(view.result_capture, "authoritative");
    assert_eq!(view.acceptance_status, "not_checked");
    assert_eq!(view.next_cursor, view.revision);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn transcript_truncation_does_not_drop_stored_result() {
    let project = std::env::temp_dir().join(format!("pm-agent-trunc-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let request = DelegateWorkRequest {
        project_path: project_path.clone(),
        task: "long log".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["keep the answer".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("worker-trunc".into()),
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(&project_path, &queued.operation_id, |_| Ok(())).unwrap();
    for n in 0..250 {
        transcript::append(&project_path, &queued.operation_id, "stdout", &format!("{n}\n"));
    }
    let finished = operations::mark_operation_finished(
        &project_path,
        &queued.operation_id,
        Some("kept final answer".into()),
        ResultCapture::Authoritative,
        false,
    )
    .unwrap();
    let page = transcript::read(&project_path, &queued.operation_id, 0).unwrap();
    assert!(page.truncated);
    assert_eq!(finished.result.as_deref(), Some("kept final answer"));
    let view = super::runtime::AgentRunView::from(finished);
    assert_eq!(view.result.as_deref(), Some("kept final answer"));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn native_and_tui_paths_write_transcript_events() {
    let project = std::env::temp_dir().join(format!("pm-agent-events-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let operation = Uuid::new_v4().to_string();
    super::runtime::record_user_task(&project_path, &operation, "inspect the outage");
    transcript::append(
        &project_path,
        &operation,
        "assistant_final",
        "The connection fails because the token expired.",
    );
    let page = transcript::read(&project_path, &operation, 0).unwrap();
    let kinds: Vec<_> = page.chunks.iter().map(|chunk| chunk.stream.as_str()).collect();
    assert!(kinds.contains(&"user_task"));
    assert!(kinds.contains(&"assistant_final"));
    let _ = fs::remove_dir_all(project);
}

fn persist_req(project: &str, handle: &str, turn: u32, task: &str) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: project.to_string(),
        task: task.into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["return a result".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        owner_session_id: Some("persist-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.into()),
        turn_index: turn,
        worker: WorkerPersist::default(),
        context_policy: None,
        checks: Vec::new(),
    }
}

fn finish_turn(project: &str, operation_id: &str, result: &str) {
    operations::run_operation_dispatch(project, operation_id, |_| Ok(())).unwrap();
    operations::mark_operation_finished(
        project,
        operation_id,
        Some(result.into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
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
fn sequential_tasks_on_one_handle_disclose_reconstructed_continuity() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-seq-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "reviewer",
        0,
        "summarize the outage",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "token expired");

    let bind = super::persist::decide_worker(&project_path, Some("reviewer"), false).unwrap();
    let super::persist::WorkerBind::Adopt { handle, previous } = bind else {
        panic!("expected adopt");
    };
    assert_eq!(handle, "reviewer");
    assert_eq!(previous.turn_index, 0);

    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "propose a fix",
        scope: Some("auth"),
        prior_result: previous.result.as_deref(),
        prior_user: None,
        selected_history: &[],
        can_resume: super::persist::snapshot_can_resume(&previous),
        is_followup: true,
    });
    assert_eq!(
        rendered.continuity,
        ContextContinuity::ReconstructedSummary
    );
    assert!(rendered.prompt.contains("token expired"));
    assert!(rendered.prompt.contains("propose a fix"));
    assert!(!rendered.prompt.contains("# Context packet"));

    let mut second_req = persist_req(&project_path, "reviewer", 1, "propose a fix");
    second_req.worker.context_policy = ContextPolicy::Packet;
    second_req.worker.context_continuity = rendered.continuity;
    operations::create_operation(second_req).unwrap();

    let inspected = super::runtime::inspect_agent(&project_path, "reviewer", Some("persist-session"))
        .unwrap();
    assert_eq!(inspected["handle"], "reviewer");
    assert_eq!(inspected["context_policy"], "packet");
    assert_eq!(inspected["context_continuity"], "reconstructed_summary");
    assert_eq!(inspected["turn_id"], "turn-1");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn completing_turn_one_does_not_close_the_worker() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-live-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "keeper",
        0,
        "first turn",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "done");
    let inspected =
        super::runtime::inspect_agent(&project_path, "keeper", Some("persist-session")).unwrap();
    assert_eq!(inspected["handle"], "keeper");
    assert_eq!(inspected["status"], "completed");
    assert_eq!(inspected["context_policy"], "packet");
    let latest = operations::latest_agent_run(&project_path, "keeper").unwrap();
    assert_eq!(latest.agent_run_id, "keeper");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn fresh_starts_a_new_conversation_when_a_name_matches() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-fresh-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "reviewer",
        0,
        "old conversation",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "old answer");

    let bind = super::persist::decide_worker(&project_path, Some("reviewer"), true).unwrap();
    let super::persist::WorkerBind::Create { handle } = bind else {
        panic!("fresh must not adopt");
    };
    assert_ne!(handle, "reviewer");
    assert!(handle.starts_with("reviewer-"));

    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Fresh,
        task: "brand new question",
        scope: None,
        prior_result: Some("old answer"),
        prior_user: None,
        selected_history: &[],
        can_resume: false,
        is_followup: false,
    });
    assert_eq!(rendered.continuity, ContextContinuity::None);
    assert_eq!(rendered.prompt, "brand new question");
    assert!(!rendered.prompt.contains("old answer"));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn requested_model_is_not_silently_remapped() {
    let choice = super::persist::resolve_model_choice(Some("luna-low"), Some("low"), None);
    assert_eq!(choice.requested_model.as_deref(), Some("luna-low"));
    assert_eq!(choice.resolved_model.as_deref(), Some("luna-low"));
    assert_eq!(choice.requested_reasoning.as_deref(), Some("low"));
    assert_eq!(choice.resolved_reasoning.as_deref(), Some("low"));
    assert!(!choice.mismatch);
    assert_ne!(choice.resolved_model.as_deref(), Some("composer-2.5"));

    let project = std::env::temp_dir().join(format!("pm-agent-persist-model-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "modeler", 0, "use this model");
    request.worker.requested_model = Some("luna-low".into());
    request.worker.resolved_model = Some("composer-2.5".into());
    request.worker.requested_reasoning = Some("low".into());
    request.worker.resolved_reasoning = Some("medium".into());
    let snapshot = operations::create_operation(request).unwrap().0;
    let view = super::runtime::AgentRunView::from(snapshot);
    assert_eq!(view.requested_model.as_deref(), Some("luna-low"));
    assert_eq!(view.resolved_model.as_deref(), Some("composer-2.5"));
    assert!(view.model_mismatch);
    let inspected =
        super::runtime::inspect_agent(&project_path, "modeler", Some("persist-session")).unwrap();
    assert_eq!(inspected["requested_model"], "luna-low");
    assert_eq!(inspected["resolved_model"], "composer-2.5");
    assert_eq!(inspected["model_mismatch"], true);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn inspect_reflects_default_context_packet_policy() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-packet-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    operations::create_operation(persist_req(&project_path, "packer", 0, "inspect me")).unwrap();
    let inspected =
        super::runtime::inspect_agent(&project_path, "packer", Some("persist-session")).unwrap();
    assert_eq!(inspected["context_policy"], "packet");
    assert_eq!(inspected["context_continuity"], "none");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn native_session_followup_reports_resume_continuity() {
    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "continue",
        scope: None,
        prior_result: Some("earlier"),
        prior_user: None,
        selected_history: &[],
        can_resume: true,
        is_followup: true,
    });
    assert_eq!(rendered.continuity, ContextContinuity::Resume);
    assert!(!rendered.prompt.contains("earlier"));
}

#[test]
fn ack_only_user_marker_is_reconstructed_when_resume_is_unavailable() {
    let project = std::env::temp_dir().join(format!("pm-ack-marker-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "luna",
        0,
        "remember ORBIT-74 and reply only ACK",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "ACK");

    let facts = super::persist::conversation_facts(&project_path, "luna").unwrap();
    assert!(facts.contains("ORBIT-74"));
    let previous = operations::latest_agent_run(&project_path, "luna").unwrap();
    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "What is the marker?",
        scope: None,
        prior_result: previous.result.as_deref(),
        prior_user: Some(facts.as_str()),
        selected_history: &[],
        can_resume: super::persist::snapshot_can_resume(&previous),
        is_followup: true,
    });
    assert_eq!(
        rendered.continuity,
        ContextContinuity::ReconstructedSummary
    );
    assert!(rendered.prompt.contains("ORBIT-74"));
    assert!(rendered.prompt.contains("What is the marker?"));
    assert_eq!(
        super::persist::followup_policy(None, ContextPolicy::Fresh),
        ContextPolicy::Packet
    );
    assert_eq!(
        super::persist::followup_policy(Some(ContextPolicy::Fresh), ContextPolicy::Packet),
        ContextPolicy::Fresh
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn captured_codex_thread_resumes_instead_of_reconstructing() {
    let project = std::env::temp_dir().join(format!("pm-codex-resume-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "luna", 0, "remember ORBIT-73");
    request.agent_type = "codex".into();
    request.worker.provider_session_id = Some("0199thread".into());
    let first = operations::create_operation(request).unwrap().0;
    finish_turn(&project_path, &first.operation_id, "ACK");
    let previous = operations::latest_agent_run(&project_path, "luna").unwrap();
    assert!(super::persist::snapshot_can_resume(&previous));
    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "What is the marker?",
        scope: None,
        prior_result: previous.result.as_deref(),
        prior_user: super::persist::conversation_facts(&project_path, "luna").as_deref(),
        selected_history: &[],
        can_resume: true,
        is_followup: true,
    });
    assert_eq!(rendered.continuity, ContextContinuity::Resume);
    assert_eq!(rendered.prompt, "What is the marker?");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn session_reset_is_visible_and_blocks_resume() {
    let project = std::env::temp_dir().join(format!("pm-session-reset-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let native = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    let rebound = operations::rebind_provider_session(
        &project_path,
        &native.operation_id,
        "ses-new",
        Some("ses-live"),
        false,
        None,
    )
    .unwrap();
    assert_eq!(rebound.worker.provider_session_id.as_deref(), Some("ses-new"));
    assert_eq!(rebound.stage.as_deref(), Some("session_replaced"));
    assert_eq!(
        rebound.required_action.as_ref().and_then(|value| value["kind"].as_str()),
        Some("session_replaced")
    );
    operations::mark_operation_finished(
        &project_path,
        &native.operation_id,
        Some("recovered".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let reset = operations::rebind_provider_session(
        &project_path,
        &native.operation_id,
        "ses-new",
        Some("ses-live"),
        true,
        None,
    )
    .unwrap();
    assert!(reset.worker.session_reset);
    assert!(!super::persist::snapshot_can_resume(&reset));
    let view = super::runtime::AgentRunView::from(reset.clone());
    assert!(view.session_reset);
    assert_eq!(view.provider_session_id.as_deref(), Some("ses-new"));
    let inspected =
        super::runtime::inspect_agent(&project_path, "native", Some("persist-session")).unwrap();
    assert_eq!(inspected["session_reset"], true);
    assert_eq!(inspected["provider_session_id"], "ses-new");
    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "continue",
        scope: None,
        prior_result: reset.result.as_deref(),
        prior_user: super::persist::conversation_facts(&project_path, "native").as_deref(),
        selected_history: &[],
        can_resume: super::persist::snapshot_can_resume(&reset),
        is_followup: true,
    });
    assert_eq!(
        rendered.continuity,
        ContextContinuity::ReconstructedSummary
    );
    let _ = fs::remove_dir_all(project);
}

fn running_turn(
    project: &str,
    handle: &str,
    agent_type: &str,
    pane_id: Option<&str>,
    pane_created: bool,
) -> operations::OperationSnapshot {
    let mut request = persist_req(project, handle, 0, "current turn");
    request.agent_type = agent_type.into();
    request.pane_id = pane_id.map(str::to_string);
    if agent_type == "opencode_native" {
        request.worker.provider_session_id = Some("ses-live".into());
    }
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(project, &queued.operation_id, |snapshot| {
        snapshot.pane_id = pane_id.map(str::to_string);
        snapshot.pane_created = pane_created;
        Ok(())
    })
    .unwrap()
}

#[test]
fn inferred_pane_result_without_checks_is_marked_unverified() {
    let project = std::env::temp_dir().join(format!("pm-inferred-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let pane = running_turn(&project_path, "pane", "cursor_agent", Some("pane-1"), true);
    let done = operations::mark_operation_finished(
        &project_path,
        &pane.operation_id,
        Some("The fix is in main.rs.".into()),
        ResultCapture::Inferred,
        false,
    )
    .unwrap();
    assert_eq!(done.status, OperationStatus::Completed);
    assert!(!done.verified);
    assert_eq!(done.acceptance_status, crate::operations::AcceptanceStatus::NotChecked);
    assert_eq!(
        done.result.as_deref(),
        Some("[unverified: read from terminal screen] The fix is in main.rs.")
    );
    let view = super::runtime::inspect_agent(&project_path, "pane", Some("persist-session")).unwrap();
    assert_eq!(view["verified"], false);
    assert!(view["warnings"]
        .to_string()
        .contains("PROVENANCE_INFERRED"));

    // An authoritative result is stored untouched.
    let head = running_turn(&project_path, "head", "cursor_agent", None, false);
    let done = operations::mark_operation_finished(
        &project_path,
        &head.operation_id,
        Some("PONG".into()),
        ResultCapture::Authoritative,
        false,
    )
    .unwrap();
    assert_eq!(done.result.as_deref(), Some("PONG"));
    let _ = fs::remove_dir_all(project);
}

fn headless_cursor_turn(
    project: &str,
    handle: &str,
    session: Option<&str>,
    pane: Option<&str>,
) -> operations::OperationSnapshot {
    let mut request = persist_req(project, handle, 1, "current turn");
    request.agent_type = "cursor_agent".into();
    request.pane_id = pane.map(str::to_string);
    request.worker.provider_session_id = session.map(str::to_string);
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(project, &queued.operation_id, |snapshot| {
        snapshot.pane_id = pane.map(str::to_string);
        Ok(())
    })
    .unwrap()
}

#[test]
fn busy_headless_cursor_with_a_session_is_steered_by_restart() {
    use super::messaging::{DeliveryMode, Disposition};
    let project = std::env::temp_dir().join(format!("pm-steer-restart-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = headless_cursor_turn(&project_path, "cur", Some("chat-1"), None);
    assert!(super::messaging::can_restart_steer(&running));
    let send = |mode, key: &str| {
        super::messaging::send_message(&project_path, "cur", "use tabs", &running, mode, Some(key))
            .unwrap()
    };
    let receipt = send(DeliveryMode::LiveOrQueue, "k1");
    assert_eq!(receipt.disposition, Disposition::AcceptedViaRestart);
    assert_eq!(
        super::messaging::receipt_value(&receipt)["disposition"],
        "accepted_via_restart"
    );
    assert_eq!(send(DeliveryMode::Live, "k2").disposition, Disposition::AcceptedViaRestart);
    assert_eq!(send(DeliveryMode::Queue, "k3").disposition, Disposition::Queued);
    // The restarted turn finishing settles the steer, with its result.
    super::messaging::mark_open_steers_processed_for_turn(&project_path, "cur", "tabs used", 2);
    let settled = super::messaging::find_receipt_by_key(&project_path, "cur", "k1").unwrap();
    assert_eq!(settled.disposition, Disposition::Processed);
    assert_eq!(settled.result.as_deref(), Some("tabs used"));
    assert!(super::capabilities::for_snapshot(&running)
        .notes
        .iter()
        .any(|note| note.starts_with("steer_via_restart")));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn steer_falls_back_to_queue_without_a_session_or_with_a_pane() {
    use super::messaging::{DeliveryMode, Disposition};
    let project = std::env::temp_dir().join(format!("pm-steer-norestart-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let no_session = headless_cursor_turn(&project_path, "nosess", None, None);
    assert!(!super::messaging::can_restart_steer(&no_session));
    let receipt = super::messaging::send_message(
        &project_path, "nosess", "m", &no_session, DeliveryMode::LiveOrQueue, Some("a"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Queued);
    let strict = super::messaging::send_message(
        &project_path, "nosess", "m", &no_session, DeliveryMode::Live, Some("b"),
    )
    .unwrap();
    assert_eq!(strict.disposition, Disposition::Unsupported);

    let pane = headless_cursor_turn(&project_path, "pane", Some("chat-2"), Some("pane-9"));
    assert!(!super::messaging::can_restart_steer(&pane));
    let receipt = super::messaging::send_message(
        &project_path, "pane", "m", &pane, DeliveryMode::LiveOrQueue, Some("c"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Queued);

    // Backends without resume never restart-steer.
    let codex_like = running_turn(&project_path, "claude", "claude", None, false);
    assert!(!super::messaging::can_restart_steer(&codex_like));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn send_message_steers_current_turn_followup_opens_next_turn() {
    let project = std::env::temp_dir().join(format!("pm-msg-vs-follow-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "codex", None, false);
    let receipt = super::messaging::send_message(
        &project_path,
        "worker",
        "use the new token",
        &running,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("steer-1"),
    )
    .unwrap();
    assert_eq!(receipt.turn_id, "turn-0");
    assert_eq!(receipt.disposition, super::messaging::Disposition::Queued);
    assert_eq!(operations::list_operations(&project_path).unwrap().len(), 1);
    assert_eq!(
        super::messaging::pending_texts(&project_path, "worker"),
        vec!["use the new token".to_string()]
    );

    let (queued, created) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "propose a fix".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(queued.turn_index, 1);
    assert_eq!(queued.status, OperationStatus::Queued);
    assert_eq!(operations::list_operations(&project_path).unwrap().len(), 2);
    let first = operations::get_operation(&project_path, &running.operation_id).unwrap();
    assert_eq!(first.turn_index, 0);
    assert_ne!(receipt.turn_id, format!("turn-{}", queued.turn_index));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn send_message_returns_receipt_and_duplicate_key_does_not_double_deliver() {
    let project = std::env::temp_dir().join(format!("pm-msg-receipt-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "codex", None, false);
    let first = super::messaging::send_message(
        &project_path,
        "worker",
        "steer once",
        &running,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("same-key"),
    )
    .unwrap();
    assert!(!first.message_id.is_empty());
    assert_eq!(first.disposition, super::messaging::Disposition::Queued);
    assert_eq!(first.idempotency_key, "same-key");
    let value = super::messaging::receipt_value(&first);
    assert_eq!(value["message_id"], first.message_id);
    assert_eq!(value["disposition"], "queued");
    assert_eq!(value["state"], "queued");

    let second = super::messaging::send_message(
        &project_path,
        "worker",
        "steer once again",
        &running,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("same-key"),
    )
    .unwrap();
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.text, first.text);
    assert_eq!(
        super::messaging::pending_texts(&project_path, "worker").len(),
        1
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn steering_reply_is_tracked_separately_as_processed() {
    let project = std::env::temp_dir().join(format!("pm-steer-result-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "opencode_native", Some("pane"), true);
    let receipt = super::messaging::send_message(
        &project_path,
        "worker",
        "change the filename",
        &running,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("steer-result"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, super::messaging::Disposition::Accepted);
    assert!(receipt.result.is_none());
    super::messaging::mark_open_steers_processed(&project_path, "worker", "renamed the file");
    let processed = super::messaging::latest_steer(&project_path, "worker").unwrap();
    assert_eq!(processed.disposition, super::messaging::Disposition::Processed);
    assert_eq!(processed.result.as_deref(), Some("renamed the file"));
    assert_eq!(processed.message_id, receipt.message_id);
    let view = super::runtime::AgentRunView::from(running);
    assert_eq!(view.last_steer.as_ref().unwrap().state, "processed");
    assert_eq!(
        view.last_steer.as_ref().unwrap().result.as_deref(),
        Some("renamed the file")
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn busy_tui_never_claims_injected_text_was_delivered() {
    let tui = super::messaging::SteerTarget {
        backend: "claude",
        busy: true,
        waiting_input: false,
        has_pane: true,
        can_live_steer: false,
        can_restart_steer: false,
    };
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::LiveOrQueue, tui),
        super::messaging::Disposition::Queued
    );
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::Live, tui),
        super::messaging::Disposition::Unsupported
    );
    let waiting = super::messaging::SteerTarget {
        waiting_input: true,
        ..tui
    };
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::LiveOrQueue, waiting),
        super::messaging::Disposition::Deferred
    );
    let headless = super::messaging::SteerTarget {
        backend: "codex",
        busy: true,
        waiting_input: false,
        has_pane: false,
        can_live_steer: false,
        can_restart_steer: false,
    };
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::LiveOrQueue, headless),
        super::messaging::Disposition::Queued
    );
    let native = super::messaging::SteerTarget {
        backend: "opencode_native",
        busy: true,
        waiting_input: false,
        has_pane: true,
        can_live_steer: true,
        can_restart_steer: false,
    };
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::LiveOrQueue, native),
        super::messaging::Disposition::Accepted
    );

    let project = std::env::temp_dir().join(format!("pm-msg-tui-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "tui", "claude", Some("pane-tui"), true);
    let receipt = super::messaging::send_message(
        &project_path,
        "tui",
        "do not inject this",
        &running,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("tui-steer"),
    )
    .unwrap();
    assert_ne!(receipt.disposition, super::messaging::Disposition::Accepted);
    assert!(matches!(
        receipt.disposition,
        super::messaging::Disposition::Queued | super::messaging::Disposition::Deferred
    ));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn queued_followup_deadline_starts_at_execution_not_enqueue() {
    assert!(
        !super::messaging::followup_timeout_expired(true, 0, None, 100, 10, 300_000),
        "queue wait must not use the execution timeout"
    );
    assert!(!super::messaging::followup_timeout_expired(
        true, 0, None, 299_999, 10, 300_000
    ));
    assert!(super::messaging::followup_timeout_expired(
        true, 0, None, 300_000, 10, 300_000
    ));
    assert!(!super::messaging::followup_timeout_expired(
        false,
        0,
        Some(200_000),
        209_999,
        10_000,
        300_000
    ));
    assert!(super::messaging::followup_timeout_expired(
        false,
        0,
        Some(200_000),
        210_000,
        10_000,
        300_000
    ));
}

#[test]
fn interrupt_stops_turn_keeps_worker_reusable_and_does_not_kill_pane() {
    let project = std::env::temp_dir().join(format!("pm-msg-interrupt-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "keeper", "claude", Some("pane-tui"), true);
    assert_eq!(
        super::messaging::interrupt_action(&running),
        Some(super::messaging::InterruptAction::SignalTui)
    );
    let mut requested = 0usize;
    let view = super::runtime::interrupt_active_turns(
        &project_path,
        "keeper",
        Some("persist-session"),
        |snapshot| {
            requested += 1;
            assert_eq!(snapshot.pane_id.as_deref(), Some("pane-tui"));
            assert!(snapshot.pane_created);
            assert_eq!(
                super::messaging::interrupt_action(snapshot),
                Some(super::messaging::InterruptAction::SignalTui)
            );
            Ok(())
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(requested, 1);
    assert_eq!(view.status, "interrupted");
    assert_eq!(view.handle, "keeper");
    let interrupted = operations::get_operation(&project_path, &running.operation_id).unwrap();
    assert_eq!(interrupted.status, OperationStatus::Cancelled);
    assert_eq!(interrupted.stage.as_deref(), Some("interrupted"));
    assert!(interrupted.keep_pane, "interrupt must not dispose the pane");
    assert_eq!(interrupted.pane_id.as_deref(), Some("pane-tui"));
    assert!(interrupted.pane_created);
    assert_eq!(
        interrupted.result.as_deref(),
        Some("turn interrupted before a final response was recorded")
    );

    let inspected =
        super::runtime::inspect_agent(&project_path, "keeper", Some("persist-session")).unwrap();
    assert_eq!(inspected["handle"], "keeper");
    assert_eq!(inspected["status"], "interrupted");
    assert_eq!(inspected["pane_id"], "pane-tui");

    let (next, created) = super::runtime::enqueue_followup(
        &project_path,
        "keeper",
        &interrupted,
        "continue after interrupt".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(next.turn_index, 1);
    let reused =
        super::runtime::inspect_agent(&project_path, "keeper", Some("persist-session")).unwrap();
    assert_eq!(reused["handle"], "keeper");
    assert_eq!(reused["turn_id"], "turn-1");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn interrupt_action_never_kills_process_close_is_dispose() {
    let project = std::env::temp_dir().join(format!("pm-msg-dispose-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let tui = running_turn(&project_path, "tui", "claude", Some("pane-tui"), true);
    let native = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    let headless = running_turn(&project_path, "headless", "codex", None, false);
    assert_eq!(
        super::messaging::interrupt_action(&tui),
        Some(super::messaging::InterruptAction::SignalTui)
    );
    assert_eq!(
        super::messaging::interrupt_action(&native),
        Some(super::messaging::InterruptAction::AbortNative)
    );
    assert_eq!(
        super::messaging::interrupt_action(&headless),
        Some(super::messaging::InterruptAction::StopHeadless)
    );
    operations::mark_operation_finished(
        &project_path,
        &tui.operation_id,
        Some("done".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let done = operations::get_operation(&project_path, &tui.operation_id).unwrap();
    assert!(super::messaging::interrupt_action(&done).is_none());
    let _ = fs::remove_dir_all(project);
}

fn note_haystack(notes: &[String]) -> String {
    notes.join("\n").to_ascii_lowercase()
}

#[test]
fn live_messages_claim_matches_delivery() {
    let project = std::env::temp_dir().join(format!("pm-cap-live-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let native = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    let native_caps = super::capabilities::for_snapshot(&native);
    assert!(native_caps.live_messages);
    let native_receipt = super::messaging::send_message(
        &project_path,
        "native",
        "steer live",
        &native,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("live-1"),
    )
    .unwrap();
    assert_eq!(
        native_receipt.disposition,
        super::messaging::Disposition::Accepted
    );

    let tui = running_turn(&project_path, "tui", "claude", Some("pane-tui"), true);
    let tui_caps = super::capabilities::for_snapshot(&tui);
    assert!(!tui_caps.live_messages);
    let tui_receipt = super::messaging::send_message(
        &project_path,
        "tui",
        "do not inject",
        &tui,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("tui-1"),
    )
    .unwrap();
    assert_ne!(
        tui_receipt.disposition,
        super::messaging::Disposition::Accepted
    );
    let started = super::runtime::AgentRunView::from(native);
    assert!(started.capabilities.live_messages);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn session_resume_claim_matches_inspect_continuity() {
    let project = std::env::temp_dir().join(format!("pm-cap-resume-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();

    let native = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    assert!(super::capabilities::for_snapshot(&native).session_resume);
    operations::mark_operation_finished(
        &project_path,
        &native.operation_id,
        Some("native answer".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let previous = operations::latest_agent_run(&project_path, "native").unwrap();
    let native_prompt = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "continue",
        scope: None,
        prior_result: previous.result.as_deref(),
        prior_user: None,
        selected_history: &[],
        can_resume: super::persist::snapshot_can_resume(&previous),
        is_followup: true,
    });
    assert_eq!(native_prompt.continuity, ContextContinuity::Resume);
    let mut native_follow = persist_req(&project_path, "native", 1, "continue");
    native_follow.agent_type = "opencode_native".into();
    native_follow.pane_id = Some("pane-native".into());
    native_follow.worker.provider_session_id = Some("ses-live".into());
    native_follow.worker.context_continuity = native_prompt.continuity;
    operations::create_operation(native_follow).unwrap();
    let native_inspect =
        super::runtime::inspect_agent(&project_path, "native", Some("persist-session")).unwrap();
    assert_eq!(native_inspect["capabilities"]["session_resume"], true);
    assert_eq!(native_inspect["context_continuity"], "resume");

    let cli = running_turn(&project_path, "cli", "codex", None, false);
    assert!(super::capabilities::for_snapshot(&cli).session_resume);
    operations::mark_operation_finished(
        &project_path,
        &cli.operation_id,
        Some("cli answer".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let previous_cli = operations::latest_agent_run(&project_path, "cli").unwrap();
    let cli_prompt = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Packet,
        task: "continue",
        scope: None,
        prior_result: previous_cli.result.as_deref(),
        prior_user: None,
        selected_history: &[],
        can_resume: super::persist::snapshot_can_resume(&previous_cli),
        is_followup: true,
    });
    assert_eq!(
        cli_prompt.continuity,
        ContextContinuity::ReconstructedSummary
    );
    let mut cli_follow = persist_req(&project_path, "cli", 1, "continue");
    cli_follow.agent_type = "codex".into();
    cli_follow.worker.context_continuity = cli_prompt.continuity;
    operations::create_operation(cli_follow).unwrap();
    let cli_inspect =
        super::runtime::inspect_agent(&project_path, "cli", Some("persist-session")).unwrap();
    assert_eq!(cli_inspect["capabilities"]["session_resume"], true);
    assert_eq!(cli_inspect["context_continuity"], "reconstructed_summary");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn graceful_interrupt_claim_vs_close_dispose() {
    let project = std::env::temp_dir().join(format!("pm-cap-interrupt-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let native = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    let caps = super::capabilities::for_snapshot(&native);
    assert!(caps.graceful_interrupt);
    assert_eq!(
        super::messaging::interrupt_action(&native),
        Some(super::messaging::InterruptAction::AbortNative)
    );
    let view = super::runtime::interrupt_active_turns(
        &project_path,
        "native",
        Some("persist-session"),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(view.status, "interrupted");
    let interrupted = operations::get_operation(&project_path, &native.operation_id).unwrap();
    assert!(interrupted.keep_pane);
    assert_eq!(interrupted.pane_id.as_deref(), Some("pane-native"));
    assert!(interrupted.pane_created);

    let tui = running_turn(&project_path, "tui", "claude", Some("pane-tui"), true);
    assert!(!super::capabilities::for_snapshot(&tui).graceful_interrupt);
    assert_eq!(
        super::messaging::interrupt_action(&tui),
        Some(super::messaging::InterruptAction::SignalTui)
    );
    let notes = note_haystack(&caps.notes);
    assert!(notes.contains("close_agent"));
    assert!(!notes.contains("exact parity"));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn queued_followups_claim_when_busy() {
    let project = std::env::temp_dir().join(format!("pm-cap-queue-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "codex", None, false);
    let caps = super::capabilities::for_snapshot(&running);
    assert!(caps.queued_followups);
    let inspected =
        super::runtime::inspect_agent(&project_path, "worker", Some("persist-session")).unwrap();
    assert_eq!(inspected["capabilities"]["queued_followups"], true);
    let (queued, created) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "next turn".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(queued.status, OperationStatus::Queued);
    assert_eq!(queued.turn_index, 1);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn visible_terminal_only_when_pane_exists() {
    let with_pane = super::capabilities::for_backend("cursor_agent", true);
    let without_pane = super::capabilities::for_backend("cursor_agent", false);
    assert!(with_pane.visible_terminal);
    assert!(!without_pane.visible_terminal);
    assert_eq!(
        with_pane.interface,
        super::capabilities::BackendInterface::TuiObservation
    );
    assert_eq!(
        without_pane.interface,
        super::capabilities::BackendInterface::StructuredHeadless
    );

    let project = std::env::temp_dir().join(format!("pm-cap-pane-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let pane = running_turn(&project_path, "tui", "cursor_agent", Some("pane-1"), true);
    let headless = running_turn(&project_path, "headless", "cursor_agent", None, false);
    assert!(super::capabilities::for_snapshot(&pane).visible_terminal);
    assert!(!super::capabilities::for_snapshot(&headless).visible_terminal);
    let pane_inspect =
        super::runtime::inspect_agent(&project_path, "tui", Some("persist-session")).unwrap();
    let headless_inspect =
        super::runtime::inspect_agent(&project_path, "headless", Some("persist-session")).unwrap();
    assert_eq!(pane_inspect["capabilities"]["visible_terminal"], true);
    assert_eq!(headless_inspect["capabilities"]["visible_terminal"], false);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_cursor_resumes_one_conversation_across_followups() {
    let caps = super::capabilities::for_backend("cursor_agent", false);
    assert!(caps.session_resume);
    assert!(!super::capabilities::for_backend("cursor_agent", true).session_resume);

    let project = std::env::temp_dir().join(format!("pm-cursor-resume-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut first = persist_req(&project_path, "cur", 0, "turn one");
    first.agent_type = "cursor_agent".into();
    let (op, _) = operations::create_operation(first).unwrap();
    // The first turn's JSON result carries the chat id; the exit path stores it on the worker.
    let session = crate::tui::profile_for("cursor_agent")
        .extract_provider_session(include_str!("../tui/cursor/fixtures/headless_result.json"))
        .unwrap();
    operations::set_provider_session(&project_path, &op.operation_id, &session).unwrap();
    let stored = operations::get_operation(&project_path, &op.operation_id).unwrap();
    assert!(super::persist::snapshot_can_resume(&stored));
    // The next turn's launch resumes that chat instead of starting a new one.
    let spec = crate::tui::headless_launch(
        "cursor_agent",
        &crate::tui::HeadlessLaunch {
            resume_session: stored.worker.provider_session_id.as_deref(),
            ..crate::tui::HeadlessLaunch::new(".", "turn two")
        },
    )
    .unwrap();
    assert!(spec
        .args
        .windows(2)
        .any(|pair| pair[0] == "--resume" && pair[1] == session));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_cursor_model_is_selectable_but_unconfirmed() {
    assert!(super::capabilities::for_backend("cursor_agent", false).model_selection);
    assert!(!super::capabilities::for_backend("cursor_agent", true).model_selection);
    assert!(!super::capabilities::for_backend("codex", false).model_selection);
    let facts = |has_pane, model_requested| crate::tui::RunFacts {
        result: Some("ok"),
        has_pane,
        completed: true,
        result_inferred: false,
        model_requested,
        model_unconfirmed: false,
    };
    let cursor = crate::tui::profile_for("cursor_agent");
    assert!(cursor
        .run_warnings(&facts(false, true))
        .iter()
        .any(|warning| warning.code == "MODEL_UNCONFIRMED"));
    assert!(cursor.run_warnings(&facts(false, false)).is_empty());
    assert!(cursor.run_warnings(&facts(true, true)).is_empty());
}

#[test]
fn cursor_tui_contract_documents_version_limits_without_parity_claims() {
    let caps = super::capabilities::for_backend("cursor_agent", true);
    assert!(!caps.live_messages);
    assert!(!caps.session_resume);
    assert!(!caps.graceful_interrupt);
    assert!(caps.queued_followups);
    let notes = note_haystack(&caps.notes);
    assert!(notes.contains("response boundaries"));
    assert!(notes.contains("question"));
    assert!(notes.contains("interrupt"));
    assert!(notes.contains("extraction") || notes.contains("screen delta"));
    assert!(notes.contains("not guaranteed"));
}

#[test]
fn fake_backend_covers_steer_answer_followup_interrupt_reuse() {
    let project = std::env::temp_dir().join(format!("pm-cap-e2e-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let started = running_turn(&project_path, "reviewer", "codex", None, false);
    let start_view = super::runtime::AgentRunView::from(started.clone());
    assert!(start_view.capabilities.queued_followups);
    assert!(!start_view.capabilities.live_messages);

    let receipt = super::messaging::send_message(
        &project_path,
        "reviewer",
        "prefer the short answer",
        &started,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("e2e-steer"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, super::messaging::Disposition::Queued);

    operations::mark_operation_finished(
        &project_path,
        &started.operation_id,
        Some("token expired".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let answered =
        super::runtime::inspect_agent(&project_path, "reviewer", Some("persist-session")).unwrap();
    assert_eq!(answered["status"], "completed");
    assert_eq!(answered["result"], "token expired");

    let previous = operations::latest_agent_run(&project_path, "reviewer").unwrap();
    let (followup, created) = super::runtime::enqueue_followup(
        &project_path,
        "reviewer",
        &previous,
        "propose a fix".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(followup.turn_index, 1);
    assert!(followup.task.contains("propose a fix"));
    operations::run_operation_dispatch(&project_path, &followup.operation_id, |_| Ok(())).unwrap();

    let interrupted = super::runtime::interrupt_active_turns(
        &project_path,
        "reviewer",
        Some("persist-session"),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(interrupted.status, "interrupted");
    assert_eq!(interrupted.handle, "reviewer");

    let latest = operations::latest_agent_run(&project_path, "reviewer").unwrap();
    let (reuse, created) = super::runtime::enqueue_followup(
        &project_path,
        "reviewer",
        &latest,
        "try again".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(reuse.turn_index, 2);
    let reused =
        super::runtime::inspect_agent(&project_path, "reviewer", Some("persist-session")).unwrap();
    assert_eq!(reused["handle"], "reviewer");
    assert_eq!(reused["turn_id"], "turn-2");
    assert_eq!(reused["capabilities"]["queued_followups"], true);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn pasted_prompt_fragments_are_not_an_assistant_result() {
    let task = "Remember the word READY ORCHID-731 and return it when asked.\n\nAcceptance criteria:\n- Return READY ORCHID-731";
    let screen = "\
[Pasted text #1 +36 lines]
Remember the word READY ORCHID-731 and return it when asked.
Acceptance criteria:
- Return READY ORCHID-731
Add follow-up >";
    assert!(clean_extracted_result(
        "[Pasted text #1 +36 lines]\nAcceptance criteria:\n- Return READY ORCHID-731",
        task
    )
    .is_none());
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >").with_task(task);
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 4);
    assert_ne!(
        matches!(decision, TuiTurnDecision::Complete { .. }),
        true,
        "prompt paste must not complete the turn: {decision:?}"
    );
}

#[test]
fn followup_does_not_inherit_previous_acceptance_criteria() {
    let project = std::env::temp_dir().join(format!("pm-accept-reset-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut first = persist_req(&project_path, "worker", 0, "remember the word");
    first.acceptance_criteria = Some(vec!["return READY ORCHID-731".into()]);
    let first = operations::create_operation(first).unwrap().0;
    finish_turn(&project_path, &first.operation_id, "ORCHID-731");
    let previous = operations::latest_agent_run(&project_path, "worker").unwrap();
    let (followup, created) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &previous,
        "summarize the outage".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert!(followup.task.contains("summarize the outage"));
    assert!(!followup.task.contains("READY ORCHID-731"));
    assert!(!followup.task.contains("Acceptance criteria"));
    assert_eq!(followup.acceptance_criteria, None);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn worker_cursor_is_monotonic_across_turns() {
    let project = std::env::temp_dir().join(format!("pm-cursor-mono-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "worker",
        0,
        "first",
    ))
    .unwrap()
    .0;
    let finished = {
        finish_turn(&project_path, &first.operation_id, "done");
        operations::latest_agent_run(&project_path, "worker").unwrap()
    };
    let first_cursor = super::runtime::AgentRunView::from(finished.clone()).next_cursor;
    assert!(first_cursor >= 2);
    let (followup, created) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &finished,
        "second".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    let second_cursor = super::runtime::AgentRunView::from(followup).next_cursor;
    assert!(
        second_cursor >= first_cursor,
        "next_cursor went backwards: {first_cursor} -> {second_cursor}"
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn interrupt_resolves_handle_without_the_original_project_path() {
    let project = std::env::temp_dir().join(format!("pm-interrupt-handle-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let other = std::env::temp_dir().join(format!("pm-interrupt-other-{}", Uuid::new_v4()));
    let other_path = other.to_string_lossy().into_owned();
    // Unique handle: resolve_agent_run scans every indexed project, so a shared
    // "keeper" collides with sibling tests and this assertion flakes.
    let handle = format!("keeper-{}", Uuid::new_v4());
    let running = running_turn(&project_path, &handle, "claude", Some("pane-tui"), true);
    let view = super::runtime::interrupt_active_turns(
        &other_path,
        &handle,
        Some("persist-session"),
        |snapshot| {
            assert_eq!(snapshot.operation_id, running.operation_id);
            Ok(())
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(view.status, "interrupted");
    assert_eq!(view.handle, handle);
    let _ = fs::remove_dir_all(project);
    let _ = fs::remove_dir_all(other);
}

#[test]
fn followup_returns_pane_gone_when_bound_pane_missing() {
    let err = super::pane_close::bound_pane_missing_error("gone-pane");
    assert_eq!(err.code, "PANE_GONE");
    assert!(err.recoverable);
    assert!(err.context.get("pane_id").is_some());
}

#[test]
fn close_agent_returns_closed_status() {
    let project = std::env::temp_dir().join(format!("pm-close-status-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "worker",
        0,
        "done work",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "answer");
    let closed = operations::mark_worker_closed(&project_path, "worker").unwrap();
    let view = super::runtime::AgentRunView::from(closed);
    assert_eq!(view.status, "closed");
    let inspected =
        super::runtime::inspect_agent(&project_path, "worker", Some("persist-session")).unwrap();
    assert_eq!(inspected["status"], "closed");
    assert_eq!(inspected["closed"], true);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn steering_a_finished_turn_is_rejected_unless_explicitly_queued() {
    let idle = super::messaging::SteerTarget {
        backend: "opencode_native",
        busy: false,
        waiting_input: false,
        has_pane: true,
        can_live_steer: true,
        can_restart_steer: false,
    };
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::LiveOrQueue, idle),
        super::messaging::Disposition::Rejected
    );
    assert_eq!(
        super::messaging::decide_delivery(super::messaging::DeliveryMode::Queue, idle),
        super::messaging::Disposition::Queued
    );
    let value = super::messaging::receipt_value(&super::messaging::MessageReceipt {
        message_id: "m1".into(),
        idempotency_key: "k1".into(),
        disposition: super::messaging::Disposition::Rejected,
        handle: "worker".into(),
        turn_id: "turn-0".into(),
        text: "too late".into(),
        result: None,
    });
    assert_eq!(value["disposition"], "rejected");
    assert!(value["suggestion"]
        .as_str()
        .unwrap()
        .contains("followup_task"));
}

#[test]
fn later_turn_view_omits_previous_steer_result() {
    let project = std::env::temp_dir().join(format!("pm-steer-compact-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "opencode_native", Some("pane"), true);
    super::messaging::send_message(
        &project_path,
        "worker",
        "change the filename",
        &running,
        super::messaging::DeliveryMode::LiveOrQueue,
        Some("steer-compact"),
    )
    .unwrap();
    super::messaging::mark_open_steers_processed(&project_path, "worker", "long previous analysis");
    operations::mark_operation_finished(
        &project_path,
        &running.operation_id,
        Some("READY".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let previous = operations::latest_agent_run(&project_path, "worker").unwrap();
    let (followup, _) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &previous,
        "one line".into(),
        Some("persist-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    let view = super::runtime::AgentRunView::from(followup);
    assert!(
        view.last_steer
            .as_ref()
            .and_then(|steer| steer.result.as_deref())
            .is_none(),
        "previous steer result must not copy into the next turn"
    );
    let compact = serde_json::to_value(&view).unwrap();
    assert!(compact.get("capabilities").is_none());
    let _ = fs::remove_dir_all(project);
}

#[test]
fn followup_idempotency_key_reuses_the_same_operation() {
    let project = std::env::temp_dir().join(format!("pm-followup-idem-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "codex", None, false);
    let opts = super::persist::FollowupOpts {
        idempotency_key: Some("followup-key".into()),
        ..Default::default()
    };
    let (first, created) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "next step".into(),
        None,
        opts.clone(),
    )
    .unwrap();
    assert!(created);
    let (second, created_again) = super::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "next step".into(),
        None,
        opts,
    )
    .unwrap();
    assert!(!created_again);
    assert_eq!(first.operation_id, second.operation_id);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn fresh_context_policy_renders_plain_task_without_packet_sections() {
    let rendered = super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
        policy: ContextPolicy::Fresh,
        task: "Review docs/orchestrator/quickstart.md",
        scope: None,
        prior_result: None,
        prior_user: None,
        selected_history: &[],
        can_resume: false,
        is_followup: false,
    });
    assert_eq!(rendered.prompt, "Review docs/orchestrator/quickstart.md");
}

#[test]
fn headless_mark_stop_failed_is_not_overwritten_by_late_exit_handler() {
    let project = std::env::temp_dir().join(format!("pm-stop-once-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let queued = operations::create_operation(persist_req(&project_path, "worker", 0, "task"))
        .unwrap()
        .0;
    let stop_error = OperationError::new("WORKER_STOP_FAILED", "taskkill failed", true);
    let failed = operations::mark_operation_state(
        &project_path,
        &queued.operation_id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("stop_failed".into()),
        None,
        Some(stop_error),
    )
    .unwrap();
    let again = operations::mark_operation_state(
        &project_path,
        &queued.operation_id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("failed".into()),
        None,
        Some(OperationError::new("WORKER_EXIT_FAILED", "late exit", true)),
    )
    .unwrap();
    assert_eq!(again.stage.as_deref(), Some("stop_failed"));
    assert_eq!(
        again.error.as_ref().map(|e| e.code.as_str()),
        Some("WORKER_STOP_FAILED")
    );
    assert_eq!(failed.operation_id, again.operation_id);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn followup_keeps_provider_session_on_same_worker() {
    let project = std::env::temp_dir().join(format!("pm-session-stable-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    let session = first
        .worker
        .provider_session_id
        .clone()
        .unwrap_or_else(|| "ses-live".into());
    let (followup, _) = super::runtime::enqueue_followup(
        &project_path,
        "native",
        &first,
        "follow one".into(),
        None,
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert_eq!(
        followup.worker.provider_session_id.as_deref(),
        Some(session.as_str())
    );
    let _ = fs::remove_dir_all(project);
}

fn run_with_checks(name: &str, checks: Vec<super::checks::Check>) -> (String, String) {
    let project = std::env::temp_dir().join(format!("pm-checks-{name}-{}", Uuid::new_v4()));
    fs::create_dir_all(&project).unwrap();
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "worker", 0, "task");
    request.checks = checks;
    let (op, _) = operations::create_operation(request).unwrap();
    for status in [OperationStatus::Starting, OperationStatus::Running] {
        operations::mark_operation_state(
            &op.project_path,
            &op.operation_id,
            status,
            StateSource::Native,
            None,
            None,
            None,
        )
        .unwrap();
    }
    (op.project_path, op.operation_id)
}

#[test]
fn failing_check_turns_completion_into_acceptance_failed() {
    let (project, id) = run_with_checks(
        "fail",
        vec![super::checks::Check::FileExists { path: "out.txt".into() }],
    );
    let done = operations::mark_operation_completed(&project, &id, "all done", true).unwrap();
    assert_eq!(done.status, OperationStatus::Failed);
    assert!(!done.verified);
    assert_eq!(done.error.as_ref().map(|e| e.code.as_str()), Some("ACCEPTANCE_FAILED"));
    let view = super::runtime::AgentRunView::from(done);
    assert_eq!(view.checks_configured, 1);
    assert_eq!(view.checks.len(), 1);
    assert!(!view.checks[0].passed);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn passing_check_completes_and_attaches_results() {
    let (project, id) = run_with_checks(
        "pass",
        vec![super::checks::Check::FileExists { path: "out.txt".into() }],
    );
    fs::write(std::path::Path::new(&project).join("out.txt"), "x").unwrap();
    let done = operations::mark_operation_completed(&project, &id, "all done", true).unwrap();
    assert_eq!(done.status, OperationStatus::Completed);
    assert!(done.verified);
    let view = super::runtime::AgentRunView::from(done);
    assert_eq!(view.checks.len(), 1);
    assert!(view.checks[0].passed);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn run_without_checks_omits_checks_in_view_json() {
    let (project, id) = run_with_checks("none", vec![]);
    let done = operations::mark_operation_completed(&project, &id, "ok", true).unwrap();
    assert_eq!(done.status, OperationStatus::Completed);
    let value = serde_json::to_value(super::runtime::AgentRunView::from(done)).unwrap();
    assert!(value.get("checks").is_none());
    let _ = fs::remove_dir_all(project);
}

#[test]
fn wait_returns_the_result_unless_the_caller_already_saw_that_revision() {
    // Regression: a run finished before wait_agents started, no cursor was passed, and the result
    // came back null with result_unchanged=true.
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.result = Some("the answer".into());
    snapshot.revision = 7;
    let fresh = super::runtime::agent_view_after_wait(snapshot.clone(), None, None);
    assert_eq!(fresh.result.as_deref(), Some("the answer"));
    assert_eq!(fresh.result_unchanged, None);
    let seen = super::runtime::agent_view_after_wait(snapshot.clone(), Some(7), None);
    assert_eq!(seen.result, None);
    assert_eq!(seen.result_unchanged, Some(true));
    let behind = super::runtime::agent_view_after_wait(snapshot, Some(3), None);
    assert_eq!(behind.result.as_deref(), Some("the answer"));
}

#[test]
fn duration_of_an_active_run_tracks_elapsed_time_not_the_last_update() {
    let mut snapshot = parked_snapshot(OperationStatus::Running, "none");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    snapshot.started_at_ms = Some(now - 45_000);
    snapshot.updated_at_ms = now - 44_971; // last event 29 ms after start
    snapshot.finished_at_ms = None;
    let view = super::runtime::AgentRunView::from(snapshot.clone());
    let duration = view.duration_ms.expect("duration");
    assert!(duration >= 45_000, "active run showed {duration} ms");
    // A finished run is measured to its finish time, not to now.
    snapshot.status = OperationStatus::Completed;
    snapshot.finished_at_ms = Some(now - 40_000);
    let done = super::runtime::AgentRunView::from(snapshot);
    assert_eq!(done.duration_ms, Some(5_000));
}

#[test]
fn followup_keeps_checks_it_was_given_and_rejects_bad_ones() {
    let (project, id) = run_with_checks("adopt", vec![]);
    let done = operations::mark_operation_completed(&project, &id, "PONG", true).unwrap();
    let handle = done.agent_run_id.clone();
    let opts = |text: &str| super::persist::FollowupOpts {
        checks: Some(vec![super::checks::Check::ResultContains { text: text.into() }]),
        locks: vec!["file:src/a.rs".into()],
        ..Default::default()
    };
    let (next, created) =
        super::runtime::enqueue_followup(&project, &handle, &done, "again".into(), None, opts("ZZZ"))
            .unwrap();
    assert!(created);
    assert_eq!(next.checks.len(), 1);
    assert_eq!(next.locks, vec!["file:src/a.rs".to_string()]);
    let bad = super::persist::FollowupOpts {
        checks: Some(vec![super::checks::Check::ResultContains { text: String::new() }]),
        idempotency_key: Some("other".into()),
        ..Default::default()
    };
    let error = super::runtime::enqueue_followup(&project, &handle, &next, "x".into(), None, bad)
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn conversation_replay_does_not_nest_prior_context() {
    let stored = "second question\n\nPrior context:\nUser: first\nAssistant: a\n\nAcceptance criteria:\n- x";
    assert_eq!(super::persist::original_task_for_test(stored), "second question");
}

#[test]
fn status_line_is_chrome_and_repeats_collapse() {
    let cleaned = super::completion::clean_extracted_result(
        "done\ndone\nComposer 2.5 · 7.2%",
        "",
    );
    assert_eq!(cleaned.as_deref(), Some("done"));
}

#[test]
fn add_a_follow_up_hint_is_chrome() {
    let cleaned = super::completion::clean_extracted_result("PONG\n→ Add a follow-up", "");
    assert_eq!(cleaned.as_deref(), Some("PONG"));
}

#[test]
fn live_pane_followup_sends_only_the_new_task() {
    let mut previous = parked_snapshot(OperationStatus::Completed, "none");
    previous.pane_id = Some("pane-1".into());
    assert!(super::persist::pane_keeps_context(&previous));
    let render = |can_resume: bool| {
        super::persist::render_turn_prompt(super::persist::TurnPromptArgs {
            policy: ContextPolicy::Packet,
            task: "second question",
            scope: None,
            prior_result: Some("first answer"),
            prior_user: Some("User: first question\nAssistant: first answer"),
            selected_history: &[],
            can_resume,
            is_followup: true,
        })
    };
    assert_eq!(render(true).prompt, "second question");
    assert!(render(false).prompt.contains("Prior context"));
    previous.pane_id = None;
    assert!(!super::persist::pane_keeps_context(&previous));
}

#[test]
fn slow_start_gets_one_enter_retry_before_failing() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    let mut seen_retry = 0;
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..NOT_SUBMITTED_TICKS {
        last = observe_tui_turn(&mut watch, "idle", "Add follow-up >", false);
        if last == TuiTurnDecision::RetrySubmit {
            seen_retry += 1;
        }
    }
    assert_eq!(seen_retry, 1);
    assert_eq!(last, TuiTurnDecision::NotSubmitted);
}

#[test]
fn busy_pane_never_triggers_submit_retry() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    observe_tui_turn(&mut watch, "running", "working", false);
    for _ in 0..NOT_SUBMITTED_TICKS {
        let decision = observe_tui_turn(&mut watch, "idle", "Add follow-up >", false);
        assert_ne!(decision, TuiTurnDecision::RetrySubmit);
        assert_ne!(decision, TuiTurnDecision::NotSubmitted);
    }
}

#[test]
fn cursor_startup_banner_and_cwd_line_are_not_result_text() {
    let screen = "Cursor Agent\nv2026.10.01-e373342\nTip: Use /plan to plan execution.\nPONG\n~\\Desktop\\pm-bench";
    assert_eq!(
        super::completion::clean_extracted_result(screen, "").as_deref(),
        Some("PONG")
    );
    // A real reply that merely mentions a version or folder is kept.
    let reply = "Upgrade to v2 now, then open ~/notes in your editor.";
    assert_eq!(
        super::completion::clean_extracted_result(reply, "").as_deref(),
        Some(reply)
    );
}

#[test]
fn native_prompt_that_never_landed_is_detected_only_when_idle_and_late() {
    let view = |ids: &[&str]| SessionMessagesView {
        pane_id: "p".into(),
        session_id: "s".into(),
        messages: ids
            .iter()
            .map(|id| MessageView {
                id: Some(id.to_string()),
                role: "user".into(),
                model: None,
                finish: None,
                error: None,
                parts: vec![],
            })
            .collect(),
        pending_question: None,
        last_assistant_text: None,
    };
    let baseline = vec!["m1".to_string()];
    let late = 1_000_000;
    let started = Some(late - super::native::NATIVE_PROMPT_LANDING_MS - 1);
    // Nothing new, idle, past the window: lost.
    assert!(super::native::native_prompt_never_landed(&view(&["m1"]), &baseline, started, late, "idle"));
    // A new message exists: it landed.
    assert!(!super::native::native_prompt_never_landed(&view(&["m1", "m2"]), &baseline, started, late, "idle"));
    // Still busy, or too early, or never started: not lost.
    assert!(!super::native::native_prompt_never_landed(&view(&["m1"]), &baseline, started, late, "running"));
    assert!(!super::native::native_prompt_never_landed(&view(&["m1"]), &baseline, Some(late - 1_000), late, "idle"));
    assert!(!super::native::native_prompt_never_landed(&view(&["m1"]), &baseline, None, late, "idle"));
    // A stuck non-idle pane with no new message is lost only after the longer cap.
    let stuck = Some(late - super::native::NATIVE_PROMPT_LANDING_MS * 3 - 1);
    assert!(super::native::native_prompt_never_landed(&view(&["m1"]), &baseline, stuck, late, "running"));
    assert!(!super::native::native_prompt_never_landed(&view(&["m2", "m1"]), &baseline, stuck, late, "running"));
}

#[test]
fn cursor_tool_activity_lines_are_not_part_of_the_result() {
    let raw = "Read todo.py\n$ cd proj && python -m pytest 11s\n… 2 output lines hidden · ctrl+o to expand\nAll 12 tests pass.\nComposer 2.5 · 7.2%";
    let cleaned = super::completion::clean_extracted_result(raw, "");
    assert_eq!(cleaned.as_deref(), Some("All 12 tests pass."));
    // Ordinary prose that starts with "Read" survives.
    let prose = super::completion::clean_extracted_result("Read the docs first.", "");
    assert_eq!(prose.as_deref(), Some("Read the docs first."));
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
    assert!(!launch(false, false).unwrap().args.iter().any(|arg| arg == "--force"));
    assert!(launch(false, true).unwrap().args.iter().any(|arg| arg == "--force"));
    assert!(launch(true, true).is_err());
}

#[test]
fn untouched_cursor_pane_is_not_a_result() {
    assert!(super::completion::clean_extracted_result("→ Plan, search, build anything\nComposer 2.5", "").is_none());
    // Text above the last tool marker is tool output, not the reply.
    let cleaned = super::completion::clean_extracted_result(
        "partial output\n$ python todo.py list 3s\nThere are 3 items.",
        "",
    );
    assert_eq!(cleaned.as_deref(), Some("There are 3 items."));
}

#[test]
fn cursor_shell_turn_capture_keeps_only_the_reply() {
    // Real capture: tool output is indented deeper than the `$` line; the reply is back at its column.
    let screen = "  Using your shell tool, run `python todo.py list` and report the output.\n\n  $ python todo.py list 14s\n    … 1 output lines hidden · ctrl+o to expand\n    2 [ ] recheck-item\n    3 [ ] should-not-exist\n\n  python todo.py list exited with code 0 and printed:\n\n  1 [ ] bench-item-B\n  2 [ ] recheck-item\n  3 [ ] should-not-exist\n\n  All three items are unchecked ([ ]).\n";
    let evidence = super::completion::tui_turn_evidence(screen, "").expect("evidence");
    let cleaned = super::completion::clean_extracted_result(
        &evidence,
        "Using your shell tool, run `python todo.py list` and report the output.",
    )
    .expect("result");
    assert!(!cleaned.contains("hidden"), "{cleaned}");
    assert!(cleaned.starts_with("python todo.py list exited with code 0"), "{cleaned}");
    assert!(cleaned.contains("1 [ ] bench-item-B"));
    assert!(cleaned.ends_with("All three items are unchecked ([ ])."));
    // The stray tool lines appear once (in the reply), not twice.
    assert_eq!(cleaned.matches("3 [ ] should-not-exist").count(), 1);
}

#[test]
fn cursor_default_model_is_parsed_from_about_output() {
    let about = "About Cursor CLI\n\nCLI Version         2026.10.01-e373342\nDefault Model:      \u{1b}[1mComposer 2.5\u{1b}[0m\nSubscription Tier   Pro+\n";
    assert_eq!(
        crate::tui::cursor::parse_default_model(about).as_deref(),
        Some("Composer 2.5")
    );
    assert!(crate::tui::cursor::parse_default_model("Version: 1").is_none());
}

#[test]
fn cursor_model_is_parsed_from_real_about_columns() {
    let about = "About Cursor CLI\n\nCLI Version         2026.10.01-e373342\nLatest              2026.10.01-e373342 (up to date)\nModel               Composer 2.5\nSubscription Tier   Pro+\n";
    assert_eq!(
        crate::tui::cursor::parse_default_model(about).as_deref(),
        Some("Composer 2.5")
    );
}

fn receipt_of(snapshot: operations::OperationSnapshot) -> super::runtime::RunReceipt {
    super::runtime::AgentRunView::from(snapshot)
        .receipt
        .expect("every view carries a receipt")
}

#[test]
fn receipt_for_unchecked_inferred_run_lists_what_was_not_verified() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.result_capture = Some(operations::ResultCapture::Inferred);
    snapshot.worker.requested_model = Some("gpt-x".into());
    let receipt = receipt_of(snapshot);
    assert_eq!(receipt.backend, "cursor_agent");
    assert_eq!(receipt.read_only, "enforced");
    assert_eq!(receipt.checks, "none configured");
    assert_eq!(receipt.result_capture, "inferred");
    assert_eq!(receipt.model.as_deref(), Some("gpt-x"));
    assert_eq!(
        receipt.not_verified,
        vec![
            "no checks configured",
            "result read from terminal screen",
            "model not confirmed"
        ]
    );
}

#[test]
fn receipt_counts_passed_checks_and_confirmed_model_is_not_flagged() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.result_capture = Some(operations::ResultCapture::Authoritative);
    snapshot.read_only = false;
    snapshot.worker.requested_model = Some("m".into());
    snapshot.worker.resolved_model = Some("m".into());
    let check = super::checks::Check::FileExists { path: "a".into() };
    snapshot.checks = vec![check.clone(), check.clone()];
    snapshot.check_results = vec![
        super::checks::CheckResult { check: check.clone(), passed: true, detail: String::new() },
        super::checks::CheckResult { check, passed: false, detail: String::new() },
    ];
    let receipt = receipt_of(snapshot);
    assert_eq!(receipt.checks, "1/2 passed");
    assert_eq!(receipt.read_only, "off");
    assert!(receipt.not_verified.is_empty(), "{:?}", receipt.not_verified);
}

#[test]
fn receipt_flags_read_only_the_backend_cannot_enforce() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.agent_type = "opencode".into();
    let receipt = receipt_of(snapshot);
    assert_eq!(receipt.read_only, "unenforced");
    assert!(receipt.not_verified.contains(&"read_only not enforced".to_string()));
}

#[test]
fn receipt_serializes_additively_and_running_runs_do_not_claim_unverified_model() {
    let mut snapshot = parked_snapshot(OperationStatus::Running, "none");
    snapshot.worker.requested_model = Some("m".into());
    let view = super::runtime::AgentRunView::from(snapshot);
    let value = serde_json::to_value(&view).unwrap();
    assert_eq!(value["receipt"]["result_capture"], "pending");
    let not_verified = value["receipt"]["not_verified"].as_array().unwrap();
    assert!(!not_verified.iter().any(|entry| entry == "model not confirmed"));
}

#[test]
fn duration_measures_prompt_to_finish_and_total_includes_setup() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.created_at_ms = 1_000;
    snapshot.started_at_ms = Some(51_000);
    snapshot.finished_at_ms = Some(62_000);
    let value = serde_json::to_value(super::runtime::AgentRunView::from(snapshot)).unwrap();
    assert_eq!(value["duration_ms"], 11_000);
    assert_eq!(value["setup_ms"], 50_000);
    assert_eq!(value["total_ms"], 61_000);
    assert_eq!(value["receipt"]["build_id"], env!("PM_BUILD_ID"));
}

#[test]
fn pane_open_is_false_while_an_auto_closing_pane_is_still_in_the_registry() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.pane_created = true;
    snapshot.keep_pane = false;
    let (open, closing, note) = super::runtime::pane_presentation(&snapshot, true, false);
    assert!(!open && closing);
    assert!(note.unwrap().contains("being closed"));
    // The dispose is skipped when another operation holds the pane: it really stays open.
    assert_eq!(super::runtime::pane_presentation(&snapshot, true, true), (true, false, None));
    // keep_pane runs and adopted panes are never auto-closed.
    snapshot.keep_pane = true;
    let (open, closing, note) = super::runtime::pane_presentation(&snapshot, true, false);
    assert!(open && !closing && note.unwrap().contains("keep_pane=true"));
    snapshot.keep_pane = false;
    snapshot.pane_created = false;
    assert_eq!(super::runtime::pane_presentation(&snapshot, true, false), (true, false, None));
}

#[test]
fn receipt_outcome_is_the_single_answer_to_did_it_work() {
    let check = super::checks::Check::FileExists { path: "a".into() };
    let passed = |ok: bool| super::checks::CheckResult { check: check.clone(), passed: ok, detail: String::new() };

    // Completed with every check passed: the requested behaviour was proven.
    let mut proven = parked_snapshot(OperationStatus::Completed, "none");
    proven.checks = vec![check.clone()];
    proven.check_results = vec![passed(true)];
    assert_eq!(receipt_of(proven).outcome, "passed_checks");

    // Completed with nothing checking it: finished is not the same as proven.
    let unchecked = parked_snapshot(OperationStatus::Completed, "none");
    assert_eq!(receipt_of(unchecked).outcome, "completed_unchecked");

    // A failing check fails the run.
    let mut failed_checks = parked_snapshot(OperationStatus::Failed, "none");
    failed_checks.checks = vec![check.clone()];
    failed_checks.check_results = vec![passed(false)];
    failed_checks.error = Some(OperationError::new("ACCEPTANCE_FAILED", "check failed", false));
    assert_eq!(receipt_of(failed_checks).outcome, "failed_checks");

    let mut failed = parked_snapshot(OperationStatus::Failed, "none");
    failed.error = Some(OperationError::new("RESULT_NOT_CAPTURED", "x", true));
    assert_eq!(receipt_of(failed).outcome, "failed");

    assert_eq!(receipt_of(parked_snapshot(OperationStatus::WaitingInput, "none")).outcome, "needs_input");
    assert_eq!(receipt_of(parked_snapshot(OperationStatus::Running, "none")).outcome, "in_progress");
    assert_eq!(receipt_of(parked_snapshot(OperationStatus::Cancelled, "none")).outcome, "cancelled");
}

#[test]
fn every_steer_disposition_says_whether_it_was_applied_and_what_to_do() {
    use super::messaging::Disposition;
    let all = [
        Disposition::Accepted,
        Disposition::AcceptedViaRestart,
        Disposition::Queued,
        Disposition::Processed,
        Disposition::Deferred,
        Disposition::Unsupported,
        Disposition::Expired,
        Disposition::Rejected,
    ];
    for disposition in all {
        assert!(!disposition.next_action().is_empty(), "{disposition:?}");
    }
    // Applied means it reached the turn it was meant for; a queued correction does not change
    // the running turn, which is exactly what the receipt must not hide.
    for applied in [Disposition::Accepted, Disposition::AcceptedViaRestart, Disposition::Processed] {
        assert!(applied.applied(), "{applied:?}");
    }
    for not_applied in [
        Disposition::Queued,
        Disposition::Deferred,
        Disposition::Unsupported,
        Disposition::Expired,
        Disposition::Rejected,
    ] {
        assert!(!not_applied.applied(), "{not_applied:?}");
    }
    assert!(Disposition::Queued.next_action().contains("followup_task"));
    assert!(Disposition::Expired.next_action().contains("followup_task"));
}

#[test]
fn a_retried_run_agent_call_gets_the_same_handle_but_different_calls_do_not() {
    let a = super::persist::stable_handle("C:/proj", "key-1");
    assert_eq!(a, super::persist::stable_handle("C:/proj", "key-1"));
    assert_eq!(a, super::persist::stable_handle("C:/proj", " key-1 "), "keys are trimmed");
    assert_ne!(a, super::persist::stable_handle("C:/proj", "key-2"));
    assert_ne!(a, super::persist::stable_handle("C:/other", "key-1"));
    // Uuid-shaped like every other handle.
    assert_eq!(a.len(), 36);
    assert!(a.chars().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() }), "{a}");
}

#[test]
fn cancel_releases_a_run_spawned_pane_unless_keep_pane_was_asked_for() {
    let mut spawned = parked_snapshot(OperationStatus::Cancelled, "none");
    spawned.pane_created = true;
    spawned.keep_pane = false;
    assert!(super::pane_close::should_dispose_after_terminal(&spawned));
    spawned.keep_pane = true;
    assert!(!super::pane_close::should_dispose_after_terminal(&spawned));
    // A pane the user opened (adopted, not created by the run) is never closed for them.
    let mut adopted = parked_snapshot(OperationStatus::Cancelled, "none");
    adopted.pane_created = false;
    adopted.keep_pane = false;
    assert!(!super::pane_close::should_dispose_after_terminal(&adopted));
}

#[test]
fn an_empty_json_reply_is_a_result_but_rules_and_box_art_are_not() {
    // Real case: the agent printed the JSON list `[]` and the result came back as None.
    assert_eq!(
        super::completion::clean_extracted_result("The JSON printed was:\n[]", "").as_deref(),
        Some("The JSON printed was:\n[]")
    );
    assert_eq!(super::completion::clean_extracted_result("[]", "").as_deref(), Some("[]"));
    assert_eq!(super::completion::clean_extracted_result("{}", "").as_deref(), Some("{}"));
    assert!(super::completion::clean_extracted_result("────────\n▄▄▄▄▄▄\n   \n", "").is_none());
}
