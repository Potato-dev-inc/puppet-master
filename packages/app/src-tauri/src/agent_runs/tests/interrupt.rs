//! Interrupt, cancel and close, and runs parked on a workspace-trust prompt.

use crate::agent_runs::headless;
use crate::operations::{self, DelegateWorkRequest, OperationStatus, ResultCapture};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

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
    let ordered = crate::agent_runs::runtime::active_turns(
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
fn interrupt_stops_turn_keeps_worker_reusable_and_does_not_kill_pane() {
    let project = std::env::temp_dir().join(format!("pm-msg-interrupt-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(&project_path, "keeper", "claude", Some("pane-tui"), true);
    assert_eq!(
        crate::agent_runs::messaging::interrupt_action(&running),
        Some(crate::agent_runs::messaging::InterruptAction::SignalTui)
    );
    let mut requested = 0usize;
    let view = crate::agent_runs::runtime::interrupt_active_turns(
        &project_path,
        "keeper",
        Some("persist-session"),
        |snapshot| {
            requested += 1;
            assert_eq!(snapshot.pane_id.as_deref(), Some("pane-tui"));
            assert!(snapshot.pane_created);
            assert_eq!(
                crate::agent_runs::messaging::interrupt_action(snapshot),
                Some(crate::agent_runs::messaging::InterruptAction::SignalTui)
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
        crate::agent_runs::runtime::inspect_agent(&project_path, "keeper", Some("persist-session"))
            .unwrap();
    assert_eq!(inspected["handle"], "keeper");
    assert_eq!(inspected["status"], "interrupted");
    assert_eq!(inspected["pane_id"], "pane-tui");

    let (next, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "keeper",
        &interrupted,
        "continue after interrupt".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(next.turn_index, 1);
    let reused =
        crate::agent_runs::runtime::inspect_agent(&project_path, "keeper", Some("persist-session"))
            .unwrap();
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
        crate::agent_runs::messaging::interrupt_action(&tui),
        Some(crate::agent_runs::messaging::InterruptAction::SignalTui)
    );
    assert_eq!(
        crate::agent_runs::messaging::interrupt_action(&native),
        Some(crate::agent_runs::messaging::InterruptAction::AbortNative)
    );
    assert_eq!(
        crate::agent_runs::messaging::interrupt_action(&headless),
        Some(crate::agent_runs::messaging::InterruptAction::StopHeadless)
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
    assert!(crate::agent_runs::messaging::interrupt_action(&done).is_none());
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
    let view = crate::agent_runs::runtime::interrupt_active_turns(
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
fn close_agent_returns_closed_status() {
    let project = std::env::temp_dir().join(format!("pm-close-status-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(&project_path, "worker", 0, "done work"))
        .unwrap()
        .0;
    finish_turn(&project_path, &first.operation_id, "answer");
    let closed = operations::mark_worker_closed(&project_path, "worker").unwrap();
    let view = crate::agent_runs::runtime::AgentRunView::from(closed);
    assert_eq!(view.status, "closed");
    let inspected =
        crate::agent_runs::runtime::inspect_agent(&project_path, "worker", Some("persist-session"))
            .unwrap();
    assert_eq!(inspected["status"], "closed");
    assert_eq!(inspected["closed"], true);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn cancel_releases_a_run_spawned_pane_unless_keep_pane_was_asked_for() {
    let mut spawned = parked_snapshot(OperationStatus::Cancelled, "none");
    spawned.pane_created = true;
    spawned.keep_pane = false;
    assert!(crate::agent_runs::pane_close::should_dispose_after_terminal(&spawned));
    spawned.keep_pane = true;
    assert!(!crate::agent_runs::pane_close::should_dispose_after_terminal(&spawned));
    // A pane the user opened (adopted, not created by the run) is never closed for them.
    let mut adopted = parked_snapshot(OperationStatus::Cancelled, "none");
    adopted.pane_created = false;
    adopted.keep_pane = false;
    assert!(!crate::agent_runs::pane_close::should_dispose_after_terminal(&adopted));
}
