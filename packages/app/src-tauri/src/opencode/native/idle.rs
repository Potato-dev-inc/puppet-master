//! When a native pane flips between `running` and `idle`.
//!
//! The reader thread used to set `Running` on every output chunk and nothing ever set `Idle`, so a
//! native pane read "Working" until it was closed (live: 374 s after the turn ended, composer idle,
//! `finish: "stop"` in the messages). Without an `idle` event the turn summary never closed the
//! turn, the phone kept the pane busy and attention never fired.
//!
//! Idle is decided the way a regular pane's is (`pty::registry::idle`): the SCREEN has held still
//! for the profile's `idle_after`. Output that leaves every visible cell alone is not activity.
//! The server adds the structured half: `GET /session/status` lists a session that is still
//! working, so a screen that is merely quiet (a long tool call) does not read idle while the
//! server says busy. When the server cannot be asked, the screen alone decides.
//!
//! Every change goes out as `pty://status` plus a `PaneStatusChanged` event, which is what the
//! cockpit, the phone SSE, push, checkpoints and turn summaries listen to.

use super::*;
use crate::pty::registry::idle::{IdleClock, IDLE_POLL};

/// What the server says about the pane's session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerSession {
    Busy,
    NotBusy,
    /// Not asked, or no usable answer: the screen alone decides.
    Unknown,
}

/// Asks the server whether the pane's session is working. Called off the reader thread.
pub(super) type SessionProbe = Arc<dyn Fn() -> ServerSession + Send + Sync>;

/// Probe the real server at `base_url`.
pub(super) fn server_probe(base_url: String, session_id: String, directory: String) -> SessionProbe {
    Arc::new(move || match client::session_is_busy(&base_url, &session_id, Some(&directory)) {
        Ok(true) => ServerSession::Busy,
        Ok(false) => ServerSession::NotBusy,
        Err(_) => ServerSession::Unknown,
    })
}

/// While the server says busy and the screen is still, ask again this often.
const PROBE_EVERY: Duration = Duration::from_secs(1);

/// The shared state behind one native pane's `running` / `idle` reading.
pub(super) struct NativeStatus {
    status: Arc<Mutex<PaneStatus>>,
    clock: IdleClock,
    last_screen: Mutex<String>,
    next_probe_at: Mutex<Option<Instant>>,
}

impl NativeStatus {
    pub(super) fn new(status: Arc<Mutex<PaneStatus>>, now: Instant) -> Self {
        Self {
            status,
            clock: IdleClock::new(now),
            last_screen: Mutex::new(String::new()),
            next_probe_at: Mutex::new(None),
        }
    }

    /// A chunk was drawn; `screen` is the visible text after it. Returns the new status when the
    /// chunk woke an idle pane. `recently_resized`: the TUI is repainting after our own resize,
    /// which is not work, so an idle pane stays idle.
    pub(super) fn on_output(&self, screen: &str, now: Instant, recently_resized: bool) -> Option<PaneStatus> {
        {
            let mut last = self.last_screen.lock();
            if *last == screen {
                return None;
            }
            *last = screen.to_string();
        }
        self.clock.screen_changed(now);
        *self.next_probe_at.lock() = None;
        let mut status = self.status.lock();
        let observed = crate::pty::status::settle_status_after_resize(
            *status,
            PaneStatus::Running,
            None,
            recently_resized,
        );
        // Only an idle pane is woken here; Error and the like are set by whoever owns them.
        if *status == PaneStatus::Idle && observed == PaneStatus::Running {
            *status = PaneStatus::Running;
            return Some(PaneStatus::Running);
        }
        None
    }

    /// One look of the idle watcher. Returns `Idle` when the pane just went idle.
    pub(super) fn tick(&self, now: Instant, idle_after: Duration, probe: &dyn Fn() -> ServerSession) -> Option<PaneStatus> {
        if *self.status.lock() != PaneStatus::Running || !self.clock.idle_due(now, idle_after) {
            return None;
        }
        if self.next_probe_at.lock().is_some_and(|at| now < at) {
            return None;
        }
        // Outside the status lock: this is an HTTP request.
        if probe() == ServerSession::Busy {
            *self.next_probe_at.lock() = Some(now + PROBE_EVERY);
            return None;
        }
        let mut status = self.status.lock();
        if *status == PaneStatus::Running {
            *status = PaneStatus::Idle;
            return Some(PaneStatus::Idle);
        }
        None
    }
}

/// Tell the app, the phone and the event log that the pane is now `status`.
pub(super) fn publish_status(app: &AppHandle, pane_id: &str, status: PaneStatus) {
    let _ = app.emit(
        "pty://status",
        serde_json::json!({ "pane_id": pane_id, "status": status.as_str() }),
    );
    crate::event_log::append_system_event(SystemEvent::PaneStatusChanged {
        pane_id: PaneId(pane_id.to_string()),
        status: status.as_str().to_string(),
    });
}

/// The idle watcher for one reader generation. Ends with the pane, or when a reattach replaced
/// this generation (the new reader starts its own).
pub(super) fn spawn_idle_watcher(
    pane_id: String,
    app: AppHandle,
    tracker: Arc<NativeStatus>,
    probe: SessionProbe,
    exited: Arc<Mutex<bool>>,
    reader_generation: u64,
    attach_generation: Arc<std::sync::atomic::AtomicU64>,
) {
    let idle_after = crate::tui::profile_for("opencode_native").idle_after();
    let _ = thread::Builder::new()
        .name(format!("native-idle-{pane_id}"))
        .spawn(move || loop {
            thread::sleep(IDLE_POLL);
            if *exited.lock() || attach_generation.load(std::sync::atomic::Ordering::SeqCst) != reader_generation {
                break;
            }
            if let Some(status) = tracker.tick(Instant::now(), idle_after, &*probe) {
                publish_status(&app, &pane_id, status);
            }
        });
}

#[cfg(test)]
mod tests;
