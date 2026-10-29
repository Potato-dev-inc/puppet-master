//! The supervisor's clock: a run must never stay `running` past its timeout, whatever its ticks say.

use super::*;
use std::sync::mpsc;
use std::time::Instant;

/// Real Claude Code capture: the trust dialog was declined, Claude quit, its host PowerShell prints a
/// prompt under the dialog's text.
const DECLINED_HOST_SHELL: &str = include_str!("../../../tui/claude/fixtures/trust_declined_host_shell.txt");
const TRUST: &str = include_str!("../../../tui/claude/fixtures/trust_dialog_2_1_299.txt");

fn run_with_timeout(timeout_ms: u64) -> OperationSnapshot {
    let project = std::env::temp_dir().join(format!("pm-sup-{}", uuid::Uuid::new_v4()));
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
        timeout_ms,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("supervisor-loop".into()),
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    crate::operations::create_operation(request).unwrap().0
}

/// Runs the loop on a thread and gives up waiting after 60 s, so a loop that never ends fails the
/// test instead of hanging it (a hang detector: a starved machine took more than the 5 s this had).
fn run_loop(
    snapshot: OperationSnapshot,
    tick: impl FnMut(&OperationSnapshot) -> Step + Send + 'static,
) -> Option<Option<OperationSnapshot>> {
    let (done, wait) = mpsc::channel();
    thread::spawn(move || {
        let mut tick = tick;
        let _ = done.send(supervise_loop(&snapshot, Duration::from_millis(10), &mut tick));
    });
    wait.recv_timeout(Duration::from_secs(60)).ok()
}

#[test]
fn a_run_whose_prompt_is_refused_every_tick_still_reaches_its_deadline() {
    // VERIFY-R7 #2b: after a declined trust dialog the supervisor asked to re-deliver the prompt on
    // every tick, was refused, and `continue`d past the deadline check: the 900 s run was still
    // `running` after 16 minutes.
    let snapshot = run_with_timeout(300);
    let started = Instant::now();
    let ended = run_loop(snapshot, |_| Step::Redelivered).expect("the loop must end at the deadline, not run for ever");
    assert!(ended.is_some(), "the deadline is reported so the run is failed with TIMEOUT");
    assert!(started.elapsed() < Duration::from_secs(50), "{:?}", started.elapsed());
}

#[test]
fn every_kind_of_tick_is_bounded_by_the_deadline() {
    for step in [Step::Continue, Step::Redelivered] {
        let snapshot = run_with_timeout(200);
        let ended = run_loop(snapshot, move |_| match step {
            Step::Continue => Step::Continue,
            _ => Step::Redelivered,
        })
        .expect("the loop ends");
        assert!(ended.is_some());
    }
}

#[test]
fn a_finished_tick_or_a_terminal_run_ends_the_loop_without_a_deadline() {
    let snapshot = run_with_timeout(60_000);
    assert!(run_loop(snapshot, |_| Step::Finished).expect("ends").is_none());
    let snapshot = run_with_timeout(60_000);
    operations::mark_operation_state(
        &snapshot.project_path,
        &snapshot.operation_id,
        OperationStatus::Failed,
        StateSource::Inferred,
        Some("failed".into()),
        None,
        Some(OperationError::new("X", "x", false)),
    )
    .unwrap();
    assert!(run_loop(snapshot, |_| panic!("a terminal run is not observed")).expect("ends").is_none());
}

#[test]
fn a_prompt_that_can_never_be_redelivered_is_a_dead_end_with_the_reason() {
    use super::super::pane_turn::redelivery_dead_end;
    // The user answered "Don't trust" (recorded): final, even though the dialog text is still on screen.
    crate::pane_prompt::mark_gate_declined("sup-declined-pane");
    let declined = redelivery_dead_end("claude", "sup-declined-pane", DECLINED_HOST_SHELL).expect("a decline is final");
    assert_eq!(declined.code, "WORKSPACE_TRUST_DECLINED");
    assert!(!declined.recoverable);
    // No decline on record, but Claude is gone and PowerShell shows its prompt: the agent exited.
    let gone = redelivery_dead_end("claude", "sup-other-pane", DECLINED_HOST_SHELL).expect("the host shell is a dead end");
    assert_eq!(gone.code, "AGENT_EXITED");
    // A dialog the user can still answer is not a dead end: the run keeps waiting.
    assert!(redelivery_dead_end("claude", "sup-other-pane", TRUST).is_none());
    // Nor is a blank, still-drawing pane.
    assert!(redelivery_dead_end("claude", "sup-other-pane", "").is_none());
}

#[test]
fn a_run_parked_at_a_usage_limit_is_not_ended_by_its_deadline_until_it_runs_again() {
    // The deadline stops the worker, which presses keys in the pane; Esc is how Claude cancels its own
    // "continue automatically at usage limit" wait. A run waiting out its limit must not be touched.
    // The loop reads a clock the test moves, so no amount of machine slowness changes what the loop sees:
    // with real time, a deadline of 1 s and then 3 s was passed before the loop's first look at a run that
    // had only just been parked, whenever the whole lib suite ran in parallel (round 13 and again in CI).
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    let snapshot = run_with_timeout(3_000);
    for status in [OperationStatus::Starting, OperationStatus::Running] {
        operations::mark_operation_state(&snapshot.project_path, &snapshot.operation_id, status, StateSource::Inferred, Some("running".into()), None, None).unwrap();
    }
    operations::mark_operation_usage_limit(&snapshot.project_path, &snapshot.operation_id, serde_json::json!({"kind": "usage_limit"})).unwrap();

    let start = snapshot.started_at_ms.unwrap_or(snapshot.created_at_ms);
    let passed = Arc::new(AtomicU64::new(0));
    let ticks = Arc::new(AtomicUsize::new(0));
    let (done, wait) = mpsc::channel();
    let looped = snapshot.clone();
    {
        let (passed, ticks) = (passed.clone(), ticks.clone());
        thread::spawn(move || {
            let clock = move || start + passed.load(Ordering::SeqCst);
            let _ = done.send(supervise_loop_on(
                &looped,
                Duration::from_millis(10),
                &mut |_| {
                    ticks.fetch_add(1, Ordering::SeqCst);
                    Step::Continue
                },
                &clock,
            ));
        });
    }
    // The loop looks at the run again (this many more times) before the test says anything about it.
    let looks = |more: usize| {
        let target = ticks.load(Ordering::SeqCst) + more;
        let began = Instant::now();
        while ticks.load(Ordering::SeqCst) < target {
            assert!(began.elapsed() < Duration::from_secs(60), "the loop stopped looking at the run");
            thread::sleep(Duration::from_millis(5));
        }
    };
    looks(3);
    // Ten seconds pass, three times the deadline: the loop is still watching and has not asked to stop the worker.
    passed.store(10_000, Ordering::SeqCst);
    looks(5);
    assert!(wait.try_recv().is_err(), "the deadline ended a run parked at a usage limit");

    // The agent carries on: the run is running again and the clock runs again. Part of the deadline is not yet
    // used up (3 s allowed, 2 s since it ran again), so the run is still not ended ...
    operations::mark_operation_observation(&snapshot.project_path, &snapshot.operation_id, Some("running".into()), None, StateSource::Inferred).unwrap();
    looks(5);
    passed.store(12_000, Ordering::SeqCst);
    looks(5);
    assert!(wait.try_recv().is_err(), "the run was ended with 1 s of its deadline left");
    // ... and once it is used up (4 s) the deadline ends it.
    passed.store(14_000, Ordering::SeqCst);
    let ended = wait.recv_timeout(Duration::from_secs(60)).expect("the loop ends once the run is running again and its deadline is used up");
    assert!(ended.is_some(), "the deadline is reported again");
}

#[test]
fn a_run_parked_on_a_startup_dialog_is_not_judged_from_its_pane() {
    // Live r13: the first Claude run in a new folder parked on the trust question; after Trust the
    // composer went idle and the supervisor completed the run with the input hint as its result,
    // before the dispatch thread typed the task.
    let mut parked = run_with_timeout(60_000);
    parked.status = OperationStatus::WaitingInput;
    parked.stage = Some("waiting_for_pane_readiness".into());
    assert!(!super::observes_turn(&parked), "a run whose task was not typed yet is the dispatch thread's");
    parked.stage = Some("waiting_input".into());
    assert!(super::observes_turn(&parked), "a turn waiting on an approval mid-run is still watched");
    parked.status = OperationStatus::Running;
    parked.stage = Some("running".into());
    assert!(super::observes_turn(&parked));
}
