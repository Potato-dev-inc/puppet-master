//! An agent that exits to its pane's host shell in the middle of a run (after its prompt was typed)
//! must fail the run with AGENT_EXITED and the last screen, whatever the agent is.
//!
//! The "working" screens are real captures (`tui/*/fixtures`); the exit is those screens followed by
//! the host shell's prompt, which is what a pane shows once the agent's process ends.

use super::super::pane_turn::{fail_if_agent_exited, HostShellWatch};
use crate::operations::{self, OperationSnapshot, OperationStatus};

const CODEX_WORKING: &str = include_str!("../../../tui/codex/fixtures/working_live.txt");
const CLAUDE_EDIT: &str = include_str!("../../../tui/claude/fixtures/result_footer_edit_live.txt");

fn running_run(agent: &str) -> OperationSnapshot {
    let project = std::env::temp_dir().join(format!("pm-host-shell-{}", uuid::Uuid::new_v4()));
    let request = operations::DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "Add a note to README.md".into(),
        agent_type: agent.into(),
        pane_id: Some("host-shell-pane".into()),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 600_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("host-shell".into()),
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let mut run = operations::delegate_work(request, |_| Ok(())).unwrap();
    // As the dispatch thread leaves it once the prompt is typed.
    run.worker.stage_log = ["dispatching", "dispatched_waiting_for_task_completion"]
        .iter()
        .enumerate()
        .map(|(at, stage)| operations::StageMark { stage: (*stage).into(), at_ms: at as u64 })
        .collect();
    run
}

fn with_prompt(screen: &str) -> String {
    format!("{}\nPS D:\\pm-demo> ", screen.trim_end())
}

#[test]
fn an_agent_that_exits_to_its_host_shell_mid_run_fails_the_run_with_its_last_screen() {
    for (agent, working) in [("codex", CODEX_WORKING), ("claude", CLAUDE_EDIT)] {
        let run = running_run(agent);
        assert_eq!(run.status, OperationStatus::Running);
        let mut watch = HostShellWatch::default();
        // Working: nothing happens, however many ticks.
        for _ in 0..5 {
            assert!(fail_if_agent_exited(&run, &mut watch, working, false).is_none(), "{agent}: a working agent is alive");
        }
        // The agent's process ends and PowerShell prints its prompt. One frame is not enough ...
        let gone = with_prompt(working);
        assert!(fail_if_agent_exited(&run, &mut watch, &gone, false).is_none(), "{agent}: one frame is not proof");
        // ... the same frame again is.
        let failed = fail_if_agent_exited(&run, &mut watch, &gone, false)
            .unwrap_or_else(|| panic!("{agent}: the run must fail once the host shell stays"));
        assert_eq!(failed.status, OperationStatus::Failed);
        let error = failed.error.as_ref().expect("a typed error");
        assert_eq!(error.code, "AGENT_EXITED");
        assert!(!error.recoverable);
        let context = error.context["last_screen"].as_str().expect("the last screen is the context");
        assert!(context.contains("PS D:\\pm-demo>"), "{context}");
        let agent_text = working.lines().find(|l| l.trim().len() > 12).unwrap().trim();
        assert!(context.contains(agent_text), "{agent}: what the agent last showed is kept: {agent_text:?}");
        // It is durable: a reader sees the same.
        let saved = operations::get_operation(&run.project_path, &run.operation_id).unwrap();
        assert_eq!(saved.status, OperationStatus::Failed);
        assert_eq!(saved.error.unwrap().code, "AGENT_EXITED");
    }
}

#[test]
fn a_single_host_prompt_frame_between_agent_frames_does_not_fail_the_run() {
    let run = running_run("codex");
    let mut watch = HostShellWatch::default();
    let gone = with_prompt(CODEX_WORKING);
    for _ in 0..4 {
        assert!(fail_if_agent_exited(&run, &mut watch, &gone, false).is_none());
        assert!(fail_if_agent_exited(&run, &mut watch, CODEX_WORKING, false).is_none(), "the agent came back: the count restarts");
    }
    assert_eq!(operations::get_operation(&run.project_path, &run.operation_id).unwrap().status, OperationStatus::Running);
}

#[test]
fn a_host_prompt_before_the_prompt_was_typed_is_not_an_exit() {
    // The pane's shell shows its prompt until the agent starts; the run is not past dispatch yet.
    let mut run = running_run("codex");
    run.worker.stage_log = vec![
        operations::StageMark { stage: "dispatching".into(), at_ms: 1 },
        operations::StageMark { stage: "waiting_for_pane_readiness".into(), at_ms: 2 },
    ];
    let mut watch = HostShellWatch::default();
    for _ in 0..5 {
        assert!(fail_if_agent_exited(&run, &mut watch, "PS D:\\pm-demo> ", false).is_none());
    }
}

#[test]
fn a_plain_shell_pane_is_never_an_agent_that_exited() {
    let mut run = running_run("codex");
    run.agent_type = "powershell".into();
    let mut watch = HostShellWatch::default();
    for _ in 0..5 {
        assert!(fail_if_agent_exited(&run, &mut watch, "PS D:\\pm-demo> ", false).is_none(), "a shell's prompt is its normal screen");
    }
}

/// Regression: `an_agent_crash_mid_run_fails_the_run_with_agent_exited` completed the run on a CI
/// runner. The agent's process had ended, but the host shell's prompt was not drawn yet, so the
/// screen was the agent's last frame plus its panic text, quiet: the screen watch saw nothing and
/// the idle completion took it for a finished reply. The process tree is the proof, not the prompt.
#[test]
fn an_agent_whose_process_ended_fails_the_run_before_the_host_prompt_is_drawn() {
    for agent in ["claude", "codex"] {
        let run = running_run(agent);
        let mut watch = HostShellWatch::default();
        let crashed = format!("{}\nthread 'main' panicked at agent.rs:42: fake crash", CLAUDE_EDIT.trim_end());
        assert!(fail_if_agent_exited(&run, &mut watch, &crashed, false).is_none(), "{agent}: alive while the process runs");
        let failed = fail_if_agent_exited(&run, &mut watch, &crashed, true)
            .unwrap_or_else(|| panic!("{agent}: a process that ended must fail the run with no prompt on screen"));
        let error = failed.error.as_ref().expect("a typed error");
        assert_eq!(error.code, "AGENT_EXITED");
        assert!(error.context["last_screen"].as_str().unwrap().contains("fake crash"), "{error:?}");
        let saved = operations::get_operation(&run.project_path, &run.operation_id).unwrap();
        assert_eq!(saved.status, OperationStatus::Failed);
    }
}

#[test]
fn an_ended_process_is_no_exit_for_a_shell_pane_or_before_the_prompt_was_typed() {
    let mut shell = running_run("codex");
    shell.agent_type = "powershell".into();
    assert!(fail_if_agent_exited(&shell, &mut HostShellWatch::default(), r"PS D:\pm-demo> ", true).is_none());
    let mut starting = running_run("codex");
    starting.worker.stage_log = vec![
        operations::StageMark { stage: "dispatching".into(), at_ms: 1 },
        operations::StageMark { stage: "waiting_for_pane_readiness".into(), at_ms: 2 },
    ];
    assert!(fail_if_agent_exited(&starting, &mut HostShellWatch::default(), "", true).is_none());
}
