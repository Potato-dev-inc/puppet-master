//! Tests for the runner (SPEC n32 NS-10). The runner is only ever driven through `FakeIo`, with a
//! fake clock. Plan ids are `fake_a`, `fake_a2` (a second login of the same company) and `fake_b`.

use super::*;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::agent_runs::checks::counts::TestCounts;
use crate::missions::model::{AgentChoice, Needs, Verdict};
use crate::night_shift::classify::MissionFacts;
use crate::night_shift::io::Touched;
use crate::night_shift::io_start::key_of;
use crate::night_shift::model::{sample_run, sample_task};
use crate::worktrees::Changes;

const T0: u64 = 10_000_000; // 02:46 on the fake clock
const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;

fn agent(id: &str) -> AgentChoice {
    AgentChoice { agent_type: id.to_string(), account: None, model: None }
}

fn cand(id: &str, label: &str, vendor: Option<&str>) -> Cand {
    Cand { agent: agent(id), label: label.to_string(), vendor: vendor.map(str::to_string), runnable: Ok(()), limited_until: None }
}

fn two_plans() -> Vec<Cand> {
    vec![cand("fake_a", "Fake A", Some("va")), cand("fake_b", "Fake B", Some("vb"))]
}

fn task(n: u8, project: &str, checks: bool) -> Task {
    let mut t = sample_task(&format!("nt-0000000{n}"));
    t.spec.project = project.to_string();
    t.checks = if checks { vec![vec!["npm".to_string(), "test".to_string()]] } else { Vec::new() };
    t
}

fn run_with(id: &str, tasks: Vec<Task>, stop_in: u64) -> NightRun {
    let mut run = sample_run(id);
    run.tasks = tasks;
    run.start_at_ms = T0;
    run.stop_at_ms = T0 + stop_in;
    run.last_tick_ms = T0;
    run.parallel = 2;
    run
}

fn limit(reset_in: u64, armed: bool, parked: u64) -> Watch {
    Watch::Limit { reset_at_ms: Some(T0 + reset_in), self_resume_armed: armed, parked_ms: parked }
}

#[derive(Clone, Copy)]
enum Mode {
    Green,
    Tamper,
    Stopped,
}

fn verdict(passed: u32, total: u32) -> Verdict {
    Verdict {
        counts: Some(TestCounts { passed, failed: total - passed, skipped: 0, total }),
        checks_passed: u32::from(passed == total),
        checks_total: 1,
        ..Verdict::default()
    }
}

fn changes(files: usize) -> Changes {
    Changes { files: Vec::new(), files_changed: files, additions: 5, deletions: 1, truncated: false }
}

fn facts_for(task: &Task, mode: Mode) -> Facts {
    let last = task.attempts.last().expect("a facts call needs an attempt");
    let copy = CopyRef {
        project: task.spec.project.clone(),
        handle: format!("mission-{}-{}", task.spec.id, last.n),
        branch: "pm/copy".into(),
        base: "main".into(),
        pane_id: None,
    };
    let mission = (!task.checks.is_empty()).then(|| match mode {
        Mode::Green => MissionFacts { state: MissionState::Green, needs: None, verdict: Some(verdict(3, 3)), baseline: None, turns_used: 2 },
        Mode::Tamper => MissionFacts { state: MissionState::NeedsYou, needs: Some(Needs::Tamper), verdict: Some(verdict(3, 3)), baseline: None, turns_used: 2 },
        Mode::Stopped => MissionFacts { state: MissionState::Stopped, needs: None, verdict: Some(verdict(9, 12)), baseline: None, turns_used: 1 },
    });
    Facts { mission, changes: Some(changes(2)), summaries: vec!["Fixed the total.".into()], copy: Some(copy), stop_hhmm: String::new(), not_run: None }
}

struct FakeIo {
    now: AtomicU64,
    cands: Mutex<Vec<Cand>>,
    others: AtomicUsize,
    watches: Mutex<HashMap<String, Watch>>,
    modes: Mutex<HashMap<String, Mode>>,
    start_script: Mutex<VecDeque<StartFail>>,
    started: Mutex<HashMap<String, Started>>,
    calls: Mutex<Vec<String>>,
    stored: Mutex<Option<NightRun>>,
    archived: AtomicUsize,
    pushes: Mutex<Vec<String>>,
    awake: Mutex<Vec<bool>>,
    audits: Mutex<Vec<String>>,
}

impl FakeIo {
    fn new(cands: Vec<Cand>) -> FakeIo {
        FakeIo {
            now: AtomicU64::new(T0),
            cands: Mutex::new(cands),
            others: AtomicUsize::new(0),
            watches: Mutex::new(HashMap::new()),
            modes: Mutex::new(HashMap::new()),
            start_script: Mutex::new(VecDeque::new()),
            started: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
            stored: Mutex::new(None),
            archived: AtomicUsize::new(0),
            pushes: Mutex::new(Vec::new()),
            awake: Mutex::new(Vec::new()),
            audits: Mutex::new(Vec::new()),
        }
    }

    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn set_watch(&self, key: &str, watch: Watch) {
        self.watches.lock().insert(key.to_string(), watch);
    }

    fn set_mode(&self, task_id: &str, mode: Mode) {
        self.modes.lock().insert(task_id.to_string(), mode);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }

    fn starts(&self) -> Vec<String> {
        self.calls().into_iter().filter(|c| c.starts_with("start ")).collect()
    }
}

impl NightIo for FakeIo {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    fn utc_offset_min(&self) -> i32 {
        0
    }

    fn load(&self) -> Option<NightRun> {
        self.stored.lock().clone()
    }

    fn persist(&self, run: &mut NightRun) {
        run.rev += 1;
        *self.stored.lock() = Some(run.clone());
    }

    fn archive(&self, _run: &NightRun) {
        self.archived.fetch_add(1, Ordering::SeqCst);
    }

    fn candidates(&self, _run: &NightRun) -> Vec<Cand> {
        self.cands.lock().clone()
    }

    fn other_active_missions(&self, _run: &NightRun) -> usize {
        self.others.load(Ordering::SeqCst)
    }

    fn start(&self, run: &NightRun, task: &Task, plan: &AgentChoice, n: u8) -> Result<Started, StartFail> {
        self.calls.lock().push(format!("start {} {} {}", task.spec.id, plan.agent_type, n));
        if let Some(fail) = self.start_script.lock().pop_front() {
            return Err(fail);
        }
        let key = key_of(&run.id, &task.spec.id, n);
        let mut started = self.started.lock();
        let found = started.entry(key).or_insert_with(|| Started {
            mission_id: (!task.checks.is_empty()).then(|| format!("m-{}-{}", task.spec.id, n)),
            handle: format!("h-{}-{}", task.spec.id, n),
            pane_ids: vec![format!("pane-{}-{}", task.spec.id, n)],
        });
        Ok(found.clone())
    }

    fn watch(&self, task: &Task, a: &Attempt) -> Watch {
        let key = format!("{}:{}", task.spec.id, a.n);
        self.calls.lock().push(format!("watch {key}"));
        self.watches.lock().get(&key).cloned().unwrap_or(Watch::Working)
    }

    fn touched(&self, _task: &Task, _a: &Attempt) -> Touched {
        Touched::default()
    }

    fn stop(&self, task: &Task, a: &Attempt, by: &str) {
        self.calls.lock().push(format!("stop {}:{} {by}", task.spec.id, a.n));
    }

    fn facts(&self, _run: &NightRun, task: &Task) -> Facts {
        // A mission is green until its try ended some other way; then it was stopped.
        let by_end = match task.attempts.last().and_then(|a| a.end.as_ref()) {
            None | Some(AttemptEnd::Completed) => Mode::Green,
            Some(_) => Mode::Stopped,
        };
        let mode = self.modes.lock().get(&task.spec.id).copied().unwrap_or(by_end);
        facts_for(task, mode)
    }

    fn keep_awake(&self, on: bool) -> Result<(), String> {
        self.awake.lock().push(on);
        Ok(())
    }

    fn push_morning(&self, run: &NightRun) {
        self.pushes.lock().push(report::line(&report::summary(run)));
    }

    fn audit(&self, kind: &str, summary: &str) {
        self.audits.lock().push(format!("{kind}: {summary}"));
    }
}

fn working(run: &NightRun) -> Vec<&Task> {
    run.tasks.iter().filter(|t| t.state == TaskState::Working).collect()
}

/// At most two tasks work at once, never two in one project, never two on one plan.
fn check_limits(run: &NightRun) {
    let tasks = working(run);
    assert!(tasks.len() <= 2, "more than two tasks working");
    let mut projects: Vec<&str> = tasks.iter().map(|t| t.spec.project.as_str()).collect();
    projects.sort();
    projects.dedup();
    assert_eq!(projects.len(), tasks.len(), "two tasks in one project");
    let mut plans: Vec<String> = run.open_attempts().iter().map(|a| plan_key(&a.plan)).collect();
    plans.sort();
    plans.dedup();
    assert_eq!(plans.len(), run.open_attempts().len(), "two attempts on one plan");
}

fn class_of(run: &NightRun, i: usize) -> Class {
    run.tasks[i].outcome.as_ref().expect("an outcome").class
}

fn headline_of(run: &NightRun, i: usize) -> String {
    run.tasks[i].outcome.as_ref().expect("an outcome").headline.clone()
}

// ---- (1) order and concurrency ----------------------------------------------------------------------

#[test]
fn four_green_tasks_on_two_plans_run_in_order_two_at_a_time_one_per_project() {
    let io = FakeIo::new(two_plans());
    let tasks = vec![task(1, "/p/one", true), task(2, "/p/one", true), task(3, "/p/two", true), task(4, "/p/three", true)];
    let mut run = run_with("ns-000000000001", tasks, 8 * HOUR);

    assert!(matches!(step(&mut run, &io), Next::Again));
    assert_eq!(io.starts(), vec!["start nt-00000001 fake_a 1", "start nt-00000003 fake_b 1"]);
    check_limits(&run);

    io.set_watch("nt-00000001:1", Watch::Done);
    io.set_watch("nt-00000003:1", Watch::Done);
    assert!(matches!(step(&mut run, &io), Next::Again));
    assert_eq!(io.starts().len(), 4);
    assert_eq!(io.starts()[2], "start nt-00000002 fake_a 1", "list order wins, and the freed project is used");
    assert_eq!(io.starts()[3], "start nt-00000004 fake_b 1");
    check_limits(&run);

    io.set_watch("nt-00000002:1", Watch::Done);
    io.set_watch("nt-00000004:1", Watch::Done);
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(run.state, RunState::Done);
    for i in 0..4 {
        assert_eq!(class_of(&run, i), Class::Ready);
    }
    assert_eq!(headline_of(&run, 0), "Tests pass 3 of 3");
    assert_eq!(report::line(&report::summary(&run)), "4 of 4 done, tests green");
}

// ---- (2) a limit that the CLI waits out itself --------------------------------------------------------

#[test]
fn an_armed_limit_inside_the_night_waits_in_place_and_uses_no_second_plan() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000002", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", limit(30 * MIN, true, 4 * MIN));
    step(&mut run, &io);
    assert_eq!(io.starts().len(), 1);
    assert!(!io.calls().iter().any(|c| c.starts_with("stop ")), "nothing is stopped while the CLI waits for its reset");
    assert_eq!(run.tasks[0].state, TaskState::Working);
    assert!(run.tasks[0].attempts[0].end.is_none());
    assert!(run.events.iter().any(|e| e.kind == "waiting"));

    io.set_watch("nt-00000001:1", Watch::Done);
    step(&mut run, &io);
    assert_eq!(run.tasks[0].attempts.len(), 1);
    assert_eq!(class_of(&run, 0), Class::Ready);
}

// ---- (3) a limit that is not armed, another company has usage ------------------------------------------

#[test]
fn an_unarmed_limit_ends_the_try_and_the_task_moves_to_another_company() {
    let cands = vec![cand("fake_a", "Fake A", Some("va")), cand("fake_a2", "Fake A2", Some("va")), cand("fake_b", "Fake B", Some("vb"))];
    let io = FakeIo::new(cands);
    let mut run = run_with("ns-000000000003", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", limit(2 * HOUR, false, 5 * MIN));
    step(&mut run, &io);

    assert_eq!(io.starts(), vec!["start nt-00000001 fake_a 1", "start nt-00000001 fake_b 2"], "never a second login of the same company");
    let attempts = &run.tasks[0].attempts;
    assert!(matches!(attempts[0].end, Some(AttemptEnd::OutOfUsage { reset_at_ms: Some(_) })));
    assert!(io.calls().contains(&"stop nt-00000001:1 night-shift".to_string()));
    assert_eq!(run.plan_out.len(), 1);
    assert_eq!(run.plan_out[0].plan_key, "fake_a");
    assert!(run.events.iter().any(|e| e.kind == "out_of_usage"));
    assert!(run.events.iter().any(|e| e.kind == "moved" && e.text == "Moved task 1 to Fake B."));

    io.set_watch("nt-00000001:2", Watch::Done);
    step(&mut run, &io);
    assert_eq!(class_of(&run, 0), Class::Ready);
    let outcome = run.tasks[0].outcome.as_ref().unwrap();
    assert!(outcome.copy.is_some());
    assert_eq!(run.tasks[0].attempts.len(), 2, "the first try stays as the earlier try");
}

// ---- (4) a limit and no other plan -----------------------------------------------------------------------

#[test]
fn with_no_other_plan_the_task_waits_for_the_reset_then_uses_the_same_plan() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-000000000004", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", limit(HOUR, false, 5 * MIN));
    step(&mut run, &io);
    match &run.tasks[0].state {
        TaskState::Waiting { until_ms, why } => {
            assert_eq!(*until_ms, Some(T0 + HOUR));
            assert!(why.starts_with("Out of usage. Waiting for the reset at "), "{why}");
        }
        other => panic!("not waiting: {other:?}"),
    }
    assert_eq!(io.starts().len(), 1);
    assert!(run.events.iter().any(|e| e.kind == "waiting"));

    io.advance(HOUR + 1_000);
    step(&mut run, &io);
    assert_eq!(io.starts(), vec!["start nt-00000001 fake_a 1", "start nt-00000001 fake_a 2"]);
    assert_eq!(run.tasks[0].state, TaskState::Working);
}

#[test]
fn a_reset_after_the_stop_time_ends_the_task_instead_of_waiting() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-000000000005", vec![task(1, "/p/one", true), task(2, "/p/two", true)], 2 * HOUR);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", limit(3 * HOUR, false, 5 * MIN));
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(class_of(&run, 0), Class::NeedsLook);
    assert_eq!(headline_of(&run, 0), "Ran out of usage");
    assert_eq!(class_of(&run, 1), Class::DidntRun);
    assert!(headline_of(&run, 1).starts_with("Didn't run: every plan is out of usage until "), "{}", headline_of(&run, 1));
}

// ---- (5) a gate is never answered ----------------------------------------------------------------------------

fn trust_gate() -> Watch {
    Watch::Gate { code: "WORKSPACE_TRUST_DECLINED".into(), sentence: "Fake A asked to trust the folder.".into() }
}

#[test]
fn a_gate_blocks_the_plan_for_that_project_and_nothing_is_answered() {
    let io = FakeIo::new(two_plans());
    let tasks = vec![task(1, "/p/one", true), task(2, "/p/one", true), task(3, "/p/two", true)];
    let mut run = run_with("ns-000000000006", tasks, 8 * HOUR);
    step(&mut run, &io);
    assert_eq!(io.starts(), vec!["start nt-00000001 fake_a 1", "start nt-00000003 fake_b 1"]);

    io.set_watch("nt-00000001:1", trust_gate());
    step(&mut run, &io);
    assert!(matches!(run.tasks[0].attempts[0].end, Some(AttemptEnd::Gate { .. })));
    assert_eq!(run.blocked.len(), 1);
    assert_eq!((run.blocked[0].plan_key.as_str(), run.blocked[0].project.as_str()), ("fake_a", "/p/one"));
    assert_eq!(run.tasks[0].state, TaskState::Queued, "another plan exists, so the task is tried once more");

    io.set_watch("nt-00000003:1", Watch::Done);
    step(&mut run, &io);
    assert!(io.starts().contains(&"start nt-00000001 fake_b 2".to_string()));

    io.set_watch("nt-00000001:2", Watch::Done);
    step(&mut run, &io);
    assert!(io.starts().contains(&"start nt-00000002 fake_b 1".to_string()), "task 2 avoids the blocked pair");
    assert!(!io.starts().iter().any(|c| c == "start nt-00000002 fake_a 1"));
    io.set_watch("nt-00000002:1", Watch::Done);
    assert!(matches!(step(&mut run, &io), Next::Finished));

    // The trait has no way to write to a pane, and the fake saw only these three kinds of call.
    for call in io.calls() {
        assert!(call.starts_with("start ") || call.starts_with("watch ") || call.starts_with("stop "), "{call}");
    }
}

#[test]
fn a_gate_with_no_other_plan_parks_the_task_for_the_morning() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-000000000007", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", trust_gate());
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(class_of(&run, 0), Class::NeedsLook);
    assert_eq!(headline_of(&run, 0), "Fake A asked to trust the folder.");
    assert_eq!(run.tasks[0].outcome.as_ref().unwrap().reason.as_deref(), Some(crate::night_shift::classify::REASON_OPEN_TERMINAL));
    assert!(io.calls().contains(&"stop nt-00000001:1 night-shift".to_string()), "the turn is stopped, the pane is left showing the question");
}

// ---- (6) a question is parked after five minutes -----------------------------------------------------------

#[test]
fn an_agent_question_is_parked_after_five_minutes_with_its_text() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000008", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    let ask = |waited_ms| Watch::Asked { question: "Which database should I use?".into(), waited_ms };
    io.set_watch("nt-00000001:1", ask(4 * MIN));
    step(&mut run, &io);
    assert_eq!(run.tasks[0].state, TaskState::Working, "the grace period is five minutes");

    io.set_watch("nt-00000001:1", ask(5 * MIN));
    assert!(matches!(step(&mut run, &io), Next::Finished));
    match &run.tasks[0].attempts[0].end {
        Some(AttemptEnd::Asked { question }) => assert_eq!(question, "Which database should I use?"),
        other => panic!("not asked: {other:?}"),
    }
    assert_eq!(headline_of(&run, 0), "Stopped to ask a question");
    assert_eq!(class_of(&run, 0), Class::NeedsLook);
    assert!(io.calls().contains(&"stop nt-00000001:1 night-shift".to_string()));
}

// ---- (7) a mission that needs the person gives its slot back -----------------------------------------------

#[test]
fn a_mission_that_needs_you_is_stopped_so_the_next_task_can_use_the_slot() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-000000000009", vec![task(1, "/p/one", true), task(2, "/p/two", true)], 8 * HOUR);
    step(&mut run, &io);
    assert_eq!(io.starts().len(), 1);

    io.set_mode("nt-00000001", Mode::Tamper);
    io.set_watch("nt-00000001:1", Watch::Done);
    step(&mut run, &io);
    assert!(io.calls().contains(&"stop nt-00000001:1 night-shift".to_string()));
    assert_eq!(class_of(&run, 0), Class::NeedsLook);
    assert_eq!(headline_of(&run, 0), "The checks pass, but the tests were changed");
    assert!(matches!(run.tasks[0].attempts[0].end, Some(AttemptEnd::NeedsYou { .. })));
    assert!(io.starts().contains(&"start nt-00000002 fake_a 1".to_string()), "the freed slot is used in the same tick");
}

// ---- (8) the hard stop -----------------------------------------------------------------------------------------

#[test]
fn the_hard_stop_cuts_off_what_is_running_and_keeps_the_copy() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-00000000000a", vec![task(1, "/p/one", true), task(2, "/p/two", true)], HOUR);
    run.keep_awake = true;
    step(&mut run, &io);
    assert_eq!(io.awake.lock().clone(), vec![true]);
    io.set_mode("nt-00000001", Mode::Stopped);
    io.advance(HOUR);
    assert!(matches!(step(&mut run, &io), Next::Finished));

    assert_eq!(run.state, RunState::Done);
    assert!(matches!(run.tasks[0].attempts[0].end, Some(AttemptEnd::CutOff)));
    assert!(io.calls().contains(&"stop nt-00000001:1 night-shift".to_string()));
    assert_eq!(class_of(&run, 0), Class::NeedsLook);
    assert!(headline_of(&run, 0).starts_with("Cut off at 03:46 while working."), "{}", headline_of(&run, 0));
    assert!(headline_of(&run, 0).contains("Last check: 9 of 12 pass"));
    assert!(run.tasks[0].outcome.as_ref().unwrap().copy.is_some(), "the copy is kept");
    assert_eq!(class_of(&run, 1), Class::DidntRun);
    assert_eq!(headline_of(&run, 1), "Didn't run: no time left before 03:46");
    assert_eq!(io.awake.lock().clone(), vec![true, false], "the computer may sleep again");
    assert!(run.events.iter().any(|e| e.kind == "cut_off"));
}

// ---- (9) the person presses Stop ----------------------------------------------------------------------------------

#[test]
fn the_person_stop_ends_the_run_as_stopped() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-00000000000b", vec![task(1, "/p/one", true), task(2, "/p/two", true)], 8 * HOUR);
    step(&mut run, &io);
    request_stop(&run.id);
    assert!(stop_requested(&run.id));
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(run.state, RunState::Stopped);
    assert!(matches!(run.tasks[0].attempts[0].end, Some(AttemptEnd::StoppedByPerson)));
    assert_eq!(headline_of(&run, 0), "Stopped by you");
    assert_eq!(headline_of(&run, 1), "Didn't run: you stopped Night Shift");
    assert!(!stop_requested(&run.id), "the flag is cleared when the run ends");
}

// ---- (10) restart --------------------------------------------------------------------------------------------------

#[test]
fn a_new_runner_adopts_a_working_task_and_starts_no_duplicate() {
    let first = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-00000000000c", vec![task(1, "/p/one", true)], 8 * HOUR);
    run.keep_awake = true;
    step(&mut run, &first);
    assert_eq!(first.starts().len(), 1);
    let mut saved = first.load().expect("saved");
    assert!(saved.awake_ok);

    let second = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    adopt(&mut saved, &second);
    assert!(!saved.awake_ok, "the hold died with the old process");
    second.advance(10 * MIN);
    step(&mut saved, &second);
    assert!(second.starts().is_empty(), "the Working task is found again, not started again");
    assert_eq!(second.awake.lock().clone(), vec![true], "the new process takes the hold again");
    assert_eq!(saved.tasks[0].state, TaskState::Working);
    assert!(crate::night_shift::quiet::is_quiet_pane("pane-nt-00000001-1"), "the quiet set is rebuilt");
    crate::night_shift::quiet::unmute_run("ns-00000000000c");
}

#[test]
fn a_task_whose_start_was_never_recorded_is_queued_again_with_the_same_key() {
    let io = FakeIo::new(vec![cand("fake_a", "Fake A", Some("va"))]);
    let mut run = run_with("ns-00000000000d", vec![task(1, "/p/one", true)], 8 * HOUR);
    run.tasks[0].state = TaskState::Working;
    run.tasks[0].attempts.push(Attempt {
        n: 1,
        plan: agent("fake_a"),
        label: "Fake A".into(),
        kind: AttemptKind::Mission,
        mission_id: None,
        handle: None,
        pane_ids: Vec::new(),
        started_ms: T0,
        ended_ms: None,
        end: None,
    });
    adopt(&mut run, &io);
    assert_eq!(run.tasks[0].state, TaskState::Queued);
    assert!(run.tasks[0].attempts.is_empty());
    step(&mut run, &io);
    assert_eq!(io.starts(), vec!["start nt-00000001 fake_a 1"], "the same attempt number, so the same start key");
}

// ---- (11) the clock ----------------------------------------------------------------------------------------------------

#[test]
fn a_long_gap_between_ticks_is_reported_as_the_computer_sleeping() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-00000000000e", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    io.advance(20 * MIN);
    step(&mut run, &io);
    let slept: Vec<_> = run.events.iter().filter(|e| e.kind == "slept").collect();
    assert_eq!(slept.len(), 1);
    assert_eq!(slept[0].text, "The computer slept 02:46 to 03:06");
    io.advance(5_000);
    step(&mut run, &io);
    assert_eq!(run.events.iter().filter(|e| e.kind == "slept").count(), 1, "a normal tick is not a sleep");
}

#[test]
fn a_scheduled_run_waits_for_its_start_time_then_starts() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-00000000000f", vec![task(1, "/p/one", true)], 8 * HOUR);
    run.state = RunState::Scheduled { start_at_ms: T0 + HOUR };
    run.keep_awake = true;
    step(&mut run, &io);
    assert!(io.starts().is_empty());
    assert!(io.awake.lock().is_empty(), "the computer is not held before the start");
    io.advance(HOUR);
    step(&mut run, &io);
    assert_eq!(run.state, RunState::Running);
    assert_eq!(io.starts().len(), 1);
    assert!(run.events.iter().any(|e| e.kind == "started"));
    assert!(run.awake_ok);
}

// ---- (12) no free mission slot -----------------------------------------------------------------------------------------

#[test]
fn no_slot_is_retried_after_thirty_seconds_and_is_not_a_failure() {
    let io = FakeIo::new(two_plans());
    io.start_script.lock().push_back(StartFail::NoSlot);
    let mut run = run_with("ns-000000000010", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    assert_eq!(io.starts().len(), 1);
    assert!(matches!(&run.tasks[0].state, TaskState::Waiting { why, .. } if why == SLOT_WHY));
    assert!(run.tasks[0].outcome.is_none());

    io.advance(10_000);
    step(&mut run, &io);
    assert_eq!(io.starts().len(), 1, "not before 30 seconds");
    io.advance(21_000);
    step(&mut run, &io);
    assert_eq!(io.starts().len(), 2);
    assert_eq!(run.tasks[0].state, TaskState::Working);
}

#[test]
fn a_refused_start_ends_the_task_as_didnt_run_with_the_plain_sentence() {
    let io = FakeIo::new(two_plans());
    io.start_script.lock().push_back(StartFail::Refused { code: "MISSION_NOT_GIT".into(), sentence: "This project has no commit yet.".into() });
    let mut run = run_with("ns-000000000011", vec![task(1, "/p/one", true)], 8 * HOUR);
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(class_of(&run, 0), Class::DidntRun);
    assert_eq!(headline_of(&run, 0), "Didn't run: This project has no commit yet");
}

// ---- (13) day-time missions hold slots --------------------------------------------------------------------------------------

#[test]
fn a_day_time_mission_holds_one_slot_so_only_one_task_starts() {
    let io = FakeIo::new(two_plans());
    io.others.store(1, Ordering::SeqCst);
    let mut run = run_with("ns-000000000012", vec![task(1, "/p/one", true), task(2, "/p/two", true)], 8 * HOUR);
    step(&mut run, &io);
    assert_eq!(io.starts(), vec!["start nt-00000001 fake_a 1"]);
    assert_eq!(run.tasks[1].state, TaskState::Queued);
}

// ---- (14) the end ----------------------------------------------------------------------------------------------------------------

#[test]
fn finishing_archives_releases_the_computer_and_pushes_once() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000013", vec![task(1, "/p/one", true)], 8 * HOUR);
    run.keep_awake = true;
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", Watch::Done);
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(io.archived.load(Ordering::SeqCst), 1);
    assert_eq!(io.awake.lock().clone(), vec![true, false]);
    assert_eq!(io.pushes.lock().clone(), vec!["1 of 1 done, tests green"]);
    assert!(run.push_sent);
    assert!(run.finished_ms.is_some());
    // A finished run is never stepped again.
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(io.pushes.lock().len(), 1);
    assert!(io.audits.lock().iter().any(|a| a.starts_with("finished: ")));
}

#[test]
fn no_push_when_the_person_already_opened_the_morning_page() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000014", vec![task(1, "/p/one", true)], 8 * HOUR);
    run.seen_ms = Some(T0 + 1);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", Watch::Done);
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert!(io.pushes.lock().is_empty());
    assert_eq!(io.archived.load(Ordering::SeqCst), 1);
}

#[test]
fn a_single_turn_task_without_declared_tests_is_labelled_nothing_was_checked() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000015", vec![task(1, "/p/one", false)], 8 * HOUR);
    step(&mut run, &io);
    assert_eq!(run.tasks[0].attempts[0].kind, AttemptKind::Single);
    io.set_watch("nt-00000001:1", Watch::Done);
    step(&mut run, &io);
    assert_eq!(class_of(&run, 0), Class::NoChecks);
    assert_eq!(headline_of(&run, 0), "Done. No tests declared, nothing was checked.");
}

#[test]
fn a_failed_single_turn_and_a_vanished_record_end_as_needs_a_look() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000016", vec![task(1, "/p/one", false), task(2, "/p/two", false)], 8 * HOUR);
    step(&mut run, &io);
    io.set_watch("nt-00000001:1", Watch::Failed { code: "AGENT_EXITED".into(), sentence: "The agent exited.".into() });
    io.set_watch("nt-00000002:1", Watch::Gone);
    assert!(matches!(step(&mut run, &io), Next::Finished));
    assert_eq!(headline_of(&run, 0), "The agent exited.");
    assert_eq!(headline_of(&run, 1), "The copy or its record disappeared");
}

#[test]
fn quiet_marks_the_panes_a_task_opens_and_the_end_of_the_run_releases_them() {
    let io = FakeIo::new(two_plans());
    let mut run = run_with("ns-000000000017", vec![task(1, "/p/one", true)], 8 * HOUR);
    step(&mut run, &io);
    assert!(crate::night_shift::quiet::is_quiet_pane("pane-nt-00000001-1"));
    assert!(crate::night_shift::quiet::is_quiet_project_handle("/p/one", "h-nt-00000001-1"));
    io.set_watch("nt-00000001:1", Watch::Done);
    step(&mut run, &io);
    assert!(!crate::night_shift::quiet::is_quiet_pane("pane-nt-00000001-1"));
}

#[test]
fn what_the_person_changed_in_the_saved_record_survives_the_runners_next_save() {
    let mut run = run_with("ns-000000000018", vec![task(1, "/p/one", true)], 8 * HOUR);
    let mut stored = run.clone();
    stored.seen_ms = Some(T0 + 5);
    stored.tasks[0].review = Review::Discarded { at_ms: T0 + 6 };
    merge_person_edits(&mut run, Some(stored));
    assert_eq!(run.seen_ms, Some(T0 + 5));
    assert_eq!(run.tasks[0].review, Review::Discarded { at_ms: T0 + 6 });
    let other = run_with("ns-000000000019", Vec::new(), HOUR);
    let mut mine = run.clone();
    merge_person_edits(&mut mine, Some(other));
    assert_eq!(mine, run, "another run's record changes nothing");
}

// ---- the thread --------------------------------------------------------------------------------------------------------------------

#[test]
fn spawn_runs_one_supervisor_per_run_and_a_stop_ends_it() {
    let io = Arc::new(FakeIo::new(two_plans()));
    let mut run = run_with("ns-00000000001a", vec![task(1, "/p/one", true)], 8 * HOUR);
    run.state = RunState::Scheduled { start_at_ms: T0 + 4 * HOUR };
    *io.stored.lock() = Some(run.clone());
    let handle = spawn(io.clone(), run.id.clone()).expect("a supervisor");
    assert!(spawn(io.clone(), run.id.clone()).is_none(), "one supervisor per run");
    assert!(is_running(&run.id));
    request_stop(&run.id);
    handle.join().expect("the supervisor ends");
    assert!(!is_running(&run.id));
    let saved = io.load().expect("saved");
    assert_eq!(saved.state, RunState::Stopped);
    assert_eq!(saved.tasks[0].outcome.as_ref().unwrap().class, Class::DidntRun);
}

#[test]
fn the_app_start_hook_does_nothing_in_unit_tests() {
    assert!(!resume());
}

// ---- (15) the source scans ------------------------------------------------------------------------------------------------------------

/// Every `.rs` file of `night_shift/` that is not a test file, cut before its first `#[cfg(test)]`,
/// without comment lines.
fn production_sources() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("night_shift");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("night_shift folder") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("source");
        let body = text.split("#[cfg(test)]").next().unwrap_or_default();
        let code: String = body.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        found.push((name, code));
    }
    assert!(found.len() >= 15, "the scan found the module files");
    found
}

#[test]
fn night_shift_has_no_write_path_into_a_pane() {
    const FORBIDDEN: [&str; 7] = ["write_input", "send_keys", "press_key", "TypingPermit", "GatePermit", "write_all", "answer_prompt"];
    for (name, code) in production_sources() {
        for word in FORBIDDEN {
            assert!(!code.contains(word), "{name} mentions {word}");
        }
    }
}

#[test]
fn night_shift_code_never_names_an_agent() {
    for (name, code) in production_sources() {
        let lower = code.to_ascii_lowercase();
        for word in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!lower.contains(word), "{name} names an agent: {word}");
        }
    }
}

#[test]
fn only_the_review_module_may_merge() {
    for (name, code) in production_sources().into_iter().filter(|(name, _)| name != "review.rs") {
        assert!(!code.contains("service::merge"), "{name} merges");
        assert!(!code.contains("worktrees::merge"), "{name} merges");
    }
}
