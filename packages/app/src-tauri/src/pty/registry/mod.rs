//! PaneRegistry — owns the live PTY sessions and their state.

use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tracing::{debug, error, info};
use uuid::Uuid;

use super::agents::{resolve_command, AgentType};
use super::ansi::strip_ansi;
use super::scrollback::Scrollback;
use super::status::PaneStatus;
use crate::events::{PaneId, SystemEvent};

mod command;
pub(crate) mod idle;
mod probe;
pub(crate) use probe::{spawn_probe, ProbePty};
mod pane_io;
pub(crate) use pane_io::snapshot_text;
mod spawn;

pub use command::*;
pub use pane_io::*;
pub use spawn::*;

const SCROLLBACK_CAP: usize = 10_000;
/// Default quiet window before a running pane reads idle; a profile may shorten it (`TuiProfile::idle_after`).
pub(crate) const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(5);

/// Recorder tap (SPEC REC-1): hands the recorder a read-only copy of a visible screen change. It does
/// nothing unless the user started recording this pane, and never for a hidden standby pane (REC-5).
pub(crate) fn recorder_tap(pane_id: &str, status: PaneStatus, screen_text: &str) {
    if !crate::agent_definitions_recorder::snapshots::any_recording() {
        return;
    }
    crate::agent_definitions_recorder::observe(pane_id, super::hidden::is_hidden(pane_id), status, screen_text);
}

/// Default project cwd used when no project path has been set.
fn default_cwd() -> String {
    crate::project_path::default_project_path()
}

/// Public, JSON-serializable pane state returned to the frontend / bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: String,
    pub agent_type: String,
    pub pid: u32,
    pub status: String,
    pub created_at: i64,
    pub last_output_at: Option<i64>,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    /// A phone viewing this pane holds its size (`pty::viewer_size`); desktop windows show a notice
    /// instead of drawing a grid that is not theirs.
    #[serde(default)]
    pub viewer_held: bool,
    /// The agent reprints its whole conversation on every resize (`TuiProfile::reprints_on_resize`):
    /// viewers wipe their scrollback at each resize in the byte stream. Derived from the profile.
    #[serde(default)]
    pub reprints_on_resize: bool,
    /// The account (id, `crate::accounts`) this pane's agent runs under; `None` = the CLI's own login.
    #[serde(default)]
    pub account: Option<String>,
    /// That account's label, for the pane header.
    #[serde(default)]
    pub account_label: Option<String>,
    /// Human title: the user's own, else derived from the first task or typed line
    /// (`ambient::title`). Filled from the title store by `PaneState::info()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Internal handle — owns the PTY master and the reader thread handle.
pub struct PaneState {
    pub info: PaneInfo,
    pub scrollback: Arc<Mutex<Scrollback>>,
    /// Headless terminal emulator mirroring the live screen + scrollback grid.
    /// Lets the frontend render stable plain-text snapshots instead of
    /// replaying chaotic raw terminal streams over IPC.
    pub screen: Arc<Mutex<vt100::Parser>>,
    pub status: Arc<Mutex<PaneStatus>>,
    pub last_output: Arc<Mutex<Instant>>,
    /// Shared so `resize` can drop the registry lock before blocking ConPTY I/O.
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// Shared so `write_input` can drop the registry lock before blocking ConPTY I/O.
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub child: Box<dyn Child + Send + Sync>,
    /// Set when the reader thread has observed EOF or the child exited.
    pub exited: Arc<Mutex<bool>>,
    /// OpenCode API worker metadata (`opencode serve` + session), when applicable.
    pub opencode: Option<crate::opencode::OpenCodeLink>,
    /// Pending key-swap event for wait_for_panes (consumed on wake).
    pub opencode_key_event: Option<crate::opencode::quota::KeySwapEvent>,
}

impl PaneState {
    pub fn info(&self) -> PaneInfo {
        // Refresh derived fields from internal state.
        let mut info = self.info.clone();
        let status = *self.status.lock();
        info.status = status.as_str().to_string();
        let last = *self.last_output.lock();
        // The last_output Instant is set when the pane is created; only
        // report a wall-clock timestamp if we've actually seen output.
        if last.elapsed() < Duration::from_secs(60 * 60 * 24 * 365 * 100) {
            info.last_output_at = Some(unix_ms_from(last));
        } else {
            info.last_output_at = None;
        }
        info.viewer_held = crate::pty::viewer_size::is_held(&info.id);
        info.reprints_on_resize = crate::tui::profile_for(&info.agent_type).reprints_on_resize();
        info.title = crate::ambient::store::get(&info.id);
        info
    }
}

fn unix_ms_from(t: Instant) -> i64 {
    // Instant doesn't carry wall-clock time; we approximate by subtracting
    // the elapsed duration from the current wall-clock millis.
    let now = Instant::now();
    let delta = now.saturating_duration_since(t);
    let now_ms = chrono_now_ms();
    now_ms.saturating_sub(delta.as_millis() as i64)
}

fn chrono_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct PaneRegistry {
    pub panes: HashMap<String, PaneState>,
    pub project_path: String,
}

impl PaneRegistry {
    pub fn new() -> Self {
        Self {
            panes: HashMap::new(),
            project_path: default_cwd(),
        }
    }

    /// The panes any pane list shows: hidden standby workers (`pty::hidden`) are left out.
    pub fn list(&self) -> Vec<PaneInfo> {
        self.panes
            .values()
            .filter(|pane| !super::hidden::is_hidden(&pane.info.id))
            .map(|p| p.info())
            .collect()
    }

    /// Every pane including hidden ones; only the standby pool needs this.
    pub fn list_all(&self) -> Vec<PaneInfo> {
        self.panes.values().map(|p| p.info()).collect()
    }

    pub fn get(&self, id: &str) -> Option<&PaneState> {
        self.panes.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut PaneState> {
        self.panes.get_mut(id)
    }

    pub fn take(&mut self, id: &str) -> Option<PaneState> {
        super::hidden::forget(id);
        super::resize_clock::forget(id);
        crate::flight::closed(id);
        self.panes.remove(id)
    }

    pub fn take_all(&mut self) -> Vec<PaneState> {
        for id in self.panes.keys() {
            super::hidden::forget(id);
            super::resize_clock::forget(id);
            crate::flight::closed(id);
        }
        std::mem::take(&mut self.panes).into_values().collect()
    }
}

impl Default for PaneRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn shutdown_pane(mut pane: PaneState) {
    crate::opencode::kill_serve_if_present(&mut pane);
    if let Some(pid) = pane.child.process_id().filter(|pid| *pid > 0) {
        super::proc_tree::kill_process_tree(pid);
    }
    let _ = pane.child.kill();
}

pub fn kill_pane(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<(), String> {
    kill_pane_with_reason(registry, pane_id, None)
}

pub fn kill_pane_with_reason(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    let pane = registry.lock().take(pane_id);
    if let Some(pane) = pane {
        shutdown_pane(pane);
    }
    crate::event_log::append_system_event(SystemEvent::PaneKilled {
        pane_id: PaneId(pane_id.to_string()),
        reason: reason.map(str::to_string),
    });
    crate::pane_windows::notify_pane_removed();
    Ok(())
}

pub fn kill_all(registry: &Mutex<PaneRegistry>) {
    let panes = registry.lock().take_all();
    for pane in panes {
        let pane_id = pane.info.id.clone();
        shutdown_pane(pane);
        crate::event_log::append_system_event(SystemEvent::PaneKilled {
            pane_id: PaneId(pane_id),
            reason: Some("kill_all".into()),
        });
    }
    crate::pane_windows::notify_pane_removed();
}

pub fn set_project_path(registry: &Mutex<PaneRegistry>, path: String) {
    if let Ok(normalized) = crate::project_path::normalize_project_path(std::path::Path::new(&path))
    {
        registry.lock().project_path = normalized.to_string_lossy().into_owned();
    }
}

pub fn get_project_path(registry: &Mutex<PaneRegistry>) -> String {
    let mut guard = registry.lock();
    if !crate::project_path::is_valid_project_path(std::path::Path::new(&guard.project_path)) {
        guard.project_path = default_cwd();
    }
    guard.project_path.clone()
}

#[cfg(test)]
impl PaneRegistry {
    pub fn test_pane_stub(id: &str) -> PaneState {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 3,
                cols: 10,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let mut cmd = CommandBuilder::new(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            cmd.args(["/C", "exit"]);
        } else {
            cmd.args(["-c", "exit"]);
        }
        let child = pair.slave.spawn_command(cmd).expect("spawn_command");
        drop(pair.slave);
        let writer = pair.master.take_writer().expect("writer");
        let info = PaneInfo {
            id: id.to_string(),
            agent_type: "opencode_native".into(),
            pid: child.process_id().unwrap_or(0),
            status: "running".into(),
            created_at: chrono_now_ms(),
            last_output_at: None,
            cwd: ".".into(),
            cols: 10,
            rows: 3,
            viewer_held: false,
            reprints_on_resize: false,
            account: None,
            account_label: None,
            title: None,
        };
        PaneState {
            info,
            scrollback: Arc::new(Mutex::new(Scrollback::new(100))),
            screen: Arc::new(Mutex::new(vt100::Parser::new(3, 10, 100))),
            status: Arc::new(Mutex::new(PaneStatus::Running)),
            last_output: Arc::new(Mutex::new(Instant::now())),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            child,
            exited: Arc::new(Mutex::new(false)),
            opencode: None,
            opencode_key_event: None,
        }
    }
}

#[cfg(test)]
mod codex_live_tests;
#[cfg(test)]
mod host_shell_tests;
#[cfg(test)]
mod live_permit_tests;
#[cfg(test)]
mod tests;
