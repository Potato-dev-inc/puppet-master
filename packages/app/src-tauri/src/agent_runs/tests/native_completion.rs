//! Deciding when a native OpenCode turn is complete from its message list.

use crate::agent_runs::native::native_completion_evidence;
use crate::agent_runs::transcript;
use crate::opencode::messages::{MessagePartView, MessageView, SessionMessagesView};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

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

fn user_message(id: &str, text: &str) -> MessageView {
    MessageView {
        id: Some(id.into()),
        role: "user".into(),
        model: None,
        finish: None,
        error: None,
        parts: vec![MessagePartView::Text { text: text.into() }],
    }
}

/// Regression for the intermittent `PROMPT_NOT_SUBMITTED` after a live steer: the answer to the
/// steer (`a3`) may already exist when the steer is acknowledged, so completion must be decided
/// from message order against the dispatch baseline, never from a baseline re-read after the steer.
#[test]
fn native_completion_follows_the_steer_even_when_its_answer_already_exists() {
    let baseline = vec!["u1".to_string()];
    let answered = view(vec![
        user_message("u1", "do the thing"),
        assistant("a1", "tool-calls", "", None),
        assistant("a2", "stop", "PONG", None),
        user_message("u2", "reply STEERED instead"),
        assistant("a3", "stop", "STEERED", None),
    ]);
    assert_eq!(
        native_completion_evidence(&answered, &baseline, false),
        Some(Ok("STEERED".into()))
    );
}

#[test]
fn native_completion_waits_for_an_answer_that_follows_the_latest_user_message() {
    let baseline = vec!["u1".to_string()];
    // The first turn's answer finished, but a steer arrived after it: that answer is not final.
    let steered = view(vec![
        user_message("u1", "do the thing"),
        assistant("a1", "stop", "PONG", None),
        user_message("u2", "reply STEERED instead"),
    ]);
    assert!(native_completion_evidence(&steered, &baseline, false).is_none());
    // A reply to an earlier assistant step that was still streaming when the steer landed.
    let racing = view(vec![
        user_message("u1", "do the thing"),
        assistant("a2", "stop", "PONG", None),
        user_message("u2", "reply STEERED instead"),
        assistant("a3", "", "", None),
    ]);
    assert!(native_completion_evidence(&racing, &baseline, false).is_none());
}

#[test]
fn native_completion_is_deferred_while_a_steer_is_being_delivered() {
    use crate::agent_runs::native::{unless_steer_in_flight, SteerGuard};
    let op = format!("steer-guard-{}", Uuid::new_v4());
    assert!(
        unless_steer_in_flight(&op, || {}),
        "no steer: completion may be decided"
    );
    let guard = SteerGuard::begin(&op);
    let mut ran = false;
    assert!(!unless_steer_in_flight(&op, || ran = true));
    assert!(!ran, "completion must wait for the in-flight steer");
    drop(guard);
    assert!(unless_steer_in_flight(&op, || ran = true));
    assert!(ran);
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
fn native_and_tui_paths_write_transcript_events() {
    let project = std::env::temp_dir().join(format!("pm-agent-events-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let operation = Uuid::new_v4().to_string();
    crate::agent_runs::runtime::record_user_task(&project_path, &operation, "inspect the outage");
    transcript::append(
        &project_path,
        &operation,
        "assistant_final",
        "The connection fails because the token expired.",
    );
    let page = transcript::read(&project_path, &operation, 0).unwrap();
    let kinds: Vec<_> = page
        .chunks
        .iter()
        .map(|chunk| chunk.stream.as_str())
        .collect();
    assert!(kinds.contains(&"user_task"));
    assert!(kinds.contains(&"assistant_final"));
    let _ = fs::remove_dir_all(project);
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
    let started = Some(late - crate::agent_runs::native::NATIVE_PROMPT_LANDING_MS - 1);
    // Nothing new, idle, past the window: lost.
    assert!(crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m1"]),
        &baseline,
        started,
        late,
        "idle"
    ));
    // A new message exists: it landed.
    assert!(!crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m1", "m2"]),
        &baseline,
        started,
        late,
        "idle"
    ));
    // Still busy, or too early, or never started: not lost.
    assert!(!crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m1"]),
        &baseline,
        started,
        late,
        "running"
    ));
    assert!(!crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m1"]),
        &baseline,
        Some(late - 1_000),
        late,
        "idle"
    ));
    assert!(!crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m1"]),
        &baseline,
        None,
        late,
        "idle"
    ));
    // A stuck non-idle pane with no new message is lost only after the longer cap.
    let stuck = Some(late - crate::agent_runs::native::NATIVE_PROMPT_LANDING_MS * 3 - 1);
    assert!(crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m1"]),
        &baseline,
        stuck,
        late,
        "running"
    ));
    assert!(!crate::agent_runs::native::native_prompt_never_landed(
        &view(&["m2", "m1"]),
        &baseline,
        stuck,
        late,
        "running"
    ));
}
