//! The progress of an import: what the screen shows, and the throttled event that carries it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

/// Where an import is. Serialised in lower case (`idle`, `looking`, `reading`, `done`, `cancelled`, `failed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Looking,
    Reading,
    Done,
    Cancelled,
    Failed,
}

impl Phase {
    /// Whether the import is over (the next `start` may run).
    pub fn is_finished(self) -> bool {
        matches!(self, Phase::Done | Phase::Cancelled | Phase::Failed)
    }
}

/// What the screen shows during and after an import. Counts and agent ids only: no path, no chat text.
/// `chats_done` counts every chat looked at (read, unchanged or skipped), so the bar reaches `chats_total`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: Phase,
    pub agent: Option<String>,
    pub chats_done: usize,
    pub chats_total: usize,
    pub items: usize,
    pub unchanged: usize,
    pub too_big: usize,
    pub unreadable: usize,
    pub message: Option<String>,
}

impl Progress {
    pub fn idle() -> Self {
        Progress {
            phase: Phase::Idle,
            agent: None,
            chats_done: 0,
            chats_total: 0,
            items: 0,
            unchanged: 0,
            too_big: 0,
            unreadable: 0,
            message: None,
        }
    }
}

/// Receives the progress (the command layer passes the Tauri emit).
pub type Emitter = Arc<dyn Fn(&Progress) + Send + Sync>;

/// The event is sent at most this often (4 a second), except when the import ends.
pub const EMIT_EVERY: Duration = Duration::from_millis(250);

/// Where the throttle reads the time. Tests pass their own, so "at most four a second" is checked against a
/// clock they move, not against how fast a busy machine happens to run the import.
pub(crate) type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The current progress and the throttled emitter. The worker updates it; anyone may read it.
pub(crate) struct Reporter {
    state: Mutex<(Progress, Option<Instant>)>,
    emit: Emitter,
    clock: Clock,
}

impl Reporter {
    pub(super) fn new(emit: Emitter) -> Self {
        Self::with_clock(emit, Arc::new(Instant::now))
    }

    pub(super) fn with_clock(emit: Emitter, clock: Clock) -> Self {
        Reporter { state: Mutex::new((Progress::idle(), None)), emit, clock }
    }

    pub(super) fn get(&self) -> Progress {
        self.state.lock().0.clone()
    }

    /// Changes the progress; the event goes out when the last one is older than `EMIT_EVERY` or the phase changed
    /// (a phase change is never dropped, so the screen always sees `reading` and the end).
    pub(super) fn update(&self, change: impl FnOnce(&mut Progress)) {
        let snapshot = {
            let mut state = self.state.lock();
            let phase = state.0.phase;
            change(&mut state.0);
            let now = (self.clock)();
            let due = match state.1 {
                Some(last) => now.saturating_duration_since(last) >= EMIT_EVERY,
                None => true,
            };
            if due || state.0.phase != phase {
                state.1 = Some(now);
                Some(state.0.clone())
            } else {
                None
            }
        };
        if let Some(progress) = snapshot {
            (self.emit)(&progress);
        }
    }

    /// Replaces the progress and always sends it (a new run starts from a clean one).
    pub(super) fn reset(&self, progress: Progress) {
        {
            let mut state = self.state.lock();
            *state = (progress.clone(), Some((self.clock)()));
        }
        (self.emit)(&progress);
    }
}
