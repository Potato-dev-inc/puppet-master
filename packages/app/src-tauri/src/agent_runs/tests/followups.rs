//! Follow-up turns on an existing handle: idempotency, inherited state, cursors.

use crate::operations::{self, ContextPolicy, OperationStatus};
use serde_json::json;
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

#[test]
fn headless_cursor_resumes_one_conversation_across_followups() {
    let _cli = crate::tui::cursor::binary::stub::installed();
    let caps = crate::agent_runs::capabilities::for_backend("cursor_agent", false);
    assert!(caps.session_resume);
    assert!(!crate::agent_runs::capabilities::for_backend("cursor_agent", true).session_resume);

    let project = std::env::temp_dir().join(format!("pm-cursor-resume-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut first = persist_req(&project_path, "cur", 0, "turn one");
    first.agent_type = "cursor_agent".into();
    let (op, _) = operations::create_operation(first).unwrap();
    // The first turn's JSON result carries the chat id; the exit path stores it on the worker.
    let session = crate::tui::profile_for("cursor_agent")
        .extract_provider_session(include_str!(
            "../../tui/cursor/fixtures/headless_result.json"
        ))
        .unwrap();
    operations::set_provider_session(&project_path, &op.operation_id, &session).unwrap();
    let stored = operations::get_operation(&project_path, &op.operation_id).unwrap();
    assert!(crate::agent_runs::persist::snapshot_can_resume(&stored));
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
    assert!(crate::agent_runs::capabilities::for_backend("cursor_agent", false).model_selection);
    assert!(!crate::agent_runs::capabilities::for_backend("cursor_agent", true).model_selection);
    assert!(!crate::agent_runs::capabilities::for_backend("codex", false).model_selection);
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
fn followup_does_not_inherit_previous_acceptance_criteria() {
    let project = std::env::temp_dir().join(format!("pm-accept-reset-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut first = persist_req(&project_path, "worker", 0, "remember the word");
    first.acceptance_criteria = Some(vec!["return READY ORCHID-731".into()]);
    let first = operations::create_operation(first).unwrap().0;
    finish_turn(&project_path, &first.operation_id, "ORCHID-731");
    let previous = operations::latest_agent_run(&project_path, "worker").unwrap();
    let (followup, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "worker",
        &previous,
        "summarize the outage".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
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
    let first = operations::create_operation(persist_req(&project_path, "worker", 0, "first"))
        .unwrap()
        .0;
    let finished = {
        finish_turn(&project_path, &first.operation_id, "done");
        operations::latest_agent_run(&project_path, "worker").unwrap()
    };
    let first_cursor = crate::agent_runs::runtime::AgentRunView::from(finished.clone()).next_cursor;
    assert!(first_cursor >= 2);
    let (followup, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "worker",
        &finished,
        "second".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    let second_cursor = crate::agent_runs::runtime::AgentRunView::from(followup).next_cursor;
    assert!(
        second_cursor >= first_cursor,
        "next_cursor went backwards: {first_cursor} -> {second_cursor}"
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn followup_returns_pane_gone_when_bound_pane_missing() {
    let err = crate::agent_runs::pane_close::bound_pane_missing_error("gone-pane");
    assert_eq!(err.code, "PANE_GONE");
    assert!(err.recoverable);
    assert!(err.context.get("pane_id").is_some());
}

#[test]
fn followup_idempotency_key_reuses_the_same_operation() {
    let project = std::env::temp_dir().join(format!("pm-followup-idem-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "codex", None, false);
    let opts = crate::agent_runs::persist::FollowupOpts {
        idempotency_key: Some("followup-key".into()),
        ..Default::default()
    };
    let (first, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "next step".into(),
        None,
        opts.clone(),
    )
    .unwrap();
    assert!(created);
    let (second, created_again) = crate::agent_runs::runtime::enqueue_followup(
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
    let (followup, _) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "native",
        &first,
        "follow one".into(),
        None,
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert_eq!(
        followup.worker.provider_session_id.as_deref(),
        Some(session.as_str())
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn live_pane_followup_sends_only_the_new_task() {
    let mut previous = parked_snapshot(OperationStatus::Completed, "none");
    previous.pane_id = Some("pane-1".into());
    assert!(crate::agent_runs::persist::pane_keeps_context(&previous));
    let render = |can_resume: bool| {
        crate::agent_runs::persist::render_turn_prompt(crate::agent_runs::persist::TurnPromptArgs {
            agent_type: &previous.agent_type,
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
    assert!(!crate::agent_runs::persist::pane_keeps_context(&previous));
}

#[test]
fn an_operation_can_be_found_again_by_its_idempotency_key() {
    let project = std::env::temp_dir().join(format!("pm-idem-{}", Uuid::new_v4()));
    fs::create_dir_all(&project).unwrap();
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "worker-x", 0, "do the thing");
    request.idempotency_key = "idem-find-me".into();
    let (op, _) = operations::create_operation(request).unwrap();
    let found = operations::find_by_idempotency_key(&project_path, "idem-find-me")
        .unwrap()
        .expect("the key was stored");
    assert_eq!(found.operation_id, op.operation_id);
    assert!(operations::find_by_idempotency_key(&project_path, "other")
        .unwrap()
        .is_none());
    let _ = fs::remove_dir_all(project);
}

#[test]
fn a_follow_up_on_a_handed_off_run_is_refused_with_the_new_handle() {
    let project = std::env::temp_dir().join(format!("pm-kg-followup-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "kg-old", "codex", None, false);
    operations::mark_operation_handed_off(
        &project_path,
        &running.operation_id,
        json!({ "keep_going": "kg-0a1b2c3d4e5f" }),
    )
    .unwrap();
    operations::set_handed_off_to(
        &project_path,
        &running.operation_id,
        json!({ "handle": "kg-new", "agent_type": "fake_b" }),
    )
    .unwrap();
    let latest = operations::latest_agent_run(&project_path, "kg-old").unwrap();
    let refusal = crate::agent_runs::runtime::refuse_handed_off(&latest).unwrap_err();
    assert_eq!(refusal.code, "HANDED_OFF");
    assert!(!refusal.recoverable);
    assert_eq!(
        refusal.message,
        "This task moved to fake_b at a usage limit. Use handle kg-new."
    );
    assert_eq!(refusal.context["keep_going"], "kg-0a1b2c3d4e5f");
    assert_eq!(refusal.context["continued_by"]["handle"], "kg-new");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn a_handed_off_run_without_a_new_handle_still_refuses_in_plain_words() {
    let project = std::env::temp_dir().join(format!("pm-kg-followup-bare-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "kg-bare", "codex", None, false);
    let handed =
        operations::mark_operation_handed_off(&project_path, &running.operation_id, json!({}))
            .unwrap();
    let refusal = crate::agent_runs::runtime::refuse_handed_off(&handed).unwrap_err();
    assert_eq!(refusal.code, "HANDED_OFF");
    assert_eq!(
        refusal.message,
        "This task moved to another agent at a usage limit."
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn only_a_handed_off_run_is_refused_as_a_follow_up_target() {
    let project = std::env::temp_dir().join(format!("pm-kg-followup-other-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    // A run still going accepts a follow-up.
    let running = running_turn(&project_path, "kg-live", "codex", None, false);
    assert!(crate::agent_runs::runtime::refuse_handed_off(&running).is_ok());
    // A run stopped at a usage limit (not handed over) is not refused by this guard.
    let other = running_turn(&project_path, "kg-limit", "codex", None, false);
    operations::mark_operation_usage_limit(
        &project_path,
        &other.operation_id,
        json!({ "kind": operations::USAGE_LIMIT_KIND }),
    )
    .unwrap();
    let stopped =
        operations::mark_operation_stopped_at_limit(&project_path, &other.operation_id, json!({}))
            .unwrap();
    assert_eq!(
        stopped.error.as_ref().map(|e| e.code.as_str()),
        Some(operations::USAGE_LIMIT_STOPPED)
    );
    assert!(crate::agent_runs::runtime::refuse_handed_off(&stopped).is_ok());
    let _ = fs::remove_dir_all(project);
}
