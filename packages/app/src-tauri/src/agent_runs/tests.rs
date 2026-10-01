use super::completion::{
    clean_extracted_result, observe_tui_turn, tui_turn_evidence, TuiTurnDecision, TuiTurnWatch,
    RECONCILE_IDLE_TICKS,
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
        RECONCILE_IDLE_TICKS,
    );
    assert_eq!(decision, TuiTurnDecision::AmbiguousIdle);
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
        ResultCapture::Inferred,
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
