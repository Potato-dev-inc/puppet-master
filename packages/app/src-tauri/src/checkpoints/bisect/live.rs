//! The live side of What broke it (SPEC n33 section 3.5): the job registry and every call the desktop
//! window makes (`commands/what_broke_it.rs` is a thin wrapper over `Live`).
//!
//! - `LiveIo` is everything the search needs from the outside (pane facts, declared tests, save points,
//!   the throw-away copy, running the command, the clock, events, audit). `RealIo` is the app's
//!   implementation; tests use a fake with a virtual clock and a scripted history.
//! - `Live` keeps one job at a time (`WB_BUSY`), starts it on a thread named `what-broke-it`, and answers
//!   `status`, `stop`, `overview`, `findings`, `diff`, `undo_plan`, `undo`, `recheck` and the settings.
//! - The search loop itself is in `job.rs`. No lock of this file is held across a test run (S5).
//!
//! S2: only the desktop window reaches this file. Nothing here is a bridge route, an MCP tool or a phone path.
//! S14: this file never writes to a pane.

#![allow(dead_code)] // `commands/what_broke_it.rs` (WB-16) is the first caller.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use parking_lot::Mutex;

use super::copy::CopyError;
use super::exec::RunResult;
use super::model::{Finding, Limits, Outcome};
use super::points;
use super::revert::{self, Op, RevertError};
use super::store::{self, Store};
use super::views::{
    self, ConflictView, DeclaredView, DiffView, FindingView, JobState, JobView, LimitsView, Overview, PointView, SuggestionView,
    UndoPlanView, UndoReportView,
};
use super::words;
use crate::checkpoints::engine::{self, pane_key, Checkpoint, CheckpointError};
use crate::checkpoints::git::Shadow;
use crate::checkpoints::PaneFacts;
use crate::night_shift::checks::{self, Declared, Suggestion};
use crate::turn_summary::TurnSummary;
use crate::worktrees::preset::WorktreePreset;

pub const WB_BUSY: &str = "WB_BUSY";
pub const WB_NO_TEST: &str = "WB_NO_TEST";
pub const WB_TEST_NOT_FOUND: &str = "WB_TEST_NOT_FOUND";
pub const WB_SETUP_FAILED: &str = "WB_SETUP_FAILED";
pub const WB_TOO_BIG: &str = "WB_TOO_BIG";
pub const WB_TOO_FEW: &str = "WB_TOO_FEW";
pub const WB_BAD_ID: &str = "WB_BAD_ID";
pub const WB_UNDO_CONFLICT: &str = "WB_UNDO_CONFLICT";
pub const WB_UNDO_RUNNING: &str = "WB_UNDO_RUNNING";
pub const WB_UNDO_GONE: &str = "WB_UNDO_GONE";
pub const WB_NOT_UNDOABLE: &str = "WB_NOT_UNDOABLE";
/// Not in the spec's list: the checks before a start (pane gone, checkpoints off, no git, no folder).
pub const WB_UNAVAILABLE: &str = "WB_UNAVAILABLE";
/// Not in the spec's list: a search stopped by an error nobody planned for.
pub const WB_INTERNAL: &str = "WB_INTERNAL";

/// Jobs kept for `status` after they end.
const KEEP_JOBS: usize = 20;

/// A refusal in plain English with its code. `Display` is the message only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WbError {
    pub code: &'static str,
    pub message: String,
}

impl WbError {
    pub fn new(code: &'static str, message: impl Into<String>) -> WbError {
        WbError { code, message: message.into() }
    }
}

impl std::fmt::Display for WbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WbError {}

impl From<CheckpointError> for WbError {
    fn from(e: CheckpointError) -> WbError {
        let code = if matches!(e, CheckpointError::TooLarge(_)) { WB_TOO_BIG } else { WB_UNAVAILABLE };
        WbError::new(code, e.to_string())
    }
}

/// The throw-away folder one job tests in. The real one wraps `copy::Copy`; dropping it removes the folder.
pub trait CopyHandle: Send {
    fn set_tree(&mut self, tree: &str) -> Result<(), CopyError>;
    fn tree_bytes(&self, tree: &str) -> Result<u64, CopyError>;
    /// Run the project's own setup steps in the copy, once.
    fn prepare(&mut self, project: &Path, preset: &WorktreePreset) -> Result<(), CopyError>;
    fn folder(&self) -> PathBuf;
    /// Remove the copy now (dropping it does the same, without the error).
    fn finish(self: Box<Self>) -> Result<(), CopyError>;
}

/// Everything the search needs from outside. Swapped for a fake in `engine_tests.rs`.
pub trait LiveIo: Send + Sync {
    fn now_ms(&self) -> u64;
    fn pane(&self, pane_id: &str) -> Option<PaneFacts>;
    /// Panes working in `folder` (id and facts), for the undo refusal.
    fn panes_in_folder(&self, folder: &Path) -> Vec<(String, PaneFacts)>;
    /// An agent run is open on the pane (asked only when the pane is running).
    fn run_open(&self, pane_id: &str, pane: &PaneFacts) -> bool;
    fn checkpoints_enabled(&self) -> bool;
    fn git_available(&self) -> bool;
    /// Where declared tests may live for this folder: the folder itself, then the origin of a worktree.
    fn check_projects(&self, cwd: &str) -> Vec<String>;
    fn declared(&self, project: &str) -> Vec<Declared>;
    fn declare(&self, project: &str, list: Vec<Declared>) -> Result<(), String>;
    fn suggestions(&self, project: &Path) -> Vec<Suggestion>;
    fn preset(&self, cwd: &str) -> WorktreePreset;
    fn summaries(&self, project: &Path) -> Vec<TurnSummary>;
    fn list_all(&self, project: &Path) -> Result<Vec<Checkpoint>, CheckpointError>;
    /// Take the "now" save point under `key` and return its tree id.
    fn snapshot_now(&self, project: &Path, key: &str, now_ms: u64) -> Result<String, CheckpointError>;
    fn new_copy(&self, project: &Path, copies_root: &Path) -> Result<Box<dyn CopyHandle>, CopyError>;
    fn run_test(&self, copy: &dyn CopyHandle, argv: &[String], timeout_ms: u64, cancel: &AtomicBool) -> RunResult;
    /// Redact test output and cut paths down, before anything is stored (S9).
    fn redact(&self, text: &str) -> String;
    fn emit(&self, job_id: &str, finding_id: Option<&str>);
    fn audit_undo(&self, summary: &str, detail: serde_json::Value);
}

/// Where a job is, for `status`.
pub(super) struct JobInfo {
    pub state: JobState,
    pub point_no: u32,
    pub points: u32,
    pub run: u32,
    pub max_runs: u32,
    pub started_ms: u64,
    pub max_ms: u64,
    pub ended_ms: Option<u64>,
    pub line: String,
    pub finding_id: Option<String>,
}

pub(super) struct Job {
    pub id: String,
    pub folder_name: String,
    pub cancel: AtomicBool,
    pub info: Mutex<JobInfo>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Job {
    fn view(&self, now_ms: u64) -> JobView {
        let info = self.info.lock();
        let end = info.ended_ms.unwrap_or(now_ms);
        JobView {
            id: self.id.clone(),
            state: info.state,
            point_no: info.point_no,
            points: info.points,
            run: info.run,
            max_runs: info.max_runs,
            elapsed_ms: end.saturating_sub(info.started_ms),
            max_ms: info.max_ms,
            line: info.line.clone(),
            finding_id: info.finding_id.clone(),
        }
    }
}

#[derive(Default)]
struct Jobs {
    map: HashMap<String, Arc<Job>>,
    order: Vec<String>,
    /// The job that runs now. Cleared by `BusyGuard`, also when the job thread panics.
    busy: Option<Arc<Job>>,
}

pub(super) struct Shared {
    pub io: Arc<dyn LiveIo>,
    pub store: Store,
    jobs: Mutex<Jobs>,
}

/// Frees the "one job at a time" slot when the job thread ends, however it ends.
pub(super) struct BusyGuard {
    shared: Arc<Shared>,
    id: String,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let mut jobs = self.shared.jobs.lock();
        if jobs.busy.as_ref().is_some_and(|job| job.id == self.id) {
            jobs.busy = None;
        }
    }
}

/// What a job thread starts from. Built by `Live::start` / `Live::recheck`, checked before it exists.
pub(super) struct Req {
    pub finding_id: String,
    pub pane_key: String,
    pub project_dir: PathBuf,
    pub project_key: String,
    pub folder: String,
    pub check: Declared,
    pub limits: Limits,
    /// (pane key, save point id) the person says passed last.
    pub picked: Option<(String, String)>,
    /// A "not sure" finding to continue: its measured answers are not run again.
    pub resume: Option<Finding>,
    pub recheck: bool,
}

/// The entry point of the window's calls.
#[derive(Clone)]
pub struct Live {
    shared: Arc<Shared>,
}

impl Live {
    pub fn new(io: Arc<dyn LiveIo>, store: Store) -> Live {
        Live { shared: Arc::new(Shared { io, store, jobs: Mutex::new(Jobs::default()) }) }
    }

    fn io(&self) -> &dyn LiveIo {
        self.shared.io.as_ref()
    }

    /// Remove copies an earlier run left behind (older than an hour, not held by this process). Call at start.
    pub fn sweep_stale(&self) -> u32 {
        super::copy::sweep_stale(&self.shared.store.copies_root(), self.io().now_ms())
    }

    pub fn settings_get(&self) -> LimitsView {
        self.shared.store.settings().into()
    }

    pub fn settings_set(&self, limits: LimitsView) -> Result<LimitsView, WbError> {
        self.shared
            .store
            .save_settings(limits.clamped())
            .map(LimitsView::from)
            .map_err(|e| WbError::new(WB_UNAVAILABLE, e))
    }

    /// The sheet's first view of a pane.
    pub fn overview(&self, pane_id: &str) -> Result<Overview, WbError> {
        let pane = self.io().pane(pane_id).ok_or_else(|| WbError::new(WB_UNAVAILABLE, words::pane_gone()))?;
        let project = PathBuf::from(&pane.cwd);
        let tests = self.declared_views(&pane.cwd);
        let suggestions = if tests.is_empty() { self.suggestion_views(&project) } else { Vec::new() };
        let mut why_not = self.unavailable(&project).err().map(|e| e.message);
        let mut points = Vec::new();
        if why_not.is_none() {
            match self.io().list_all(&project) {
                Ok(all) => points = self.point_views(&project, &all),
                Err(e) => why_not = Some(e.to_string()),
            }
            if why_not.is_none() && points.is_empty() {
                why_not = Some(words::too_few().to_string());
            }
        }
        let busy = self.busy_text();
        let can = why_not.is_none() && busy.is_none();
        if busy.is_some() && why_not.is_none() {
            why_not = busy.clone();
        }
        Ok(Overview {
            tests,
            suggestions,
            points,
            limits: self.shared.store.settings().into(),
            can,
            why_not: if can { None } else { why_not },
            busy,
        })
    }

    /// Save a test command the person typed or picked (the same list Night Shift uses). The command is
    /// split into words and checked by `parse_command`; shell operators are refused with its message.
    pub fn declare(&self, pane_id: &str, name: &str, command_text: &str, timeout_min: u32) -> Result<DeclaredView, WbError> {
        let pane = self.io().pane(pane_id).ok_or_else(|| WbError::new(WB_UNAVAILABLE, words::pane_gone()))?;
        let argv = checks::parse_command(command_text).map_err(|m| WbError::new(WB_NO_TEST, m))?;
        let project = self.io().check_projects(&pane.cwd).last().cloned().unwrap_or_else(|| pane.cwd.clone());
        let mut list = self.io().declared(&project);
        let entry = Declared { name: name.trim().to_string(), argv: argv.clone(), timeout_min, declared_ms: 0 };
        match list.iter().position(|d| d.argv == argv) {
            Some(i) => list[i] = Declared { declared_ms: list[i].declared_ms, ..entry },
            None => list.push(entry),
        }
        self.io().declare(&project, list).map_err(|m| WbError::new(WB_NO_TEST, m))?;
        let saved = self.io().declared(&project);
        saved
            .iter()
            .position(|d| d.argv == argv)
            .map(|i| declared_view(i, &saved[i], &project))
            .ok_or_else(|| WbError::new(WB_NO_TEST, words::no_test()))
    }

    /// Start a search. Checks every input, then returns the first `JobView` at once; the work runs on a thread.
    pub fn start(
        &self,
        pane_id: &str,
        check_index: usize,
        good_checkpoint_id: Option<String>,
        limits: Option<LimitsView>,
        continue_from: Option<String>,
    ) -> Result<JobView, WbError> {
        let pane = self.io().pane(pane_id).ok_or_else(|| WbError::new(WB_UNAVAILABLE, words::pane_gone()))?;
        let project_dir = PathBuf::from(&pane.cwd);
        self.unavailable(&project_dir)?;
        let declared = self.declared_list(&pane.cwd);
        if declared.is_empty() {
            return Err(WbError::new(WB_NO_TEST, words::no_test()));
        }
        let check = declared.get(check_index).cloned().ok_or_else(|| WbError::new(WB_NO_TEST, words::bad_check()))?;
        let limits = limits.map(LimitsView::clamped).unwrap_or_else(|| self.shared.store.settings());
        let project_key = crate::project_path::comparable_project_path(&project_dir);
        let resume = match continue_from {
            Some(id) => {
                let finding = self.finding_by_id(&id)?;
                if finding.project != project_key || !matches!(finding.outcome, Outcome::NotSure { .. }) {
                    return Err(WbError::new(WB_BAD_ID, words::bad_id()));
                }
                Some(finding)
            }
            None => None,
        };
        let picked = match good_checkpoint_id {
            Some(raw) => Some(self.parse_picked(&project_dir, &raw)?),
            None => None,
        };
        let req = Req {
            finding_id: store::new_id(),
            pane_key: pane_key(pane_id),
            project_dir,
            project_key,
            folder: pane.cwd.clone(),
            check,
            limits,
            picked,
            resume,
            recheck: false,
        };
        self.launch(req)
    }

    /// One run on the folder as it is now, after an undo: do the tests pass?
    pub fn recheck(&self, finding_id: &str) -> Result<JobView, WbError> {
        let finding = self.finding_by_id(finding_id)?;
        if !matches!(finding.outcome, Outcome::Culprit { .. }) {
            return Err(WbError::new(WB_NOT_UNDOABLE, words::not_undoable()));
        }
        let project_dir = PathBuf::from(&finding.folder);
        self.unavailable(&project_dir)?;
        // Only a test the person still declares can run, by the same argv.
        let check = self
            .declared_list(&finding.folder)
            .into_iter()
            .find(|d| d.argv == finding.check_argv)
            .ok_or_else(|| WbError::new(WB_NO_TEST, words::no_test()))?;
        let mut limits = self.shared.store.settings();
        limits.max_runs = limits.max_runs.max(2);
        let req = Req {
            finding_id: store::new_id(),
            pane_key: finding.pane_key.clone(),
            project_dir,
            project_key: finding.project.clone(),
            folder: finding.folder.clone(),
            check,
            limits,
            picked: None,
            resume: None,
            recheck: true,
        };
        self.launch(req)
    }

    pub fn status(&self, job_id: &str) -> Result<JobView, WbError> {
        let job = self.shared.jobs.lock().map.get(job_id).cloned();
        job.map(|job| job.view(self.io().now_ms())).ok_or_else(|| WbError::new(WB_BAD_ID, words::bad_id()))
    }

    /// Ask a running job to stop. The test's process tree is killed and the job ends with "Stopped".
    pub fn stop(&self, job_id: &str) -> Result<(), WbError> {
        let job = self.shared.jobs.lock().map.get(job_id).cloned();
        match job {
            Some(job) => {
                job.cancel.store(true, Ordering::SeqCst);
                Ok(())
            }
            None => Err(WbError::new(WB_BAD_ID, words::bad_id())),
        }
    }

    pub fn finding(&self, finding_id: &str) -> Result<FindingView, WbError> {
        let finding = self.finding_by_id(finding_id)?;
        Ok(self.view_of(&finding))
    }

    /// The folder's findings, newest first. The folder is the pane's folder.
    pub fn findings(&self, pane_id: &str) -> Result<Vec<FindingView>, WbError> {
        let pane = self.io().pane(pane_id).ok_or_else(|| WbError::new(WB_UNAVAILABLE, words::pane_gone()))?;
        let key = crate::project_path::comparable_project_path(Path::new(&pane.cwd));
        Ok(self.shared.store.findings().iter().filter(|f| f.project == key).map(|f| self.view_of(f)).collect())
    }

    /// The files the culprit turn changed and, with `path`, one file's diff (text only, cut at 100 KiB).
    pub fn diff(&self, finding_id: &str, path: Option<&str>) -> Result<DiffView, WbError> {
        let (finding, span) = self.culprit_span(finding_id)?;
        let project = PathBuf::from(&finding.folder);
        let (Some(before), Some(after)) = (span.before_tree.as_deref(), span.after_tree.as_deref()) else {
            return Err(WbError::new(WB_UNDO_GONE, words::undo_gone()));
        };
        if !engine::tree_exists(&project, before) || !engine::tree_exists(&project, after) {
            return Err(WbError::new(WB_UNDO_GONE, words::undo_gone()));
        }
        let shadow = Shadow::new(&project);
        let files = engine::changes_between(&shadow, before, after)?;
        let Some(path) = path else {
            return Ok(DiffView { files, patch: None, truncated: false });
        };
        // S11: only a path in the turn's own list reaches git.
        if !files.iter().any(|f| f.path == path) {
            return Err(WbError::new(WB_BAD_ID, words::bad_id()));
        }
        let (patch, truncated) = engine::patch_between(&project, before, after, path, 100 * 1024)?;
        Ok(DiffView { files, patch: Some(patch), truncated })
    }

    /// The dry run of "Undo only this turn". Writes nothing.
    pub fn undo_plan(&self, finding_id: &str) -> Result<UndoPlanView, WbError> {
        let (finding, span) = self.culprit_span(finding_id)?;
        if finding.undone_ms.is_some() {
            return Err(WbError::new(WB_NOT_UNDOABLE, words::already_undone()));
        }
        let project = PathBuf::from(&finding.folder);
        let plan = self.plan_for(&project, &span)?;
        Ok(UndoPlanView {
            writes: plan.files.iter().filter(|f| matches!(f.op, Op::Write(_))).map(|f| f.path.clone()).collect(),
            removes: plan.files.iter().filter(|f| f.op == Op::Remove).map(|f| f.path.clone()).collect(),
            conflicts: plan.conflicts.iter().map(|c| ConflictView { path: c.path.clone(), why: c.why.clone() }).collect(),
            blocked_by: self.working_in(&project).then(|| words::undo_running().to_string()),
        })
    }

    /// Undo only the culprit turn. Re-plans now (an old plan is never trusted) and writes all or nothing.
    pub fn undo(&self, finding_id: &str) -> Result<UndoReportView, WbError> {
        let (mut finding, span) = self.culprit_span(finding_id)?;
        if finding.undone_ms.is_some() {
            return Err(WbError::new(WB_NOT_UNDOABLE, words::already_undone()));
        }
        let project = PathBuf::from(&finding.folder);
        if self.working_in(&project) {
            return Err(WbError::new(WB_UNDO_RUNNING, words::undo_running()));
        }
        let plan = self.plan_for(&project, &span)?;
        if !plan.conflicts.is_empty() {
            let paths: Vec<String> = plan.conflicts.iter().map(|c| c.path.clone()).collect();
            return Err(WbError::new(WB_UNDO_CONFLICT, words::undo_refused(&paths)));
        }
        let now = self.io().now_ms();
        let report = revert::apply(&project, &plan, now, &engine::Limits::default()).map_err(|e| match e {
            RevertError::Refused(conflicts) => {
                let paths: Vec<String> = conflicts.iter().map(|c| c.path.clone()).collect();
                WbError::new(WB_UNDO_CONFLICT, words::undo_refused(&paths))
            }
            RevertError::Changed(path) => WbError::new(WB_UNDO_CONFLICT, words::undo_changed(&path)),
            RevertError::NoSafetyPoint(m) | RevertError::Write(m) => WbError::new(WB_UNAVAILABLE, m),
        })?;
        finding.undone_ms = Some(now);
        self.shared.store.put(&finding).map_err(|m| WbError::new(WB_UNAVAILABLE, m))?;
        if let Outcome::Culprit { suspect, .. } = &finding.outcome {
            let folder = folder_name(&finding.folder);
            // S10: the audit line holds the folder's last name, never its path.
            self.io().audit_undo(
                &words::undo_audit_summary(suspect.turn, &suspect.agent_label, &folder),
                serde_json::json!({ "turn": suspect.turn, "files": report.written + report.removed }),
            );
        }
        Ok(UndoReportView { undone_at_ms: now, written: report.written, removed: report.removed })
    }

    /// Wait for the job thread to end (tests).
    #[cfg(test)]
    pub(super) fn join(&self, job_id: &str) {
        let job = self.shared.jobs.lock().map.get(job_id).cloned();
        if let Some(job) = job {
            let handle = job.handle.lock().take();
            if let Some(handle) = handle {
                let _ = handle.join();
            }
        }
    }

    #[cfg(test)]
    pub(super) fn store(&self) -> &Store {
        &self.shared.store
    }

    fn launch(&self, req: Req) -> Result<JobView, WbError> {
        let name = folder_name(&req.folder);
        let now = self.io().now_ms();
        let job = Arc::new(Job {
            id: store::new_id(),
            folder_name: name.clone(),
            cancel: AtomicBool::new(false),
            info: Mutex::new(JobInfo {
                state: JobState::Preparing,
                point_no: 0,
                points: 0,
                run: 0,
                max_runs: req.limits.max_runs,
                started_ms: now,
                max_ms: u64::from(req.limits.max_minutes) * 60_000,
                ended_ms: None,
                line: words::getting_ready().to_string(),
                finding_id: None,
            }),
            handle: Mutex::new(None),
        });
        {
            let mut jobs = self.shared.jobs.lock();
            if let Some(running) = &jobs.busy {
                return Err(WbError::new(WB_BUSY, words::busy(&running.folder_name)));
            }
            jobs.busy = Some(job.clone());
            jobs.map.insert(job.id.clone(), job.clone());
            jobs.order.push(job.id.clone());
            while jobs.order.len() > KEEP_JOBS {
                let old = jobs.order.remove(0);
                jobs.map.remove(&old);
            }
        }
        let guard = BusyGuard { shared: self.shared.clone(), id: job.id.clone() };
        let shared = self.shared.clone();
        let thread_job = job.clone();
        let spawned = std::thread::Builder::new().name("what-broke-it".into()).spawn(move || {
            super::job::run(&shared, &thread_job, req, guard);
        });
        match spawned {
            Ok(handle) => *job.handle.lock() = Some(handle),
            Err(e) => {
                // The guard moved into the failed closure and was dropped with it: the slot is free again.
                return Err(WbError::new(WB_UNAVAILABLE, e.to_string()));
            }
        }
        Ok(job.view(now))
    }

    /// Checkpoints on, git present, the folder exists.
    fn unavailable(&self, project: &Path) -> Result<(), WbError> {
        if !self.io().checkpoints_enabled() {
            return Err(WbError::new(WB_UNAVAILABLE, CheckpointError::Disabled.to_string()));
        }
        if !self.io().git_available() {
            return Err(WbError::new(WB_UNAVAILABLE, CheckpointError::GitMissing.to_string()));
        }
        if !project.is_dir() {
            return Err(WbError::new(WB_UNAVAILABLE, "The project folder doesn't exist."));
        }
        Ok(())
    }

    fn busy_text(&self) -> Option<String> {
        self.shared.jobs.lock().busy.as_ref().map(|job| words::busy(&job.folder_name))
    }

    /// The declared tests that apply to this folder: the first project in `check_projects` that has any.
    fn declared_list(&self, cwd: &str) -> Vec<Declared> {
        self.io().check_projects(cwd).iter().map(|p| self.io().declared(p)).find(|l| !l.is_empty()).unwrap_or_default()
    }

    fn declared_views(&self, cwd: &str) -> Vec<DeclaredView> {
        let project = self.io().check_projects(cwd).into_iter().find(|p| !self.io().declared(p).is_empty()).unwrap_or_default();
        self.declared_list(cwd).iter().enumerate().map(|(i, d)| declared_view(i, d, &project)).collect()
    }

    fn suggestion_views(&self, project: &Path) -> Vec<SuggestionView> {
        self.io()
            .suggestions(project)
            .into_iter()
            .map(|s| SuggestionView {
                command_text: crate::worktrees::preset::display(&s.argv),
                name: s.name,
                source: s.source,
            })
            .collect()
    }

    /// The `after` save points of the folder, newest first, at most 100, with `agent · turn n · time` labels.
    fn point_views(&self, project: &Path, all: &[Checkpoint]) -> Vec<PointView> {
        let summaries = self.io().summaries(project);
        let mut after: Vec<&Checkpoint> = all.iter().filter(|c| c.kind == "after" && !c.pane_id.starts_with("wb-")).collect();
        after.sort_by(|a, b| (b.at_ms, &b.pane_id).cmp(&(a.at_ms, &a.pane_id)));
        after.truncate(super::model::MAX_POINTS);
        after
            .into_iter()
            .map(|c| {
                let agent = summaries
                    .iter()
                    .find(|s| s.turn == c.turn && pane_key(&s.pane_id) == c.pane_id)
                    .map(|s| s.agent.clone())
                    .unwrap_or_else(|| words::UNKNOWN_AGENT.to_string());
                PointView {
                    checkpoint_id: format!("{}:{}", c.pane_id, c.id),
                    label: format!("{agent} · turn {} · {}", c.turn, views::clock(c.at_ms)),
                    at_ms: c.at_ms,
                }
            })
            .collect()
    }

    /// `<pane key>:<id>` from the list, checked against the folder's real save points.
    fn parse_picked(&self, project: &Path, raw: &str) -> Result<(String, String), WbError> {
        let bad = || WbError::new(WB_BAD_ID, words::bad_point());
        let (pane, id) = raw.split_once(':').ok_or_else(bad)?;
        let all = self.io().list_all(project)?;
        all.iter()
            .any(|c| c.kind == "after" && c.pane_id == pane && c.id == id && !pane.starts_with("wb-"))
            .then(|| (pane.to_string(), id.to_string()))
            .ok_or_else(bad)
    }

    fn finding_by_id(&self, id: &str) -> Result<Finding, WbError> {
        if !store::valid_id(id) {
            return Err(WbError::new(WB_BAD_ID, words::bad_id()));
        }
        self.shared.store.get(id).ok_or_else(|| WbError::new(WB_BAD_ID, words::bad_id()))
    }

    fn view_of(&self, finding: &Finding) -> FindingView {
        let no_setup = matches!(finding.outcome, Outcome::AlwaysRed { .. }) && self.io().preset(&finding.folder).is_empty();
        views::finding_view(finding, no_setup)
    }

    /// The finding (a culprit) and the saved turn it points at.
    fn culprit_span(&self, finding_id: &str) -> Result<(Finding, super::model::Span), WbError> {
        let finding = self.finding_by_id(finding_id)?;
        let Outcome::Culprit { suspect, .. } = &finding.outcome else {
            return Err(WbError::new(WB_NOT_UNDOABLE, words::not_undoable()));
        };
        let project = PathBuf::from(&finding.folder);
        let all = self.io().list_all(&project)?;
        let span = points::build(&all, "", 0)
            .spans
            .into_iter()
            .find(|s| s.pane_key == suspect.pane_key && s.turn == suspect.turn)
            .ok_or_else(|| WbError::new(WB_UNDO_GONE, words::undo_gone()))?;
        Ok((finding, span))
    }

    fn plan_for(&self, project: &Path, span: &super::model::Span) -> Result<revert::RevertPlan, WbError> {
        let gone = || WbError::new(WB_UNDO_GONE, words::undo_gone());
        let (before, after) = match (span.before_tree.as_deref(), span.after_tree.as_deref()) {
            (Some(b), Some(a)) => (b, a),
            _ => return Err(gone()),
        };
        if !engine::tree_exists(project, before) || !engine::tree_exists(project, after) {
            return Err(gone());
        }
        Ok(revert::plan(&Shadow::new(project), before, after, project)?)
    }

    /// Any agent in the folder is working (status `running` or an open run).
    fn working_in(&self, project: &Path) -> bool {
        self.io()
            .panes_in_folder(project)
            .iter()
            .any(|(id, facts)| facts.status == "running" || self.io().run_open(id, facts))
    }
}

/// The last name of a folder path (what the audit line and the busy text show).
pub(super) fn folder_name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "this folder".to_string())
}

fn declared_view(index: usize, d: &Declared, project: &str) -> DeclaredView {
    DeclaredView {
        index,
        name: d.name.clone(),
        command_text: crate::worktrees::preset::display(&d.argv),
        timeout_min: d.timeout_min,
        warnings: checks::warnings(Path::new(project), d),
    }
}

/// The app's own implementation of `LiveIo`.
#[cfg(not(test))]
pub struct RealIo;

#[cfg(not(test))]
struct RealCopy(super::copy::Copy);

#[cfg(not(test))]
impl CopyHandle for RealCopy {
    fn set_tree(&mut self, tree: &str) -> Result<(), CopyError> {
        self.0.set_tree(tree).map(|_| ())
    }
    fn tree_bytes(&self, tree: &str) -> Result<u64, CopyError> {
        self.0.tree_bytes(tree)
    }
    fn prepare(&mut self, project: &Path, preset: &WorktreePreset) -> Result<(), CopyError> {
        let log = self.0.setup_log();
        self.0.prepare(project, preset, &log).map(|_| ())
    }
    fn folder(&self) -> PathBuf {
        self.0.folder().to_path_buf()
    }
    fn finish(self: Box<Self>) -> Result<(), CopyError> {
        self.0.finish()
    }
}

#[cfg(not(test))]
impl LiveIo for RealIo {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
    fn pane(&self, pane_id: &str) -> Option<PaneFacts> {
        crate::bridge::checkpoint_pane_facts(pane_id)
    }
    fn panes_in_folder(&self, folder: &Path) -> Vec<(String, PaneFacts)> {
        let Some(registry) = crate::bridge::live_registry() else { return Vec::new() };
        let key = crate::project_path::comparable_project_path(folder);
        let guard = registry.lock();
        guard
            .panes
            .iter()
            .filter(|(_, pane)| crate::project_path::comparable_project_path(Path::new(&pane.info.cwd)) == key)
            .map(|(id, pane)| (id.clone(), crate::bridge::pane_facts(pane)))
            .collect()
    }
    fn run_open(&self, pane_id: &str, pane: &PaneFacts) -> bool {
        crate::bridge::pane_has_open_run(pane_id, &pane.cwd)
    }
    fn checkpoints_enabled(&self) -> bool {
        crate::bridge::checkpoint_public_settings()
            .as_ref()
            .and_then(|v| v.get("checkpoints_enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }
    fn git_available(&self) -> bool {
        crate::checkpoints::git_available()
    }
    fn check_projects(&self, cwd: &str) -> Vec<String> {
        let mut out = vec![cwd.to_string()];
        if let Some(origin) = crate::worktrees::origin_of_cwd(cwd) {
            out.push(origin);
        }
        out
    }
    fn declared(&self, project: &str) -> Vec<Declared> {
        checks::declared(&crate::night_shift::store::Store::live(), project)
    }
    fn declare(&self, project: &str, list: Vec<Declared>) -> Result<(), String> {
        checks::declare(&crate::night_shift::store::Store::live(), project, list)
    }
    fn suggestions(&self, project: &Path) -> Vec<Suggestion> {
        checks::suggestions(project)
    }
    fn preset(&self, cwd: &str) -> WorktreePreset {
        let origin = crate::worktrees::origin_of_cwd(cwd);
        // A worktree keeps its setup steps in the origin project.
        origin
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(cwd))
            .filter_map(|p| crate::worktrees::preset::load(Path::new(p)).ok())
            .find(|p| !p.is_empty())
            .unwrap_or_default()
    }
    fn summaries(&self, project: &Path) -> Vec<TurnSummary> {
        crate::turn_summary::store::read_project(project)
    }
    fn list_all(&self, project: &Path) -> Result<Vec<Checkpoint>, CheckpointError> {
        engine::list_all(project)
    }
    fn snapshot_now(&self, project: &Path, key: &str, now_ms: u64) -> Result<String, CheckpointError> {
        let made = engine::snapshot(project, key, engine::Kind::After, Some("What broke it: now"), now_ms, &engine::Limits::default())?;
        match made {
            engine::Snapshot::Created(cp) | engine::Snapshot::Unchanged(Some(cp)) => Ok(cp.tree),
            engine::Snapshot::Unchanged(None) => Err(CheckpointError::NotFound),
        }
    }
    fn new_copy(&self, project: &Path, copies_root: &Path) -> Result<Box<dyn CopyHandle>, CopyError> {
        let shadow = Shadow::new(project);
        super::copy::Copy::create(project, &shadow.git_dir, copies_root).map(|c| Box::new(RealCopy(c)) as Box<dyn CopyHandle>)
    }
    fn run_test(&self, copy: &dyn CopyHandle, argv: &[String], timeout_ms: u64, cancel: &AtomicBool) -> RunResult {
        let spec = super::exec::RunSpec { cwd: copy.folder(), argv: argv.to_vec(), timeout_ms, env_drop_prefixes: Vec::new() };
        super::exec::run_test(&spec, cancel)
    }
    fn redact(&self, text: &str) -> String {
        crate::session_chat::export_text::cut_paths(text, &crate::redact::known::context())
    }
    fn emit(&self, job_id: &str, finding_id: Option<&str>) {
        if let Some(app) = crate::bridge::live_app() {
            use tauri::Emitter;
            let _ = app.emit("what-broke-it://changed", serde_json::json!({ "jobId": job_id, "findingId": finding_id }));
        }
    }
    fn audit_undo(&self, summary: &str, detail: serde_json::Value) {
        crate::audit::record(crate::audit::Kind::WhatBrokeIt, crate::audit::Who::Local, summary, detail);
    }
}

/// The app's one `Live`, on the real folders. Not built in tests, which never touch the app data folder.
#[cfg(not(test))]
pub fn live() -> &'static Live {
    use once_cell::sync::Lazy;
    static LIVE: Lazy<Live> = Lazy::new(|| Live::new(Arc::new(RealIo), Store::live()));
    &LIVE
}
