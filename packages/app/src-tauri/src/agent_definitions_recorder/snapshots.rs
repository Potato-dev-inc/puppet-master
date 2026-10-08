//! Per-pane recording sessions: sanitized screen snapshots taken on idle/working/prompt transitions.
//!
//! Off unless the user starts it (REC-5); never on a standby-pool pane (hidden panes); raw text is
//! redacted before it is stored, and captures stay in memory until the app closes (SPEC 11 Q8).

use super::redact::{redact, RedactContext};
use crate::pty::status::PaneStatus;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_PER_KIND: usize = 8;
const WORKING_EVERY_MS: i64 = 1500;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapKind {
    /// Ready for input before any work started.
    Idle,
    Working,
    /// Waiting on a dialog or question.
    Prompt,
    /// Idle again after a working stretch: the finished reply is on screen.
    Reply,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub kind: SnapKind,
    pub text: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub pane_id: String,
    pub agent_type: String,
    pub started_ms: i64,
    pub stopped_ms: Option<i64>,
    pub snapshots: Vec<Snapshot>,
    #[serde(skip)]
    last_kind: Option<SnapKind>,
    #[serde(skip)]
    saw_working: bool,
    #[serde(skip)]
    last_working_ms: i64,
}

impl Session {
    pub(super) fn new(pane_id: &str, agent_type: &str, now: i64) -> Self {
        Session {
            pane_id: pane_id.to_string(),
            agent_type: agent_type.to_string(),
            started_ms: now,
            stopped_ms: None,
            snapshots: Vec::new(),
            last_kind: None,
            saw_working: false,
            last_working_ms: 0,
        }
    }

    pub fn of_kind(&self, kind: SnapKind) -> Vec<&str> {
        self.snapshots.iter().filter(|s| s.kind == kind).map(|s| s.text.as_str()).collect()
    }
}

#[derive(Default)]
struct Store {
    active: HashMap<String, Session>,
    /// Stopped sessions, kept in memory until the app closes so a draft can be generated from them.
    finished: HashMap<String, Session>,
}

static STORE: Lazy<Mutex<Store>> = Lazy::new(|| Mutex::new(Store::default()));
static ACTIVE_COUNT: AtomicUsize = AtomicUsize::new(0);
static CTX: Lazy<RedactContext> = Lazy::new(RedactContext::from_process);

/// Cheap check for the reader thread: is any pane being recorded at all?
pub fn any_recording() -> bool {
    ACTIVE_COUNT.load(Ordering::Relaxed) > 0
}

pub fn is_recording(pane_id: &str) -> bool {
    any_recording() && STORE.lock().active.contains_key(pane_id)
}

/// Start recording `pane_id`. Refused for a hidden (standby-pool) pane. `initial` is the current
/// status and screen so a pane that is already idle contributes its idle snapshot.
pub fn start(pane_id: &str, agent_type: &str, hidden: bool, initial: Option<(PaneStatus, &str)>) -> Result<(), String> {
    if hidden {
        return Err("RECORDER_STANDBY_PANE: standby-pool panes are never recorded".into());
    }
    let mut store = STORE.lock();
    if store.active.contains_key(pane_id) {
        return Err("RECORDER_ALREADY_RECORDING: this pane is already being recorded".into());
    }
    store.finished.remove(pane_id);
    let now = now_ms();
    let mut session = Session::new(pane_id, agent_type, now);
    if let Some((status, text)) = initial {
        observe_into(&mut session, status, text, now, &CTX);
    }
    store.active.insert(pane_id.to_string(), session);
    ACTIVE_COUNT.store(store.active.len(), Ordering::Relaxed);
    Ok(())
}

/// Stop recording; the session stays readable until the app closes.
pub fn stop(pane_id: &str) -> Option<Session> {
    let mut store = STORE.lock();
    let mut session = store.active.remove(pane_id)?;
    ACTIVE_COUNT.store(store.active.len(), Ordering::Relaxed);
    session.stopped_ms = Some(now_ms());
    store.finished.insert(pane_id.to_string(), session.clone());
    Some(session)
}

/// The session for `pane_id`, active or stopped.
pub fn session(pane_id: &str) -> Option<Session> {
    let store = STORE.lock();
    store.active.get(pane_id).or_else(|| store.finished.get(pane_id)).cloned()
}

/// Reader-thread tap: a visible screen change on `pane_id` with its classified status. Read-only.
pub fn observe(pane_id: &str, hidden: bool, status: PaneStatus, screen_text: &str) {
    if !any_recording() {
        return;
    }
    let mut store = STORE.lock();
    if hidden {
        // A recorded pane that turned into a standby pane stops being recorded.
        if let Some(mut s) = store.active.remove(pane_id) {
            s.stopped_ms = Some(now_ms());
            store.finished.insert(pane_id.to_string(), s);
            ACTIVE_COUNT.store(store.active.len(), Ordering::Relaxed);
        }
        return;
    }
    if let Some(session) = store.active.get_mut(pane_id) {
        observe_into(session, status, screen_text, now_ms(), &CTX);
    }
}

pub(super) fn observe_into(session: &mut Session, status: PaneStatus, text: &str, now: i64, ctx: &RedactContext) {
    let kind = match status {
        PaneStatus::Running => SnapKind::Working,
        PaneStatus::WaitingInput => SnapKind::Prompt,
        PaneStatus::Idle if session.saw_working => SnapKind::Reply,
        PaneStatus::Idle => SnapKind::Idle,
        PaneStatus::Error => return,
    };
    let transition = session.last_kind != Some(kind);
    let working_refresh = kind == SnapKind::Working && now - session.last_working_ms >= WORKING_EVERY_MS;
    if !(transition || working_refresh) {
        return;
    }
    if kind == SnapKind::Working {
        session.saw_working = true;
        session.last_working_ms = now;
    }
    if kind == SnapKind::Reply {
        session.saw_working = false;
    }
    session.last_kind = Some(kind);
    if session.snapshots.iter().filter(|s| s.kind == kind).count() >= MAX_PER_KIND || text.trim().is_empty() {
        return;
    }
    let mut clipped = text;
    if clipped.len() > MAX_SNAPSHOT_BYTES {
        let mut end = MAX_SNAPSHOT_BYTES;
        while !clipped.is_char_boundary(end) {
            end -= 1;
        }
        clipped = &clipped[..end];
    }
    session.snapshots.push(Snapshot { kind, text: redact(clipped, ctx), at_ms: now });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
