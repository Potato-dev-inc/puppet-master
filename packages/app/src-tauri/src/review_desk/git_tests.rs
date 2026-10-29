//! Review Desk: real-git tests (SPEC n33 section 5, RD-12). Temp git repositories, the real `git` CLI, real worktrees
//! and real checkpoints. No app, no window, no Tauri.
//!
//! - A copy with a changed test file and a `.env` is tier 2 (the secret file) then tier 4 (the test change).
//! - A clean copy is kept: merged into the base branch, its folder gone.
//! - A second copy is thrown away: its branch and folder gone.
//! - In one folder, two checkpointed turns of one pane are thrown away back to before turn 1, and a "Before undo"
//!   save point exists.
//! - Another pane's later turn in the same folder refuses the throw; nothing is restored.
//!
//! `RealIo` is the live layer's shape over the real modules (`worktrees::service`, a checkpoint `Runtime`, the
//! redaction and the line scan). Night Shift, issues, missions and the turn tracker are not under test, so they are
//! empty. The audit log is kept in memory. Every temp folder is removed by a guard.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde_json::Value;

use super::act;
use super::gather::{gather_items, gather_one};
use super::live::{changes_of, clean_line, redact_patch, scan_patch, CopyRead, DeskIo, OpRead};
use super::model::{Clean, Flag, ItemKey, Source, Throw, PATCH_SCAN_MAX};
use super::sources::copies::MissionFacts;
use super::sources::folder::PaneFacts;
use crate::checkpoints::{test_snapshot, PaneFacts as RuntimePane, Runtime, TestEnv, TestKind, TestLimits, TestSettings};
use crate::issue_intake::model::IssueRun;
use crate::issue_intake::pushcheck::scan_added;
use crate::night_shift::model::NightRun;
use crate::night_shift::review::KeepView as NightKeepView;
use crate::operations::OperationError;
use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use crate::redact::rules::{find_spans, Mode, RedactContext};
use crate::turn_summary::model::{EndState, FileStats, TurnSummary};
use crate::worktrees::service::{self, ListItem};
use crate::worktrees::{ChangeSummary, MergeOutcome};

const HOME: &str = r"C:\Users\you";

// ------------------------------------------------------------------------------------------ helpers

/// Removes a temp folder on drop. Git marks objects read-only, so those bits are cleared first.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        clear_readonly(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn clear_readonly(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&path, perms);
            if meta.is_dir() {
                clear_readonly(&path);
            }
        }
    }
}

fn temp_dir(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("pm-rd12-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

/// The real `git` CLI, hidden like every helper process.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = crate::proc_util::hidden_command("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

/// A repo on `main` with `a.txt`, `b.txt` and `src/cart.test.ts` committed. Returns the guard and the path.
fn project(tag: &str) -> (Dir, String) {
    let dir = temp_dir(tag);
    let root = dir.0.clone();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    write(&root, "a.txt", "alpha\n");
    write(&root, "b.txt", "bravo\n");
    write(&root, "src/cart.test.ts", "test('cart adds up', () => {});\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    let path = root.to_string_lossy().into_owned();
    (dir, path)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn test_ctx() -> RedactContext {
    RedactContext { env_values: Vec::new(), home: Some(HOME.to_string()) }
}

/// The same secret and home-folder scan the live layer runs on a patch (`LiveIo::scan_added`).
fn scan_lines(diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
    let ctx = test_ctx();
    let hit = |line: &str| !find_spans(line, &ctx, Mode::Secrets).is_empty();
    scan_added(diff, &hit, Some(HOME))
}

fn turn(pane: &str, number: u32, started: u64, ended: u64, paths: &[&str], sentence: &str) -> (TurnSummary, Option<PathBuf>) {
    let summary = TurnSummary {
        pane_id: pane.to_string(),
        agent: "Demo agent".to_string(),
        agent_id: None,
        account: None,
        turn: number,
        started_at_ms: started,
        ended_at_ms: ended,
        duration_ms: ended - started,
        files: Some(FileStats {
            count: paths.len() as u32,
            added: paths.len() as u32,
            deleted: 0,
            paths: paths.iter().map(|path| path.to_string()).collect(),
        }),
        approvals: Vec::new(),
        end: EndState { kind: "idle".to_string(), prompt: None },
        receipt: None,
        sentence: sentence.to_string(),
    };
    (summary, None)
}

fn pane(id: &str, cwd: &str) -> PaneFacts {
    PaneFacts { id: id.to_string(), cwd: PathBuf::from(cwd), title: String::new(), status: PaneStatus::Idle, parent: None, alive: true }
}

/// Takes a save point of `root` for `pane` at the given time (the test sets the clock of each turn edge).
fn snap(root: &str, pane: &str, kind: TestKind, at_ms: u64) {
    test_snapshot(Path::new(root), pane, kind, None, at_ms, &TestLimits::default()).expect("save point taken");
}

// ------------------------------------------------------------------------------------------ the io

/// The checkpoint runtime's environment: every pane works in `cwd`.
struct TestRuntimeEnv {
    cwd: String,
}

impl TestEnv for TestRuntimeEnv {
    fn pane(&self, _pane_id: &str) -> Option<RuntimePane> {
        Some(RuntimePane { cwd: self.cwd.clone(), agent_type: "demo".to_string(), status: "idle".to_string() })
    }
    fn settings(&self) -> TestSettings {
        TestSettings { enabled: true, keep: 50 }
    }
    fn now_ms(&self) -> u64 {
        now_ms()
    }
    fn agent_takes_part(&self, _agent_type: &str) -> bool {
        true
    }
}

/// The live layer's shape over the real modules (see the module note).
struct RealIo {
    registry: Arc<Mutex<PaneRegistry>>,
    project: String,
    marks: PathBuf,
    runtime: Runtime,
    turns: Vec<(TurnSummary, Option<PathBuf>)>,
    panes: Vec<PaneFacts>,
    audits: RefCell<Vec<String>>,
}

impl RealIo {
    fn new(project: &str, marks: &Dir, panes: Vec<PaneFacts>, turns: Vec<(TurnSummary, Option<PathBuf>)>) -> RealIo {
        RealIo {
            registry: Arc::new(Mutex::new(PaneRegistry::new())),
            project: project.to_string(),
            marks: marks.0.clone(),
            runtime: Runtime::new(Arc::new(TestRuntimeEnv { cwd: project.to_string() })),
            turns,
            panes,
            audits: RefCell::new(Vec::new()),
        }
    }

    /// The fingerprint the person would see for `key` right now.
    fn fingerprint(&self, key: &str) -> String {
        let key = ItemKey::parse(key).expect("valid key");
        gather_one(self, &key).unwrap_or_else(|| panic!("{} is not listed", key.as_str())).fingerprint
    }
}

fn not_used() -> OperationError {
    OperationError::new("NOT_IN_THIS_TEST", "Not part of this test.", false)
}

impl DeskIo for RealIo {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
    fn clean(&self) -> Clean {
        Clean { text: Box::new(|raw| clean_line(raw, &test_ctx(), Some(HOME))) }
    }
    fn clean_patch(&self, raw: &str) -> String {
        redact_patch(raw, &test_ctx(), Some(HOME))
    }
    fn marks_root(&self) -> PathBuf {
        self.marks.clone()
    }

    fn night_run(&self) -> Result<Option<NightRun>, String> {
        Ok(None)
    }
    fn issue_runs(&self) -> Result<Vec<IssueRun>, String> {
        Ok(Vec::new())
    }
    fn issue_folder(&self, _repo_id: &str) -> Option<String> {
        None
    }
    fn copies(&self) -> Result<Vec<ListItem>, String> {
        Ok(service::list(Some(&self.project), &self.registry))
    }
    fn keep_target(&self, _project: &str) -> Option<String> {
        None
    }
    fn label_of(&self, _agent_type: &str) -> String {
        "Demo agent".to_string()
    }

    fn copy_read(&self, project: &str, handle: &str, _summary: Option<ChangeSummary>) -> CopyRead {
        match service::diff(Some(project), handle, None, &self.registry) {
            Ok(view) => CopyRead {
                scan: Some(scan_patch(&view.patch, view.patch_truncated, &scan_lines)),
                changes: Some(view.changes),
            },
            Err(_) => CopyRead::default(),
        }
    }
    fn copy_patch(&self, project: &str, handle: &str) -> Option<(String, bool)> {
        let view = service::diff(Some(project), handle, None, &self.registry).ok()?;
        Some((view.patch, view.patch_truncated))
    }
    fn latest_op(&self, _project: &str, _handle: &str) -> Option<OpRead> {
        None
    }
    fn mission(&self, _project: &str, _mission_id: &str) -> Option<MissionFacts> {
        None
    }
    fn mission_headline(&self, _project: &str, _mission_id: &str) -> Option<String> {
        None
    }
    fn turns(&self, since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)> {
        self.turns.iter().filter(|(turn, _)| turn.ended_at_ms >= since_ms).cloned().collect()
    }
    fn panes(&self) -> Vec<PaneFacts> {
        self.panes.clone()
    }
    fn checkpoint_before(&self, pane_id: &str, started_at_ms: u64) -> Option<String> {
        self.runtime.before_of_turns(pane_id, started_at_ms)
    }
    fn checkpoint_last_after(&self, pane_id: &str) -> Option<String> {
        self.runtime.last_after_id(pane_id)
    }
    fn range_read(&self, pane_id: &str, from_id: &str, to_id: &str) -> CopyRead {
        let Ok(points) = self.runtime.changes_between_ids(pane_id, from_id, to_id) else {
            return CopyRead::default();
        };
        let scan = self
            .runtime
            .patch_between(pane_id, from_id, to_id, PATCH_SCAN_MAX)
            .ok()
            .flatten()
            .map(|patch| scan_patch(&patch.patch, patch.truncated, &scan_lines));
        CopyRead { changes: Some(changes_of(&points)), scan }
    }
    fn range_patch(&self, pane_id: &str, from_id: &str, to_id: &str, max_bytes: usize) -> Option<(String, bool)> {
        let patch = self.runtime.patch_between(pane_id, from_id, to_id, max_bytes).ok().flatten()?;
        Some((patch.patch, patch.truncated))
    }

    fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError> {
        service::merge(Some(project), handle, &self.registry)
    }
    fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError> {
        service::discard(Some(project), handle, &self.registry)
    }
    fn night_keep(&self, _task_id: &str) -> Result<NightKeepView, OperationError> {
        Err(not_used())
    }
    fn night_throw(&self, _task_id: &str) -> Result<(), OperationError> {
        Err(not_used())
    }
    fn issue_act(&self, _run_id: &str, _action: &str) -> Result<(), OperationError> {
        Err(not_used())
    }
    fn restore(&self, pane_id: &str, checkpoint_id: &str) -> Result<(), OperationError> {
        self.runtime
            .restore(pane_id, checkpoint_id)
            .map(|_| ())
            .map_err(|error| OperationError::new("RESTORE_FAILED", error.to_string(), true))
    }
    fn put_draft(&self, _pane_id: &str, _text: String) {
        unreachable!("Ask to fix is not part of these tests");
    }
    fn audit(&self, summary: &str, _detail: Value) {
        self.audits.borrow_mut().push(summary.to_string());
    }
}

// ------------------------------------------------------------------------------------------ copies

#[test]
fn a_copy_with_a_test_change_and_an_env_file_is_tier_two_then_tier_four() {
    let (_guard, root) = project("risky");
    let risky = crate::worktrees::create(&root, "run-risky").expect("copy made");
    write(Path::new(&risky.path), "src/cart.test.ts", "test('cart adds up', () => { expect(2).toBe(2); });\n");
    write(Path::new(&risky.path), ".env", "TOKEN=test-only-value\n");
    write(Path::new(&risky.path), "a.txt", "alpha changed\n");
    let marks = temp_dir("marks-risky");
    let io = RealIo::new(&root, &marks, Vec::new(), Vec::new());

    let items = gather_items(&io).items;
    let item = items.iter().find(|item| item.key.as_str() == "copy:run-risky").expect("the copy is listed");
    assert_eq!(item.source, Source::Copy);
    assert_eq!(item.risk.tier, 2, "a secret file is the riskiest flag here: {:?}", item.risk.reasons);
    let secret = item
        .risk
        .reasons
        .iter()
        .position(|reason| reason.flag == Flag::Sensitive && reason.sentence.contains(".env"))
        .expect("the .env file is a tier 2 reason");
    let tests = item
        .risk
        .reasons
        .iter()
        .position(|reason| reason.flag == Flag::TestsChanged && reason.sentence.contains("src/cart.test.ts"))
        .expect("the test change is a tier 4 reason");
    assert!(secret < tests, "tier 2 is listed before tier 4: {:?}", item.risk.reasons);
    assert_eq!(items[0].key.as_str(), "copy:run-risky", "the riskiest copy is first");
}

#[test]
fn keeping_a_clean_copy_merges_it_into_the_base_branch_and_the_copy_is_gone() {
    let (_guard, root) = project("keep");
    let clean = crate::worktrees::create(&root, "run-clean").expect("copy made");
    write(Path::new(&clean.path), "a.txt", "alpha changed\n");
    let marks = temp_dir("marks-keep");
    let io = RealIo::new(&root, &marks, Vec::new(), Vec::new());
    let head_before = git(Path::new(&root), &["rev-parse", "HEAD"]);

    let fingerprint = io.fingerprint("copy:run-clean");
    let view = act::keep(&io, "copy:run-clean", &fingerprint).expect("kept");

    assert!(view.gone, "the row left the desk");
    assert!(view.sentence.starts_with("Merged 1 file into main"), "{}", view.sentence);
    assert_eq!(read(Path::new(&root), "a.txt").as_deref(), Some("alpha changed\n"), "the change is on main");
    assert_ne!(git(Path::new(&root), &["rev-parse", "HEAD"]), head_before, "main moved");
    assert!(!Path::new(&clean.path).exists(), "the copy folder is gone");
    assert!(io.audits.borrow().iter().any(|line| line.contains("Kept")), "the keep is audited");
}

#[test]
fn throwing_away_a_copy_removes_its_branch_and_folder() {
    let (_guard, root) = project("throw");
    let thrown = crate::worktrees::create(&root, "run-throw").expect("copy made");
    write(Path::new(&thrown.path), "b.txt", "bravo changed\n");
    let marks = temp_dir("marks-throw");
    let io = RealIo::new(&root, &marks, Vec::new(), Vec::new());
    assert!(git(Path::new(&root), &["branch", "--list", "pm/run-throw"]).contains("pm/run-throw"), "the copy has its branch");

    let fingerprint = io.fingerprint("copy:run-throw");
    let view = act::throw_away(&io, "copy:run-throw", &fingerprint).expect("thrown away");

    assert!(view.gone);
    assert!(!Path::new(&thrown.path).exists(), "the copy folder is gone");
    assert!(git(Path::new(&root), &["branch", "--list", "pm/run-throw"]).is_empty(), "the copy's branch is gone");
    assert_eq!(read(Path::new(&root), "b.txt").as_deref(), Some("bravo\n"), "main was not touched");
}

// ------------------------------------------------------------------------------------------ folder

#[test]
fn two_checkpointed_turns_in_one_folder_are_thrown_away_back_to_before_turn_one() {
    let (_guard, root) = project("folder");
    let pane_id = "pane-f";
    let t0 = now_ms() - 600_000;
    // Turn 1: a.txt changes, n.txt is new.
    snap(&root, pane_id, TestKind::Before, t0);
    write(Path::new(&root), "a.txt", "beta\n");
    write(Path::new(&root), "n.txt", "new\n");
    snap(&root, pane_id, TestKind::After, t0 + 5_000);
    // Turn 2: a.txt changes again, b.txt is removed.
    snap(&root, pane_id, TestKind::Before, t0 + 10_000);
    write(Path::new(&root), "a.txt", "gamma\n");
    std::fs::remove_file(Path::new(&root).join("b.txt")).unwrap();
    snap(&root, pane_id, TestKind::After, t0 + 15_000);
    // A hand edit after the last turn, not in any save point yet. When nothing differs from the newest save point,
    // Undo reuses that point and writes no "Before undo" one, so this edit is what makes the safety point needed.
    write(Path::new(&root), "c.txt", "typed by hand\n");

    let turns = vec![
        turn(pane_id, 1, t0, t0 + 5_000, &["a.txt", "n.txt"], "Changed two files"),
        turn(pane_id, 2, t0 + 10_000, t0 + 15_000, &["a.txt", "b.txt"], "Changed a file and removed one"),
    ];
    let marks = temp_dir("marks-folder");
    let io = RealIo::new(&root, &marks, vec![pane(pane_id, &root)], turns);
    let key = ItemKey::parse("folder:pane-f:1-2").unwrap();
    let item = gather_one(&io, &key).expect("the folder work is listed");
    assert_eq!(item.source, Source::Folder);
    assert_eq!(item.facts.files_total, 3, "the exact changes between the save points: {:?}", item.facts.files);
    assert!(matches!(item.throw, Throw::Restore { .. }), "{:?}", item.throw);

    let view = act::throw_away(&io, key.as_str(), &item.fingerprint).expect("thrown away");

    assert!(view.gone, "{}", view.sentence);
    let folder = Path::new(&root);
    assert_eq!(read(folder, "a.txt").as_deref(), Some("alpha\n"), "back to before turn 1");
    assert_eq!(read(folder, "b.txt").as_deref(), Some("bravo\n"), "the removed file is back");
    assert!(read(folder, "n.txt").is_none(), "the file made in turn 1 is gone");
    assert!(read(folder, "c.txt").is_none(), "the hand edit is gone from the folder");
    let saved = io.runtime.list(pane_id).expect("save points listed");
    let undo = saved.iter().find(|point| point.kind == "undo" && point.label == "Before undo");
    let undo = undo.unwrap_or_else(|| panic!("a Before undo save point exists: {:?}", saved.iter().map(|p| (&p.kind, &p.label)).collect::<Vec<_>>()));
    assert!(undo.files >= 1, "the Before undo point keeps the hand edit: {undo:?}");
    assert!(gather_one(&io, &key).is_none(), "the thrown work is marked and no longer listed");
}

#[test]
fn another_panes_later_turn_in_the_same_folder_refuses_throw_away() {
    let (_guard, root) = project("shared");
    let t0 = now_ms() - 600_000;
    snap(&root, "pane-a", TestKind::Before, t0);
    write(Path::new(&root), "a.txt", "beta\n");
    snap(&root, "pane-a", TestKind::After, t0 + 5_000);
    // Pane B changes the same folder later.
    write(Path::new(&root), "b.txt", "other\n");
    let turns = vec![
        turn("pane-a", 1, t0, t0 + 5_000, &["a.txt"], "Changed a file"),
        turn("pane-b", 1, t0 + 20_000, t0 + 25_000, &["b.txt"], "Changed another file"),
    ];
    let marks = temp_dir("marks-shared");
    let io = RealIo::new(&root, &marks, vec![pane("pane-a", &root), pane("pane-b", &root)], turns);
    let key = ItemKey::parse("folder:pane-a:1-1").unwrap();
    let item = gather_one(&io, &key).expect("pane A's work is listed");
    match &item.throw {
        Throw::No { why } => assert_eq!(why, "Another agent changed this folder after this. Use History to undo."),
        other => panic!("throw away must be refused: {other:?}"),
    }

    let refused = act::throw_away(&io, key.as_str(), &item.fingerprint);

    match refused {
        Err(error) => assert_eq!(error.code, act::CANNOT_THROW),
        Ok(view) => panic!("throw away must be refused, got: {}", view.sentence),
    }
    assert_eq!(read(Path::new(&root), "a.txt").as_deref(), Some("beta\n"), "nothing was restored");
    assert_eq!(read(Path::new(&root), "b.txt").as_deref(), Some("other\n"), "pane B's file is untouched");
}
