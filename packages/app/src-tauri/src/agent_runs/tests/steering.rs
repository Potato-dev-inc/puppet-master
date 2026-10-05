//! Steering, queued follow-ups, interrupt, cancel, close and trust-parked runs.

use crate::operations::{self, OperationStatus, ResultCapture};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

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
    assert_eq!(
        done.acceptance_status,
        crate::operations::AcceptanceStatus::NotChecked
    );
    assert_eq!(
        done.result.as_deref(),
        Some("[unverified: read from terminal screen] The fix is in main.rs.")
    );
    let view =
        crate::agent_runs::runtime::inspect_agent(&project_path, "pane", Some("persist-session"))
            .unwrap();
    assert_eq!(view["verified"], false);
    assert!(view["warnings"].to_string().contains("PROVENANCE_INFERRED"));

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
    use crate::agent_runs::messaging::{DeliveryMode, Disposition};
    let project = std::env::temp_dir().join(format!("pm-steer-restart-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = headless_cursor_turn(&project_path, "cur", Some("chat-1"), None);
    assert!(crate::agent_runs::messaging::can_restart_steer(&running));
    let send = |mode, key: &str| {
        crate::agent_runs::messaging::send_message(
            &project_path,
            "cur",
            "use tabs",
            &running,
            mode,
            Some(key),
        )
        .unwrap()
    };
    let receipt = send(DeliveryMode::LiveOrQueue, "k1");
    assert_eq!(receipt.disposition, Disposition::AcceptedViaRestart);
    assert_eq!(
        crate::agent_runs::messaging::receipt_value(&receipt)["disposition"],
        "accepted_via_restart"
    );
    assert_eq!(
        send(DeliveryMode::Live, "k2").disposition,
        Disposition::AcceptedViaRestart
    );
    assert_eq!(
        send(DeliveryMode::Queue, "k3").disposition,
        Disposition::Queued
    );
    // The restarted turn finishing settles the steer, with its result.
    crate::agent_runs::messaging::mark_open_steers_processed_for_turn(
        &project_path,
        "cur",
        "tabs used",
        2,
    );
    let settled =
        crate::agent_runs::messaging::find_receipt_by_key(&project_path, "cur", "k1").unwrap();
    assert_eq!(settled.disposition, Disposition::Processed);
    assert_eq!(settled.result.as_deref(), Some("tabs used"));
    assert!(crate::agent_runs::capabilities::for_snapshot(&running)
        .notes
        .iter()
        .any(|note| note.starts_with("steer_via_restart")));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn steer_falls_back_to_queue_without_a_session_or_with_a_pane() {
    use crate::agent_runs::messaging::{DeliveryMode, Disposition};
    let project = std::env::temp_dir().join(format!("pm-steer-norestart-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let no_session = headless_cursor_turn(&project_path, "nosess", None, None);
    assert!(!crate::agent_runs::messaging::can_restart_steer(
        &no_session
    ));
    let receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "nosess",
        "m",
        &no_session,
        DeliveryMode::LiveOrQueue,
        Some("a"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Queued);
    let strict = crate::agent_runs::messaging::send_message(
        &project_path,
        "nosess",
        "m",
        &no_session,
        DeliveryMode::Live,
        Some("b"),
    )
    .unwrap();
    assert_eq!(strict.disposition, Disposition::Unsupported);

    let pane = headless_cursor_turn(&project_path, "pane", Some("chat-2"), Some("pane-9"));
    assert!(!crate::agent_runs::messaging::can_restart_steer(&pane));
    let receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "pane",
        "m",
        &pane,
        DeliveryMode::LiveOrQueue,
        Some("c"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Queued);

    // Backends without resume never restart-steer.
    let codex_like = running_turn(&project_path, "claude", "claude", None, false);
    assert!(!crate::agent_runs::messaging::can_restart_steer(
        &codex_like
    ));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn send_message_steers_current_turn_followup_opens_next_turn() {
    let project = std::env::temp_dir().join(format!("pm-msg-vs-follow-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "worker", "codex", None, false);
    let receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "worker",
        "use the new token",
        &running,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("steer-1"),
    )
    .unwrap();
    assert_eq!(receipt.turn_id, "turn-0");
    assert_eq!(
        receipt.disposition,
        crate::agent_runs::messaging::Disposition::Queued
    );
    assert_eq!(operations::list_operations(&project_path).unwrap().len(), 1);
    assert_eq!(
        crate::agent_runs::messaging::pending_texts(&project_path, "worker"),
        vec!["use the new token".to_string()]
    );

    let (queued, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "propose a fix".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
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
    let first = crate::agent_runs::messaging::send_message(
        &project_path,
        "worker",
        "steer once",
        &running,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("same-key"),
    )
    .unwrap();
    assert!(!first.message_id.is_empty());
    assert_eq!(
        first.disposition,
        crate::agent_runs::messaging::Disposition::Queued
    );
    assert_eq!(first.idempotency_key, "same-key");
    let value = crate::agent_runs::messaging::receipt_value(&first);
    assert_eq!(value["message_id"], first.message_id);
    assert_eq!(value["disposition"], "queued");
    assert_eq!(value["state"], "queued");

    let second = crate::agent_runs::messaging::send_message(
        &project_path,
        "worker",
        "steer once again",
        &running,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("same-key"),
    )
    .unwrap();
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.text, first.text);
    assert_eq!(
        crate::agent_runs::messaging::pending_texts(&project_path, "worker").len(),
        1
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn steering_reply_is_tracked_separately_as_processed() {
    let project = std::env::temp_dir().join(format!("pm-steer-result-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(
        &project_path,
        "worker",
        "opencode_native",
        Some("pane"),
        true,
    );
    let receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "worker",
        "change the filename",
        &running,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("steer-result"),
    )
    .unwrap();
    assert_eq!(
        receipt.disposition,
        crate::agent_runs::messaging::Disposition::Accepted
    );
    assert!(receipt.result.is_none());
    crate::agent_runs::messaging::mark_open_steers_processed(
        &project_path,
        "worker",
        "renamed the file",
    );
    let processed = crate::agent_runs::messaging::latest_steer(&project_path, "worker").unwrap();
    assert_eq!(
        processed.disposition,
        crate::agent_runs::messaging::Disposition::Processed
    );
    assert_eq!(processed.result.as_deref(), Some("renamed the file"));
    assert_eq!(processed.message_id, receipt.message_id);
    let view = crate::agent_runs::runtime::AgentRunView::from(running);
    assert_eq!(view.last_steer.as_ref().unwrap().state, "processed");
    assert_eq!(
        view.last_steer.as_ref().unwrap().result.as_deref(),
        Some("renamed the file")
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn busy_tui_never_claims_injected_text_was_delivered() {
    let tui = crate::agent_runs::messaging::SteerTarget {
        backend: "claude",
        busy: true,
        waiting_input: false,
        has_pane: true,
        can_live_steer: false,
        can_restart_steer: false,
    };
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
            tui
        ),
        crate::agent_runs::messaging::Disposition::Queued
    );
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::Live,
            tui
        ),
        crate::agent_runs::messaging::Disposition::Unsupported
    );
    let waiting = crate::agent_runs::messaging::SteerTarget {
        waiting_input: true,
        ..tui
    };
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
            waiting
        ),
        crate::agent_runs::messaging::Disposition::Deferred
    );
    let headless = crate::agent_runs::messaging::SteerTarget {
        backend: "codex",
        busy: true,
        waiting_input: false,
        has_pane: false,
        can_live_steer: false,
        can_restart_steer: false,
    };
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
            headless
        ),
        crate::agent_runs::messaging::Disposition::Queued
    );
    let native = crate::agent_runs::messaging::SteerTarget {
        backend: "opencode_native",
        busy: true,
        waiting_input: false,
        has_pane: true,
        can_live_steer: true,
        can_restart_steer: false,
    };
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
            native
        ),
        crate::agent_runs::messaging::Disposition::Accepted
    );

    let project = std::env::temp_dir().join(format!("pm-msg-tui-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "tui", "claude", Some("pane-tui"), true);
    let receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "tui",
        "do not inject this",
        &running,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("tui-steer"),
    )
    .unwrap();
    assert_ne!(
        receipt.disposition,
        crate::agent_runs::messaging::Disposition::Accepted
    );
    assert!(matches!(
        receipt.disposition,
        crate::agent_runs::messaging::Disposition::Queued
            | crate::agent_runs::messaging::Disposition::Deferred
    ));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn queued_followup_deadline_starts_at_execution_not_enqueue() {
    assert!(
        !crate::agent_runs::messaging::followup_timeout_expired(true, 0, None, 100, 10, 300_000),
        "queue wait must not use the execution timeout"
    );
    assert!(!crate::agent_runs::messaging::followup_timeout_expired(
        true, 0, None, 299_999, 10, 300_000
    ));
    assert!(crate::agent_runs::messaging::followup_timeout_expired(
        true, 0, None, 300_000, 10, 300_000
    ));
    assert!(!crate::agent_runs::messaging::followup_timeout_expired(
        false,
        0,
        Some(200_000),
        209_999,
        10_000,
        300_000
    ));
    assert!(crate::agent_runs::messaging::followup_timeout_expired(
        false,
        0,
        Some(200_000),
        210_000,
        10_000,
        300_000
    ));
}

#[test]
fn steering_a_finished_turn_is_rejected_unless_explicitly_queued() {
    let idle = crate::agent_runs::messaging::SteerTarget {
        backend: "opencode_native",
        busy: false,
        waiting_input: false,
        has_pane: true,
        can_live_steer: true,
        can_restart_steer: false,
    };
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
            idle
        ),
        crate::agent_runs::messaging::Disposition::Rejected
    );
    assert_eq!(
        crate::agent_runs::messaging::decide_delivery(
            crate::agent_runs::messaging::DeliveryMode::Queue,
            idle
        ),
        crate::agent_runs::messaging::Disposition::Queued
    );
    let value = crate::agent_runs::messaging::receipt_value(
        &crate::agent_runs::messaging::MessageReceipt {
            message_id: "m1".into(),
            idempotency_key: "k1".into(),
            disposition: crate::agent_runs::messaging::Disposition::Rejected,
            handle: "worker".into(),
            turn_id: "turn-0".into(),
            text: "too late".into(),
            result: None,
        },
    );
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
    let running = running_turn(
        &project_path,
        "worker",
        "opencode_native",
        Some("pane"),
        true,
    );
    crate::agent_runs::messaging::send_message(
        &project_path,
        "worker",
        "change the filename",
        &running,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("steer-compact"),
    )
    .unwrap();
    crate::agent_runs::messaging::mark_open_steers_processed(
        &project_path,
        "worker",
        "long previous analysis",
    );
    operations::mark_operation_finished(
        &project_path,
        &running.operation_id,
        Some("READY".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let previous = operations::latest_agent_run(&project_path, "worker").unwrap();
    let (followup, _) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "worker",
        &previous,
        "one line".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    let view = crate::agent_runs::runtime::AgentRunView::from(followup);
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
fn every_steer_disposition_says_whether_it_was_applied_and_what_to_do() {
    use crate::agent_runs::messaging::Disposition;
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
    for applied in [
        Disposition::Accepted,
        Disposition::AcceptedViaRestart,
        Disposition::Processed,
    ] {
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
