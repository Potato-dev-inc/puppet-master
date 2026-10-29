//! What broke it, end to end (SPEC n33 section 5, WB-18).
//!
//! A temp project gets a real shadow repository (`checkpoints::engine`), its history is written turn by turn
//! (three agents, six turns each), and the person's declared command (`node check.mjs`) runs for real in a copy
//! through the real `Live` (job thread, search, blame, undo, recheck). What is fake: the pane list (a test has no
//! bridge), the window events (recorded, not emitted) and the agents' turns (the test writes the files, the save
//! points and the turn summary file the runtime would write).
//!
//! Run (Windows; the test exe links the whole app, so it needs the feature that embeds the Common Controls manifest
//! and roots app data in a temp folder):
//!
//! ```text
//! cargo test --features dev-fake-tui --test what_broke_it_e2e -- --test-threads=1
//! ```
//!
//! The scenarios share one process and one app data root, so they run one at a time (a lock). Each test skips with a
//! printed note when `node` is not installed.

#![cfg(all(windows, feature = "dev-fake-tui"))]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use puppet_master_app_lib::app_paths;
use puppet_master_app_lib::checkpoints::bisect::copy::CopyError;
use puppet_master_app_lib::checkpoints::bisect::exec::RunResult;
use puppet_master_app_lib::checkpoints::bisect::live::{CopyHandle, Live, LiveIo, RealIo, WB_NO_TEST, WB_UNDO_CONFLICT};
use puppet_master_app_lib::checkpoints::bisect::model::{Finding, Outcome, UnsureWhy};
use puppet_master_app_lib::checkpoints::bisect::store::Store;
use puppet_master_app_lib::checkpoints::bisect::views::{JobState, JobView, LimitsView};
use puppet_master_app_lib::checkpoints::engine::{self, Checkpoint, CheckpointError, Kind, Limits, Snapshot};
use puppet_master_app_lib::checkpoints::PaneFacts;
use puppet_master_app_lib::night_shift::checks::{Declared, Suggestion};
use puppet_master_app_lib::proc_util::hidden_command;
use puppet_master_app_lib::turn_summary::model::TurnSummary;
use puppet_master_app_lib::worktrees::preset::WorktreePreset;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The three agents of the history: pane id, the name the summaries carry, the agent id.
const PANES: [(&str, &str, &str); 3] = [
    ("pane-codex", "Codex", "codex"),
    ("pane-claude", "Claude Code", "claude"),
    ("pane-cursor", "Cursor", "cursor"),
];
/// Six turns per agent, interleaved: global turn `g` is pane `g % 3`, its turn `g / 3 + 1`.
const TURNS: u32 = 18;
/// Global turn 10 = pane-claude's turn 4: the turn that breaks `cart.mjs`.
const CULPRIT_G: u32 = 10;
const CULPRIT_SENTENCE: &str = "Changed the cart total to count only the last item.";
/// Global turn 7 = pane-claude's turn 3: a state where the tests still pass (used as the picked good point).
const LAST_GOOD_G: usize = 7;
/// Each turn: starts at its slot, ends half a minute later. Slots are a minute apart, so turns never overlap.
const SLOT_MS: u64 = 60_000;
const TURN_MS: u64 = 30_000;

const CART_OK: &str = r#"// Cart totals for the shop (fixture).
export function total(items) {
  let sum = 0;
  for (const item of items) sum += item.price * item.qty;
  return Math.round(sum * 100) / 100;
}

export function count(items) {
  return items.reduce((n, item) => n + item.qty, 0);
}
"#;
const LINE_OK: &str = "  for (const item of items) sum += item.price * item.qty;";
const LINE_BUG: &str = "  for (const item of items) sum = item.price * item.qty;";
/// A later agent edits the same line the culprit changed.
const LINE_SAME_LATER: &str = "  for (const item of items) sum = item.price * item.qty * 1;";
/// A later agent adds lines at the end of the file (other lines than the culprit's).
const LABEL_FN: &str = "\nexport function label(n) {\n  return 'Items: ' + n;\n}\n";

/// The declared test. Markers in the folder change it: `SLOW` sleeps 20 s, `FLAKY` fails every first run of a
/// pair (the counter lives in the copy, so the copy is what decides). It writes its pid so a Stop test can find it.
const CHECK_MJS: &str = r#"import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { setTimeout as wait } from 'node:timers/promises';
import { total, count } from './cart.mjs';

writeFileSync('pid.txt', String(process.pid));
if (existsSync('SLOW')) await wait(20000);
if (existsSync('FLAKY')) {
  const n = existsSync('runs.count') ? Number(readFileSync('runs.count', 'utf8')) : 0;
  writeFileSync('runs.count', String(n + 1));
  if (n % 2 === 0) {
    console.log('FAIL flaky: the first run of each pair fails');
    process.exit(1);
  }
}
if (total([{ price: 0.1, qty: 3 }, { price: 0.2, qty: 1 }]) !== 0.5) {
  console.log('FAIL total: 3 x 0.10 plus 1 x 0.20 should be 0.50');
  process.exit(1);
}
if (count([{ qty: 2 }, { qty: 3 }]) !== 5) {
  console.log('FAIL count: 2 plus 3 should be 5');
  process.exit(1);
}
console.log('ok: cart totals are right');
"#;

/// The fake pane list and the window events. Everything else is the app's own `RealIo`.
struct TestIo {
    panes: Mutex<BTreeMap<String, PaneFacts>>,
    events: Mutex<Vec<(String, Option<String>)>>,
}

impl LiveIo for TestIo {
    fn now_ms(&self) -> u64 {
        RealIo.now_ms()
    }
    fn pane(&self, pane_id: &str) -> Option<PaneFacts> {
        self.panes.lock().unwrap_or_else(|e| e.into_inner()).get(pane_id).cloned()
    }
    fn panes_in_folder(&self, _folder: &Path) -> Vec<(String, PaneFacts)> {
        // Every fake pane of a test works in the test's one folder.
        self.panes.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(id, f)| (id.clone(), f.clone())).collect()
    }
    fn run_open(&self, _pane_id: &str, _pane: &PaneFacts) -> bool {
        false
    }
    fn checkpoints_enabled(&self) -> bool {
        true
    }
    fn git_available(&self) -> bool {
        RealIo.git_available()
    }
    fn check_projects(&self, cwd: &str) -> Vec<String> {
        RealIo.check_projects(cwd)
    }
    fn declared(&self, project: &str) -> Vec<Declared> {
        RealIo.declared(project)
    }
    fn declare(&self, project: &str, list: Vec<Declared>) -> Result<(), String> {
        RealIo.declare(project, list)
    }
    fn suggestions(&self, project: &Path) -> Vec<Suggestion> {
        RealIo.suggestions(project)
    }
    fn preset(&self, cwd: &str) -> WorktreePreset {
        RealIo.preset(cwd)
    }
    fn summaries(&self, project: &Path) -> Vec<TurnSummary> {
        RealIo.summaries(project)
    }
    fn list_all(&self, project: &Path) -> Result<Vec<Checkpoint>, CheckpointError> {
        RealIo.list_all(project)
    }
    fn snapshot_now(&self, project: &Path, key: &str, now_ms: u64) -> Result<String, CheckpointError> {
        RealIo.snapshot_now(project, key, now_ms)
    }
    fn new_copy(&self, project: &Path, copies_root: &Path) -> Result<Box<dyn CopyHandle>, CopyError> {
        RealIo.new_copy(project, copies_root)
    }
    fn run_test(&self, copy: &dyn CopyHandle, argv: &[String], timeout_ms: u64, cancel: &AtomicBool) -> RunResult {
        RealIo.run_test(copy, argv, timeout_ms, cancel)
    }
    fn redact(&self, text: &str) -> String {
        RealIo.redact(text)
    }
    fn emit(&self, job_id: &str, finding_id: Option<&str>) {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push((job_id.to_string(), finding_id.map(str::to_string)));
    }
    fn audit_undo(&self, summary: &str, detail: Value) {
        RealIo.audit_undo(summary, detail)
    }
}

/// One scenario: its folder, its `Live`, and the lock that keeps scenarios apart.
struct Env {
    _lock: MutexGuard<'static, ()>,
    base: PathBuf,
    project: PathBuf,
    io: Arc<TestIo>,
    live: Live,
    store: Store,
}

static LOCK: Mutex<()> = Mutex::new(());
static ROOT: Once = Once::new();

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn node_available() -> bool {
    hidden_command("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn dump<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|e| format!("<unserializable: {e}>"))
}

impl Env {
    /// A fresh folder with the cart project. `markers` are empty files in the first state (`SLOW`, `FLAKY`).
    /// The app data root is one temp folder for the process; each scenario has its own folder under it.
    fn new(name: &str, markers: &[&str]) -> Env {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("pm-wb18-e2e-{}", std::process::id()));
        ROOT.call_once(|| app_paths::set_data_root_for_e2e(root.join("app")));
        // Guard: nothing this test writes may land outside its temp folder.
        assert!(
            app_paths::app_data_dir().starts_with(&root),
            "app data is {}, not under the test folder {}",
            app_paths::app_data_dir().display(),
            root.display()
        );
        let base = root.join(name);
        let project = base.join("project");
        fs::create_dir_all(project.join("notes")).expect("create the project folder");
        fs::write(project.join("cart.mjs"), CART_OK).expect("write cart.mjs");
        fs::write(project.join("check.mjs"), CHECK_MJS).expect("write check.mjs");
        for marker in markers {
            fs::write(project.join(marker), "").expect("write a marker");
        }
        let cwd = project.display().to_string();
        let panes = PANES
            .iter()
            .map(|(id, _, _)| {
                (id.to_string(), PaneFacts { cwd: cwd.clone(), agent_type: "fake".to_string(), status: "idle".to_string() })
            })
            .collect();
        let io = Arc::new(TestIo { panes: Mutex::new(panes), events: Mutex::new(Vec::new()) });
        let store = Store::live();
        let live = Live::new(io.clone(), Store::live());
        Env { _lock: lock, base, project, io, live, store }
    }

    fn project_str(&self) -> String {
        self.project.display().to_string()
    }

    /// Starts a search from pane-codex (the test command must already be declared).
    fn start(&self, limits: Option<LimitsView>, good: Option<String>, continue_from: Option<String>) -> JobView {
        self.live.start("pane-codex", 0, good, limits, continue_from).expect("the search starts")
    }

    fn finding(&self, job: &JobView) -> Finding {
        let id = job.finding_id.clone().unwrap_or_else(|| panic!("the job ended without a finding: {}", dump(job)));
        self.store.get(&id).unwrap_or_else(|| panic!("finding {id} was not saved"))
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.project.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }

    fn copies_root(&self) -> PathBuf {
        self.store.copies_root()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // Best effort: the shadow repository's read-only objects may stay behind on Windows.
        if std::thread::panicking() {
            return;
        }
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Declares the test and waits for the search to end. Returns the job view when it is done.
fn declare_and_search(env: &Env) -> JobView {
    env.live.declare("pane-codex", "Cart tests", "node check.mjs", 1).expect("the test command is declared");
    let job = env.start(None, None, None);
    run_job(&env.live, &job)
}

/// Polls the job until it is done (a bounded wait, so a stuck job fails the test instead of hanging it).
fn run_job(live: &Live, job: &JobView) -> JobView {
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        let view = live.status(&job.id).expect("the job is known");
        if view.state == JobState::Done {
            return view;
        }
        assert!(Instant::now() < deadline, "the job did not end: {}", dump(&view));
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Writes the history: for each global turn, a before save point, the turn's edits, an after save point and the
/// turn summary. Returns the after save point ids in turn order (index = global turn).
fn build_history(env: &Env, later: Later, bug: bool) -> Vec<String> {
    let base = now_ms() - 3_600_000;
    let mut ids = Vec::new();
    let mut summary_rows: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for g in 0..TURNS {
        let (pane, agent, agent_id) = PANES[(g % 3) as usize];
        let turn = g / 3 + 1;
        let start = base + u64::from(g) * SLOT_MS;
        snap(&env.project, pane, Kind::Before, "The turn starts", start, turn);
        let note = format!("notes/{pane}-{turn}.txt");
        fs::write(env.project.join(&note), format!("Turn {turn} by {agent}.\n")).expect("write the turn's note");
        if bug && g == CULPRIT_G {
            replace_in_cart(env, LINE_OK, LINE_BUG);
        }
        if g == TURNS - 1 {
            match later {
                Later::None => {}
                Later::OtherLines => append_to_cart(env, LABEL_FN),
                Later::SameLine => replace_in_cart(env, LINE_BUG, LINE_SAME_LATER),
            }
        }
        let after = snap(&env.project, pane, Kind::After, "The turn ends", start + TURN_MS, turn);
        ids.push(after);
        let sentence = if bug && g == CULPRIT_G { CULPRIT_SENTENCE.to_string() } else { format!("Wrote a note for turn {turn}.") };
        summary_rows.entry(pane.to_string()).or_default().push(json!({
            "paneId": pane,
            "agent": agent,
            "agentId": agent_id,
            "turn": turn,
            "startedAtMs": start,
            "endedAtMs": start + TURN_MS,
            "durationMs": TURN_MS,
            "files": { "count": 1, "added": 1, "deleted": 0, "paths": [note] },
            "approvals": [],
            "end": { "kind": "idle" },
            "sentence": sentence,
        }));
    }
    let ring = json!({ "version": 1, "panes": summary_rows });
    let dir = env.project.join(".puppet-master");
    fs::create_dir_all(&dir).expect("create the .puppet-master folder");
    fs::write(dir.join("turn-summaries.json"), ring.to_string()).expect("write the turn summaries");
    ids
}

/// A later agent's edit in the middle of the history (the culprit's own lines are not touched by it).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Later {
    None,
    OtherLines,
    SameLine,
}

fn replace_in_cart(env: &Env, from: &str, to: &str) {
    let text = env.read("cart.mjs");
    assert!(text.contains(from), "cart.mjs has no line to change: {from}");
    fs::write(env.project.join("cart.mjs"), text.replace(from, to)).expect("write cart.mjs");
}

fn append_to_cart(env: &Env, extra: &str) {
    let mut text = env.read("cart.mjs");
    text.push_str(extra);
    fs::write(env.project.join("cart.mjs"), text).expect("write cart.mjs");
}

/// One save point of the folder for a pane's turn. Returns the checkpoint id.
fn snap(project: &Path, pane: &str, kind: Kind, note: &str, at_ms: u64, turn: u32) -> String {
    let made = engine::snapshot_numbered(project, &engine::pane_key(pane), kind, Some(note), at_ms, &Limits::default(), Some(turn))
        .unwrap_or_else(|e| panic!("save point for {pane} turn {turn}: {e}"));
    match made {
        Snapshot::Created(cp) | Snapshot::Unchanged(Some(cp)) => cp.id,
        Snapshot::Unchanged(None) => panic!("save point for {pane} turn {turn} was not taken"),
    }
}

/// Every file of the folder (except `.puppet-master/`) with its SHA-256. Equal maps mean equal folders.
fn folder_digest(root: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).expect("read the folder") {
            let path = entry.expect("read an entry").path();
            let rel = path.strip_prefix(root).expect("inside the folder").to_string_lossy().replace('\\', "/");
            if rel == ".puppet-master" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let bytes = fs::read(&path).expect("read a file");
                out.insert(rel, format!("{:x}", Sha256::digest(&bytes)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Nothing is left in the copies folder: the copy of every job is removed on every exit path.
fn assert_no_copies(env: &Env) {
    let left: Vec<String> = match fs::read_dir(env.copies_root()) {
        Ok(entries) => entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    };
    assert!(left.is_empty(), "copies left behind: {left:?}");
}

/// Is a process with this pid running? Only the pid is asked (never an image name).
fn process_alive(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    let out = hidden_command("tasklist").args(["/FI", filter.as_str(), "/NH"]).output().expect("run tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!(" {pid} "))
}

#[test]
fn three_agents_six_turns_name_the_turn_that_broke_the_tests() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let env = Env::new("culprit", &[]);
    build_history(&env, Later::None, true);
    let before = folder_digest(&env.project);

    // The sheet's first view: nothing declared, so no command is guessed, and every turn is a point.
    let overview = env.live.overview("pane-codex").expect("the overview");
    assert!(overview.tests.is_empty(), "a test was guessed: {}", dump(&overview.tests));
    assert_eq!(overview.points.len(), TURNS as usize);
    assert!(overview.can, "the sheet should be ready: {}", dump(&overview));
    let refused = env.live.start("pane-codex", 0, None, None, None).expect_err("no test is declared yet");
    assert_eq!(refused.code, WB_NO_TEST);

    let view = declare_and_search(&env);
    let finding = env.finding(&view);
    match &finding.outcome {
        Outcome::Culprit { suspect, .. } => {
            assert_eq!(suspect.pane_key, "pane-claude");
            assert_eq!(suspect.turn, 4);
            assert_eq!(suspect.agent_label, "Claude Code");
            assert_eq!(suspect.sentence.as_deref(), Some(CULPRIT_SENTENCE));
        }
        other => panic!("expected the culprit turn, got {}", dump(other)),
    }
    assert!(finding.tail.contains("FAIL total"), "the failing output is kept: {}", finding.tail);

    let shown = env.live.finding(&finding.id).expect("the finding view");
    assert_eq!(shown.title, "Turn 4 by Claude Code broke the tests");
    assert!(!dump(&shown).contains(&env.project_str()), "the finding view shows the folder path");
    assert!(env.io.events.lock().unwrap().iter().any(|(job, fid)| *job == view.id && fid.as_deref() == Some(finding.id.as_str())));

    assert_eq!(folder_digest(&env.project), before, "the person's folder changed during the search");
    assert_no_copies(&env);
}

#[test]
fn undo_only_the_turn_keeps_later_lines_and_the_recheck_passes() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let env = Env::new("undo-other-lines", &[]);
    build_history(&env, Later::OtherLines, true);
    let view = declare_and_search(&env);
    let finding = env.finding(&view);
    assert!(matches!(finding.outcome, Outcome::Culprit { .. }), "expected a culprit: {}", dump(&finding.outcome));

    let plan = env.live.undo_plan(&finding.id).expect("the dry run");
    assert!(plan.writes.iter().any(|p| p.ends_with("cart.mjs")), "the plan writes cart.mjs: {}", dump(&plan));
    assert!(plan.removes.iter().any(|p| p.ends_with("pane-claude-4.txt")), "the turn's own note is removed: {}", dump(&plan));
    assert!(plan.conflicts.is_empty(), "no conflict expected: {}", dump(&plan.conflicts));
    assert!(plan.blocked_by.is_none());

    let report = env.live.undo(&finding.id).expect("the undo");
    assert!(report.written >= 1 && report.removed >= 1, "nothing was written: {}", dump(&report));
    let cart = env.read("cart.mjs");
    assert!(cart.contains(LINE_OK), "the culprit's line is put back");
    assert!(!cart.contains(LINE_BUG), "the culprit's line is still there");
    assert!(cart.contains("export function label"), "a later turn's lines were lost");
    assert!(!env.project.join("notes/pane-claude-4.txt").exists(), "the turn's own note is still there");
    assert!(env.project.join("notes/pane-codex-1.txt").exists(), "another turn's note was removed");

    let all = engine::list_all(&env.project).expect("the save points");
    assert!(all.iter().any(|c| c.kind == "undo"), "no safety save point was taken before the undo");
    assert!(env.store.get(&finding.id).expect("the finding").undone_ms.is_some(), "the finding is not marked undone");

    let recheck = env.live.recheck(&finding.id).expect("check it again");
    let rechecked = env.finding(&run_job(&env.live, &recheck));
    assert!(matches!(rechecked.outcome, Outcome::TestsPass), "the tests should pass now: {}", dump(&rechecked.outcome));
    assert_no_copies(&env);
}

#[test]
fn undo_refuses_when_a_later_turn_changed_the_same_line() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let env = Env::new("undo-same-line", &[]);
    build_history(&env, Later::SameLine, true);
    let view = declare_and_search(&env);
    let finding = env.finding(&view);
    assert!(matches!(finding.outcome, Outcome::Culprit { .. }), "expected a culprit: {}", dump(&finding.outcome));

    let plan = env.live.undo_plan(&finding.id).expect("the dry run");
    assert!(plan.conflicts.iter().any(|c| c.path.ends_with("cart.mjs")), "the conflict is not named: {}", dump(&plan.conflicts));

    let before = folder_digest(&env.project);
    let refused = env.live.undo(&finding.id).expect_err("a later turn touched the same lines");
    assert_eq!(refused.code, WB_UNDO_CONFLICT);
    assert!(refused.message.contains("cart.mjs"), "the refusal does not name the file: {}", refused.message);
    assert_eq!(folder_digest(&env.project), before, "a refused undo wrote something");
    let all = engine::list_all(&env.project).expect("the save points");
    assert!(all.iter().all(|c| c.kind != "undo"), "a safety save point was taken for a refused undo");
    assert_no_copies(&env);
}

#[test]
fn a_flaky_test_is_not_sure_not_a_culprit() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let env = Env::new("flaky", &["FLAKY"]);
    build_history(&env, Later::None, false);
    let view = declare_and_search(&env);
    let finding = env.finding(&view);
    assert!(
        matches!(finding.outcome, Outcome::NotSure { why: UnsureWhy::Flaky, .. }),
        "expected not sure (flaky), got {}",
        dump(&finding.outcome)
    );
    assert_no_copies(&env);
}

#[test]
fn a_run_cap_gives_a_narrowed_range_and_keep_looking_finds_the_turn() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let env = Env::new("run-cap", &[]);
    let ids = build_history(&env, Later::None, true);
    env.live.declare("pane-codex", "Cart tests", "node check.mjs", 1).expect("declare");
    // Four runs: the newest point fails twice (a failure is run again), the picked good point passes once, and
    // a probe that could need a confirm is not started with fewer runs left than it could need.
    let limits = LimitsView { max_runs: 4, max_minutes: 20 };
    let picked = format!("pane-claude:{}", ids[LAST_GOOD_G]);
    let first = env.start(Some(limits), Some(picked), None);
    let view = run_job(&env.live, &first);
    let finding = env.finding(&view);
    let Outcome::NotSure { why, narrowed: Some(narrowed) } = &finding.outcome else {
        panic!("expected not sure with a range, got {}", dump(&finding.outcome));
    };
    assert!(matches!(why, UnsureWhy::OutOfRuns), "expected the run cap, got {}", dump(why));
    assert!(narrowed.turns >= 2, "the range is too small: {}", narrowed.turns);
    assert!(view.run <= 4, "more runs than the cap: {}", view.run);

    // "Keep looking" continues from the saved answers, so it finds the turn with the rest of the budget.
    let more = env.start(None, None, Some(finding.id.clone()));
    let found = env.finding(&run_job(&env.live, &more));
    match &found.outcome {
        Outcome::Culprit { suspect, .. } => {
            assert_eq!(suspect.pane_key, "pane-claude");
            assert_eq!(suspect.turn, 4);
        }
        other => panic!("keep looking should find the culprit, got {}", dump(other)),
    }
    assert_no_copies(&env);
}

#[test]
fn stop_ends_a_long_run_in_under_three_seconds_and_kills_only_its_own_process() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let env = Env::new("stop", &["SLOW"]);
    build_history(&env, Later::None, false);
    env.live.declare("pane-codex", "Cart tests", "node check.mjs", 1).expect("declare");
    let job = env.start(None, None, None);

    // The check writes its pid into its copy once it starts; wait for that file.
    let deadline = Instant::now() + Duration::from_secs(60);
    let pid: u32 = loop {
        let found = fs::read_dir(env.copies_root())
            .ok()
            .and_then(|entries| entries.filter_map(Result::ok).map(|e| e.path().join("pid.txt")).find(|p| p.is_file()));
        if let Some(path) = found {
            if let Some(pid) = fs::read_to_string(&path).ok().and_then(|text| text.trim().parse::<u32>().ok()) {
                break pid;
            }
        }
        assert!(Instant::now() < deadline, "the check never started");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(process_alive(pid), "the check should be running");

    let asked = Instant::now();
    env.live.stop(&job.id).expect("stop");
    let view = run_job(&env.live, &job);
    let took = asked.elapsed();
    assert!(took < Duration::from_secs(3), "stop took {took:?}");

    let finding = env.finding(&view);
    assert!(
        matches!(finding.outcome, Outcome::NotSure { why: UnsureWhy::Stopped, .. }),
        "expected stopped, got {}",
        dump(&finding.outcome)
    );
    assert!(!process_alive(pid), "the check process is still running after Stop");
    assert_no_copies(&env);
}
