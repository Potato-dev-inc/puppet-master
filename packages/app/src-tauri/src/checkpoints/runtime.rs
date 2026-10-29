//! Turn boundaries: decides *when* to snapshot. Agent-agnostic by construction: it sees only pane
//! status transitions, prompt dispatch and prompt answers, never agent names. The one per-agent
//! knob is `TuiProfile::checkpoints()` (plain shells opt out: they have no turns).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use super::engine::{self, Checkpoint, CheckpointError, FileChange, Kind, Limits, RestoreReport, Snapshot};

/// No more than one automatic snapshot per pane in this window.
pub const DEBOUNCE_MS: u64 = 10_000;

#[derive(Debug, Clone)]
pub struct PaneFacts {
    pub cwd: String,
    pub agent_type: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub enabled: bool,
    pub keep: usize,
}

/// Everything the runtime needs from the outside world (swapped for a fake in tests).
pub trait Env: Send + Sync {
    fn pane(&self, pane_id: &str) -> Option<PaneFacts>;
    fn settings(&self) -> Settings;
    fn now_ms(&self) -> u64;
    /// Whether the agent's profile takes part in checkpoints.
    fn agent_takes_part(&self, agent_type: &str) -> bool;
    /// The pane's own turn number for a snapshot of `kind` taken now (`None` = number by
    /// checkpoint history alone). Keeps History and the turn summaries on one numbering.
    fn turn_hint(&self, _pane_id: &str, _kind: Kind) -> Option<u32> {
        None
    }

    /// Whether an agent run is still open on the pane (only asked when the pane is running).
    fn run_open(&self, _pane_id: &str, _pane: &PaneFacts) -> bool {
        false
    }

    /// New data one snapshot may add before the folder counts as too big (tests lower it).
    fn max_total_bytes(&self) -> u64 {
        Limits::default().max_total_bytes
    }
}

/// What an automatic trigger did.
#[derive(Debug, Clone)]
#[allow(dead_code)] // payloads are read by tests and logs
pub enum Outcome {
    Skipped(&'static str),
    Debounced { retry_in_ms: u64 },
    Unchanged,
    Created(Checkpoint),
    /// The folder has too much new data to snapshot (bytes measured, or remembered from the last
    /// measurement). Reported to the user once, as a card in History, not on every turn.
    TooBig(u64),
    Failed(String),
}

/// A folder whose snapshot was refused for size.
#[derive(Debug, Clone, Copy)]
struct TooBig {
    bytes: u64,
    checked_ms: u64,
}

/// A folder known to be too big is measured again at most this often (walking a multi-gigabyte
/// folder on every turn is what made it slow and noisy). Adding the big folders to `.gitignore`
/// brings Undo back within this time.
pub const TOO_BIG_RECHECK_MS: u64 = 5 * 60_000;

#[derive(Default)]
struct State {
    last_status: HashMap<String, String>,
    last_at: HashMap<String, u64>,
}

pub struct Runtime {
    env: Arc<dyn Env>,
    state: Mutex<State>,
    /// One capture at a time per pane: a capture that arrives while another is writing its snapshot
    /// waits for it. Without this a debounced capture returned at once, "settling" the turn summary
    /// before the snapshot it was waiting for existed (the summary read "0 files").
    capturing: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Project folders whose last snapshot was refused for size.
    too_big: Mutex<HashMap<PathBuf, TooBig>>,
}

fn is_turn_end(status: &str) -> bool {
    matches!(status, "idle" | "waiting_input" | "error")
}

impl Runtime {
    pub fn new(env: Arc<dyn Env>) -> Self {
        Self {
            env,
            state: Mutex::new(State::default()),
            capturing: Mutex::new(HashMap::new()),
            too_big: Mutex::new(HashMap::new()),
        }
    }

    fn limits(&self) -> Limits {
        Limits {
            keep: self.env.settings().keep.clamp(5, 500),
            max_total_bytes: self.env.max_total_bytes(),
            ..Limits::default()
        }
    }

    /// Records the new status and says which snapshot (if any) it calls for.
    pub fn decide(&self, pane_id: &str, status: &str) -> Option<Kind> {
        let prev = self.state.lock().last_status.insert(pane_id.to_string(), status.to_string());
        if prev.as_deref() == Some(status) {
            return None;
        }
        if status == "running" {
            return Some(Kind::Before);
        }
        if prev.as_deref() == Some("running") && is_turn_end(status) {
            return Some(Kind::After);
        }
        None
    }

    /// A prompt was typed into `pane_id`: it is working from now on, whether or not its status has
    /// said so. A pane the run just spawned never read `idle` before its first task, so no
    /// `running` event ever comes and its first `idle` would not read as the end of a turn (no
    /// "after" snapshot). Any pane: after the first prompt it counts as running.
    pub fn note_dispatched(&self, pane_id: &str) {
        self.state.lock().last_status.insert(pane_id.to_string(), "running".to_string());
    }

    /// Take (or skip) a snapshot for `pane_id`. Blocking.
    pub fn capture(&self, pane_id: &str, kind: Kind, note: Option<&str>) -> Outcome {
        match self.capture_inner(pane_id, kind, note, false) {
            Ok(Snapshot::Created(c)) => Outcome::Created(c),
            Ok(Snapshot::Unchanged(_)) => Outcome::Unchanged,
            Err(outcome) => outcome,
        }
    }

    /// Before "Stop all agents" interrupts `pane_id`: a snapshot of its project that ignores the
    /// debounce window (the panic must not lose its checkpoint to a snapshot taken a moment ago).
    /// Returns the checkpoint id; when nothing changed since the newest checkpoint, that one.
    /// `None` when checkpoints are off, the agent opted out, the folder is too big, or git failed.
    pub fn before_stop(&self, pane_id: &str) -> Option<String> {
        match self.capture_inner(pane_id, Kind::Stop, None, true) {
            Ok(Snapshot::Created(c)) => Some(c.id),
            Ok(Snapshot::Unchanged(newest)) => newest.map(|c| c.id),
            Err(Outcome::Failed(message)) => {
                tracing::warn!(pane = %pane_id, error = %message, "checkpoint before stop failed");
                None
            }
            Err(_) => None,
        }
    }

    /// Bytes of new data that made the folder of `pane_id` too big to checkpoint, if it is known
    /// to be (`None` = fine, or never measured).
    pub fn folder_too_big(&self, pane_id: &str) -> Option<u64> {
        let (project, _) = self.project_of(pane_id).ok()?;
        self.too_big.lock().get(&project).map(|known| known.bytes)
    }

    /// Remember that `project` is too big. True the first time (the caller logs it), false while
    /// it stays too big.
    pub(super) fn note_too_big(&self, project: &std::path::Path, bytes: u64, now_ms: u64) -> bool {
        self.too_big.lock().insert(project.to_path_buf(), TooBig { bytes, checked_ms: now_ms }).is_none()
    }

    fn capture_inner(&self, pane_id: &str, kind: Kind, note: Option<&str>, bypass_debounce: bool) -> Result<Snapshot, Outcome> {
        let pane_lock = self.capturing.lock().entry(pane_id.to_string()).or_default().clone();
        let _one_capture_at_a_time = pane_lock.lock();
        let settings = self.env.settings();
        if !settings.enabled {
            return Err(Outcome::Skipped("off"));
        }
        let Some(pane) = self.env.pane(pane_id) else { return Err(Outcome::Skipped("no pane")) };
        if !self.env.agent_takes_part(&pane.agent_type) {
            return Err(Outcome::Skipped("agent opted out"));
        }
        let now = self.env.now_ms();
        let project = PathBuf::from(&pane.cwd);
        if let Some(known) = self.too_big.lock().get(&project) {
            if now.saturating_sub(known.checked_ms) < TOO_BIG_RECHECK_MS {
                return Err(Outcome::TooBig(known.bytes));
            }
        }
        {
            let mut state = self.state.lock();
            if let Some(last) = state.last_at.get(pane_id) {
                let age = now.saturating_sub(*last);
                if age < DEBOUNCE_MS && !bypass_debounce {
                    return Err(Outcome::Debounced { retry_in_ms: DEBOUNCE_MS - age });
                }
            }
            state.last_at.insert(pane_id.to_string(), now);
        }
        let turn = self.env.turn_hint(pane_id, kind);
        match engine::snapshot_numbered(&project, pane_id, kind, note, now, &self.limits(), turn) {
            Ok(snapshot) => {
                if self.too_big.lock().remove(&project).is_some() {
                    tracing::info!(project = %project.display(), "checkpoints are back on for this folder");
                }
                Ok(snapshot)
            }
            Err(CheckpointError::TooLarge(bytes)) => {
                if self.note_too_big(&project, bytes, now) {
                    // Once per folder, not once per turn.
                    tracing::warn!(project = %project.display(), megabytes = bytes / (1024 * 1024), "checkpoints are off for this folder: too much new data");
                }
                Err(Outcome::TooBig(bytes))
            }
            Err(e) => Err(Outcome::Failed(e.to_string())),
        }
    }

    /// Capture for a transition. A turn-end snapshot that lands inside the debounce window is
    /// retried once when the window closes, so the state at the end of a short turn is not lost.
    pub fn capture_with_retry(&self, pane_id: &str, kind: Kind) -> Outcome {
        match self.capture(pane_id, kind, None) {
            Outcome::Debounced { retry_in_ms } if kind == Kind::After => {
                std::thread::sleep(std::time::Duration::from_millis(retry_in_ms + 50));
                // Only if the pane is still at rest: a new turn would own the next snapshot.
                if self.env.pane(pane_id).is_some_and(|p| p.status != "running") {
                    self.capture(pane_id, Kind::After, None)
                } else {
                    Outcome::Skipped("pane running again")
                }
            }
            other => other,
        }
    }

    /// Before a prompt is typed into the pane. Blocking by design: the snapshot must exist
    /// before the agent can touch anything.
    pub fn before_dispatch(&self, pane_id: &str, task: &str) -> Outcome {
        self.capture(pane_id, Kind::Before, Some(task))
    }

    /// Before an answer to a pane prompt is sent.
    pub fn before_answer(&self, pane_id: &str) -> Outcome {
        self.capture(pane_id, Kind::Answer, None)
    }

    fn project_of(&self, pane_id: &str) -> Result<(PathBuf, PaneFacts), CheckpointError> {
        if !self.env.settings().enabled {
            return Err(CheckpointError::Disabled);
        }
        let pane = self.env.pane(pane_id).ok_or_else(|| CheckpointError::Other("That pane is gone.".into()))?;
        Ok((PathBuf::from(&pane.cwd), pane))
    }

    pub fn list(&self, pane_id: &str) -> Result<Vec<Checkpoint>, CheckpointError> {
        let (project, _) = self.project_of(pane_id)?;
        engine::list(&project, pane_id)
    }

    pub fn diff(&self, pane_id: &str, id: &str) -> Result<Vec<FileChange>, CheckpointError> {
        let (project, _) = self.project_of(pane_id)?;
        engine::diff(&project, pane_id, id)
    }

    /// Files the turn that started at `started_at_ms` changed (`None` = no baseline to compare with).
    pub fn turn_changes(&self, pane_id: &str, started_at_ms: u64) -> Result<Option<Vec<FileChange>>, CheckpointError> {
        let (project, _) = self.project_of(pane_id)?;
        engine::turn_changes(&project, pane_id, started_at_ms)
    }

    /// The unified diff of the pane's latest finished turn (`None` = no before/after pair).
    pub fn last_turn_patch(&self, pane_id: &str, max_bytes: usize) -> Result<Option<engine::TurnPatch>, CheckpointError> {
        let (project, _) = self.project_of(pane_id)?;
        engine::last_turn_patch(&project, pane_id, max_bytes)
    }

    /// Restore files to checkpoint `id`. Refused while the pane is running.
    pub fn restore(&self, pane_id: &str, id: &str) -> Result<RestoreReport, CheckpointError> {
        let (project, pane) = self.project_of(pane_id)?;
        if pane.status == "running" && self.env.run_open(pane_id, &pane) {
            return Err(CheckpointError::RunOpen);
        }
        // Read before the files move. Best effort: a target that cannot be found is simply not noted.
        let target = self.list(pane_id).ok().and_then(|items| items.into_iter().find(|c| c.id == id));
        let report = engine::restore(&project, pane_id, id, pane.status == "running", self.env.now_ms(), &self.limits())?;
        // The safety snapshot counts as this pane's latest automatic snapshot.
        self.state.lock().last_at.insert(pane_id.to_string(), self.env.now_ms());
        // Scores counts the turns this Undo put back. Best effort: a log error never breaks Undo.
        if let Some(target) = target {
            crate::scores::undo_log::note_restore(pane_id, &target.kind, target.at_ms, self.env.now_ms());
        }
        Ok(report)
    }

    /// The unified diff from checkpoint `from_id` to checkpoint `to_id` (Review Desk: a range of turns).
    #[allow(dead_code)] // read by Review Desk (its sources and detail)
    pub fn patch_between(&self, pane_id: &str, from_id: &str, to_id: &str, max_bytes: usize) -> Result<Option<engine::TurnPatch>, CheckpointError> {
        let (project, _) = self.project_of(pane_id)?;
        engine::patch_between(&project, pane_id, from_id, to_id, max_bytes)
    }

    /// Files that differ between checkpoint `from_id` and checkpoint `to_id`.
    #[allow(dead_code)] // read by Review Desk (its sources and detail)
    pub fn changes_between_ids(&self, pane_id: &str, from_id: &str, to_id: &str) -> Result<Vec<FileChange>, CheckpointError> {
        let (project, _) = self.project_of(pane_id)?;
        engine::changes_between_ids(&project, pane_id, from_id, to_id)
    }

    /// The id of the checkpoint a turn that started at `started_at_ms` began from (`None` = no
    /// baseline, checkpoints are off, or the pane has none yet).
    #[allow(dead_code)] // read by Review Desk (its sources and detail)
    pub fn before_of_turns(&self, pane_id: &str, started_at_ms: u64) -> Option<String> {
        let items = self.list(pane_id).ok()?;
        engine::before_of(&items, started_at_ms).map(|c| c.id.clone())
    }

    /// The id of the newest "after" checkpoint (the end of the latest finished turn). `None` = checkpoints
    /// are off or the pane has no finished turn yet (Review Desk: the end of a range of turns).
    #[allow(dead_code)] // read by Review Desk (its gather and detail)
    pub fn last_after_id(&self, pane_id: &str) -> Option<String> {
        let items = self.list(pane_id).ok()?;
        engine::latest_after(&items).map(|c| c.id.clone())
    }

    /// Restore to the newest checkpoint taken before the latest turn (for "Undo last turn").
    pub fn undo_last_turn(&self, pane_id: &str) -> Result<RestoreReport, CheckpointError> {
        let items = self.list(pane_id)?;
        let target = last_turn_start(&items).ok_or(CheckpointError::NotFound)?;
        self.restore(pane_id, &target.id)
    }
}

/// The checkpoint that marks the start of the latest turn: the earliest "before" checkpoint of
/// the highest turn number.
pub fn last_turn_start(items: &[Checkpoint]) -> Option<&Checkpoint> {
    let turn = items.iter().filter(|c| c.kind != "undo").map(|c| c.turn).max()?;
    items.iter().find(|c| c.turn == turn && c.kind == "before")
}

struct LiveEnv;

impl Env for LiveEnv {
    fn pane(&self, pane_id: &str) -> Option<PaneFacts> {
        crate::bridge::checkpoint_pane_facts(pane_id)
    }
    fn settings(&self) -> Settings {
        let value = crate::bridge::checkpoint_public_settings();
        let enabled = value.as_ref().and_then(|v| v.get("checkpoints_enabled")).and_then(|v| v.as_bool()).unwrap_or(true);
        let keep = value.as_ref().and_then(|v| v.get("checkpoints_keep")).and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        Settings { enabled, keep }
    }
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    fn agent_takes_part(&self, agent_type: &str) -> bool {
        crate::tui::profile_for(agent_type).checkpoints()
    }
    fn turn_hint(&self, pane_id: &str, kind: Kind) -> Option<u32> {
        crate::turn_summary::checkpoint_turn(pane_id, kind == Kind::Before)
    }
    fn run_open(&self, pane_id: &str, pane: &PaneFacts) -> bool {
        crate::bridge::pane_has_open_run(pane_id, &pane.cwd)
    }
}

static LIVE: Lazy<Runtime> = Lazy::new(|| Runtime::new(Arc::new(LiveEnv)));

pub fn live() -> &'static Runtime {
    &LIVE
}

/// Test seam: the Undo log calls this runtime made on this thread (see `scores::undo_log`). It lives
/// here so the checkpoint tests never need to name the scores module.
#[cfg(test)]
pub(crate) fn take_noted_restores() -> Vec<(String, String, u64, u64)> {
    crate::scores::undo_log::take_noted_restores()
}

/// Name of the worker thread that takes automatic snapshots.
pub(crate) const CHECKPOINT_THREAD: &str = "pm-checkpoint";

/// Hook for `bridge/sse.rs::push_pane_status_sse`. Cheap on the caller's thread (which is the PTY
/// reader thread); the snapshot itself runs on a short-lived worker thread.
pub fn on_pane_status(pane_id: &str, status: &str) {
    on_status_with(live(), pane_id, status);
}

/// [`on_pane_status`] against any runtime (tests). Returns the worker's handle when one started.
pub(crate) fn on_status_with(rt: &'static Runtime, pane_id: &str, status: &str) -> Option<std::thread::JoinHandle<()>> {
    let kind = rt.decide(pane_id, status)?;
    let pane_id = pane_id.to_string();
    // The ticket is taken here, on the status-event thread, so the summary hook that runs right
    // after this one waits for THIS rest point's capture and no earlier one's.
    let ticket = (kind == Kind::After).then(|| crate::turn_summary::checkpoint_requested(&pane_id));
    let spawned = std::thread::Builder::new().name(CHECKPOINT_THREAD.into()).spawn(move || {
        let outcome = rt.capture_with_retry(&pane_id, kind);
        if let Outcome::Failed(message) = outcome {
            tracing::warn!(pane = %pane_id, error = %message, "checkpoint failed");
        }
        // The turn summary reads the checkpoints, so it waits for this capture to settle.
        if let Some(ticket) = ticket {
            crate::turn_summary::checkpoint_settled(&pane_id, ticket);
        }
    });
    match spawned {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::warn!(error = %e, "could not start checkpoint thread");
            None
        }
    }
}

/// Hook for `bridge/dispatch.rs`: the prompt was typed into the pane (see [`Runtime::note_dispatched`]).
pub fn note_dispatched(pane_id: &str) {
    live().note_dispatched(pane_id);
}

/// Hook for `operations/dispatch.rs` (a follow-up on a known pane) and `bridge/dispatch.rs` (any run,
/// including the first one on a pane it just spawned): just before the prompt is typed.
pub fn before_dispatch(pane_id: &str, task: &str) {
    if let Outcome::Failed(message) = live().before_dispatch(pane_id, task) {
        tracing::warn!(pane = %pane_id, error = %message, "checkpoint before dispatch failed");
    }
}

/// Hook for `panic::stop_all`: before a running pane is interrupted. Returns the checkpoint id.
pub fn before_stop(pane_id: &str) -> Option<String> {
    live().before_stop(pane_id)
}

/// Hook for `answer_pane_prompt` / `POST /agents/answer`: before the answer is sent.
pub fn before_answer(pane_id: &str) {
    if let Outcome::Failed(message) = live().before_answer(pane_id) {
        tracing::warn!(pane = %pane_id, error = %message, "checkpoint before answer failed");
    }
}
