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
}

/// What an automatic trigger did.
#[derive(Debug, Clone)]
#[allow(dead_code)] // payloads are read by tests and logs
pub enum Outcome {
    Skipped(&'static str),
    Debounced { retry_in_ms: u64 },
    Unchanged,
    Created(Checkpoint),
    Failed(String),
}

#[derive(Default)]
struct State {
    last_status: HashMap<String, String>,
    last_at: HashMap<String, u64>,
}

pub struct Runtime {
    env: Arc<dyn Env>,
    state: Mutex<State>,
}

fn is_turn_end(status: &str) -> bool {
    matches!(status, "idle" | "waiting_input" | "error")
}

impl Runtime {
    pub fn new(env: Arc<dyn Env>) -> Self {
        Self { env, state: Mutex::new(State::default()) }
    }

    fn limits(&self) -> Limits {
        Limits { keep: self.env.settings().keep.clamp(5, 500), ..Limits::default() }
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

    /// Take (or skip) a snapshot for `pane_id`. Blocking.
    pub fn capture(&self, pane_id: &str, kind: Kind, note: Option<&str>) -> Outcome {
        let settings = self.env.settings();
        if !settings.enabled {
            return Outcome::Skipped("off");
        }
        let Some(pane) = self.env.pane(pane_id) else { return Outcome::Skipped("no pane") };
        if !self.env.agent_takes_part(&pane.agent_type) {
            return Outcome::Skipped("agent opted out");
        }
        let now = self.env.now_ms();
        {
            let mut state = self.state.lock();
            if let Some(last) = state.last_at.get(pane_id) {
                let age = now.saturating_sub(*last);
                if age < DEBOUNCE_MS {
                    return Outcome::Debounced { retry_in_ms: DEBOUNCE_MS - age };
                }
            }
            state.last_at.insert(pane_id.to_string(), now);
        }
        match engine::snapshot(&PathBuf::from(&pane.cwd), pane_id, kind, note, now, &self.limits()) {
            Ok(Snapshot::Created(c)) => Outcome::Created(c),
            Ok(Snapshot::Unchanged(_)) => Outcome::Unchanged,
            Err(e) => Outcome::Failed(e.to_string()),
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

    /// Restore files to checkpoint `id`. Refused while the pane is running.
    pub fn restore(&self, pane_id: &str, id: &str) -> Result<RestoreReport, CheckpointError> {
        let (project, pane) = self.project_of(pane_id)?;
        let report = engine::restore(&project, pane_id, id, pane.status == "running", self.env.now_ms(), &self.limits())?;
        // The safety snapshot counts as this pane's latest automatic snapshot.
        self.state.lock().last_at.insert(pane_id.to_string(), self.env.now_ms());
        Ok(report)
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
}

static LIVE: Lazy<Runtime> = Lazy::new(|| Runtime::new(Arc::new(LiveEnv)));

pub fn live() -> &'static Runtime {
    &LIVE
}

/// Hook for `bridge/sse.rs::push_pane_status_sse`. Cheap on the caller's thread; the snapshot
/// itself runs on a short-lived worker thread.
pub fn on_pane_status(pane_id: &str, status: &str) {
    let Some(kind) = live().decide(pane_id, status) else { return };
    let pane_id = pane_id.to_string();
    let spawned = std::thread::Builder::new().name("pm-checkpoint".into()).spawn(move || {
        let outcome = live().capture_with_retry(&pane_id, kind);
        if let Outcome::Failed(message) = outcome {
            tracing::warn!(pane = %pane_id, error = %message, "checkpoint failed");
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start checkpoint thread");
    }
}

/// Hook for `operations/dispatch.rs`: just before the prompt is typed.
pub fn before_dispatch(pane_id: &str, task: &str) {
    if let Outcome::Failed(message) = live().before_dispatch(pane_id, task) {
        tracing::warn!(pane = %pane_id, error = %message, "checkpoint before dispatch failed");
    }
}

/// Hook for `answer_pane_prompt` / `POST /agents/answer`: before the answer is sent.
pub fn before_answer(pane_id: &str) {
    if let Outcome::Failed(message) = live().before_answer(pane_id) {
        tracing::warn!(pane = %pane_id, error = %message, "checkpoint before answer failed");
    }
}
