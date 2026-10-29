//! Review Desk: the live layer (SPEC n33 section 3.5). One trait, [`DeskIo`], holds everything the desk reads from
//! or does to the world: the Night Shift and issue stores, the copies, the changes and patches, the run facts, the
//! turn summaries, the panes, the save points, the marks folder, the five actions (merge, discard, night keep and
//! throw, issue push and throw, restore), the chat draft and the audit log. `gather` and `act` take a `&dyn DeskIo`,
//! so the tests run them on a fake with no git, no registry and no window.
//!
//! [`LiveIo`] is the real one, over the modules of SPEC section 2. It reads and acts only on what the person's
//! desktop commands ask (S1). The facts of one copy are read once per minute (S15): a cache keyed by the handle and the
//! copy's live change totals, so a copy that changed is read again at once (S3).

use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;

use super::model::{strip_controls, Clean, FACTS_TTL_MS, PATCH_SCAN_MAX};
use super::sources::copies::{MissionFacts, OpFacts, ScanFacts};
use super::sources::folder::PaneFacts;
use crate::agent_runs::checks::Check;
use crate::checkpoints::FileChange as PointChange;
use crate::issue_intake::model::IssueRun;
use crate::night_shift::model::NightRun;
use crate::night_shift::review::KeepView as NightKeepView;
use crate::operations::OperationError;
use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use crate::redact::rules::RedactContext;
use crate::turn_summary::model::TurnSummary;
use crate::worktrees::service::ListItem;
use crate::worktrees::{ChangeSummary, Changes, FileChange, MergeOutcome};

type Registry = Arc<Mutex<PaneRegistry>>;

/// What was read from one place that changed files: a copy, or a range of save points.
#[derive(Debug, Clone, Default)]
pub struct CopyRead {
    /// `None`: the changes could not be read.
    pub changes: Option<Changes>,
    /// `None`: no patch was read.
    pub scan: Option<ScanFacts>,
}

/// The last run in a copy, and how many turns it took.
#[derive(Debug, Clone)]
pub struct OpRead {
    pub facts: OpFacts,
    pub turns: u32,
}

/// Everything the desk reads and does. Reads never change anything. Actions are called only from `act`, which
/// the desktop commands call after the person clicked (S1, S2).
pub trait DeskIo {
    fn now_ms(&self) -> u64;
    /// Strict redaction, home folder as `~`, control characters removed. For one line of text (a title, a name).
    fn clean(&self) -> Clean;
    /// Strict redaction and home folder as `~` for a whole patch. Keeps line breaks and tabs.
    fn clean_patch(&self, raw: &str) -> String;
    /// The folder of `marks.json`.
    fn marks_root(&self) -> PathBuf;

    // Stores. An `Err` is one plain failure; the desk then lists the other sources and adds a note.
    fn night_run(&self) -> Result<Option<NightRun>, String>;
    fn issue_runs(&self) -> Result<Vec<IssueRun>, String>;
    /// The folder of a watched repository (`IssueRun::repo_id`).
    fn issue_folder(&self, repo_id: &str) -> Option<String>;
    /// Every separate copy of every known project.
    fn copies(&self) -> Result<Vec<ListItem>, String>;
    /// The branch a Night Shift Keep would merge into (`None` = unknown).
    fn keep_target(&self, project: &str) -> Option<String>;
    /// The agent's label from the registry; never typed here.
    fn label_of(&self, agent_type: &str) -> String;

    // Facts.
    /// Changes and the line scan of one copy. Cached; `summary` is the copy's live totals (part of the cache key).
    fn copy_read(&self, project: &str, handle: &str, summary: Option<ChangeSummary>) -> CopyRead;
    /// The copy's whole patch, `(text, truncated)`, raw. For the detail only.
    fn copy_patch(&self, project: &str, handle: &str) -> Option<(String, bool)>;
    fn latest_op(&self, project: &str, handle: &str) -> Option<OpRead>;
    /// A mission copy's last verdict. `mission_id` is the handle without `mission-`.
    fn mission(&self, project: &str, mission_id: &str) -> Option<MissionFacts>;
    /// The mission's own first sentence (what it needs or how it ended), raw. For the detail.
    fn mission_headline(&self, project: &str, mission_id: &str) -> Option<String>;
    /// Turns that ended at or after `since_ms`, each with its pane's project folder (`None` = unknown).
    fn turns(&self, since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)>;
    /// The open panes. Titles are already redacted.
    fn panes(&self) -> Vec<PaneFacts>;
    /// The save point taken just before a turn that started at `started_at_ms`.
    fn checkpoint_before(&self, pane_id: &str, started_at_ms: u64) -> Option<String>;
    /// The newest save point taken after a turn.
    fn checkpoint_last_after(&self, pane_id: &str) -> Option<String>;
    /// Changes and the line scan between two save points of a pane. Cached like `copy_read`.
    fn range_read(&self, pane_id: &str, from_id: &str, to_id: &str) -> CopyRead;
    /// The patch between two save points, `(text, truncated)`, raw.
    fn range_patch(&self, pane_id: &str, from_id: &str, to_id: &str, max_bytes: usize) -> Option<(String, bool)>;

    // Actions (the person's click, never a timer).
    fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError>;
    fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError>;
    fn night_keep(&self, task_id: &str) -> Result<NightKeepView, OperationError>;
    fn night_throw(&self, task_id: &str) -> Result<(), OperationError>;
    /// `push` or `throw_away`, as the person from the desktop.
    fn issue_act(&self, run_id: &str, action: &str) -> Result<(), OperationError>;
    fn restore(&self, pane_id: &str, checkpoint_id: &str) -> Result<(), OperationError>;
    /// Keeps the text as the pane's pending Chat draft. Nothing is sent.
    fn put_draft(&self, pane_id: &str, text: String);
    fn audit(&self, summary: &str, detail: Value);
}

// ------------------------------------------------------------------------------------------ pure helpers

/// A small cache with a lifetime per entry. `get` drops an entry that is too old.
pub struct TtlCache<V> {
    map: std::collections::HashMap<String, (u64, V)>,
    cap: usize,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(cap: usize) -> Self {
        Self { map: std::collections::HashMap::new(), cap }
    }

    pub fn get(&mut self, key: &str, now: u64, ttl: u64) -> Option<V> {
        match self.map.get(key) {
            Some((at, value)) if now.saturating_sub(*at) < ttl => Some(value.clone()),
            Some(_) => {
                self.map.remove(key);
                None
            }
            None => None,
        }
    }

    pub fn put(&mut self, key: String, now: u64, ttl: u64, value: V) {
        if self.map.len() >= self.cap {
            self.map.retain(|_, (at, _)| now.saturating_sub(*at) < ttl);
        }
        if self.map.len() >= self.cap {
            self.map.clear();
        }
        self.map.insert(key, (now, value));
    }
}

/// The cache key of a copy's facts: the handle and the change totals the list just read. A copy that changed has
/// other totals, so its facts are read again at once.
pub fn copy_key(handle: &str, summary: Option<ChangeSummary>) -> String {
    match summary {
        Some(s) => format!("copy|{handle}|{}|{}|{}", s.files_changed, s.additions, s.deletions),
        None => format!("copy|{handle}|-"),
    }
}

pub fn range_key(pane_id: &str, from_id: &str, to_id: &str) -> String {
    format!("range|{pane_id}|{from_id}|{to_id}")
}

/// The scan of one patch. A truncated or over-size patch is not scanned (`scanned = false`): only names were checked.
pub fn scan_patch(
    patch: &str,
    truncated: bool,
    scan: &dyn Fn(&str) -> (Vec<(String, u32)>, Vec<(String, u32)>),
) -> ScanFacts {
    if truncated || patch.len() > PATCH_SCAN_MAX {
        return ScanFacts { secret_lines: Vec::new(), home_lines: Vec::new(), scanned: false };
    }
    let (secret_lines, home_lines) = scan(patch);
    ScanFacts { secret_lines, home_lines, scanned: true }
}

/// A whole patch made safe to show (S5): strict redaction, the home folder as `~`, control characters other than
/// line breaks and tabs removed, bidi overrides removed.
pub fn redact_patch(raw: &str, ctx: &RedactContext, home: Option<&str>) -> String {
    let redacted = crate::redact::strict_with(raw, ctx).into_inner();
    let tilde = crate::session_chat::home_to_tilde(&redacted, home);
    tilde
        .chars()
        .filter(|c| matches!(c, '\n' | '\t') || (!c.is_control() && !matches!(*c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')))
        .collect()
}

/// A line of text made safe to show: strict redaction, the home folder as `~`, controls removed.
pub fn clean_line(raw: &str, ctx: &RedactContext, home: Option<&str>) -> String {
    let redacted = crate::redact::strict_with(raw, ctx).into_inner();
    strip_controls(&crate::session_chat::home_to_tilde(&redacted, home))
}

/// The check changes of save points as the copy's `Changes` (so one fill path serves both).
pub fn changes_of(points: &[PointChange]) -> Changes {
    let files: Vec<FileChange> = points
        .iter()
        .map(|change| FileChange {
            path: change.path.clone(),
            status: match change.status.chars().next() {
                Some('A') => "added",
                Some('D') => "deleted",
                _ => "modified",
            }
            .to_string(),
            additions: change.added,
            deletions: change.deleted,
            binary: change.binary,
        })
        .collect();
    Changes {
        files_changed: files.len(),
        additions: points.iter().map(|change| change.added).sum(),
        deletions: points.iter().map(|change| change.deleted).sum(),
        truncated: false,
        files,
    }
}

/// What a run's check is called on a row.
pub fn check_name(check: &Check) -> String {
    match check {
        Check::CommandExits { command, .. } => crate::worktrees::preset::display(command),
        Check::FileExists { path } => format!("{path} exists"),
        Check::FileAbsent { path } => format!("{path} is gone"),
        Check::FileContains { path, .. } => format!("{path} contains the text"),
        Check::ResultContains { .. } => "the reply contains the text".to_string(),
    }
}

fn pane_status(raw: &str) -> PaneStatus {
    match raw {
        "running" => PaneStatus::Running,
        "waiting_input" => PaneStatus::WaitingInput,
        "error" => PaneStatus::Error,
        _ => PaneStatus::Idle,
    }
}

/// The `+` lines of a patch that look like a secret, and those that hold the home folder: `(path, line)`. The same
/// test as the push check (`issue_intake::live::scan_added`).
fn scan_added(diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
    let ctx = crate::redact::known::context();
    let hit = |line: &str| !crate::redact::rules::find_spans(line, &ctx, crate::redact::rules::Mode::Secrets).is_empty();
    let home = crate::project_path::home_dir();
    crate::issue_intake::pushcheck::scan_added(diff, &hit, home.as_deref().and_then(|path| path.to_str()))
}

/// Night Shift's own store. Tests never open the real one.
fn night_store() -> crate::night_shift::store::Store {
    #[cfg(not(test))]
    {
        crate::night_shift::store::Store::live()
    }
    #[cfg(test)]
    {
        crate::night_shift::store::Store::at(std::env::temp_dir().join(format!("pm-review-desk-night-never-used-{}", std::process::id())))
    }
}

static FACTS: Lazy<Mutex<TtlCache<CopyRead>>> = Lazy::new(|| Mutex::new(TtlCache::new(256)));
/// Marks are read and written one at a time.
pub static MARKS_LOCK: Mutex<()> = Mutex::new(());

// ------------------------------------------------------------------------------------------ the real io

/// The real world, over the modules of SPEC section 2. Built per command call (`LiveIo { registry, app }`).
pub struct LiveIo {
    pub registry: Registry,
    pub app: tauri::AppHandle,
}

impl LiveIo {
    fn context(&self) -> (Arc<RedactContext>, Option<String>) {
        let home = crate::project_path::home_dir().map(|path| path.to_string_lossy().into_owned());
        (crate::redact::known::context(), home)
    }

    fn cached(&self, key: String, read: impl FnOnce() -> CopyRead) -> CopyRead {
        let now = self.now_ms();
        if let Some(hit) = FACTS.lock().get(&key, now, FACTS_TTL_MS) {
            return hit;
        }
        let fresh = read();
        // A failed read is not kept: the next look tries again.
        if fresh.changes.is_some() {
            FACTS.lock().put(key, now, FACTS_TTL_MS, fresh.clone());
        }
        fresh
    }
}

impl DeskIo for LiveIo {
    fn now_ms(&self) -> u64 {
        crate::questions::now_ms()
    }

    fn clean(&self) -> Clean {
        let (ctx, home) = self.context();
        Clean { text: Box::new(move |raw| clean_line(raw, &ctx, home.as_deref())) }
    }

    fn clean_patch(&self, raw: &str) -> String {
        let (ctx, home) = self.context();
        redact_patch(raw, &ctx, home.as_deref())
    }

    fn marks_root(&self) -> PathBuf {
        #[cfg(not(test))]
        {
            super::marks::marks_root()
        }
        #[cfg(test)]
        {
            std::env::temp_dir().join(format!("pm-review-desk-never-used-{}", std::process::id()))
        }
    }

    fn night_run(&self) -> Result<Option<NightRun>, String> {
        Ok(night_store().load())
    }

    fn issue_runs(&self) -> Result<Vec<IssueRun>, String> {
        Ok(crate::issue_intake::live::store().runs())
    }

    fn issue_folder(&self, repo_id: &str) -> Option<String> {
        crate::issue_intake::live::config().repos.into_iter().find(|repo| repo.id == repo_id).map(|repo| repo.folder)
    }

    fn copies(&self) -> Result<Vec<ListItem>, String> {
        Ok(crate::worktrees::service::list(None, &self.registry))
    }

    fn keep_target(&self, project: &str) -> Option<String> {
        crate::night_shift::review::keep_target(project)
    }

    fn label_of(&self, agent_type: &str) -> String {
        crate::crews::live::label_of(agent_type)
    }

    fn copy_read(&self, project: &str, handle: &str, summary: Option<ChangeSummary>) -> CopyRead {
        self.cached(copy_key(handle, summary), || {
            match crate::worktrees::service::diff(Some(project), handle, None, &self.registry) {
                Ok(view) => CopyRead {
                    scan: Some(scan_patch(&view.patch, view.patch_truncated, &scan_added)),
                    changes: Some(view.changes),
                },
                Err(_) => CopyRead::default(),
            }
        })
    }

    fn copy_patch(&self, project: &str, handle: &str) -> Option<(String, bool)> {
        let view = crate::worktrees::service::diff(Some(project), handle, None, &self.registry).ok()?;
        Some((view.patch, view.patch_truncated))
    }

    fn latest_op(&self, project: &str, handle: &str) -> Option<OpRead> {
        let op = crate::operations::latest_agent_run(project, handle).ok()?;
        let check_results = op.check_results.iter().map(|result| (check_name(&result.check), result.passed)).collect();
        Some(OpRead {
            turns: op.turn_index.saturating_add(1),
            facts: OpFacts {
                status: format!("{:?}", op.status).to_lowercase(),
                task: op.task.clone(),
                finished_at_ms: op.finished_at_ms.unwrap_or(op.updated_at_ms),
                check_results,
                account: op.worker.account.clone(),
            },
        })
    }

    fn mission(&self, project: &str, mission_id: &str) -> Option<MissionFacts> {
        let mission = crate::missions::store::load(project, mission_id).ok()?;
        let verdict = mission.rounds.iter().rev().find_map(|round| round.verdict.as_ref());
        Some(MissionFacts {
            objective: mission.objective.clone(),
            checks_passed: verdict.map_or(0, |v| v.checks_passed),
            checks_total: verdict.map_or(0, |v| v.checks_total),
            weakened: verdict.map(|v| v.test_changes.blocking_sentences()).unwrap_or_default(),
        })
    }

    fn mission_headline(&self, project: &str, mission_id: &str) -> Option<String> {
        crate::missions::store::load(project, mission_id).ok()?.sentence
    }

    fn turns(&self, since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)> {
        crate::turn_summary::tracker().since_with_project(since_ms)
    }

    fn panes(&self) -> Vec<PaneFacts> {
        let clean = self.clean();
        let panes = self.registry.lock().list();
        panes
            .into_iter()
            .map(|pane| PaneFacts {
                id: pane.id,
                cwd: PathBuf::from(pane.cwd),
                title: clean.apply(pane.title.as_deref().unwrap_or("")),
                status: pane_status(&pane.status),
                parent: pane.parent_pane_id,
                alive: true,
            })
            .collect()
    }

    fn checkpoint_before(&self, pane_id: &str, started_at_ms: u64) -> Option<String> {
        crate::checkpoints::live().before_of_turns(pane_id, started_at_ms)
    }

    fn checkpoint_last_after(&self, pane_id: &str) -> Option<String> {
        crate::checkpoints::live().last_after_id(pane_id)
    }

    fn range_read(&self, pane_id: &str, from_id: &str, to_id: &str) -> CopyRead {
        self.cached(range_key(pane_id, from_id, to_id), || {
            let runtime = crate::checkpoints::live();
            let Ok(points) = runtime.changes_between_ids(pane_id, from_id, to_id) else {
                return CopyRead::default();
            };
            let scan = runtime
                .patch_between(pane_id, from_id, to_id, PATCH_SCAN_MAX)
                .ok()
                .flatten()
                .map(|patch| scan_patch(&patch.patch, patch.truncated, &scan_added));
            CopyRead { changes: Some(changes_of(&points)), scan }
        })
    }

    fn range_patch(&self, pane_id: &str, from_id: &str, to_id: &str, max_bytes: usize) -> Option<(String, bool)> {
        let patch = crate::checkpoints::live().patch_between(pane_id, from_id, to_id, max_bytes).ok().flatten()?;
        Some((patch.patch, patch.truncated))
    }

    fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError> {
        crate::worktrees::service::merge(Some(project), handle, &self.registry)
    }

    fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError> {
        crate::worktrees::service::discard(Some(project), handle, &self.registry)
    }

    fn night_keep(&self, task_id: &str) -> Result<NightKeepView, OperationError> {
        let ops = crate::night_shift::review::Live { registry: &self.registry };
        crate::night_shift::review::keep_with(
            &night_store(),
            &ops,
            task_id,
            "desktop",
            self.now_ms(),
        )
    }

    fn night_throw(&self, task_id: &str) -> Result<(), OperationError> {
        let ops = crate::night_shift::review::Live { registry: &self.registry };
        crate::night_shift::review::throw_away_with(
            &night_store(),
            &ops,
            task_id,
            "desktop",
            self.now_ms(),
        )
    }

    fn issue_act(&self, run_id: &str, action: &str) -> Result<(), OperationError> {
        crate::issue_intake::live::act(run_id, action, "desktop").map(|_| ())
    }

    fn restore(&self, pane_id: &str, checkpoint_id: &str) -> Result<(), OperationError> {
        crate::checkpoints::live()
            .restore(pane_id, checkpoint_id)
            .map(|_| ())
            .map_err(|error| OperationError::new("RESTORE_FAILED", error.to_string(), true))
    }

    fn put_draft(&self, pane_id: &str, text: String) {
        crate::commands::point::put_chat_draft(&self.app, pane_id, text);
    }

    fn audit(&self, summary: &str, detail: Value) {
        crate::audit::record(crate::audit::Kind::ReviewDesk, crate::audit::Who::Desktop, summary, detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::rules::RedactContext;

    fn ctx() -> RedactContext {
        RedactContext { env_values: Vec::new(), home: Some(r"C:\Users\you".to_string()) }
    }

    #[test]
    fn the_cache_serves_inside_the_lifetime_and_drops_after_it() {
        let mut cache: TtlCache<u32> = TtlCache::new(8);
        cache.put("a".into(), 1_000, FACTS_TTL_MS, 7);
        assert_eq!(cache.get("a", 1_000 + FACTS_TTL_MS - 1, FACTS_TTL_MS), Some(7));
        assert_eq!(cache.get("a", 1_000 + FACTS_TTL_MS, FACTS_TTL_MS), None);
        assert_eq!(cache.get("a", 1_001, FACTS_TTL_MS), None, "a dropped entry is gone");
    }

    #[test]
    fn the_cache_stays_small() {
        let mut cache: TtlCache<u32> = TtlCache::new(2);
        cache.put("a".into(), 0, 100, 1);
        cache.put("b".into(), 1, 100, 2);
        cache.put("c".into(), 2, 100, 3);
        assert_eq!(cache.get("c", 2, 100), Some(3));
        assert!(cache.map.len() <= 2);
    }

    #[test]
    fn a_copy_that_changed_has_another_cache_key() {
        let a = ChangeSummary { files_changed: 2, additions: 5, deletions: 1 };
        let b = ChangeSummary { files_changed: 3, additions: 5, deletions: 1 };
        assert_eq!(copy_key("h1", Some(a)), copy_key("h1", Some(a)));
        assert_ne!(copy_key("h1", Some(a)), copy_key("h1", Some(b)));
        assert_ne!(copy_key("h1", Some(a)), copy_key("h2", Some(a)));
        assert_ne!(copy_key("h1", None), copy_key("h1", Some(a)));
        assert_ne!(range_key("p", "1", "2"), range_key("p", "1", "3"));
    }

    #[test]
    fn a_big_or_cut_patch_is_not_scanned() {
        let scan = |_: &str| (vec![("a.ts".to_string(), 3)], Vec::new());
        let small = scan_patch("+x", false, &scan);
        assert!(small.scanned);
        assert_eq!(small.secret_lines, vec![("a.ts".to_string(), 3)]);
        let cut = scan_patch("+x", true, &scan);
        assert!(!cut.scanned && cut.secret_lines.is_empty());
        let huge = scan_patch(&"x".repeat(PATCH_SCAN_MAX + 1), false, &scan);
        assert!(!huge.scanned && huge.secret_lines.is_empty() && huge.home_lines.is_empty());
        let exact = scan_patch(&"x".repeat(PATCH_SCAN_MAX), false, &scan);
        assert!(exact.scanned);
    }

    #[test]
    fn a_patch_loses_secrets_the_home_folder_and_hostile_characters() {
        let raw = [
            "+key = sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "+token = ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "+aws = AKIAABCDEFGHIJKLMNOP",
            r"+path = C:\Users\you\notes\a.md",
            "+\u{202E}evil\u{0007}\tkept",
        ]
        .join("\n");
        let out = redact_patch(&raw, &ctx(), Some(r"C:\Users\you"));
        for secret in ["sk-ant-api03", "ghp_AAAA", "AKIAABCDEFGHIJKLMNOP", r"C:\Users\you"] {
            assert!(!out.contains(secret), "{secret} left in {out}");
        }
        assert!(out.contains("~"), "the home folder becomes ~: {out}");
        assert!(!out.contains('\u{202E}') && !out.contains('\u{0007}'));
        assert!(out.contains("evil\tkept"), "tabs and text stay: {out}");
        assert_eq!(out.lines().count(), 5, "line breaks stay");
    }

    #[test]
    fn a_line_is_cleaned_to_one_safe_line() {
        let out = clean_line("fix sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA in C:\\Users\\you\\a\u{202E}\n", &ctx(), Some(r"C:\Users\you"));
        assert!(!out.contains("sk-ant-api03") && !out.contains("Users") && !out.contains('\n') && !out.contains('\u{202E}'));
    }

    #[test]
    fn save_point_changes_become_copy_changes() {
        let points = vec![
            PointChange { path: "a.ts".into(), status: "A".into(), added: 3, deleted: 0, binary: false },
            PointChange { path: "b.ts".into(), status: "D".into(), added: 0, deleted: 4, binary: false },
            PointChange { path: "c.png".into(), status: "M".into(), added: 0, deleted: 0, binary: true },
        ];
        let changes = changes_of(&points);
        let statuses: Vec<&str> = changes.files.iter().map(|f| f.status.as_str()).collect();
        assert_eq!(statuses, vec!["added", "deleted", "modified"]);
        assert_eq!((changes.files_changed, changes.additions, changes.deletions), (3, 3, 4));
        assert!(changes.files[2].binary);
    }

    #[test]
    fn checks_have_plain_names() {
        let command = Check::CommandExits { command: vec!["npm".into(), "test".into()], expect_exit: 0, timeout_ms: None };
        assert_eq!(check_name(&command), "npm test");
        assert_eq!(check_name(&Check::FileExists { path: "a.txt".into() }), "a.txt exists");
        assert_eq!(check_name(&Check::ResultContains { text: "x".into() }), "the reply contains the text");
    }

    #[test]
    fn pane_status_words_map_to_the_enum() {
        assert_eq!(pane_status("running"), PaneStatus::Running);
        assert_eq!(pane_status("waiting_input"), PaneStatus::WaitingInput);
        assert_eq!(pane_status("error"), PaneStatus::Error);
        assert_eq!(pane_status("idle"), PaneStatus::Idle);
        assert_eq!(pane_status("anything else"), PaneStatus::Idle);
    }
}
