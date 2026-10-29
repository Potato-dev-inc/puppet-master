//! Tests of the live job (SPEC n33 section 3.5): the whole loop with a fake `LiveIo`, a virtual clock and a
//! scripted history of 8 turns by two fake agents, plus a few runs on a real shadow repository for undo.
//! Nothing here starts a real test command, touches the app data folder or uses a real clock.
//!
//! Covered: culprit found and named, run count under the bound, a failing run repeated, flaky is "not sure",
//! the run cap and "Keep looking" reuse, the minute cap, Stop mid-run, several suspects, a gap between turns,
//! the refine probe both ways, tests that pass, all red, a red start the person picked, too few save points,
//! a failing setup, the copy removed on every exit path (also after a panic), one job at a time, undo refused
//! while an agent works, an undo conflict that writes nothing, a good undo that snapshots and audits,
//! redaction of everything stored, and source scans for S2, S13 and S14.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value;

use super::copy::CopyError;
use super::exec::RunResult;
use super::live::{
    CopyHandle, Live, LiveIo, WbError, WB_BAD_ID, WB_BUSY, WB_INTERNAL, WB_NOT_UNDOABLE, WB_SETUP_FAILED, WB_TEST_NOT_FOUND,
    WB_TOO_FEW, WB_UNDO_CONFLICT, WB_UNDO_RUNNING,
};
use super::model::{Finding, Limits, Outcome, UnsureWhy, Verdict};
use super::store::Store;
use super::views::LimitsView;
use crate::agent_runs::checks::counts::TestCounts;
use crate::checkpoints::engine::{self, Checkpoint, CheckpointError, Kind};
use crate::checkpoints::PaneFacts;
use crate::night_shift::checks::{Declared, Suggestion};
use crate::redact::RedactContext;
use crate::turn_summary::model::EndState;
use crate::turn_summary::{FileStats, TurnSummary};
use crate::worktrees::preset::WorktreePreset;

const PANE: &str = "pane-main";

// ---------------------------------------------------------------------------------------------
// The fake world
// ---------------------------------------------------------------------------------------------

/// What one fake run of the test command says.
#[derive(Clone)]
struct Run {
    ok: bool,
    timed_out: bool,
    not_started: bool,
    output: String,
}

impl Run {
    fn pass() -> Run {
        Run { ok: true, timed_out: false, not_started: false, output: "ok 1 - cart adds\n".into() }
    }

    fn fail() -> Run {
        Run { ok: false, timed_out: false, not_started: false, output: "not ok 1 - cart adds\nnot ok 2 - cart removes\n".into() }
    }
}

/// `(tree the copy holds, how many times that tree has run so far)` -> the answer.
type Script = Box<dyn Fn(&str, u32) -> Run + Send + Sync>;

struct Fake {
    clock: AtomicU64,
    run_ms: u64,
    pane: Mutex<Option<PaneFacts>>,
    in_folder: Mutex<Vec<(String, PaneFacts)>>,
    declared: Mutex<Vec<Declared>>,
    preset: Mutex<WorktreePreset>,
    /// `Some`: the scripted save points. `None`: the real engine on `project`.
    cps: Mutex<Option<Vec<Checkpoint>>>,
    now_tree: String,
    summaries: Vec<TurnSummary>,
    script: Script,
    calls: Mutex<HashMap<String, u32>>,
    order: Mutex<Vec<String>>,
    timeouts: Mutex<Vec<u64>>,
    tree: Arc<Mutex<Option<String>>>,
    created: AtomicU32,
    removed: Arc<AtomicU32>,
    prepare_err: Mutex<Option<CopyError>>,
    /// Press Stop during the run with this number (1-based).
    stop_at_run: AtomicU32,
    /// Block every run until the job is cancelled.
    hold: AtomicBool,
    started_run: AtomicBool,
    events: Mutex<Vec<(String, Option<String>)>>,
    audits: Mutex<Vec<(String, Value)>>,
    project: PathBuf,
}

impl Fake {
    fn new(project: &Path, script: Script) -> Fake {
        Fake {
            clock: AtomicU64::new(100_000),
            run_ms: 1_000,
            pane: Mutex::new(Some(PaneFacts {
                cwd: project.to_string_lossy().to_string(),
                agent_type: "fake".into(),
                status: "idle".into(),
            })),
            in_folder: Mutex::new(Vec::new()),
            declared: Mutex::new(vec![Declared {
                name: "npm test".into(),
                argv: vec!["npm".into(), "test".into()],
                timeout_min: 10,
                declared_ms: 1,
            }]),
            preset: Mutex::new(WorktreePreset::default()),
            cps: Mutex::new(Some(Vec::new())),
            now_tree: "tnow".into(),
            summaries: Vec::new(),
            script,
            calls: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            timeouts: Mutex::new(Vec::new()),
            tree: Arc::new(Mutex::new(None)),
            created: AtomicU32::new(0),
            removed: Arc::new(AtomicU32::new(0)),
            prepare_err: Mutex::new(None),
            stop_at_run: AtomicU32::new(0),
            hold: AtomicBool::new(false),
            started_run: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
            audits: Mutex::new(Vec::new()),
            project: project.to_path_buf(),
        }
    }

    fn runs_of(&self, tree: &str) -> u32 {
        self.calls.lock().get(tree).copied().unwrap_or(0)
    }

    fn total_runs(&self) -> usize {
        self.order.lock().len()
    }
}

struct FakeCopy {
    tree: Arc<Mutex<Option<String>>>,
    removed: Arc<AtomicU32>,
    done: bool,
    prepare_err: Option<CopyError>,
    folder: PathBuf,
}

impl CopyHandle for FakeCopy {
    fn set_tree(&mut self, tree: &str) -> Result<(), CopyError> {
        *self.tree.lock() = Some(tree.to_string());
        Ok(())
    }
    fn tree_bytes(&self, _tree: &str) -> Result<u64, CopyError> {
        Ok(10)
    }
    fn prepare(&mut self, _project: &Path, _preset: &WorktreePreset) -> Result<(), CopyError> {
        match self.prepare_err.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    fn folder(&self) -> PathBuf {
        self.folder.clone()
    }
    fn finish(mut self: Box<Self>) -> Result<(), CopyError> {
        self.done = true;
        self.removed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for FakeCopy {
    fn drop(&mut self) {
        if !self.done {
            self.removed.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl LiveIo for Fake {
    fn now_ms(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }
    fn pane(&self, _pane_id: &str) -> Option<PaneFacts> {
        self.pane.lock().clone()
    }
    fn panes_in_folder(&self, _folder: &Path) -> Vec<(String, PaneFacts)> {
        self.in_folder.lock().clone()
    }
    fn run_open(&self, _pane_id: &str, _pane: &PaneFacts) -> bool {
        false
    }
    fn checkpoints_enabled(&self) -> bool {
        true
    }
    fn git_available(&self) -> bool {
        true
    }
    fn check_projects(&self, cwd: &str) -> Vec<String> {
        vec![cwd.to_string()]
    }
    fn declared(&self, _project: &str) -> Vec<Declared> {
        self.declared.lock().clone()
    }
    fn declare(&self, _project: &str, list: Vec<Declared>) -> Result<(), String> {
        *self.declared.lock() = list;
        Ok(())
    }
    fn suggestions(&self, _project: &Path) -> Vec<Suggestion> {
        vec![Suggestion { name: "npm test".into(), argv: vec!["npm".into(), "test".into()], source: "package.json".into() }]
    }
    fn preset(&self, _cwd: &str) -> WorktreePreset {
        self.preset.lock().clone()
    }
    fn summaries(&self, _project: &Path) -> Vec<TurnSummary> {
        self.summaries.clone()
    }
    fn list_all(&self, project: &Path) -> Result<Vec<Checkpoint>, CheckpointError> {
        match self.cps.lock().clone() {
            Some(list) => Ok(list),
            None => engine::list_all(project),
        }
    }
    fn snapshot_now(&self, project: &Path, key: &str, now_ms: u64) -> Result<String, CheckpointError> {
        if self.cps.lock().is_some() {
            return Ok(self.now_tree.clone());
        }
        match engine::snapshot(project, key, Kind::After, Some("What broke it: now"), now_ms, &engine::Limits::default())? {
            engine::Snapshot::Created(cp) | engine::Snapshot::Unchanged(Some(cp)) => Ok(cp.tree),
            engine::Snapshot::Unchanged(None) => Err(CheckpointError::NotFound),
        }
    }
    fn new_copy(&self, _project: &Path, copies_root: &Path) -> Result<Box<dyn CopyHandle>, CopyError> {
        self.created.fetch_add(1, Ordering::SeqCst);
        *self.tree.lock() = None;
        Ok(Box::new(FakeCopy {
            tree: self.tree.clone(),
            removed: self.removed.clone(),
            done: false,
            prepare_err: self.prepare_err.lock().take(),
            folder: copies_root.join("deadbeef"),
        }))
    }
    fn run_test(&self, _copy: &dyn CopyHandle, argv: &[String], timeout_ms: u64, cancel: &AtomicBool) -> RunResult {
        self.started_run.store(true, Ordering::SeqCst);
        assert_eq!(argv, ["npm", "test"], "only the declared argv runs");
        while self.hold.load(Ordering::SeqCst) && !cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let stopped = |ms| RunResult {
            exit_ok: false,
            timed_out: false,
            stopped: true,
            not_started: None,
            counts: None,
            output_tail: String::new(),
            ms,
        };
        if cancel.load(Ordering::SeqCst) {
            return stopped(0);
        }
        let tree = self.tree.lock().clone().expect("the copy holds a tree before a run");
        let nth = {
            let mut calls = self.calls.lock();
            let n = calls.entry(tree.clone()).or_insert(0);
            *n += 1;
            *n
        };
        self.order.lock().push(tree.clone());
        self.timeouts.lock().push(timeout_ms);
        let run = (self.script)(&tree, nth);
        self.clock.fetch_add(self.run_ms, Ordering::SeqCst);
        let stop_at = self.stop_at_run.load(Ordering::SeqCst);
        if stop_at != 0 && self.order.lock().len() as u32 == stop_at {
            cancel.store(true, Ordering::SeqCst);
            return stopped(self.run_ms);
        }
        RunResult {
            exit_ok: run.ok,
            timed_out: run.timed_out,
            stopped: false,
            not_started: run.not_started.then(|| "npm was not found".to_string()),
            counts: (!run.ok).then_some(TestCounts { passed: 1, failed: 2, skipped: 0, total: 3 }),
            output_tail: run.output,
            ms: self.run_ms,
        }
    }
    fn redact(&self, text: &str) -> String {
        let ctx = RedactContext { env_values: Vec::new(), home: Some("C:\\Users\\you".to_string()) };
        crate::session_chat::export_text::cut_paths(text, &ctx)
    }
    fn emit(&self, job_id: &str, finding_id: Option<&str>) {
        self.events.lock().push((job_id.to_string(), finding_id.map(str::to_string)));
    }
    fn audit_undo(&self, summary: &str, detail: Value) {
        self.audits.lock().push((summary.to_string(), detail));
    }
}

struct World {
    live: Live,
    fake: Arc<Fake>,
    dir: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-wb-live-{tag}-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn world(tag: &str, script: Script) -> World {
    world_with(tag, script, |_| {})
}

fn world_with(tag: &str, script: Script, tweak: impl FnOnce(&mut Fake)) -> World {
    let dir = temp_dir(tag);
    let project = dir.join("shop");
    fs::create_dir_all(&project).expect("project");
    let mut fake = Fake::new(&project, script);
    tweak(&mut fake);
    let fake = Arc::new(fake);
    let live = Live::new(fake.clone(), Store::at(dir.join("wb")));
    World { live, fake, dir }
}

// ---------------------------------------------------------------------------------------------
// Scripted history
// ---------------------------------------------------------------------------------------------

struct T {
    pane: &'static str,
    turn: u32,
    start: u64,
    end: u64,
    before: String,
    after: String,
}

/// Eight turns, one every second, agents alternating between two panes.
fn eight() -> Vec<T> {
    (1..=8u32)
        .map(|k| T {
            pane: if k % 2 == 1 { "pa" } else { "pb" },
            turn: k.div_ceil(2),
            start: u64::from(k) * 1000 - 400,
            end: u64::from(k) * 1000,
            before: format!("t{}", k - 1),
            after: format!("t{k}"),
        })
        .collect()
}

fn checkpoints_of(turns: &[T]) -> Vec<Checkpoint> {
    let mut out = Vec::new();
    let mut n = 0u32;
    for t in turns {
        for (kind, at, tree) in [("before", t.start, &t.before), ("after", t.end, &t.after)] {
            n += 1;
            out.push(Checkpoint {
                id: n.to_string(),
                commit: format!("c{n}"),
                pane_id: t.pane.into(),
                turn: t.turn,
                kind: kind.into(),
                label: String::new(),
                note: String::new(),
                at_ms: at,
                files: 0,
                added: 0,
                deleted: 0,
                tree: tree.clone(),
            });
        }
    }
    out
}

fn summaries_of(turns: &[T]) -> Vec<TurnSummary> {
    turns
        .iter()
        .map(|t| TurnSummary {
            pane_id: t.pane.into(),
            agent: if t.pane == "pa" { "Agent One".into() } else { "Agent Two".into() },
            agent_id: Some(format!("id-{}", t.pane)),
            account: None,
            turn: t.turn,
            started_at_ms: t.start,
            ended_at_ms: t.end,
            duration_ms: t.end - t.start,
            files: Some(FileStats { count: 2, added: 5, deleted: 1, paths: Vec::new() }),
            approvals: Vec::new(),
            end: EndState { kind: "idle".into(), prompt: None },
            receipt: None,
            sentence: format!("work {}", t.end / 1000),
        })
        .collect()
}

/// The tests fail from tree `t<b>` on (and for "tnow").
fn break_from(b: u32) -> Script {
    Box::new(move |tree, _| if tree_number(tree) >= b { Run::fail() } else { Run::pass() })
}

fn tree_number(tree: &str) -> u32 {
    if tree == "tnow" {
        99
    } else {
        tree.trim_start_matches('t').parse().unwrap_or(0)
    }
}

fn world_of(tag: &str, turns: &[T], script: Script) -> World {
    let summaries = summaries_of(turns);
    let cps = checkpoints_of(turns);
    let w = world_with(tag, script, |f| f.summaries = summaries);
    *w.fake.cps.lock() = Some(cps);
    w
}

/// Starts a job, waits for its thread, returns the saved finding.
fn run_job(w: &World, good: Option<String>, limits: Option<LimitsView>, from: Option<String>) -> Finding {
    let view = w.live.start(PANE, 0, good, limits, from).expect("start");
    w.live.join(&view.id);
    let done = w.live.status(&view.id).expect("status");
    let id = done.finding_id.expect("a finding was saved");
    w.live.store().get(&id).expect("the finding is stored")
}

fn limits(runs: u32, minutes: u32) -> Option<LimitsView> {
    Some(LimitsView { max_runs: runs, max_minutes: minutes })
}

fn culprit(f: &Finding) -> (&super::model::Suspect, &Vec<String>) {
    match &f.outcome {
        Outcome::Culprit { suspect, failing, .. } => (suspect, failing),
        other => panic!("expected a culprit, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------------------------

#[test]
fn the_culprit_turn_is_found_and_named() {
    let turns = eight();
    let w = world_of("culprit", &turns, break_from(5));
    let f = run_job(&w, None, None, None);
    let (suspect, failing) = culprit(&f);
    // t5 is the 5th turn: pane `pa`, its 3rd turn.
    assert_eq!((suspect.pane_key.as_str(), suspect.turn), ("pa", 3));
    assert_eq!(suspect.agent_label, "Agent One");
    assert_eq!(suspect.agent_id.as_deref(), Some("id-pa"));
    assert_eq!(suspect.sentence.as_deref(), Some("work 5"));
    assert_eq!(failing, &vec!["cart adds".to_string(), "cart removes".to_string()]);
    match &f.outcome {
        Outcome::Culprit { counts, last_good_ms, .. } => {
            assert_eq!(counts.map(|c| c.failed), Some(2));
            assert_eq!(*last_good_ms, 4_000);
        }
        _ => unreachable!(),
    }
    assert_eq!(f.points, 9);
    assert_eq!((f.good_idx, f.bad_idx), (Some(3), Some(4)));
    assert!(f.probes.iter().any(|p| p.tree == "t5" && p.verdict == Verdict::Fail));
    assert!(f.probes.iter().any(|p| p.tree == "t4" && p.verdict == Verdict::Pass));
}

#[test]
fn the_run_count_stays_under_the_bound() {
    let turns = eight();
    let w = world_of("bound", &turns, break_from(5));
    let f = run_job(&w, None, None, None);
    assert!(f.runs <= 16, "{} runs", f.runs);
    assert_eq!(f.runs as usize, w.fake.total_runs());
    assert!(matches!(f.outcome, Outcome::Culprit { .. }));
}

#[test]
fn a_failing_point_is_run_twice_and_a_passing_one_once() {
    let turns = eight();
    let w = world_of("twice", &turns, break_from(5));
    run_job(&w, None, None, None);
    for tree in ["t5", "tnow"] {
        assert_eq!(w.fake.runs_of(tree), 2, "{tree} fails, so it is run again");
    }
    for tree in ["t1", "t2", "t3"] {
        assert!(w.fake.runs_of(tree) <= 1, "{tree} passes, so one run is enough");
    }
    // The last good point is run once more at the end.
    assert_eq!(w.fake.runs_of("t4"), 2);
}

#[test]
fn flaky_tests_are_not_blamed_on_a_turn() {
    let turns = eight();
    // t5 fails on its first run and passes on its second.
    let script: Script = Box::new(|tree, nth| {
        if tree == "t5" {
            if nth == 1 { Run::fail() } else { Run::pass() }
        } else if tree_number(tree) > 5 {
            Run::fail()
        } else {
            Run::pass()
        }
    });
    let w = world_of("flaky", &turns, script);
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::NotSure { why, .. } => assert_eq!(*why, UnsureWhy::Flaky),
        other => panic!("expected not sure, got {other:?}"),
    }
}

#[test]
fn the_run_cap_narrows_and_keep_looking_reuses_the_answers() {
    let turns = eight();
    let script = break_from(5);
    let w = world_of("cap", &turns, script);
    let first = run_job(&w, None, limits(10, 20), None);
    let (why, narrowed) = match &first.outcome {
        Outcome::NotSure { why, narrowed } => (*why, narrowed.clone()),
        other => panic!("expected not sure, got {other:?}"),
    };
    assert_eq!(why, UnsureWhy::OutOfRuns);
    assert!(first.runs <= 10);
    assert!(narrowed.is_some(), "a good point and a bad point are known: {first:?}");
    let before = w.fake.total_runs();

    let second = run_job(&w, None, limits(16, 20), Some(first.id.clone()));
    let (suspect, _) = culprit(&second);
    assert_eq!((suspect.pane_key.as_str(), suspect.turn), ("pa", 3));
    let extra = w.fake.total_runs() - before;
    // A search from scratch needs more runs than the second leg did.
    let fresh_world = world_of("cap-fresh", &turns, break_from(5));
    let fresh = run_job(&fresh_world, None, None, None);
    assert!((extra as u32) < fresh.runs, "{extra} extra runs vs {} fresh", fresh.runs);
    // A tree measured in the first leg is not run again in the second.
    let again: Vec<String> = w.fake.order.lock()[before..].to_vec();
    for probe in &first.probes {
        assert!(!again.contains(&probe.tree), "{} was cached, so it is not run again", probe.tree);
    }
}

#[test]
fn keep_looking_needs_a_not_sure_finding_of_the_same_folder() {
    let turns = eight();
    let w = world_of("keep-bad", &turns, break_from(5));
    let done = run_job(&w, None, None, None);
    let err = w.live.start(PANE, 0, None, None, Some(done.id.clone())).unwrap_err();
    assert_eq!(err.code, WB_BAD_ID);
    let err = w.live.start(PANE, 0, None, None, Some("../x".into())).unwrap_err();
    assert_eq!(err.code, WB_BAD_ID);
}

#[test]
fn the_minute_cap_uses_the_virtual_clock() {
    let turns = eight();
    let w = world_with("minutes", break_from(5), |f| f.run_ms = 40_000);
    *w.fake.cps.lock() = Some(checkpoints_of(&turns));
    let f = run_job(&w, None, limits(30, 1), None);
    match &f.outcome {
        Outcome::NotSure { why, .. } => assert_eq!(*why, UnsureWhy::OutOfTime),
        other => panic!("expected out of time, got {other:?}"),
    }
    assert!(w.fake.total_runs() <= 3, "{} runs", w.fake.total_runs());
    // Every run is also bounded by the minutes left.
    assert!(w.fake.timeouts.lock().iter().all(|ms| *ms <= 60_000));
}

#[test]
fn stop_in_the_middle_of_a_run_ends_with_not_sure_and_removes_the_copy() {
    let turns = eight();
    let w = world_of("stop", &turns, break_from(5));
    w.fake.stop_at_run.store(3, Ordering::SeqCst);
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::NotSure { why, .. } => assert_eq!(*why, UnsureWhy::Stopped),
        other => panic!("expected stopped, got {other:?}"),
    }
    assert_eq!(w.fake.created.load(Ordering::SeqCst), 1);
    assert_eq!(w.fake.removed.load(Ordering::SeqCst), 1);
}

#[test]
fn the_stop_button_cancels_a_running_job() {
    let turns = eight();
    let w = world_of("stop-button", &turns, break_from(5));
    w.fake.hold.store(true, Ordering::SeqCst);
    let view = w.live.start(PANE, 0, None, None, None).unwrap();
    while !w.fake.started_run.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }
    w.live.stop(&view.id).unwrap();
    w.live.join(&view.id);
    let done = w.live.status(&view.id).unwrap();
    let f = w.live.store().get(&done.finding_id.unwrap()).unwrap();
    assert!(matches!(f.outcome, Outcome::NotSure { why: UnsureWhy::Stopped, .. }));
    assert_eq!(w.live.stop("wb-000000000000").unwrap_err().code, WB_BAD_ID);
}

#[test]
fn two_overlapping_turns_are_several_and_sorted() {
    // Pane pb's turn (500..2000) and pane pa's second turn (1200..2500) overlap the window where it broke.
    let turns = vec![
        T { pane: "pa", turn: 1, start: 100, end: 1000, before: "t0".into(), after: "t1".into() },
        T { pane: "pb", turn: 1, start: 500, end: 2000, before: "t0".into(), after: "t2".into() },
        T { pane: "pa", turn: 2, start: 1200, end: 2500, before: "t1".into(), after: "t3".into() },
    ];
    let w = world_of("several", &turns, break_from(2));
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::Several { suspects, last_good_ms, first_bad_ms } => {
            assert_eq!(suspects.len(), 2);
            let ends: Vec<u64> = suspects.iter().map(|s| s.at_ms).collect();
            let mut sorted = ends.clone();
            sorted.sort();
            assert_eq!(ends, sorted, "suspects are listed by when their turn ended");
            assert_eq!((*last_good_ms, *first_bad_ms), (1000, 2000));
        }
        other => panic!("expected several, got {other:?}"),
    }
    let view = w.live.finding(&f.id).unwrap();
    assert!(!view.can_undo, "no Undo for several suspects");
    assert_eq!(view.kind, "several");
}

#[test]
fn a_failure_in_a_gap_between_turns_is_between_turns() {
    let turns = vec![T { pane: "pa", turn: 1, start: 100, end: 1000, before: "t0".into(), after: "t1".into() }];
    // The folder broke after the turn ended: only "now" fails.
    let script: Script = Box::new(|tree, _| if tree == "tnow" { Run::fail() } else { Run::pass() });
    let w = world_of("gap", &turns, script);
    let f = run_job(&w, None, None, None);
    assert!(matches!(f.outcome, Outcome::BetweenTurns { last_good_ms: 1000, .. }), "{:?}", f.outcome);
    assert!(!w.live.finding(&f.id).unwrap().can_undo);
}

#[test]
fn a_turn_that_started_from_other_files_is_measured_at_its_start() {
    // Turn 3 began from "tx", which is not the last good tree "t2": the search also measures "tx".
    let turns = vec![
        T { pane: "pa", turn: 1, start: 100, end: 1000, before: "t0".into(), after: "t1".into() },
        T { pane: "pb", turn: 1, start: 1100, end: 2000, before: "t1".into(), after: "t2".into() },
        T { pane: "pa", turn: 2, start: 2100, end: 3000, before: "tx".into(), after: "t3".into() },
    ];
    // The start tree passes: the turn did it.
    let passes: Script = Box::new(|tree, _| if tree == "t3" || tree == "tnow" { Run::fail() } else { Run::pass() });
    let w = world_of("refine-pass", &turns, passes);
    let f = run_job(&w, None, None, None);
    let (suspect, _) = culprit(&f);
    assert_eq!((suspect.pane_key.as_str(), suspect.turn), ("pa", 2));
    assert!(w.fake.runs_of("tx") >= 1, "the turn's own start was measured");

    // The start tree already fails: the files broke before the turn began.
    let fails: Script = Box::new(|tree, _| if matches!(tree, "t3" | "tnow" | "tx") { Run::fail() } else { Run::pass() });
    let w = world_of("refine-fail", &turns, fails);
    let f = run_job(&w, None, None, None);
    assert!(matches!(f.outcome, Outcome::BetweenTurns { .. }), "{:?}", f.outcome);
}

#[test]
fn tests_that_pass_in_the_copy_say_so() {
    let turns = eight();
    let w = world_of("passes", &turns, break_from(1000));
    let f = run_job(&w, None, None, None);
    assert_eq!(f.outcome, Outcome::TestsPass);
    assert_eq!(w.fake.total_runs(), 1, "only the newest state was run");
    let view = w.live.finding(&f.id).unwrap();
    assert!(view.title.contains("clean copy"), "{}", view.title);
}

#[test]
fn tests_that_never_passed_are_always_red() {
    let turns = eight();
    let w = world_of("red", &turns, break_from(0));
    let f = run_job(&w, None, None, None);
    assert!(matches!(f.outcome, Outcome::AlwaysRed { oldest_ms: 1000 }), "{:?}", f.outcome);
    // Without setup steps the card says old red results may miss packages.
    assert_eq!(w.live.finding(&f.id).unwrap().subtitle, super::words::no_setup_note());
}

#[test]
fn a_red_start_the_person_picked_is_named() {
    let turns = eight();
    let w = world_of("good-red", &turns, break_from(3));
    // Pane pa's first turn ends at t1; the person says the tests passed at t3 (fails).
    let picked = "pa:5".to_string(); // checkpoint ids: turn k has before = 2k-1, after = 2k; t3 = id 6 of pane pa
    let _ = picked;
    let cps = w.fake.cps.lock().clone().unwrap();
    let t3 = cps.iter().find(|c| c.kind == "after" && c.tree == "t3").unwrap();
    let id = format!("{}:{}", t3.pane_id, t3.id);
    let f = run_job(&w, Some(id), None, None);
    assert!(matches!(f.outcome, Outcome::GoodWasRed { at_ms: 3000 }), "{:?}", f.outcome);
}

#[test]
fn a_picked_start_that_is_not_in_the_list_is_refused() {
    let turns = eight();
    let w = world_of("good-bad", &turns, break_from(5));
    for bad in ["nope", "pa:999", "pa", "wb-000000000000:1", "../x:1"] {
        let err = w.live.start(PANE, 0, Some(bad.into()), None, None).unwrap_err();
        assert_eq!(err.code, WB_BAD_ID, "{bad}");
    }
    assert_eq!(w.fake.created.load(Ordering::SeqCst), 0);
}

#[test]
fn a_good_start_the_person_picked_shortens_the_search() {
    let turns = eight();
    let w = world_of("good-ok", &turns, break_from(5));
    let cps = w.fake.cps.lock().clone().unwrap();
    let t1 = cps.iter().find(|c| c.kind == "after" && c.tree == "t1").unwrap();
    let f = run_job(&w, Some(format!("{}:{}", t1.pane_id, t1.id)), None, None);
    assert!(matches!(f.outcome, Outcome::Culprit { .. }));
    assert!(w.fake.runs_of("t1") >= 1, "the picked point was run");
}

#[test]
fn too_few_save_points_say_so() {
    let turns = vec![T { pane: "pa", turn: 1, start: 100, end: 1000, before: "t0".into(), after: "tnow".into() }];
    let w = world_of("few", &turns, break_from(1));
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::Failed { code, message } => {
            assert_eq!(code, WB_TOO_FEW);
            assert_eq!(message, super::words::too_few());
        }
        other => panic!("expected failed, got {other:?}"),
    }
    assert_eq!(w.fake.created.load(Ordering::SeqCst), 0, "no copy for nothing to compare");
}

#[test]
fn a_test_command_that_cannot_start_is_a_plain_failure() {
    let turns = eight();
    let script: Script = Box::new(|_, _| Run { ok: false, timed_out: false, not_started: true, output: String::new() });
    let w = world_of("missing", &turns, script);
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::Failed { code, message } => {
            assert_eq!(code, WB_TEST_NOT_FOUND);
            assert_eq!(message, "npm was not found on this PC.");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(w.fake.removed.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failing_setup_is_a_failure_with_a_redacted_tail() {
    let turns = eight();
    let w = world_of("setup", &turns, break_from(5));
    *w.fake.prepare_err.lock() = Some(CopyError::SetupFailed {
        step: "npm ci".into(),
        tail: "npm ERR! at C:\\Users\\you\\shop\\package.json\ntoken ghp_abcdefghijklmnopqrstuvwxyz0123456789\n".into(),
    });
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::Failed { code, message } => {
            assert_eq!(code, WB_SETUP_FAILED);
            assert_eq!(message, "Getting the copy ready failed: npm ci.");
        }
        other => panic!("{other:?}"),
    }
    assert!(f.tail.contains("npm ERR!"));
    assert!(!f.tail.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"), "{}", f.tail);
    assert!(!f.tail.contains("C:\\Users\\you"), "{}", f.tail);
    assert_eq!(w.fake.total_runs(), 0);
    assert_eq!(w.fake.removed.load(Ordering::SeqCst), 1);
}

#[test]
fn a_link_in_a_save_point_is_a_copy_that_cannot_be_trusted() {
    let turns = eight();
    let w = world_of("link", &turns, break_from(5));
    *w.fake.prepare_err.lock() = Some(CopyError::Link { path: "bin/tool".into() });
    let f = run_job(&w, None, None, None);
    assert!(matches!(f.outcome, Outcome::CopyDiffers { .. }), "{:?}", f.outcome);
    assert_eq!(w.live.finding(&f.id).unwrap().kind, "copy_differs");
}

// ---------------------------------------------------------------------------------------------
// The copy, the slot, the guards
// ---------------------------------------------------------------------------------------------

#[test]
fn the_copy_is_removed_on_every_exit_path() {
    let turns = eight();
    // Normal end, a stop, a flaky end and a culprit each made exactly one copy and removed it.
    for (tag, script, stop) in [
        ("exit-ok", break_from(5), 0),
        ("exit-stop", break_from(5), 2),
        ("exit-pass", break_from(1000), 0),
    ] {
        let w = world_of(tag, &turns, script);
        w.fake.stop_at_run.store(stop, Ordering::SeqCst);
        run_job(&w, None, None, None);
        assert_eq!(w.fake.created.load(Ordering::SeqCst), 1, "{tag}");
        assert_eq!(w.fake.removed.load(Ordering::SeqCst), 1, "{tag}");
    }
}

#[test]
fn a_panic_in_the_loop_still_removes_the_copy_and_frees_the_slot() {
    let turns = eight();
    let w = world_of("panic", &turns, Box::new(|_, _| panic!("the fake test runner blew up")));
    let f = run_job(&w, None, None, None);
    match &f.outcome {
        Outcome::Failed { code, .. } => assert_eq!(code, WB_INTERNAL),
        other => panic!("{other:?}"),
    }
    assert_eq!(w.fake.created.load(Ordering::SeqCst), 1);
    assert_eq!(w.fake.removed.load(Ordering::SeqCst), 1, "Drop removed the copy");
    // The slot is free: the next job starts.
    let again = w.live.start(PANE, 0, None, None, None);
    assert!(again.is_ok());
    w.live.join(&again.unwrap().id);
}

#[test]
fn a_second_start_is_busy_until_the_first_ends() {
    let turns = eight();
    let w = world_of("busy", &turns, break_from(5));
    w.fake.hold.store(true, Ordering::SeqCst);
    let first = w.live.start(PANE, 0, None, None, None).unwrap();
    let err: WbError = w.live.start(PANE, 0, None, None, None).unwrap_err();
    assert_eq!(err.code, WB_BUSY);
    assert_eq!(err.message, "Already looking in shop.");
    let overview = w.live.overview(PANE).unwrap();
    assert!(!overview.can);
    assert_eq!(overview.busy.as_deref(), Some("Already looking in shop."));
    w.live.stop(&first.id).unwrap();
    w.live.join(&first.id);
    w.fake.hold.store(false, Ordering::SeqCst);
    let next = w.live.start(PANE, 0, None, None, None).expect("free again");
    w.live.join(&next.id);
}

#[test]
fn inputs_are_checked_before_a_job_exists() {
    let turns = eight();
    let w = world_of("inputs", &turns, break_from(5));
    assert_eq!(w.live.start(PANE, 7, None, None, None).unwrap_err().message, super::words::bad_check());
    *w.fake.pane.lock() = None;
    assert_eq!(w.live.start(PANE, 0, None, None, None).unwrap_err().message, super::words::pane_gone());
    assert!(w.live.status("wb-000000000000").is_err());
    assert_eq!(w.fake.created.load(Ordering::SeqCst), 0);
    // Declared tests: none -> a clear message.
    let w = world_of("inputs-none", &turns, break_from(5));
    w.fake.declared.lock().clear();
    assert_eq!(w.live.start(PANE, 0, None, None, None).unwrap_err().message, super::words::no_test());
}

#[test]
fn the_overview_lists_tests_points_and_limits() {
    let turns = eight();
    let w = world_of("overview", &turns, break_from(5));
    let o = w.live.overview(PANE).unwrap();
    assert!(o.can && o.busy.is_none());
    assert_eq!(o.tests.len(), 1);
    assert_eq!(o.tests[0].command_text, "npm test");
    assert!(o.suggestions.is_empty(), "suggestions only show while nothing is declared");
    assert_eq!(o.points.len(), 8);
    assert!(o.points[0].at_ms > o.points[7].at_ms, "newest first");
    assert!(o.points[0].label.starts_with("Agent Two · turn 4 · "), "{}", o.points[0].label);
    assert!(o.points[0].checkpoint_id.starts_with("pb:"));
    assert_eq!(o.limits, LimitsView { max_runs: 16, max_minutes: 20 });
    w.fake.declared.lock().clear();
    let o = w.live.overview(PANE).unwrap();
    assert_eq!(o.suggestions.len(), 1);
    assert_eq!(o.suggestions[0].command_text, "npm test");
}

#[test]
fn declare_refuses_shell_operators_and_keeps_one_entry_per_command() {
    let turns = eight();
    let w = world_of("declare", &turns, break_from(5));
    let err = w.live.declare(PANE, "x", "npm test && rm x", 5).unwrap_err();
    assert!(err.message.contains('&'), "{}", err.message);
    let saved = w.live.declare(PANE, "unit", "npm test", 5).unwrap();
    assert_eq!((saved.index, saved.timeout_min, saved.command_text.as_str()), (0, 5, "npm test"));
    assert_eq!(w.fake.declared.lock().len(), 1);
    let other = w.live.declare(PANE, "lint", "npm run lint", 5).unwrap();
    assert_eq!(other.index, 1);
}

#[test]
fn settings_are_clamped_and_kept() {
    let turns = eight();
    let w = world_of("settings", &turns, break_from(5));
    let kept = w.live.settings_set(LimitsView { max_runs: 0, max_minutes: 999 }).unwrap();
    assert_eq!(kept, LimitsView { max_runs: 1, max_minutes: 60 });
    assert_eq!(w.live.settings_get(), kept);
    let used = Limits::clamped(kept.max_runs, kept.max_minutes);
    assert_eq!(used, Limits { max_runs: 1, max_minutes: 60 });
}

#[test]
fn findings_are_listed_for_the_folder_and_a_bad_id_is_refused() {
    let turns = eight();
    let w = world_of("findings", &turns, break_from(5));
    let f = run_job(&w, None, None, None);
    let list = w.live.findings(PANE).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, f.id);
    assert_eq!(w.live.finding("../x").unwrap_err().code, WB_BAD_ID);
    assert_eq!(w.live.finding("wb-ffffffffffff").unwrap_err().code, WB_BAD_ID);
    // Events: every change names the job; the last one carries the finding.
    let events = w.fake.events.lock().clone();
    assert!(!events.is_empty());
    assert_eq!(events.last().unwrap().1.as_deref(), Some(f.id.as_str()));
    // A culprit from a scripted history has no real save points: undo is gone, not a crash.
    assert!(w.live.undo_plan(&f.id).is_err());
    // Only a culprit can be undone.
    let pass = world_of("findings-pass", &turns, break_from(1000));
    let g = run_job(&pass, None, None, None);
    assert_eq!(pass.live.undo(&g.id).unwrap_err().code, WB_NOT_UNDOABLE);
    assert_eq!(pass.live.recheck(&g.id).unwrap_err().code, WB_NOT_UNDOABLE);
}

// ---------------------------------------------------------------------------------------------
// Privacy
// ---------------------------------------------------------------------------------------------

const SECRETS: [&str; 3] = ["sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789", "ghp_abcdefghijklmnopqrstuvwxyz0123456789", "AKIAIOSFODNN7EXAMPLE"];

#[test]
fn secrets_and_paths_in_test_output_never_reach_the_finding_or_its_view() {
    let turns = eight();
    let output = format!(
        "not ok 1 - cart adds\n  error at C:\\Users\\you\\shop\\src\\cart.ts:4\n  key {}\n  token {}\n  aws {}\nnot ok 2 - cart removes {}\n",
        SECRETS[0], SECRETS[1], SECRETS[2], SECRETS[1]
    );
    let script: Script = Box::new(move |tree, _| {
        if tree_number(tree) >= 5 {
            Run { ok: false, timed_out: false, not_started: false, output: output.clone() }
        } else {
            Run::pass()
        }
    });
    let w = world_of("secrets", &turns, script);
    let f = run_job(&w, None, None, None);
    let stored = fs::read_to_string(w.dir.join("wb").join("findings.json")).unwrap();
    let view = serde_json::to_string(&w.live.finding(&f.id).unwrap()).unwrap();
    for text in [&stored, &view, &f.tail] {
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} leaked");
        }
        assert!(!text.contains("Users\\\\you") && !text.contains("Users\\you"), "a home path leaked: {text}");
    }
    assert!(f.tail.contains("cart adds"), "the useful part is kept: {}", f.tail);
    // The view never carries the folder or the argv.
    assert!(!view.contains(w.dir.to_string_lossy().as_ref()) && !view.contains("checkArgv") && !view.contains("\"npm\""), "{view}");
}

#[test]
fn only_the_last_forty_lines_of_one_failing_run_are_kept() {
    let turns = eight();
    let long: String = (1..=120).map(|i| format!("line {i}\n")).collect::<String>() + "not ok 1 - cart adds\n";
    let script: Script = Box::new(move |tree, _| {
        if tree_number(tree) >= 5 {
            Run { ok: false, timed_out: false, not_started: false, output: long.clone() }
        } else {
            Run::pass()
        }
    });
    let w = world_of("tail", &turns, script);
    let f = run_job(&w, None, None, None);
    assert_eq!(f.tail.lines().count(), super::model::TAIL_LINES);
    assert!(f.tail.contains("line 120") && !f.tail.contains("line 10\n"), "{}", f.tail);
}

// ---------------------------------------------------------------------------------------------
// A real shadow repository: undo, and the folder stays as it was
// ---------------------------------------------------------------------------------------------

/// A project with three turns saved by the real engine. The tests fail once `state.txt` says `bad`.
struct Repo {
    world: World,
    project: PathBuf,
}

fn write(project: &Path, rel: &str, text: &str) {
    let path = project.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn snap(project: &Path, pane: &str, kind: Kind, turn: u32, at: u64) {
    engine::snapshot_numbered(project, pane, kind, None, at, &engine::Limits::default(), Some(turn)).expect("snapshot");
}

fn real_script(project: PathBuf) -> Script {
    Box::new(move |tree, _| {
        let state = engine::file_at(&project, tree, "state.txt").ok().flatten().unwrap_or_default();
        if state == b"bad" { Run::fail() } else { Run::pass() }
    })
}

/// Turn 1 (pa) adds notes, turn 2 (pb) breaks the state and edits line 2 of the cart, turn 3 (pa) edits `later`.
/// `same_line`: turn 3 edits line 2 of the cart too (a conflict for an undo of turn 2).
fn repo(tag: &str, same_line: bool) -> Repo {
    let dir = temp_dir(tag);
    let project = dir.join("shop");
    fs::create_dir_all(&project).unwrap();
    write(&project, "cart.txt", "l1\nl2\nl3\nl4\nl5\n");
    write(&project, "state.txt", "ok");
    let script = real_script(project.clone());
    let mut fake = Fake::new(&project, script);
    fake.summaries = vec![
        summary("pa", 1, "Agent One", 1_000),
        summary("pb", 1, "Agent Two", 2_000),
        summary("pa", 2, "Agent One", 3_000),
    ];
    *fake.cps.lock() = None;
    let fake = Arc::new(fake);
    let live = Live::new(fake.clone(), Store::at(dir.join("wb")));

    snap(&project, "pa", Kind::Before, 1, 500);
    write(&project, "notes.txt", "hello");
    snap(&project, "pa", Kind::After, 1, 1_000);

    snap(&project, "pb", Kind::Before, 1, 1_500);
    write(&project, "state.txt", "bad");
    write(&project, "cart.txt", "l1\nl2-agent\nl3\nl4\nl5\n");
    snap(&project, "pb", Kind::After, 1, 2_000);

    snap(&project, "pa", Kind::Before, 2, 2_500);
    if same_line {
        write(&project, "cart.txt", "l1\nl2-later\nl3\nl4\nl5\n");
    } else {
        write(&project, "cart.txt", "l1\nl2-agent\nl3\nl4\nl5-later\n");
    }
    snap(&project, "pa", Kind::After, 2, 3_000);

    *fake.pane.lock() = Some(PaneFacts { cwd: project.to_string_lossy().to_string(), agent_type: "fake".into(), status: "idle".into() });
    Repo { world: World { live, fake, dir }, project }
}

fn summary(pane: &str, turn: u32, agent: &str, end: u64) -> TurnSummary {
    TurnSummary {
        pane_id: pane.into(),
        agent: agent.into(),
        agent_id: Some(format!("id-{pane}")),
        account: None,
        turn,
        started_at_ms: end - 500,
        ended_at_ms: end,
        duration_ms: 500,
        files: Some(FileStats { count: 2, added: 3, deleted: 1, paths: Vec::new() }),
        approvals: Vec::new(),
        end: EndState { kind: "idle".into(), prompt: None },
        receipt: None,
        sentence: format!("work at {end}"),
    }
}

/// Every file under `dir` outside `.puppet-master` as `(relative path, bytes)`.
fn hash_folder(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if rel.starts_with(".puppet-master") {
                continue;
            }
            if path.is_dir() {
                out.push((format!("{rel}/"), Vec::new()));
                walk(root, &path, out);
            } else {
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn a_full_job_on_a_real_repository_finds_the_turn_and_leaves_the_folder_alone() {
    let r = repo("real-job", false);
    let before = hash_folder(&r.project);
    let f = run_job(&r.world, None, None, None);
    let (suspect, _) = culprit(&f);
    assert_eq!((suspect.pane_key.as_str(), suspect.turn, suspect.agent_label.as_str()), ("pb", 1, "Agent Two"));
    assert_eq!(hash_folder(&r.project), before, "S1: names, sizes and bytes are the same after a job");
    assert_eq!(r.world.fake.removed.load(Ordering::SeqCst), 1);
    // The job's own "now" save point is kept under its finding id and never counts as a turn.
    let all = engine::list_all(&r.project).unwrap();
    assert!(all.iter().any(|c| c.pane_id == f.id));
}

#[test]
fn undo_only_this_turn_keeps_later_work_snapshots_first_and_audits() {
    let r = repo("real-undo", false);
    let f = run_job(&r.world, None, None, None);
    let live = &r.world.live;
    let plan = live.undo_plan(&f.id).unwrap();
    assert!(plan.conflicts.is_empty() && plan.blocked_by.is_none(), "{plan:?}");
    assert!(plan.writes.contains(&"state.txt".to_string()) && plan.writes.contains(&"cart.txt".to_string()), "{plan:?}");
    assert_eq!(fs::read_to_string(r.project.join("state.txt")).unwrap(), "bad", "a plan writes nothing");

    let before_cps = engine::list_all(&r.project).unwrap().len();
    let report = live.undo(&f.id).unwrap();
    assert!(report.written >= 2);
    assert_eq!(fs::read_to_string(r.project.join("state.txt")).unwrap(), "ok");
    assert_eq!(fs::read_to_string(r.project.join("cart.txt")).unwrap(), "l1\nl2\nl3\nl4\nl5-later\n", "later lines stay");
    // A safety save point was taken first.
    let cps = engine::list_all(&r.project).unwrap();
    assert!(cps.len() > before_cps && cps.iter().any(|c| c.pane_id == "wb-undo" && c.kind == "undo"));
    // The finding is marked, and the audit line has the folder's last name and no path.
    assert_eq!(live.store().get(&f.id).unwrap().undone_ms, Some(report.undone_at_ms));
    let audits = r.world.fake.audits.lock().clone();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].0, "Undid turn 1 of Agent Two in shop");
    assert!(!audits[0].1.to_string().contains("Users"));
    // Undoing twice is refused, and the card no longer offers it.
    assert_eq!(live.undo(&f.id).unwrap_err().code, WB_NOT_UNDOABLE);
    assert!(!live.finding(&f.id).unwrap().can_undo);
}

#[test]
fn an_undo_conflict_writes_nothing() {
    let r = repo("real-conflict", true);
    let f = run_job(&r.world, None, None, None);
    let before = hash_folder(&r.project);
    let plan = r.world.live.undo_plan(&f.id).unwrap();
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].path, "cart.txt");
    let err = r.world.live.undo(&f.id).unwrap_err();
    assert_eq!(err.code, WB_UNDO_CONFLICT);
    assert!(err.message.contains("cart.txt") && err.message.contains("Nothing was changed"), "{}", err.message);
    assert_eq!(hash_folder(&r.project), before, "byte for byte the same");
    assert!(r.world.fake.audits.lock().is_empty());
    assert!(r.world.live.store().get(&f.id).unwrap().undone_ms.is_none());
}

#[test]
fn undo_is_refused_while_an_agent_in_the_folder_works() {
    let r = repo("real-running", false);
    let f = run_job(&r.world, None, None, None);
    let before = hash_folder(&r.project);
    r.world.fake.in_folder.lock().push((
        "pane-other".into(),
        PaneFacts { cwd: r.project.to_string_lossy().to_string(), agent_type: "fake".into(), status: "running".into() },
    ));
    let err = r.world.live.undo(&f.id).unwrap_err();
    assert_eq!(err.code, WB_UNDO_RUNNING);
    assert_eq!(err.message, super::words::undo_running());
    assert_eq!(r.world.live.undo_plan(&f.id).unwrap().blocked_by.as_deref(), Some(super::words::undo_running()));
    assert_eq!(hash_folder(&r.project), before);
}

#[test]
fn check_it_again_runs_once_after_an_undo() {
    let r = repo("real-recheck", false);
    let f = run_job(&r.world, None, None, None);
    let used = r.world.fake.total_runs();
    r.world.live.undo(&f.id).unwrap();
    let job = r.world.live.recheck(&f.id).unwrap();
    r.world.live.join(&job.id);
    let done = r.world.live.status(&job.id).unwrap();
    let again = r.world.live.finding(&done.finding_id.unwrap()).unwrap();
    assert_eq!(again.kind, "tests_pass");
    assert_eq!(r.world.fake.total_runs() - used, 1, "a passing state takes one run");
    assert_eq!(r.world.fake.removed.load(Ordering::SeqCst), 2);
}

#[test]
fn check_it_again_names_what_still_fails() {
    let r = repo("real-recheck-red", false);
    let f = run_job(&r.world, None, None, None);
    // Undo, then something puts the tests back to red (a later edit outside the turn).
    r.world.live.undo(&f.id).unwrap();
    write(&r.project, "state.txt", "bad");
    let job = r.world.live.recheck(&f.id).unwrap();
    r.world.live.join(&job.id);
    let done = r.world.live.status(&job.id).unwrap();
    let again = r.world.live.finding(&done.finding_id.unwrap()).unwrap();
    assert_eq!(again.kind, "failed");
    assert_eq!(again.failing, vec!["cart adds".to_string(), "cart removes".to_string()]);
}

#[test]
fn the_diff_lists_the_turn_files_and_one_files_patch() {
    let r = repo("real-diff", false);
    let f = run_job(&r.world, None, None, None);
    let list = r.world.live.diff(&f.id, None).unwrap();
    let mut paths: Vec<&str> = list.files.iter().map(|c| c.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["cart.txt", "state.txt"]);
    let one = r.world.live.diff(&f.id, Some("cart.txt")).unwrap();
    assert!(one.patch.unwrap().contains("l2-agent"));
    // S11: a path outside the turn's own list is refused.
    assert_eq!(r.world.live.diff(&f.id, Some("notes.txt")).unwrap_err().code, WB_BAD_ID);
    assert_eq!(r.world.live.diff(&f.id, Some("../secret")).unwrap_err().code, WB_BAD_ID);
}

// ---------------------------------------------------------------------------------------------
// Source scans (S2, S13, S14)
// ---------------------------------------------------------------------------------------------

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// A source file without its test part (everything from the first `#[cfg(test)]`).
fn non_test_text(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn bisect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    rust_files(&src_dir().join("checkpoints").join("bisect"), &mut files);
    files.retain(|p| p.file_name().is_some_and(|n| n != "engine_tests.rs"));
    files
}

#[test]
fn s2_nothing_outside_the_desktop_window_reaches_the_search() {
    for dir in ["bridge", "tool_registry", "mcp_shim", "companion", "push"] {
        let mut files = Vec::new();
        rust_files(&src_dir().join(dir), &mut files);
        for file in files {
            let text = fs::read_to_string(&file).unwrap();
            assert!(!text.contains("checkpoints::bisect") && !text.contains("bisect::live"), "{} reaches What broke it", file.display());
        }
    }
}

#[test]
fn s13_generic_code_names_no_agent() {
    let files = bisect_files();
    assert!(files.len() >= 10);
    for file in files {
        let text = non_test_text(&file).to_lowercase();
        for name in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!text.contains(name), "{} names an agent: {name}", file.display());
        }
    }
}

#[test]
fn s14_nothing_is_typed_into_a_pane() {
    for file in bisect_files() {
        let text = non_test_text(&file);
        for needle in ["write_input", "send_keys", "press_key", "TypingPermit", "GatePermit"] {
            assert!(!text.contains(needle), "{} uses {needle}", file.display());
        }
    }
}

#[test]
fn s3_the_live_code_takes_an_index_never_command_text() {
    let text = non_test_text(&src_dir().join("checkpoints").join("bisect").join("live.rs"));
    assert!(text.contains("check_index: usize"));
    // The job runs the declared argv through the runner and never builds a shell line.
    let job = non_test_text(&src_dir().join("checkpoints").join("bisect").join("job.rs"));
    assert!(!job.contains("sh -c") && !job.contains("cmd /c") && !job.contains("powershell"));
}
