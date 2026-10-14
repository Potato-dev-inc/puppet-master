//! A visible run in a folder the agent has not trusted: the run parks on the trust dialog, the task
//! is typed only after the dialog is gone, and a declined dialog fails the run (no headless fallback).
//!
//! The screens are real Claude Code v2.1.299 captures (`tui/claude/fixtures`); the pane is a stub
//! whose emulator is fed those screens, so the same functions the dispatch thread calls run here.

use super::approvals::{startup_manual_approval_required, wait_for_startup_permission_resolution};
use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TRUST: &str = include_str!("../tui/claude/fixtures/trust_dialog_2_1_299.txt");
/// What Claude draws once the folder is trusted (live capture of the pane right after the answer).
const READY: &str = "\n \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}   Claude Code v2.1.299\n\u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{259b}\u{2580}  Haiku 5.5 \u{b7} Claude Pro\n \u{259d}\u{259d}   \u{259d}\u{259d}   D:\\pm-demo-untrusted-2\n\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Try \"edit <filepath> to...\"\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n";

/// Replace what the pane shows (clear, home, then the text with CRLF line ends).
fn show(registry: &Arc<Mutex<PaneRegistry>>, pane: &str, screen: &str) {
    let screen_handle = registry.lock().panes.get(pane).unwrap().screen.clone();
    let mut bytes = b"\x1b[2J\x1b[H".to_vec();
    bytes.extend(screen.replace('\n', "\r\n").as_bytes());
    screen_handle.lock().process(&bytes);
}

fn set_status(registry: &Arc<Mutex<PaneRegistry>>, pane: &str, status: PaneStatus) {
    let handle = registry.lock().panes.get(pane).unwrap().status.clone();
    *handle.lock() = status;
}

fn claude_pane(id: &str) -> Arc<Mutex<PaneRegistry>> {
    let mut registry = PaneRegistry::new();
    let mut pane = PaneRegistry::test_pane_stub(id);
    pane.info.agent_type = "claude".into();
    pane.screen = Arc::new(Mutex::new(vt100::Parser::new(40, 120, 0)));
    registry.panes.insert(id.into(), pane);
    Arc::new(Mutex::new(registry))
}

fn operation(project: &std::path::Path) -> crate::operations::OperationSnapshot {
    let request = crate::operations::DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "Reply with only the word PONG. Use no tools.".into(),
        agent_type: "claude".into(),
        pane_id: None,
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 60_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("startup-gate".into()),
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    crate::operations::create_operation(request).unwrap().0
}

fn wait_for<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_trust_dialog_parks_the_run_whatever_the_pane_status_says() {
    // Live: the pane read `running`, nothing parked the run, and the task was typed into the dialog.
    for status in [None, Some("running"), Some("idle"), Some("waiting_input")] {
        assert!(startup_manual_approval_required("claude", status, TRUST, &[]), "{status:?}");
    }
    assert!(!startup_manual_approval_required("claude", Some("idle"), READY, &[]), "a trusted folder is not parked");
}

#[test]
fn the_run_holds_while_the_dialog_shows_and_continues_once_it_is_accepted() {
    let pane = "gate-accept-pane";
    let registry = claude_pane(pane);
    show(&registry, pane, TRUST);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let project = std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4()));
    let op = operation(&project);

    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "claude"))
    };
    // The gate is still on screen: the wait does not return, so the task is not typed.
    std::thread::sleep(Duration::from_millis(400));
    assert!(!handle.is_finished(), "the run must hold while the trust dialog is showing");
    crate::pane_wait_notify::bump_waiters();
    std::thread::sleep(Duration::from_millis(200));
    assert!(!handle.is_finished(), "a redraw of the same dialog does not release it");

    // The user trusts the folder: Claude draws its own screen.
    show(&registry, pane, READY);
    set_status(&registry, pane, PaneStatus::Idle);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    assert_eq!(handle.join().unwrap().expect("released after the accept"), "idle");
}

#[test]
fn declining_the_dialog_fails_the_run_with_a_clear_code() {
    let pane = "gate-decline-pane";
    let registry = claude_pane(pane);
    show(&registry, pane, TRUST);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let project = std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4()));
    let op = operation(&project);

    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "claude"))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished());

    // "No, exit": Claude quits, the pane's process is gone (status error, screen blank).
    show(&registry, pane, "");
    set_status(&registry, pane, PaneStatus::Error);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let error = handle.join().unwrap().expect_err("a declined trust dialog fails the run");
    assert_eq!(error.code, "WORKSPACE_TRUST_DECLINED");
    assert!(!error.recoverable, "not retried");
    assert!(error.message.contains("D:\\pm-demo-untrusted-1"), "{}", error.message);
    // (agent_runs/headless/fallback.rs tests that this code never falls back to a headless run.)
}

#[test]
fn a_pane_that_dies_without_a_gate_keeps_the_old_error() {
    let pane = "gate-none-pane";
    let registry = claude_pane(pane);
    show(&registry, pane, READY);
    set_status(&registry, pane, PaneStatus::Running);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || {
            // Nothing blocks: returns at once with the pane status.
            let ready = wait_for_startup_permission_resolution(&registry, &project, &id, pane, "claude");
            set_status(&registry, pane, PaneStatus::Error);
            let died = wait_for_startup_permission_resolution(&registry, &project, &id, pane, "claude");
            (ready, died)
        })
    };
    let (ready, died) = handle.join().unwrap();
    assert_eq!(ready.unwrap(), "running");
    assert_eq!(died.unwrap_err().code, "PANE_READINESS_FAILED");
}
