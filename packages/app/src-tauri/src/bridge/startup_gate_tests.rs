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

/// Real Claude Code capture (this branch, 2026-10-14, folder never trusted): after "No, exit" Claude
/// quits, the dialog's text stays above, and the pane's host PowerShell prints its prompt on the
/// last line. The pane stays alive and reads `idle`.
const DECLINED_HOST_SHELL: &str = include_str!("../tui/claude/fixtures/trust_declined_host_shell.txt");

#[test]
fn declining_the_dialog_while_the_host_shell_stays_alive_still_fails_the_run() {
    // VERIFY #2b: the pane never exits and shows no error, so the wait used to see "the gate is
    // gone" and release the run, which then typed the task into PowerShell.
    let pane = "gate-decline-host-shell";
    let registry = claude_pane(pane);
    show(&registry, pane, TRUST);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "claude"))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished());

    show(&registry, pane, DECLINED_HOST_SHELL);
    set_status(&registry, pane, PaneStatus::Idle);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let error = handle.join().unwrap().expect_err("a declined trust dialog must not release the run");
    assert_eq!(error.code, "WORKSPACE_TRUST_DECLINED");
    assert!(!error.recoverable);
}

#[test]
fn an_explicit_deny_on_the_gate_is_final_without_looking_at_the_pane() {
    let pane = "gate-decline-explicit";
    let registry = claude_pane(pane);
    show(&registry, pane, TRUST);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "claude"))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished(), "held while the dialog shows");
    // The user clicked "Don't trust" on the card; the pane still shows the dialog for a moment.
    crate::pane_prompt::mark_gate_declined(pane);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let error = handle.join().unwrap().expect_err("a decline is final");
    assert_eq!(error.code, "WORKSPACE_TRUST_DECLINED");
}

#[test]
fn a_task_is_never_typed_when_the_agent_has_fallen_back_to_the_host_shell() {
    use crate::pane_prompt::dispatch_blocker;
    let blocked = dispatch_blocker("claude", DECLINED_HOST_SHELL).expect("a shell prompt where the agent was blocks the task");
    assert_eq!(blocked.code, "AGENT_EXITED");
    assert!(!blocked.recoverable);
    // Generic: another agent's pane in the same state is refused too, a plain shell pane is not.
    assert!(dispatch_blocker("codex", "some output\nPS D:\\pm-demo> ").is_some());
    assert!(dispatch_blocker("powershell", "PS D:\\pm-demo> ").is_none(), "a shell's own prompt is its input box");
    // Agent screens, however they end, are not host shells.
    for screen in [READY, TRUST, include_str!("../tui/claude/fixtures/side_diff_panel_screen.txt")] {
        assert_ne!(
            dispatch_blocker("claude", screen).map(|e| e.code),
            Some("AGENT_EXITED".to_string()),
            "{screen:.60}"
        );
    }
    assert!(crate::tui::agent_gone_to_host_shell("claude", DECLINED_HOST_SHELL));
    assert!(!crate::tui::agent_gone_to_host_shell("claude", READY));
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

// ---- one model for every agent -----------------------------------------------------------------
//
// `TuiProfile::startup_gate` is the one hook. A gate with `dismiss` (Codex's update notice) is
// closed with those keys and is never a card. A gate without it (folder trust, sign-in) becomes a
// user-answered prompt card (`workspace_trust` / `startup_screen`) that parks the run; declining it
// (the agent exits) fails the run. Claude, Cursor and Codex have their own answerable trust card
// (trust / deny with keys); a gate a profile cannot answer (Codex sign-in) is a card without choices
// (the user answers in the pane).

/// Hand-written from the Codex CLI's layout (no live capture of these two exists).
const CODEX_TRUST: &str = "> You are running Codex in ~/sandbox\n\n  Do you trust the files in this folder?\n\n\u{203a} 1. Yes, continue\n  2. No, quit\n\n  Press enter to continue\n";
const CODEX_LOGIN: &str = ">_ Welcome to Codex\n\n  1. Sign in with ChatGPT\n  2. Provide your own API key\n";
const CODEX_UPDATE_MENU: &str = include_str!("../tui/codex/fixtures/update_prompt_live.txt");
const CODEX_READY: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");

fn pane_for(agent: &str, id: &str) -> Arc<Mutex<PaneRegistry>> {
    let registry = claude_pane(id);
    registry.lock().panes.get_mut(id).unwrap().info.agent_type = agent.into();
    registry
}

#[test]
fn every_agents_non_dismissible_gate_is_a_prompt_card_that_parks_the_run() {
    use crate::pane_prompt::detect_in_screen;
    // Claude: its own answerable card (trust / deny), and the profile reports the gate too.
    let claude = detect_in_screen("claude", TRUST, false).expect("claude trust card");
    assert_eq!(claude["kind"], "workspace_trust");
    assert_eq!(claude["choices"], serde_json::json!(["trust", "deny"]));
    let gate = crate::tui::profile_for("claude").startup_gate(TRUST).expect("claude reports the gate");
    assert_eq!((gate.id, gate.dismiss), ("workspace_trust", None));
    // Codex: its own answerable card too (live r15: the card had no buttons), same kind.
    let codex = detect_in_screen("codex", CODEX_TRUST, false).expect("codex trust card");
    assert_eq!(codex["kind"], "workspace_trust");
    assert_eq!(codex["choices"], serde_json::json!(["trust", "deny"]));
    assert!(codex["prompt_id"].as_str().is_some_and(|id| !id.is_empty()));
    // Codex sign-in: a startup_screen card.
    let login = detect_in_screen("codex", CODEX_LOGIN, false).expect("codex login card");
    assert_eq!(login["kind"], "startup_screen");
    assert_eq!(login["choices"], serde_json::json!([]), "Puppet Master does not know the sign-in keys");
    // Every one of them parks a run and refuses a typed task, whatever the pane status says.
    for (agent, screen) in [("claude", TRUST), ("codex", CODEX_TRUST), ("codex", CODEX_LOGIN)] {
        for status in [None, Some("running"), Some("idle")] {
            assert!(startup_manual_approval_required(agent, status, screen, &[]), "{agent} {status:?}");
        }
        assert!(crate::pane_prompt::dispatch_blocker(agent, screen).is_some(), "{agent}");
    }
}

#[test]
fn a_dismissible_gate_is_closed_with_its_keys_and_is_never_a_card() {
    use crate::pane_prompt::detect_in_screen;
    let gate = crate::tui::profile_for("codex").startup_gate(CODEX_UPDATE_MENU).expect("update menu");
    assert_eq!(gate.dismiss, Some("\u{1b}"));
    assert!(detect_in_screen("codex", CODEX_UPDATE_MENU, false).is_none(), "a notice is not a card");
    // Still up after the keys were tried: the run waits for the user rather than typing into it.
    assert!(startup_manual_approval_required("codex", Some("idle"), CODEX_UPDATE_MENU, &[]));
    // The ready screen (the banner above an empty composer) is no gate at all.
    assert!(!startup_manual_approval_required("codex", Some("idle"), CODEX_READY, &[]));
}

#[test]
fn declining_codex_trust_fails_with_the_same_code_as_claude() {
    let pane = "gate-codex-decline";
    let registry = pane_for("codex", pane);
    show(&registry, pane, CODEX_TRUST);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "codex"))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished(), "the run holds while the gate shows");
    // "No, quit": Codex exits.
    show(&registry, pane, "");
    set_status(&registry, pane, PaneStatus::Error);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let error = handle.join().unwrap().expect_err("a declined trust screen fails the run");
    assert_eq!(error.code, "WORKSPACE_TRUST_DECLINED");
    assert!(!error.recoverable);
}

#[test]
fn leaving_a_sign_in_screen_unfinished_fails_with_its_own_code_and_a_finished_one_releases_the_run() {
    // Finished: the composer appears and the run continues.
    let pane = "gate-codex-login-ok";
    let registry = pane_for("codex", pane);
    show(&registry, pane, CODEX_LOGIN);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "codex"))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished());
    show(&registry, pane, CODEX_READY);
    set_status(&registry, pane, PaneStatus::Idle);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    assert_eq!(handle.join().unwrap().expect("released"), "idle");

    // Unfinished: the agent exits on the sign-in screen.
    let pane = "gate-codex-login-exit";
    let registry = pane_for("codex", pane);
    show(&registry, pane, CODEX_LOGIN);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || wait_for_startup_permission_resolution(&registry, &project, &id, pane, "codex"))
    };
    std::thread::sleep(Duration::from_millis(300));
    show(&registry, pane, "");
    set_status(&registry, pane, PaneStatus::Error);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let error = handle.join().unwrap().expect_err("an unfinished sign-in fails the run");
    assert_eq!(error.code, "STARTUP_SCREEN_NOT_COMPLETED");
    assert!(!error.recoverable);
}

// ---- the whole run, not just the wait (VERIFY-R7 #2b: 3 of 3 runs stuck `running`) --------------
//
// The wait above returned `WORKSPACE_TRUST_DECLINED` correctly in every unit test, yet live the run
// stayed `running` for ever: `run_operation_dispatch` dropped the dispatch error because the run was
// no longer `starting` (it is `waiting_input` while parked, and `running` once a pane observer read
// the dialog's redraw). These tests run the real dispatch wrapper and the real parking function
// against the real captured screens, with the supervisor's and the pane observer's writes played by
// the test, and look at what is stored on the run.

use crate::operations::{OperationSnapshot, OperationStatus, StateSource};

/// How the user turns the trust dialog down.
#[derive(Clone, Copy, Debug)]
enum Decline {
    /// The card's "Don't trust" button (`pane_prompt::answer`): key sent, decline recorded.
    Card,
    /// `POST /agents/answer {choice: "deny"}`: same, then the answer guard marks the run `running`.
    Handle,
    /// The user typed "No, exit" in the pane themselves: nothing recorded, Claude just leaves.
    InThePane,
}

/// A queued run, dispatched on its own thread through `run_operation_dispatch`, whose body is the
/// parking step `await_pane_ready` runs for a pane showing the trust dialog.
fn parked_run(pane: &'static str, card_screen: &'static str) -> (Arc<Mutex<PaneRegistry>>, OperationSnapshot, std::thread::JoinHandle<Result<OperationSnapshot, crate::operations::OperationError>>) {
    let registry = claude_pane(pane);
    show(&registry, pane, card_screen);
    set_status(&registry, pane, PaneStatus::WaitingInput);
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || {
            crate::operations::run_operation_dispatch(&project, &id, |operation| {
                operation.pane_id = Some(pane.into());
                operation.stage = Some("waiting_for_pane_readiness".into());
                *operation = crate::operations::save_operation_snapshot(operation)?;
                super::dispatch::park_on_startup_dialog(operation, &registry, pane, "claude", card_screen, &[])
            })
        })
    };
    (registry, op, handle)
}

fn stored(op: &OperationSnapshot) -> OperationSnapshot {
    crate::operations::get_operation(&op.project_path, &op.operation_id).unwrap()
}

fn decline_and_finish(pane: &'static str, how: Decline, supervisor_sees_the_dialog: bool) {
    decline_and_finish_on(pane, how, supervisor_sees_the_dialog, TRUST, DECLINED_HOST_SHELL);
}

fn decline_and_finish_on(
    pane: &'static str,
    how: Decline,
    supervisor_sees_the_dialog: bool,
    card_screen: &'static str,
    declined_screen: &'static str,
) {
    let (registry, op, handle) = parked_run(pane, card_screen);
    // Parked on the card.
    let parked = wait_for(|| {
        let now = stored(&op);
        (now.status == OperationStatus::WaitingInput && now.required_action.is_some()).then_some(now)
    });
    assert_eq!(parked.stage.as_deref(), Some("waiting_for_pane_readiness"));
    let card = parked.required_action.clone().unwrap();
    assert_eq!(card["kind"], "workspace_trust", "{card}");

    if supervisor_sees_the_dialog {
        // The run's supervisor thread watches the same pane and reports the dialog as the run's
        // pending input; the pane observer then sees the redraw and reports `running`.
        crate::operations::mark_operation_waiting_input(
            &op.project_path,
            &op.operation_id,
            Some("waiting_input".into()),
            card.clone(),
            StateSource::Inferred,
        )
        .unwrap();
        crate::operations::mark_operation_observation(
            &op.project_path,
            &op.operation_id,
            Some("running".into()),
            None,
            StateSource::Inferred,
        )
        .unwrap();
        let now = stored(&op);
        assert_eq!(now.status, OperationStatus::WaitingInput, "{how:?}: a parked run stays parked");
        assert_eq!(now.stage.as_deref(), Some("waiting_for_pane_readiness"), "{how:?}: the dispatch thread owns the stage");
    }
    std::thread::sleep(Duration::from_millis(150));
    assert!(!handle.is_finished(), "{how:?}: held while the dialog shows");

    // The user turns the dialog down; Claude exits to the host shell (real capture).
    if !matches!(how, Decline::InThePane) {
        crate::pane_prompt::note_gate_answer(pane, &card["raw_prompt"], "deny");
    }
    show(&registry, pane, declined_screen);
    set_status(&registry, pane, PaneStatus::Idle);
    if matches!(how, Decline::Handle) {
        // answer_guard::finish: the answered run is marked `running` while the pane redraws.
        let _ = crate::operations::mark_operation_observation(
            &op.project_path,
            &op.operation_id,
            Some("running".into()),
            None,
            StateSource::Native,
        );
    }
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let _ = handle.join().unwrap();

    let end = stored(&op);
    assert_eq!(end.status, OperationStatus::Failed, "{how:?}, supervisor={supervisor_sees_the_dialog}: the run must end, not stay {:?}", end.status);
    let error = end.error.expect("the run carries the reason");
    assert_eq!(error.code, "WORKSPACE_TRUST_DECLINED", "{how:?}");
    assert!(!error.recoverable);
    assert!(end.finished_at_ms.is_some(), "{how:?}: a finish time");
    assert_eq!(end.stage.as_deref(), Some("failed"));
    assert!(end.required_action.is_none() || end.status.terminal());
}

#[test]
fn a_declined_trust_card_ends_the_run_failed_not_running() {
    decline_and_finish("e2e-decline-card", Decline::Card, false);
}

#[test]
fn it_ends_the_run_even_when_the_supervisor_and_pane_observer_touched_it_meanwhile() {
    decline_and_finish("e2e-decline-card-observed", Decline::Card, true);
}

#[test]
fn the_handle_based_answer_path_ends_the_run_the_same_way() {
    decline_and_finish("e2e-decline-handle", Decline::Handle, true);
}

#[test]
fn declining_in_the_pane_itself_ends_the_run_too() {
    // Nothing was recorded: the dialog is gone and Claude's host shell shows a prompt.
    decline_and_finish("e2e-decline-in-pane", Decline::InThePane, false);
}

#[test]
fn accepting_the_dialog_still_resumes_the_dispatch() {
    let pane = "e2e-accept";
    let (registry, op, handle) = parked_run(pane, TRUST);
    wait_for(|| (stored(&op).status == OperationStatus::WaitingInput).then_some(()));
    // The supervisor and pane observer touch the parked run; it must still resume afterwards.
    let card = stored(&op).required_action.unwrap();
    crate::operations::mark_operation_waiting_input(&op.project_path, &op.operation_id, Some("waiting_input".into()), card, StateSource::Inferred).unwrap();
    crate::operations::mark_operation_observation(&op.project_path, &op.operation_id, Some("running".into()), None, StateSource::Inferred).unwrap();
    show(&registry, pane, READY);
    set_status(&registry, pane, PaneStatus::Idle);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    let _ = handle.join().unwrap();
    let end = stored(&op);
    assert_eq!(end.status, OperationStatus::Running, "dispatch resumed and the run went on: {:?}", end.error);
    assert!(end.error.is_none());
}

/// The very screens of the live run that got stuck (`D:\pm-nighterify-r7\evidence	2b-pane-at-card.txt`
/// and `t2b-pane-final.txt`, build 4ea9f63, folder D:\pm-verify2	rust-1), through both answer paths.
const LIVE_CARD: &str = include_str!("../tui/claude/fixtures/trust_card_verify_r7_live.txt");
const LIVE_DECLINED: &str = include_str!("../tui/claude/fixtures/trust_declined_verify_r7_live.txt");

#[test]
fn the_live_stuck_run_of_round_7_ends_failed_through_the_card_and_through_the_handle() {
    decline_and_finish_on("e2e-live-card", Decline::Card, true, LIVE_CARD, LIVE_DECLINED);
    decline_and_finish_on("e2e-live-handle", Decline::Handle, true, LIVE_CARD, LIVE_DECLINED);
    decline_and_finish_on("e2e-live-pane", Decline::InThePane, false, LIVE_CARD, LIVE_DECLINED);
}

// ---- an unknown screen parks the run (typing permit, K1) -----------------------------------------
//
// Delivery found no screen that takes a task (here: Codex 0.162.1 still loading, a real capture).
// The run is parked as `waiting_input` with the screen and a plain explanation, keeps being looked
// at, and resumes by itself once the composer shows; or ends `SCREEN_NOT_READY` with the screen.
// Never typed into, never a headless fallback.

const CODEX_LOADING: &str = include_str!("../tui/codex/fixtures/loading_0_162_1_live.txt");

fn codex_pane(id: &str) -> Arc<Mutex<PaneRegistry>> {
    let mut registry = PaneRegistry::new();
    let mut pane = PaneRegistry::test_pane_stub(id);
    pane.info.agent_type = "codex".into();
    pane.info.pid = 0; // no process facts: the screen decides
    pane.screen = Arc::new(Mutex::new(vt100::Parser::new(40, 120, 0)));
    registry.panes.insert(id.into(), pane);
    Arc::new(Mutex::new(registry))
}

fn parked_on_unknown(pane: &'static str, park_for: Duration) -> (Arc<Mutex<PaneRegistry>>, OperationSnapshot, std::thread::JoinHandle<Result<OperationSnapshot, crate::operations::OperationError>>) {
    let registry = codex_pane(pane);
    show(&registry, pane, CODEX_LOADING);
    let mut op = operation(&std::env::temp_dir().join(format!("pm-park-{}", uuid::Uuid::new_v4())));
    op.agent_type = "codex".into();
    let handle = {
        let (registry, project, id) = (registry.clone(), op.project_path.clone(), op.operation_id.clone());
        std::thread::spawn(move || {
            crate::operations::run_operation_dispatch(&project, &id, |operation| {
                operation.pane_id = Some(pane.into());
                operation.stage = Some("dispatching".into());
                *operation = crate::operations::save_operation_snapshot(operation)?;
                let screen = crate::pty::registry_read_snapshot_full(registry.as_ref(), pane).unwrap();
                let not_ready = crate::agent_runs::readiness::not_ready_error("the screen does not show the agent's input box", &screen, pane);
                super::dispatch::park_on_unknown_screen(operation, &registry, pane, "codex", &not_ready, park_for)
            })
        })
    };
    (registry, op, handle)
}

#[test]
fn an_unknown_screen_parks_the_run_with_the_screen_and_resumes_when_the_box_shows() {
    let pane = "park-unknown-resumes";
    let (registry, op, handle) = parked_on_unknown(pane, Duration::from_secs(20));
    let parked = wait_for(|| {
        let now = stored(&op);
        (now.status == OperationStatus::WaitingInput && now.required_action.is_some()).then_some(now)
    });
    let action = parked.required_action.clone().unwrap();
    assert_eq!(action["kind"], "screen_not_ready", "{action}");
    assert!(action["detail"].as_str().unwrap().contains("continues by itself"), "{action}");
    assert!(action["detail"].as_str().unwrap().contains("did not switch to a background run"), "{action}");
    assert!(action["screen"].as_str().unwrap().contains("Ask Codex"), "{action}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished(), "held while the screen is unknown");
    // Codex finishes loading.
    show(&registry, pane, CODEX_READY);
    wait_for(|| handle.is_finished().then_some(()));
    let resumed = handle.join().unwrap().expect("the dispatch resumes");
    assert_eq!(resumed.required_action, None);
    assert!(resumed.worker.fallback.is_none(), "never a headless fallback");
}

#[test]
fn an_unknown_screen_that_never_clears_ends_screen_not_ready_without_a_fallback() {
    let pane = "park-unknown-times-out";
    let (_registry, op, handle) = parked_on_unknown(pane, Duration::from_millis(600));
    wait_for(|| handle.is_finished().then_some(()));
    let _ = handle.join().unwrap();
    let now = stored(&op);
    assert_eq!(now.status, OperationStatus::Failed, "{now:?}");
    let error = now.error.clone().expect("an error");
    assert_eq!(error.code, crate::agent_runs::readiness::SCREEN_NOT_READY);
    assert!(error.context["screen"].as_str().unwrap().contains("Ask Codex"));
    assert!(now.worker.fallback.is_none(), "never a headless fallback");
}

/// Review R12 1.4: every pane status change rewrote `required_action` (to `None` for idle/running,
/// to the id-less manual action for waiting_input), so a run parked on `screen_not_ready` lost its
/// explanation and screen the first time the person touched the pane.
#[test]
fn a_parked_run_keeps_its_screen_not_ready_explanation_through_pane_status_changes() {
    let pane = "park-unknown-keeps-reason";
    let (registry, op, handle) = parked_on_unknown(pane, Duration::from_secs(20));
    wait_for(|| {
        let now = stored(&op);
        (now.status == OperationStatus::WaitingInput && now.required_action.is_some()).then_some(())
    });
    let manual = serde_json::json!({"kind": "manual_input_required", "pane_id": pane, "detail": "Inspect the pane"});
    // What `observe_operation_pane_status` records for running, idle and an unrecognised waiting_input.
    for (state, action) in [("running", None), ("idle", None), ("waiting_input", Some(manual))] {
        let now = crate::operations::mark_operation_observation(
            &op.project_path,
            &op.operation_id,
            Some(state.into()),
            action,
            crate::operations::StateSource::Inferred,
        )
        .unwrap();
        let kept = now.required_action.clone().unwrap_or_else(|| panic!("{state}: the explanation was dropped"));
        assert_eq!(kept["kind"], "screen_not_ready", "{state}: {kept}");
        assert!(kept["screen"].as_str().unwrap().contains("Ask Codex"), "{state}: {kept}");
        assert_eq!(now.status, OperationStatus::WaitingInput, "{state}");
        assert_eq!(now.stage.as_deref(), Some("waiting_for_pane_readiness"), "{state}");
    }
    // A dialog the profile recognises is news: it replaces the explanation.
    let dialog = serde_json::json!({"kind": "command", "prompt_id": "p-1", "pane_id": pane, "summary": "Run npm test?"});
    let now = crate::operations::mark_operation_observation(
        &op.project_path,
        &op.operation_id,
        Some("waiting_input".into()),
        Some(dialog.clone()),
        crate::operations::StateSource::Inferred,
    )
    .unwrap();
    assert_eq!(now.required_action, Some(dialog));
    show(&registry, pane, CODEX_READY);
    wait_for(|| handle.is_finished().then_some(()));
    let _ = handle.join().unwrap();
}

/// Live (plain mode, r12): Claude's folder-trust question parked the run; a redraw of the same
/// dialog read for one look as "no dialog" (Ink clears and redraws top-down), the run was released,
/// the settle wait then found the dialog idle and still, and the delivery right after failed
/// PANE_BLOCKED_BY_PROMPT while the person was still answering.
#[test]
fn a_half_drawn_redraw_of_the_trust_dialog_does_not_release_the_parked_run() {
    let pane = "gate-trust-redraw";
    let (registry, op, handle) = parked_run(pane, TRUST);
    wait_for(|| {
        let now = stored(&op);
        (now.status == OperationStatus::WaitingInput && now.required_action.is_some()).then_some(())
    });
    // One look at the first row of the redraw, then the dialog again.
    let first_row: String = TRUST.lines().find(|l| !l.trim().is_empty()).unwrap().to_string();
    show(&registry, pane, &first_row);
    set_status(&registry, pane, PaneStatus::Idle);
    crate::pane_wait_notify::bump_waiters();
    std::thread::sleep(Duration::from_millis(60));
    show(&registry, pane, TRUST);
    crate::pane_wait_notify::bump_waiters();
    std::thread::sleep(Duration::from_millis(1_500));
    assert!(!handle.is_finished(), "the run was released while the trust question was still on screen");
    let now = stored(&op);
    assert_eq!(now.status, OperationStatus::WaitingInput, "{now:?}");
    assert_eq!(now.required_action.as_ref().map(|a| a["kind"].clone()), Some(serde_json::json!("workspace_trust")));
    // The person trusts the folder: the run goes on.
    show(&registry, pane, READY);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    handle.join().unwrap().expect("resumed after the real answer");
    assert_eq!(stored(&op).status, OperationStatus::Running);
}

// ---- a gate that draws after the readiness look (VERIFY-R13 #1) --------------------------------

/// Real Codex 0.162 capture of the first-launch folder-trust screen.
const CODEX_TRUST_LIVE: &str = include_str!("../tui/codex/fixtures/trust_folder_live.txt");

#[test]
fn a_trust_screen_that_drew_after_the_readiness_look_parks_the_run_instead_of_failing_it() {
    // Live (Codex, untrusted folder, isolated run): the pane read `running` before Codex painted, so
    // the readiness look saw nothing; delivery then met the trust screen and the run ended
    // PANE_BLOCKED_BY_PROMPT with no card. The dispatch loop now parks on such a refusal.
    let pane = "gate-late-codex";
    let registry = pane_for("codex", pane);
    show(&registry, pane, CODEX_TRUST_LIVE);
    set_status(&registry, pane, PaneStatus::Running);
    let screen = crate::pty::registry_read_snapshot(&registry, pane).unwrap();
    let refusal = crate::pane_prompt::dispatch_blocker("codex", &screen).expect("delivery refuses to type over the screen");
    assert_eq!(refusal.code, "PANE_BLOCKED_BY_PROMPT");
    let gate = super::dispatch::late_startup_gate(&refusal, "codex", &registry, pane).expect("a user-only gate is parked on");
    assert_eq!(gate["kind"], "workspace_trust");
    // The parked run carries the card's prompt (Inbox: "Trust this folder").
    let op = operation(&std::env::temp_dir().join(format!("pm-gate-{}", uuid::Uuid::new_v4())));
    let (project, id) = (op.project_path.clone(), op.operation_id.clone());
    let handle = {
        let (registry, project, id) = (registry.clone(), project.clone(), id.clone());
        std::thread::spawn(move || {
            crate::operations::run_operation_dispatch(&project, &id, |operation| {
                operation.pane_id = Some(pane.into());
                operation.stage = Some("waiting_for_pane_readiness".into());
                *operation = crate::operations::save_operation_snapshot(operation)?;
                super::dispatch::park_on_startup_dialog(operation, &registry, pane, "codex", &screen, &[])
            })
        })
    };
    let parked = wait_for(|| {
        let snapshot = crate::operations::get_operation(&project, &id).ok()?;
        (snapshot.status == crate::operations::OperationStatus::WaitingInput).then_some(snapshot)
    });
    let action = parked.required_action.expect("the parked run carries the card's prompt");
    assert_eq!(action["kind"], "workspace_trust", "{action}");
    assert!(!handle.is_finished(), "parked while the gate shows");
    // The user trusts the folder in the pane: Codex draws its composer and the run is released.
    show(&registry, pane, CODEX_READY);
    set_status(&registry, pane, PaneStatus::Idle);
    crate::pane_wait_notify::bump_waiters();
    wait_for(|| handle.is_finished().then_some(()));
    handle.join().unwrap().expect("released after the user trusted the folder");
}

#[test]
fn only_a_user_only_gate_is_parked_on_late() {
    let registry = pane_for("codex", "gate-late-other");
    let refusal = |code: &str| crate::operations::OperationError::new(code, "x", true);
    // A command approval is a prompt, not a startup gate: it keeps its own handling.
    show(&registry, "gate-late-other", "Allow command?\n  Run this command: ls\n  1. Yes\n  2. No\n");
    assert!(super::dispatch::late_startup_gate(&refusal("PANE_BLOCKED_BY_PROMPT"), "codex", &registry, "gate-late-other").is_none());
    // A trust screen but another failure code: not ours.
    show(&registry, "gate-late-other", CODEX_TRUST_LIVE);
    assert!(super::dispatch::late_startup_gate(&refusal("PROMPT_NOT_SUBMITTED"), "codex", &registry, "gate-late-other").is_none());
    // Nothing blocking: no park.
    show(&registry, "gate-late-other", CODEX_READY);
    assert!(super::dispatch::late_startup_gate(&refusal("PANE_BLOCKED_BY_PROMPT"), "codex", &registry, "gate-late-other").is_none());
}
