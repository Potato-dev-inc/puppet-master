//! Backend capability claims stay consistent with what delivery and inspect really do.

use crate::operations::{self, ContextContinuity, ContextPolicy, OperationStatus, ResultCapture};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

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
    let native_caps = crate::agent_runs::capabilities::for_snapshot(&native);
    assert!(native_caps.live_messages);
    let native_receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "native",
        "steer live",
        &native,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("live-1"),
    )
    .unwrap();
    assert_eq!(
        native_receipt.disposition,
        crate::agent_runs::messaging::Disposition::Accepted
    );

    let tui = running_turn(&project_path, "tui", "claude", Some("pane-tui"), true);
    let tui_caps = crate::agent_runs::capabilities::for_snapshot(&tui);
    assert!(!tui_caps.live_messages);
    let tui_receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "tui",
        "do not inject",
        &tui,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("tui-1"),
    )
    .unwrap();
    assert_ne!(
        tui_receipt.disposition,
        crate::agent_runs::messaging::Disposition::Accepted
    );
    let started = crate::agent_runs::runtime::AgentRunView::from(native);
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
    assert!(crate::agent_runs::capabilities::for_snapshot(&native).session_resume);
    operations::mark_operation_finished(
        &project_path,
        &native.operation_id,
        Some("native answer".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let previous = operations::latest_agent_run(&project_path, "native").unwrap();
    let native_prompt = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            agent_type: "opencode_native",
            policy: ContextPolicy::Packet,
            task: "continue",
            scope: None,
            prior_result: previous.result.as_deref(),
            prior_user: None,
            selected_history: &[],
            can_resume: crate::agent_runs::persist::snapshot_can_resume(&previous),
            is_followup: true,
        },
    );
    assert_eq!(native_prompt.continuity, ContextContinuity::Resume);
    let mut native_follow = persist_req(&project_path, "native", 1, "continue");
    native_follow.agent_type = "opencode_native".into();
    native_follow.pane_id = Some("pane-native".into());
    native_follow.worker.provider_session_id = Some("ses-live".into());
    native_follow.worker.context_continuity = native_prompt.continuity;
    operations::create_operation(native_follow).unwrap();
    let native_inspect =
        crate::agent_runs::runtime::inspect_agent(&project_path, "native", Some("persist-session"))
            .unwrap();
    assert_eq!(native_inspect["capabilities"]["session_resume"], true);
    assert_eq!(native_inspect["context_continuity"], "resume");

    let cli = running_turn(&project_path, "cli", "codex", None, false);
    assert!(crate::agent_runs::capabilities::for_snapshot(&cli).session_resume);
    operations::mark_operation_finished(
        &project_path,
        &cli.operation_id,
        Some("cli answer".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let previous_cli = operations::latest_agent_run(&project_path, "cli").unwrap();
    let cli_prompt = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            agent_type: "codex",
            policy: ContextPolicy::Packet,
            task: "continue",
            scope: None,
            prior_result: previous_cli.result.as_deref(),
            prior_user: None,
            selected_history: &[],
            can_resume: crate::agent_runs::persist::snapshot_can_resume(&previous_cli),
            is_followup: true,
        },
    );
    assert_eq!(
        cli_prompt.continuity,
        ContextContinuity::ReconstructedSummary
    );
    let mut cli_follow = persist_req(&project_path, "cli", 1, "continue");
    cli_follow.agent_type = "codex".into();
    cli_follow.worker.context_continuity = cli_prompt.continuity;
    operations::create_operation(cli_follow).unwrap();
    let cli_inspect =
        crate::agent_runs::runtime::inspect_agent(&project_path, "cli", Some("persist-session"))
            .unwrap();
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
    let caps = crate::agent_runs::capabilities::for_snapshot(&native);
    assert!(caps.graceful_interrupt);
    assert_eq!(
        crate::agent_runs::messaging::interrupt_action(&native),
        Some(crate::agent_runs::messaging::InterruptAction::AbortNative)
    );
    let view = crate::agent_runs::runtime::interrupt_active_turns(
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
    assert!(!crate::agent_runs::capabilities::for_snapshot(&tui).graceful_interrupt);
    assert_eq!(
        crate::agent_runs::messaging::interrupt_action(&tui),
        Some(crate::agent_runs::messaging::InterruptAction::SignalTui)
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
    let caps = crate::agent_runs::capabilities::for_snapshot(&running);
    assert!(caps.queued_followups);
    let inspected =
        crate::agent_runs::runtime::inspect_agent(&project_path, "worker", Some("persist-session"))
            .unwrap();
    assert_eq!(inspected["capabilities"]["queued_followups"], true);
    let (queued, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "worker",
        &running,
        "next turn".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(queued.status, OperationStatus::Queued);
    assert_eq!(queued.turn_index, 1);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn visible_terminal_only_when_pane_exists() {
    let with_pane = crate::agent_runs::capabilities::for_backend("cursor_agent", true);
    let without_pane = crate::agent_runs::capabilities::for_backend("cursor_agent", false);
    assert!(with_pane.visible_terminal);
    assert!(!without_pane.visible_terminal);
    assert_eq!(
        with_pane.interface,
        crate::agent_runs::capabilities::BackendInterface::TuiObservation
    );
    assert_eq!(
        without_pane.interface,
        crate::agent_runs::capabilities::BackendInterface::StructuredHeadless
    );

    let project = std::env::temp_dir().join(format!("pm-cap-pane-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let pane = running_turn(&project_path, "tui", "cursor_agent", Some("pane-1"), true);
    let headless = running_turn(&project_path, "headless", "cursor_agent", None, false);
    assert!(crate::agent_runs::capabilities::for_snapshot(&pane).visible_terminal);
    assert!(!crate::agent_runs::capabilities::for_snapshot(&headless).visible_terminal);
    let pane_inspect =
        crate::agent_runs::runtime::inspect_agent(&project_path, "tui", Some("persist-session"))
            .unwrap();
    let headless_inspect = crate::agent_runs::runtime::inspect_agent(
        &project_path,
        "headless",
        Some("persist-session"),
    )
    .unwrap();
    assert_eq!(pane_inspect["capabilities"]["visible_terminal"], true);
    assert_eq!(headless_inspect["capabilities"]["visible_terminal"], false);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn cursor_tui_contract_documents_version_limits_without_parity_claims() {
    let caps = crate::agent_runs::capabilities::for_backend("cursor_agent", true);
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
    let start_view = crate::agent_runs::runtime::AgentRunView::from(started.clone());
    assert!(start_view.capabilities.queued_followups);
    assert!(!start_view.capabilities.live_messages);

    let receipt = crate::agent_runs::messaging::send_message(
        &project_path,
        "reviewer",
        "prefer the short answer",
        &started,
        crate::agent_runs::messaging::DeliveryMode::LiveOrQueue,
        Some("e2e-steer"),
    )
    .unwrap();
    assert_eq!(
        receipt.disposition,
        crate::agent_runs::messaging::Disposition::Queued
    );

    operations::mark_operation_finished(
        &project_path,
        &started.operation_id,
        Some("token expired".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let answered = crate::agent_runs::runtime::inspect_agent(
        &project_path,
        "reviewer",
        Some("persist-session"),
    )
    .unwrap();
    assert_eq!(answered["status"], "completed");
    assert_eq!(answered["result"], "token expired");

    let previous = operations::latest_agent_run(&project_path, "reviewer").unwrap();
    let (followup, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "reviewer",
        &previous,
        "propose a fix".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(followup.turn_index, 1);
    assert!(followup.task.contains("propose a fix"));
    operations::run_operation_dispatch(&project_path, &followup.operation_id, |_| Ok(())).unwrap();

    let interrupted = crate::agent_runs::runtime::interrupt_active_turns(
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
    let (reuse, created) = crate::agent_runs::runtime::enqueue_followup(
        &project_path,
        "reviewer",
        &latest,
        "try again".into(),
        Some("persist-session".into()),
        crate::agent_runs::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(reuse.turn_index, 2);
    let reused = crate::agent_runs::runtime::inspect_agent(
        &project_path,
        "reviewer",
        Some("persist-session"),
    )
    .unwrap();
    assert_eq!(reused["handle"], "reviewer");
    assert_eq!(reused["turn_id"], "turn-2");
    assert_eq!(reused["capabilities"]["queued_followups"], true);
    let _ = fs::remove_dir_all(project);
}
