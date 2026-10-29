//! Review Desk: end-to-end tests of `gather` and `act` on a fake [`DeskIo`] (no git, no registry, no window).
//! The fake records every action in `calls`, so a test can say "nothing was called" or "exactly this was called".

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

use super::act;
use super::gather::{gather, gather_items, gather_one, PANE_CLOSED};
use super::live::{redact_patch, CopyRead, DeskIo, OpRead};
use super::model::{
    Clean, How, ItemKey, Keep, Marks, Source, Throw, DETAIL_PATCH_MAX, ITEMS_CAP, WINDOW_MS,
};
use super::sources::copies::{MissionFacts, OpFacts, ScanFacts};
use super::sources::folder::PaneFacts;
use crate::issue_intake::model::{AgentPick, CleanNotes, IssueRun, IssueSnapshot, PushFacts, RunState};
use crate::night_shift::model::{Review, NightRun};
use crate::night_shift::review::KeepView as NightKeepView;
use crate::operations::OperationError;
use crate::pty::status::PaneStatus;
use crate::redact::rules::RedactContext;
use crate::turn_summary::model::{EndState, FileStats, TurnSummary};
use crate::worktrees::service::ListItem;
use crate::worktrees::{ChangeSummary, Changes, FileChange, MergeOutcome, WorktreeEntry, WorktreeRef};

const HOME: &str = r"C:\Users\you";
const NOW: u64 = 10_000_000;
const REPO: &str = "/work/app";

static COUNTER: AtomicU32 = AtomicU32::new(0);

// ------------------------------------------------------------------------------------------------ the fake

struct Fake {
    now: Cell<u64>,
    night: RefCell<Result<Option<NightRun>, String>>,
    issues: RefCell<Result<Vec<IssueRun>, String>>,
    copies: RefCell<Result<Vec<ListItem>, String>>,
    reads: RefCell<HashMap<String, CopyRead>>,
    range_reads: RefCell<HashMap<String, CopyRead>>,
    ops: RefCell<HashMap<String, OpRead>>,
    turns: RefCell<Vec<(TurnSummary, Option<PathBuf>)>>,
    panes: RefCell<Vec<PaneFacts>>,
    before: RefCell<HashMap<String, String>>,
    after: RefCell<HashMap<String, String>>,
    patches: RefCell<HashMap<String, String>>,
    keep_target: RefCell<Option<String>>,
    marks_dir: PathBuf,
    calls: RefCell<Vec<String>>,
    drafts: RefCell<Vec<(String, String)>>,
    audits: RefCell<Vec<(String, Value)>>,
    reenter: RefCell<Option<(String, String)>>,
    reenter_result: RefCell<Option<Result<super::view::ActView, OperationError>>>,
}

impl Fake {
    fn new() -> Fake {
        let dir = std::env::temp_dir().join(format!(
            "pm-rd9-engine-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        Fake {
            now: Cell::new(NOW),
            night: RefCell::new(Ok(None)),
            issues: RefCell::new(Ok(Vec::new())),
            copies: RefCell::new(Ok(Vec::new())),
            reads: RefCell::new(HashMap::new()),
            range_reads: RefCell::new(HashMap::new()),
            ops: RefCell::new(HashMap::new()),
            turns: RefCell::new(Vec::new()),
            panes: RefCell::new(vec![pane("pane-1", REPO)]),
            before: RefCell::new(HashMap::new()),
            after: RefCell::new(HashMap::new()),
            patches: RefCell::new(HashMap::new()),
            keep_target: RefCell::new(None),
            marks_dir: dir,
            calls: RefCell::new(Vec::new()),
            drafts: RefCell::new(Vec::new()),
            audits: RefCell::new(Vec::new()),
            reenter: RefCell::new(None),
            reenter_result: RefCell::new(None),
        }
    }

    fn add_copy(&self, handle: &str, pane: Option<&str>, read: CopyRead, task: &str) {
        let summary = read.changes.as_ref().map(Changes::summary);
        self.copies.borrow_mut().as_mut().unwrap().push(list_item(handle, pane, summary));
        self.reads.borrow_mut().insert(handle.to_string(), read);
        self.ops.borrow_mut().insert(handle.to_string(), op(task));
    }

    fn set_read(&self, handle: &str, read: CopyRead) {
        let summary = read.changes.as_ref().map(Changes::summary);
        for copy in self.copies.borrow_mut().as_mut().unwrap().iter_mut() {
            if copy.entry.worktree.handle == handle {
                copy.summary = summary;
            }
        }
        self.reads.borrow_mut().insert(handle.to_string(), read);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn key(&self, raw: &str) -> (ItemKey, String) {
        let key = ItemKey::parse(raw).expect("valid key");
        let item = gather_one(self, &key).unwrap_or_else(|| panic!("{raw} is not listed"));
        (key, item.fingerprint)
    }

    fn remove_copy(&self, handle: &str) {
        self.copies.borrow_mut().as_mut().unwrap().retain(|c| c.entry.worktree.handle != handle);
    }

    fn mark_review(&self, task_id: &str, review: Review) {
        if let Ok(Some(run)) = &mut *self.night.borrow_mut() {
            for task in &mut run.tasks {
                if task.spec.id == task_id {
                    task.review = review.clone();
                }
            }
        }
    }

    fn set_issue_state(&self, run_id: &str, state: RunState) {
        if let Ok(runs) = &mut *self.issues.borrow_mut() {
            for run in runs {
                if run.id == run_id {
                    run.state = state.clone();
                }
            }
        }
    }
}

impl DeskIo for Fake {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }

    fn clean(&self) -> Clean {
        Clean { text: Box::new(|raw| raw.replace(HOME, "~").replace("rounds", "[r]")) }
    }

    fn clean_patch(&self, raw: &str) -> String {
        redact_patch(raw, &RedactContext { env_values: Vec::new(), home: Some(HOME.to_string()) }, Some(HOME))
    }

    fn marks_root(&self) -> PathBuf {
        self.marks_dir.clone()
    }

    fn night_run(&self) -> Result<Option<NightRun>, String> {
        self.night.borrow().clone()
    }

    fn issue_runs(&self) -> Result<Vec<IssueRun>, String> {
        self.issues.borrow().clone()
    }

    fn issue_folder(&self, repo_id: &str) -> Option<String> {
        (repo_id == "rw-0123abcd").then(|| REPO.to_string())
    }

    fn copies(&self) -> Result<Vec<ListItem>, String> {
        self.copies.borrow().clone()
    }

    fn keep_target(&self, _project: &str) -> Option<String> {
        self.keep_target.borrow().clone()
    }

    fn label_of(&self, _agent_type: &str) -> String {
        "Fixture agent".to_string()
    }

    fn copy_read(&self, _project: &str, handle: &str, _summary: Option<ChangeSummary>) -> CopyRead {
        self.reads.borrow().get(handle).cloned().unwrap_or_default()
    }

    fn copy_patch(&self, _project: &str, handle: &str) -> Option<(String, bool)> {
        self.patches.borrow().get(handle).map(|patch| (patch.clone(), false))
    }

    fn latest_op(&self, _project: &str, handle: &str) -> Option<OpRead> {
        self.ops.borrow().get(handle).cloned()
    }

    fn mission(&self, _project: &str, _mission_id: &str) -> Option<MissionFacts> {
        None
    }

    fn mission_headline(&self, _project: &str, _mission_id: &str) -> Option<String> {
        None
    }

    fn turns(&self, since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)> {
        self.turns.borrow().iter().filter(|(turn, _)| turn.ended_at_ms >= since_ms).cloned().collect()
    }

    fn panes(&self) -> Vec<PaneFacts> {
        self.panes.borrow().clone()
    }

    fn checkpoint_before(&self, pane_id: &str, _started_at_ms: u64) -> Option<String> {
        self.before.borrow().get(pane_id).cloned()
    }

    fn checkpoint_last_after(&self, pane_id: &str) -> Option<String> {
        self.after.borrow().get(pane_id).cloned()
    }

    fn range_read(&self, pane_id: &str, _from: &str, _to: &str) -> CopyRead {
        self.range_reads.borrow().get(pane_id).cloned().unwrap_or_default()
    }

    fn range_patch(&self, pane_id: &str, _from: &str, _to: &str, _max: usize) -> Option<(String, bool)> {
        self.patches.borrow().get(pane_id).map(|patch| (patch.clone(), false))
    }

    fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError> {
        self.calls.borrow_mut().push(format!("merge:{project}:{handle}"));
        let again = self.reenter.borrow_mut().take();
        if let Some((key, fingerprint)) = again {
            *self.reenter_result.borrow_mut() = Some(act::keep(self, &key, &fingerprint));
        }
        self.remove_copy(handle);
        Ok(MergeOutcome {
            merged: true,
            reason: None,
            merge_commit: Some("abc1234".to_string()),
            files: vec!["src/a.ts".to_string(), "src/b.ts".to_string()],
            branch: format!("pm/{handle}"),
            into: "main".to_string(),
        })
    }

    fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError> {
        self.calls.borrow_mut().push(format!("discard:{project}:{handle}"));
        self.remove_copy(handle);
        Ok(())
    }

    fn night_keep(&self, task_id: &str) -> Result<NightKeepView, OperationError> {
        self.calls.borrow_mut().push(format!("night_keep:{task_id}"));
        self.mark_review(task_id, Review::Kept { at_ms: 1, into: "main".into() });
        Ok(NightKeepView {
            branch: "ns/x".to_string(),
            into: "main".to_string(),
            files: vec!["a".to_string()],
            merged: true,
            reason: None,
        })
    }

    fn night_throw(&self, task_id: &str) -> Result<(), OperationError> {
        self.calls.borrow_mut().push(format!("night_throw:{task_id}"));
        self.mark_review(task_id, Review::Discarded { at_ms: 1 });
        Ok(())
    }

    fn issue_act(&self, run_id: &str, action: &str) -> Result<(), OperationError> {
        self.calls.borrow_mut().push(format!("issue:{run_id}:{action}"));
        self.set_issue_state(run_id, RunState::ThrownAway);
        Ok(())
    }

    fn restore(&self, pane_id: &str, checkpoint_id: &str) -> Result<(), OperationError> {
        self.calls.borrow_mut().push(format!("restore:{pane_id}:{checkpoint_id}"));
        Ok(())
    }

    fn put_draft(&self, pane_id: &str, text: String) {
        self.calls.borrow_mut().push(format!("put_draft:{pane_id}"));
        self.drafts.borrow_mut().push((pane_id.to_string(), text));
    }

    fn audit(&self, summary: &str, detail: Value) {
        self.audits.borrow_mut().push((summary.to_string(), detail));
    }
}

// ------------------------------------------------------------------------------------------------ fixtures

fn pane(id: &str, cwd: &str) -> PaneFacts {
    PaneFacts {
        id: id.to_string(),
        cwd: PathBuf::from(cwd),
        title: format!("Title {id}"),
        status: PaneStatus::Idle,
        parent: None,
        alive: true,
    }
}

fn list_item(handle: &str, pane_id: Option<&str>, summary: Option<ChangeSummary>) -> ListItem {
    ListItem {
        entry: WorktreeEntry {
            worktree: WorktreeRef {
                path: format!("/work/.puppet-master/worktrees/{handle}"),
                cwd: format!("/work/.puppet-master/worktrees/{handle}"),
                branch: format!("pm/{handle}"),
                base: "main".to_string(),
                base_commit: "abc1234".to_string(),
                repo: REPO.to_string(),
                handle: handle.to_string(),
                created_at_ms: 1_000,
                closed: None,
                changes: None,
                setup: None,
                pr: None,
            },
            exists: true,
        },
        state: "unreviewed".to_string(),
        agent_type: Some("fixture-agent".to_string()),
        pane_id: pane_id.map(str::to_string),
        task: Some("Item task text".to_string()),
        summary,
    }
}

fn op(task: &str) -> OpRead {
    OpRead {
        turns: 2,
        facts: OpFacts {
            status: "completed".to_string(),
            task: task.to_string(),
            finished_at_ms: 5_000,
            check_results: vec![("unit".to_string(), true), ("lint".to_string(), true)],
            account: None,
        },
    }
}

/// Changed files (one line added each) and an optional secret-looking line.
fn read(paths: &[&str], secret: Option<(&str, u32)>, home: Option<(&str, u32)>) -> CopyRead {
    let files: Vec<FileChange> = paths
        .iter()
        .map(|path| FileChange {
            path: path.to_string(),
            status: "modified".to_string(),
            additions: 1,
            deletions: 0,
            binary: false,
        })
        .collect();
    CopyRead {
        changes: Some(Changes {
            files_changed: files.len(),
            additions: files.len() as u32,
            deletions: 0,
            files,
            truncated: false,
        }),
        scan: Some(ScanFacts {
            secret_lines: secret.map(|(p, n)| (p.to_string(), n)).into_iter().collect(),
            home_lines: home.map(|(p, n)| (p.to_string(), n)).into_iter().collect(),
            scanned: true,
        }),
    }
}

fn turn(pane_id: &str, n: u32, started: u64, ended: u64, paths: &[&str]) -> TurnSummary {
    TurnSummary {
        pane_id: pane_id.to_string(),
        agent: "Fixture agent".to_string(),
        agent_id: None,
        account: None,
        turn: n,
        started_at_ms: started,
        ended_at_ms: ended,
        duration_ms: ended - started,
        files: Some(FileStats {
            count: paths.len() as u32,
            added: 2 * paths.len() as u32,
            deleted: 0,
            paths: paths.iter().map(|p| p.to_string()).collect(),
        }),
        approvals: Vec::new(),
        end: EndState { kind: "idle".to_string(), prompt: None },
        receipt: None,
        sentence: format!("Turn {n} changed {} files", paths.len()),
    }
}

fn push_facts() -> PushFacts {
    PushFacts {
        files: 3,
        additions: 12,
        deletions: 3,
        ci_files: Vec::new(),
        secret_files: Vec::new(),
        secret_lines: Vec::new(),
        home_path_lines: Vec::new(),
        internal_files: Vec::new(),
        git_changed: false,
        remote_branch_exists: false,
        issue_changed: false,
        run_status: "completed".into(),
        turns: 4,
        minutes: 30,
    }
}

fn issue_run(n: u64, state: RunState, handle: &str) -> IssueRun {
    IssueRun {
        id: format!("ir-{n:012x}"),
        repo_id: "rw-0123abcd".into(),
        issue: IssueSnapshot {
            owner_repo: "acme/app".into(),
            number: 12,
            url: "https://github.com/acme/app/issues/12".into(),
            title: "Cart total rounds down".into(),
            body: String::new(),
            author: "maintainer".into(),
            author_association: "MEMBER".into(),
            labeled_by: "maintainer".into(),
            labeled_by_bot: false,
            label_event_id: 1,
            labeled_at_ms: 1,
            notes: CleanNotes::default(),
            sha8: "00000000".into(),
        },
        agent: AgentPick { agent_type: "fixture-agent".into(), account: None },
        agent_label: "Fixture agent".into(),
        at_ms: 1_000,
        started_at_ms: Some(2_000),
        forced_ask: Vec::new(),
        approved_by: Some("desktop".into()),
        handle: Some(handle.to_string()),
        branch: None,
        base: None,
        git_fingerprint: None,
        state,
    }
}

fn ask(facts: PushFacts) -> RunState {
    RunState::AskPush { question_id: "q-1".into(), facts }
}

fn held() -> RunState {
    RunState::Held { code: "GIT_HOOKS_CHANGED".into(), message: "The git hooks changed".into() }
}

/// Three plain copies: a secret line, a test file, a clean change. pane-1 is the pane of all three.
fn plain_copies() -> Fake {
    let io = Fake::new();
    io.copies.replace(Ok(Vec::new()));
    io.add_copy("plain-a", Some("pane-1"), read(&["src/a.ts"], Some(("src/a.ts", 3)), None), "Add the secret thing");
    io.add_copy("plain-b", Some("pane-1"), read(&["src/cart.test.ts"], None, None), "Fix the cart total");
    io.add_copy("plain-c", Some("pane-1"), read(&["src/c.ts"], None, None), "Tidy the helper");
    io
}

/// A folder item: pane-1 did one turn that changed two files; save points exist; the scan finds a secret line.
fn folder_fake() -> Fake {
    let io = Fake::new();
    io.turns.borrow_mut().push((turn("pane-1", 1, 1_000, 2_000, &["src/x.ts", "src/y.ts"]), Some(PathBuf::from(REPO))));
    io.before.borrow_mut().insert("pane-1".into(), "cp1".into());
    io.after.borrow_mut().insert("pane-1".into(), "cp3".into());
    io.range_reads.borrow_mut().insert("pane-1".into(), read(&["src/x.ts", "src/y.ts"], Some(("src/x.ts", 7)), None));
    io
}

fn keys(io: &Fake) -> Vec<String> {
    gather_items(io).items.iter().map(|item| item.key.as_str().to_string()).collect()
}

fn code(result: Result<impl std::fmt::Debug, OperationError>) -> String {
    result.expect_err("should be refused").code
}

// ------------------------------------------------------------------------------------------------ gather

#[test]
fn a_night_copy_and_an_issue_copy_never_show_as_plain_copies_and_the_order_is_the_table() {
    let io = plain_copies();
    io.night.replace(Ok(Some(crate::night_shift::view::morning_fixture())));
    // The fixture's two tasks share the copy "run-1".
    io.add_copy("run-1", Some("pane-42"), read(&["src/cart.ts"], None, None), "Night task");
    io.add_copy("issue-12-ab12", None, read(&["src/x.ts"], None, None), "Issue task");
    io.issues.replace(Ok(vec![issue_run(1, ask(push_facts()), "issue-12-ab12")]));

    let listed = keys(&io);
    assert!(!listed.iter().any(|k| k == "copy:run-1" || k == "copy:issue-12-ab12"), "{listed:?}");
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec![
            "copy:plain-a", "copy:plain-b", "copy:plain-c", "issue:ir-000000000001", "night:nt-00000001",
            "night:nt-00000002"
        ]
    );
    // Tier 1 (the night task whose tests changed), tier 2 (a secret line), tier 4 (a test file), then the rest.
    assert_eq!(&listed[..3], ["night:nt-00000002", "copy:plain-a", "copy:plain-b"]);
    let items = gather_items(&io).items;
    assert!(items[3..].iter().all(|item| item.risk.tier == 7), "{:?}", items.iter().map(|i| i.risk.tier).collect::<Vec<_>>());
    assert_eq!(items[3].key.as_str(), "issue:ir-000000000001", "the bigger change comes first inside a tier");
    assert_eq!(io.calls(), Vec::<String>::new(), "listing never acts");
    assert!(io.audits.borrow().is_empty());

    let desk = gather(&io);
    assert_eq!(desk.counts.total, 6);
    assert_eq!((desk.counts.high, desk.counts.check, desk.counts.low), (2, 1, 3));
}

#[test]
fn a_failing_source_adds_one_note_and_the_rest_still_shows() {
    let io = plain_copies();
    io.night.replace(Err("broken".into()));
    let desk = gather(&io);
    assert_eq!(desk.notes, vec!["Could not read Night Shift. Its tasks are not listed.".to_string()]);
    assert_eq!(desk.items.len(), 3);

    io.issues.replace(Err("broken".into()));
    io.copies.replace(Err("broken".into()));
    let desk = gather(&io);
    assert_eq!(desk.notes.len(), 3);
    assert!(desk.notes.contains(&"Could not read your separate copies. They are not listed.".to_string()));
    assert!(desk.notes.contains(&"Could not read Assign an issue. Its runs are not listed.".to_string()));
    assert!(desk.items.is_empty());
}

#[test]
fn night_items_get_the_keep_target_files_scans_and_a_live_pane() {
    let io = Fake::new();
    io.night.replace(Ok(Some(crate::night_shift::view::morning_fixture())));
    io.add_copy(
        "run-1",
        None,
        read(&["src/cart.ts", ".env", ".github/workflows/ci.yml"], Some(("src/cart.ts", 4)), Some(("notes.md", 3))),
        "Night task",
    );
    io.keep_target.replace(Some("release".into()));
    io.panes.borrow_mut().push(pane("pane-42", REPO));

    let items = gather_items(&io).items;
    let ready = items.iter().find(|i| i.key.as_str() == "night:nt-00000001").unwrap();
    assert_eq!(ready.keep, Keep::NightKeep { task_id: "nt-00000001".into(), into: "release".into() });
    assert_eq!(ready.facts.files.as_ref().map(Vec::len), Some(3), "file names come from the copy");
    assert_eq!(ready.facts.files_total, 3);
    assert_eq!(ready.facts.secret_files, vec![".env".to_string()]);
    assert_eq!(ready.facts.ci_files, vec![".github/workflows/ci.yml".to_string()]);
    assert_eq!(ready.facts.secret_lines, vec![("src/cart.ts".to_string(), 4)]);
    assert_eq!(ready.facts.home_lines, vec![("notes.md".to_string(), 3)]);
    assert!(ready.facts.lines_scanned);
    assert_eq!(ready.risk.tier, 2);
    assert!(ready.risk.reasons.iter().all(|r| r.flag != super::model::Flag::Unknown));
    // pane-42 is open, pane-43 is not.
    assert_eq!(ready.pane_id.as_deref(), Some("pane-42"));
    assert_eq!(ready.fix, Ok("pane-42".to_string()));
    let needs = items.iter().find(|i| i.key.as_str() == "night:nt-00000002").unwrap();
    assert_eq!(needs.pane_id, None);
    assert_eq!(needs.fix, Err(PANE_CLOSED.to_string()));

    // Unknown target: the converter's words stay.
    io.keep_target.replace(None);
    let ready = gather_one(&io, &ItemKey::parse("night:nt-00000001").unwrap()).unwrap();
    assert_eq!(ready.keep, Keep::NightKeep { task_id: "nt-00000001".into(), into: "your current branch".into() });
}

#[test]
fn issue_items_get_their_folder_file_names_pane_and_a_live_clean_title() {
    let io = Fake::new();
    io.add_copy("issue-12-ab12", Some("pane-1"), read(&["src/x.ts"], None, None), "Issue task");
    io.add_copy("issue-13-ef56", None, read(&["src/y.ts", ".env"], Some(("src/y.ts", 2)), None), "Held task");
    let mut second = issue_run(2, held(), "issue-13-ef56");
    second.issue.number = 13;
    io.issues.replace(Ok(vec![issue_run(1, ask(push_facts()), "issue-12-ab12"), second]));

    let items = gather_items(&io).items;
    let pushed = items.iter().find(|i| i.key.as_str() == "issue:ir-000000000001").unwrap();
    assert_eq!(pushed.project, REPO, "the repo id is swapped for the folder");
    assert_eq!(pushed.pane_id.as_deref(), Some("pane-1"));
    assert_eq!(pushed.title, "#12 Cart total [r] down", "the live redaction ran on the title");
    assert_eq!(pushed.facts.files.as_ref().map(Vec::len), Some(1));
    assert_eq!(pushed.facts.files_total, 3, "the push check's counts stay");
    assert_eq!(pushed.risk.tier, 7, "no 'could not see which files changed'");

    let stopped = items.iter().find(|i| i.key.as_str() == "issue:ir-000000000002").unwrap();
    assert!(matches!(stopped.keep, Keep::No { .. }));
    assert_eq!(stopped.facts.secret_files, vec![".env".to_string()], "a held run is scanned from its copy");
    assert_eq!(stopped.facts.secret_lines, vec![("src/y.ts".to_string(), 2)]);
    assert_eq!(stopped.risk.tier, 2);
    assert_eq!(stopped.pane_id, None);
}

#[test]
fn a_plain_copy_gets_its_label_turns_and_a_group() {
    let io = plain_copies();
    io.panes.borrow_mut().push(PaneFacts { parent: Some("pane-1".into()), ..pane("pane-2", REPO) });
    let item = gather_one(&io, &ItemKey::parse("copy:plain-b").unwrap()).unwrap();
    assert_eq!(item.agent_label, "Fixture agent");
    assert_eq!(item.turns, 2);
    assert_eq!(item.group.as_ref().map(|g| g.label.as_str()), Some("Title pane-1 and helpers"));
    assert_eq!(item.pane_id.as_deref(), Some("pane-1"));
    // A copy whose pane was closed has no pane and no group.
    io.panes.borrow_mut().clear();
    let item = gather_one(&io, &ItemKey::parse("copy:plain-b").unwrap()).unwrap();
    assert_eq!((item.pane_id.clone(), item.group.clone()), (None, None));
    assert_eq!(item.fix, Err(PANE_CLOSED.to_string()));
}

#[test]
fn the_window_and_the_cap_are_applied_here() {
    let io = Fake::new();
    io.add_copy("old", None, read(&["a.ts"], None, None), "Old work");
    io.add_copy("fresh", None, read(&["b.ts"], None, None), "Fresh work");
    io.ops.borrow_mut().get_mut("fresh").unwrap().facts.finished_at_ms = WINDOW_MS + 50_000;
    io.now.set(WINDOW_MS + 60_000);
    assert_eq!(keys(&io), vec!["copy:fresh"], "work older than 14 days is not listed");

    let big = Fake::new();
    for n in 0..(ITEMS_CAP + 5) {
        big.add_copy(&format!("c{n}"), None, read(&["a.ts"], None, None), "Work");
    }
    let desk = gather(&big);
    assert_eq!(desk.items.len(), ITEMS_CAP);
    assert_eq!(desk.counts.total as usize, ITEMS_CAP);
}

#[test]
fn a_folder_item_takes_its_files_and_scan_from_the_save_points() {
    let io = folder_fake();
    let item = gather_one(&io, &ItemKey::parse("folder:pane-1:1-1").unwrap()).unwrap();
    assert_eq!(item.source, Source::Folder);
    assert_eq!(item.keep, Keep::Mark { pane_id: "pane-1".into(), through_ms: 2_000 });
    assert_eq!(item.throw, Throw::Restore { pane_id: "pane-1".into(), checkpoint_id: "cp1".into(), files: 2 });
    assert_eq!(item.facts.secret_lines, vec![("src/x.ts".to_string(), 7)]);
    assert!(item.facts.lines_scanned);
    assert_eq!(item.risk.tier, 2);

    // Without save points: names only, no line scan.
    io.range_reads.borrow_mut().clear();
    let item = gather_one(&io, &ItemKey::parse("folder:pane-1:1-1").unwrap()).unwrap();
    assert!(!item.facts.lines_scanned);
    assert_eq!(item.facts.files.as_ref().map(Vec::len), Some(2));
}

#[test]
fn turns_in_a_separate_copy_are_not_folder_items() {
    let io = folder_fake();
    io.turns.borrow_mut().clear();
    // The pane's folder is inside a copy checkout.
    io.panes.borrow_mut()[0].cwd = PathBuf::from("/work/.puppet-master/worktrees/plain-b/sub");
    io.add_copy("plain-b", Some("pane-1"), read(&["src/b.ts"], None, None), "Work");
    io.turns.borrow_mut().push((turn("pane-1", 1, 1_000, 2_000, &["src/b.ts"]), None));
    assert_eq!(keys(&io), vec!["copy:plain-b"]);
}

// ------------------------------------------------------------------------------------------------ act

#[test]
fn keep_on_a_copy_merges_once_with_project_and_handle_and_audits_without_text() {
    let io = plain_copies();
    let (_, fingerprint) = io.key("copy:plain-b");
    let done = act::keep(&io, "copy:plain-b", &fingerprint).unwrap();
    assert_eq!(io.calls(), vec![format!("merge:{REPO}:plain-b")]);
    assert_eq!(done.sentence, "Merged 2 files into main.");
    assert!(done.gone);

    let audits = io.audits.borrow();
    assert_eq!(audits.len(), 1);
    let detail = &audits[0].1;
    assert_eq!(detail["action"], "keep");
    assert_eq!(detail["source"], "copy");
    assert_eq!(detail["key"], "copy:plain-b");
    assert_eq!(detail["tier"], 4);
    let text = format!("{} {detail}", audits[0].0);
    for private in ["Fix the cart total", "/work", "cart.test.ts"] {
        assert!(!text.contains(private), "{private} in the audit: {text}");
    }
}

#[test]
fn a_stale_fingerprint_is_refused_and_nothing_is_called() {
    let io = plain_copies();
    let (_, fingerprint) = io.key("copy:plain-b");
    // One more file changes in the copy between the list and the click.
    io.set_read("plain-b", read(&["src/cart.test.ts", "src/more.ts"], None, None));
    assert_eq!(code(act::keep(&io, "copy:plain-b", &fingerprint)), "REVIEW_ITEM_CHANGED");
    assert_eq!(code(act::throw_away(&io, "copy:plain-b", &fingerprint)), "REVIEW_ITEM_CHANGED");
    assert_eq!(code(act::fix(&io, "copy:plain-b", &fingerprint, "")), "REVIEW_ITEM_CHANGED");
    assert!(io.calls().is_empty() && io.audits.borrow().is_empty());
    assert_eq!(code(act::keep(&io, "copy:gone", "000000000000")), "REVIEW_ITEM_NOT_FOUND");
}

#[test]
fn bad_keys_are_refused_before_anything_is_read() {
    let io = plain_copies();
    let long = format!("copy:{}", "a".repeat(300));
    for bad in ["copy:../x", "copy:", "night:nt-xyz", "folder:a:5-2", "issue:ir-zz", long.as_str(), "plain"] {
        assert_eq!(code(act::keep(&io, bad, "x")), "REVIEW_BAD_KEY", "{bad}");
        assert_eq!(code(act::throw_away(&io, bad, "x")), "REVIEW_BAD_KEY");
        assert_eq!(code(act::fix(&io, bad, "x", "")), "REVIEW_BAD_KEY");
        assert_eq!(code(act::detail(&io, bad)), "REVIEW_BAD_KEY");
    }
    assert_eq!(code(act::unmark(&io, "../x")), "REVIEW_BAD_KEY");
    assert!(io.calls().is_empty());
}

#[test]
fn keep_on_a_held_issue_is_refused_and_a_pushable_one_only_pushes() {
    let io = Fake::new();
    io.add_copy("issue-12-ab12", None, read(&["src/x.ts"], None, None), "Issue task");
    io.add_copy("issue-13-ef56", None, read(&["src/y.ts"], None, None), "Held task");
    io.issues.replace(Ok(vec![issue_run(1, ask(push_facts()), "issue-12-ab12"), issue_run(2, held(), "issue-13-ef56")]));

    let (_, held_fp) = io.key("issue:ir-000000000002");
    assert_eq!(code(act::keep(&io, "issue:ir-000000000002", &held_fp)), "REVIEW_CANNOT_KEEP");
    assert!(io.calls().is_empty());

    let (_, push_fp) = io.key("issue:ir-000000000001");
    let done = act::keep(&io, "issue:ir-000000000001", &push_fp).unwrap();
    assert_eq!(io.calls(), vec!["issue:ir-000000000001:push".to_string()]);
    assert!(done.sentence.contains("draft"));
}

#[test]
fn keep_and_throw_route_night_and_issue_items_to_their_own_paths() {
    let io = Fake::new();
    io.night.replace(Ok(Some(crate::night_shift::view::morning_fixture())));
    io.add_copy("run-1", Some("pane-42"), read(&["src/cart.ts"], None, None), "Night task");
    io.add_copy("issue-13-ef56", None, read(&["src/y.ts"], None, None), "Held task");
    io.issues.replace(Ok(vec![issue_run(2, held(), "issue-13-ef56")]));

    let (_, fp) = io.key("night:nt-00000001");
    let kept = act::keep(&io, "night:nt-00000001", &fp).unwrap();
    assert!(kept.gone && kept.sentence.starts_with("Merged"));
    let (_, fp) = io.key("night:nt-00000002");
    act::throw_away(&io, "night:nt-00000002", &fp).unwrap();
    let (_, fp) = io.key("issue:ir-000000000002");
    act::throw_away(&io, "issue:ir-000000000002", &fp).unwrap();
    assert_eq!(
        io.calls(),
        vec![
            "night_keep:nt-00000001".to_string(),
            "night_throw:nt-00000002".to_string(),
            "issue:ir-000000000002:throw_away".to_string(),
        ]
    );
}

#[test]
fn throw_away_on_a_copy_discards_it_with_project_and_handle() {
    let io = plain_copies();
    let (_, fp) = io.key("copy:plain-c");
    let done = act::throw_away(&io, "copy:plain-c", &fp).unwrap();
    assert_eq!(io.calls(), vec![format!("discard:{REPO}:plain-c")]);
    assert!(done.gone);
    assert_eq!(io.audits.borrow()[0].1["action"], "throw_away");
}

#[test]
fn looks_good_sets_a_mark_the_item_goes_and_unmark_brings_it_back() {
    let io = folder_fake();
    let (_, fp) = io.key("folder:pane-1:1-1");
    let done = act::keep(&io, "folder:pane-1:1-1", &fp).unwrap();
    assert_eq!(done.sentence, "Marked as looked at.");
    assert!(done.gone);
    assert!(io.calls().is_empty(), "a mark merges and restores nothing");
    let marks = Marks::load(&io.marks_dir);
    assert_eq!(marks.get("pane-1").map(|m| (m.through_ms, m.how)), Some((2_000, How::Kept)));
    assert!(keys(&io).is_empty());

    act::unmark(&io, "pane-1").unwrap();
    assert_eq!(keys(&io), vec!["folder:pane-1:1-1"]);
    act::unmark(&io, "pane-1").unwrap();
}

#[test]
fn restore_checks_the_other_panes_again_at_the_moment_of_the_throw() {
    let io = folder_fake();
    let (_, fp) = io.key("folder:pane-1:1-1");
    // Between the list and the end of the Undo window another agent changes the same folder.
    io.panes.borrow_mut().push(pane("pane-2", REPO));
    io.turns.borrow_mut().push((turn("pane-2", 1, 2_500, 3_000, &["src/z.ts"]), Some(PathBuf::from(REPO))));
    assert_eq!(code(act::throw_away(&io, "folder:pane-1:1-1", &fp)), "REVIEW_CANNOT_THROW");
    assert!(io.calls().is_empty(), "nothing was restored");

    io.turns.borrow_mut().pop();
    let done = act::throw_away(&io, "folder:pane-1:1-1", &fp).unwrap();
    assert_eq!(io.calls(), vec!["restore:pane-1:cp1".to_string()]);
    assert!(done.gone);
    let marks = Marks::load(&io.marks_dir);
    assert_eq!(marks.get("pane-1").map(|m| (m.through_ms, m.how)), Some((2_000, How::ThrownAway)));
}

#[test]
fn a_running_pane_or_a_missing_save_point_cannot_be_restored() {
    let io = folder_fake();
    io.panes.borrow_mut()[0].status = PaneStatus::Running;
    let (_, fp) = io.key("folder:pane-1:1-1");
    assert_eq!(code(act::throw_away(&io, "folder:pane-1:1-1", &fp)), "REVIEW_CANNOT_THROW");
    io.panes.borrow_mut()[0].status = PaneStatus::Idle;
    io.before.borrow_mut().clear();
    // Without a save point the work shows names only, so the fingerprint is new: look again first.
    let (_, fp) = io.key("folder:pane-1:1-1");
    let why = act::throw_away(&io, "folder:pane-1:1-1", &fp).unwrap_err();
    assert_eq!(why.code, "REVIEW_CANNOT_THROW");
    assert!(why.message.starts_with("No save point"), "{}", why.message);
    assert!(io.calls().is_empty());
}

#[test]
fn ask_to_fix_puts_one_draft_on_the_right_pane_and_calls_nothing_else() {
    let io = plain_copies();
    let (_, fp) = io.key("copy:plain-b");
    let pane = act::fix(&io, "copy:plain-b", &fp, "Please keep the old test").unwrap();
    assert_eq!(pane, "pane-1");
    assert_eq!(io.calls(), vec!["put_draft:pane-1".to_string()]);
    let drafts = io.drafts.borrow();
    assert_eq!(drafts.len(), 1);
    assert!(drafts[0].1.starts_with("Before I keep this, please fix:"));
    assert!(drafts[0].1.contains("- Test files changed: src/cart.test.ts"));
    assert!(drafts[0].1.contains("Please keep the old test"));
    assert!(!drafts[0].1.contains("Fix the cart total"), "no title in the draft");
    assert!(io.audits.borrow().is_empty(), "nothing changed, so no audit line");
    drop(drafts);

    io.panes.borrow_mut().clear();
    let (_, fp) = io.key("copy:plain-b");
    assert_eq!(code(act::fix(&io, "copy:plain-b", &fp, "")), "REVIEW_NO_PANE");
    assert_eq!(io.drafts.borrow().len(), 1);
}

#[test]
fn the_detail_patch_is_redacted_and_the_extra_lines_come_from_the_runs() {
    let io = Fake::new();
    io.night.replace(Ok(Some(crate::night_shift::view::morning_fixture())));
    io.add_copy("run-1", None, read(&["src/cart.ts"], None, None), "Night task");
    let raw = format!(
        "+key = sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n+path = {HOME}\\notes\\a.md\n+ok = 1\n"
    );
    io.patches.borrow_mut().insert("run-1".into(), raw);

    let detail = act::detail(&io, "night:nt-00000002").unwrap();
    assert!(!detail.patch.contains("sk-ant-api03") && !detail.patch.contains("Users"), "{}", detail.patch);
    assert!(detail.patch.contains('~') && detail.patch.contains("+ok = 1"));
    assert!(!detail.patch_truncated);
    assert_eq!(detail.extra, vec!["Look at the failing tests before you keep it.".to_string()]);
    assert_eq!(detail.item.key, "night:nt-00000002");

    // A ready task has no extra line; a patch over the limit is cut and says so.
    io.patches.borrow_mut().insert("run-1".into(), "+x\n".repeat(DETAIL_PATCH_MAX));
    let detail = act::detail(&io, "night:nt-00000001").unwrap();
    assert!(detail.extra.is_empty());
    assert!(detail.patch_truncated && detail.patch.len() <= DETAIL_PATCH_MAX);

    // No patch readable: said in plain words.
    io.patches.borrow_mut().clear();
    let detail = act::detail(&io, "night:nt-00000001").unwrap();
    assert_eq!(detail.extra, vec!["Puppet Master could not read the changes.".to_string()]);
    assert_eq!(code(act::detail(&io, "copy:nothing")), "REVIEW_ITEM_NOT_FOUND");
}

#[test]
fn the_issue_detail_lists_the_push_check_reasons_and_a_folder_detail_reads_the_save_points() {
    let io = folder_fake();
    io.add_copy("issue-12-ab12", None, read(&["src/x.ts"], None, None), "Issue task");
    let facts = PushFacts { secret_files: vec![".env".into()], ..push_facts() };
    io.issues.replace(Ok(vec![issue_run(1, ask(facts), "issue-12-ab12")]));
    io.patches.borrow_mut().insert("issue-12-ab12".into(), "+a\n".into());
    io.patches.borrow_mut().insert("pane-1".into(), format!("+home {HOME}\\x\n"));

    let issue = act::detail(&io, "issue:ir-000000000001").unwrap();
    assert!(issue.extra.iter().any(|line| line.contains(".env")), "{:?}", issue.extra);
    let folder = act::detail(&io, "folder:pane-1:1-1").unwrap();
    assert!(folder.patch.contains("+home ~"), "{}", folder.patch);
}

#[test]
fn a_second_action_on_the_same_key_while_one_runs_is_busy() {
    let io = plain_copies();
    let (_, fp) = io.key("copy:plain-b");
    io.reenter.replace(Some(("copy:plain-b".to_string(), fp.clone())));
    act::keep(&io, "copy:plain-b", &fp).unwrap();
    let inner = io.reenter_result.borrow_mut().take().expect("the hook ran");
    assert_eq!(code(inner), "REVIEW_BUSY");
    assert_eq!(io.calls().len(), 1, "merged once");
    // The key is free again afterwards.
    let (_, fp) = io.key("copy:plain-c");
    assert!(act::keep(&io, "copy:plain-c", &fp).is_ok());
}
