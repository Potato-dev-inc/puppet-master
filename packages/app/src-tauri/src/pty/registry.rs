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
use super::status::{looks_like_prompt, PaneStatus};

const SCROLLBACK_CAP: usize = 10_000;
const IDLE_AFTER: Duration = Duration::from_secs(5);
const READ_CHUNK_MS: u64 = 30;

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
}

/// Internal handle — owns the PTY master and the reader thread handle.
#[allow(dead_code)]
pub struct PaneState {
    pub info: PaneInfo,
    pub scrollback: Arc<Mutex<Scrollback>>,
    /// Headless terminal emulator mirroring the live screen + scrollback grid.
    /// Lets the frontend render stable plain-text snapshots instead of
    /// replaying chaotic raw terminal streams over IPC.
    pub screen: Arc<Mutex<vt100::Parser>>,
    pub status: Arc<Mutex<PaneStatus>>,
    pub last_output: Arc<Mutex<Instant>>,
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    /// Set when the reader thread has observed EOF or the child exited.
    pub exited: Arc<Mutex<bool>>,
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

#[allow(dead_code)]
impl PaneRegistry {
    pub fn new() -> Self {
        Self {
            panes: HashMap::new(),
            project_path: default_cwd(),
        }
    }

    pub fn list(&self) -> Vec<PaneInfo> {
        self.panes.values().map(|p| p.info()).collect()
    }

    pub fn get(&self, id: &str) -> Option<&PaneState> {
        self.panes.get(id)
    }

    #[allow(dead_code)]
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PaneState> {
        self.panes.get_mut(id)
    }

    pub fn kill(&mut self, id: &str) {
        if let Some(mut pane) = self.panes.remove(id) {
            // Try graceful terminate first.
            let _ = pane.child.kill();
            // Drop the master — closes the PTY.
        }
    }

    pub fn kill_all(&mut self) {
        let ids: Vec<String> = self.panes.keys().cloned().collect();
        for id in ids {
            self.kill(&id);
        }
    }
}

impl Default for PaneRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a CommandBuilder for the given agent, using the given cwd.
/// On Windows, `.cmd`/`.bat` shims must run under `cmd.exe /C`.
fn build_command(agent: AgentType, cwd: &str, extra_args: &[String]) -> CommandBuilder {
    let (cmd, base_args) = resolve_command(agent);

    #[cfg(windows)]
    {
        let is_cmd_shim = cmd.ends_with(".cmd") || cmd.ends_with(".bat");
        let mut cmd_builder = if is_cmd_shim {
            let mut b = CommandBuilder::new("cmd.exe");
            b.arg("/C");
            b.arg(cmd);
            b
        } else {
            CommandBuilder::new(cmd)
        };
        for a in base_args {
            cmd_builder.arg(a);
        }
        for a in extra_args {
            cmd_builder.arg(a);
        }
        cmd_builder.cwd(cwd);
        cmd_builder.env("PATH", crate::shell_env::path_for_spawn());
        #[cfg(windows)]
        cmd_builder.env("Path", crate::shell_env::path_for_spawn());
        cmd_builder.env("TERM", "xterm-256color");
        cmd_builder.env("COLORTERM", "truecolor");
        return cmd_builder;
    }

    #[cfg(not(windows))]
    {
        let mut cmd_builder = CommandBuilder::new(cmd);
        for a in base_args {
            cmd_builder.arg(a);
        }
        for a in extra_args {
            cmd_builder.arg(a);
        }
        cmd_builder.cwd(cwd);
        cmd_builder.env("PATH", crate::shell_env::path_for_spawn());
        #[cfg(windows)]
        cmd_builder.env("Path", crate::shell_env::path_for_spawn());
        cmd_builder.env("TERM", "xterm-256color");
        cmd_builder.env("COLORTERM", "truecolor");
        cmd_builder
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnPaneArgs {
    pub agent_type: String,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub extra_args: Option<Vec<String>>,
    pub pane_id: Option<String>,
}

/// Spawn a pane. The reader thread pushes raw bytes to the scrollback,
/// emits Tauri events, and updates status heuristics.
///
/// Returns the new pane id.
pub fn spawn_pane(
    registry: &Mutex<PaneRegistry>,
    app: &AppHandle,
    args: SpawnPaneArgs,
) -> Result<String, String> {
    let agent = AgentType::parse(&args.agent_type)
        .ok_or_else(|| format!("unknown agent_type: {}", args.agent_type))?;

    let cwd = crate::project_path::resolve_spawn_cwd(
        args.cwd.clone(),
        registry.lock().project_path.clone(),
    )?
    .to_string_lossy()
    .to_string();

    let cols = args.cols.unwrap_or(120);
    let rows = args.rows.unwrap_or(30);
    let pane_id = args.pane_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    {
        let mut reg = registry.lock();
        if reg.panes.contains_key(&pane_id) {
            reg.kill(&pane_id);
        }
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let cmd = build_command(agent, &cwd, args.extra_args.as_deref().unwrap_or(&[]));
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn_command: {e}"))?;
    let pid = child.process_id().unwrap_or(0);

    // Slave handle is now owned by the child; drop our copy so the master
    // gets EOF when the child exits.
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone_reader: {e}"))?;

    let scrollback = Arc::new(Mutex::new(Scrollback::new(SCROLLBACK_CAP)));
    // Mirror the live screen at the PTY's dimensions with a matching scrollback
    // depth so reattach can reconstruct a coherent grid.
    let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK_CAP)));
    let status = Arc::new(Mutex::new(PaneStatus::Running));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let exited = Arc::new(Mutex::new(false));

    let info = PaneInfo {
        id: pane_id.clone(),
        agent_type: args.agent_type.clone(),
        pid,
        status: "running".into(),
        created_at: chrono_now_ms(),
        last_output_at: Some(chrono_now_ms()),
        cwd: cwd.clone(),
        cols,
        rows,
    };

    let pane = PaneState {
        info,
        scrollback: scrollback.clone(),
        screen: screen.clone(),
        status: status.clone(),
        last_output: last_output.clone(),
        master: pair.master,
        writer,
        child,
        exited: exited.clone(),
    };

    // Spawn the reader thread.
    {
        let pane_id = pane_id.clone();
        let app = app.clone();
        let scrollback = scrollback.clone();
        let screen = screen.clone();
        let status = status.clone();
        let last_output = last_output.clone();
        let exited = exited.clone();
        thread::Builder::new()
            .name(format!("pty-reader-{pane_id}"))
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 4096];
                let mut last_snapshot = String::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            debug!(pane = %pane_id, "pty reader EOF");
                            break;
                        }
                        Ok(n) => {
                            // Push RAW BYTES to scrollback — never decode to
                            // String here, as from_utf8_lossy corrupts multi-byte
                            // UTF-8 at chunk boundaries.
                            scrollback.lock().push_chunk(&buf[..n]);
                            // Emit raw bytes for the xterm.js renderer, while
                            // also feeding the headless emulator used by MCP
                            // buffer reads and reattach snapshots.
                            let chunk = buf[..n].to_vec();
                            let _ = app.emit(
                                "terminal-data",
                                TerminalDataEvent {
                                    pane_id: pane_id.clone(),
                                    data: chunk,
                                },
                            );
                            let snapshot = {
                                let mut parser = screen.lock();
                                parser.process(&buf[..n]);
                                parser.screen().contents()
                            };
                            *last_output.lock() = Instant::now();

                            // Status heuristic: if recent output looks like a
                            // prompt, mark waiting_input.
                            let new_status = if looks_like_prompt(&scrollback.lock().tail_text(8)) {
                                PaneStatus::WaitingInput
                            } else {
                                PaneStatus::Running
                            };
                            let changed = {
                                let mut s = status.lock();
                                if *s != new_status {
                                    *s = new_status;
                                    true
                                } else {
                                    false
                                }
                            };

                            if snapshot != last_snapshot {
                                last_snapshot = snapshot.clone();
                                let _ = app.emit(
                                    "terminal-snapshot",
                                    TerminalSnapshotEvent {
                                        pane_id: pane_id.clone(),
                                        snapshot,
                                    },
                                );
                            }

                            if changed {
                                let s = *status.lock();
                                let _ = app.emit(
                                    "pty://status",
                                    PaneStatusEvent {
                                        pane_id: pane_id.clone(),
                                        status: s.as_str().to_string(),
                                    },
                                );
                            }
                        }
                        Err(e) => {
                            // On Windows ConPTY, ERROR_BROKEN_PIPE (after child exit) is expected.
                            if e.kind() == std::io::ErrorKind::BrokenPipe
                                || e.kind() == std::io::ErrorKind::UnexpectedEof
                            {
                                debug!(pane = %pane_id, "pty broken pipe: {e}");
                                break;
                            }
                            // Treat any other read error as fatal for the reader thread.
                            error!(pane = %pane_id, "pty read error: {e}");
                            break;
                        }
                    }
                }
                *exited.lock() = true;
                *status.lock() = PaneStatus::Error;
                let _ = app.emit(
                    "pty://exit",
                    PaneExitEvent {
                        pane_id: pane_id.clone(),
                    },
                );
            })
            .map_err(|e| format!("spawn reader thread: {e}"))?;
    }

    // Idle watcher thread — flips Running -> Idle after IDLE_AFTER with no output.
    {
        let pane_id = pane_id.clone();
        let app = app.clone();
        let last_output = last_output.clone();
        let status = status.clone();
        let exited = exited.clone();
        thread::Builder::new()
            .name(format!("pty-idle-{pane_id}"))
            .spawn(move || loop {
                thread::sleep(Duration::from_millis(READ_CHUNK_MS * 20));
                if *exited.lock() {
                    break;
                }
                let since = last_output.lock().elapsed();
                if since >= IDLE_AFTER {
                    let mut s = status.lock();
                    if *s == PaneStatus::Running {
                        *s = PaneStatus::Idle;
                        let _ = app.emit(
                            "pty://status",
                            PaneStatusEvent {
                                pane_id: pane_id.clone(),
                                status: PaneStatus::Idle.as_str().to_string(),
                            },
                        );
                    }
                }
            })
            .map_err(|e| format!("spawn idle thread: {e}"))?;
    }

    let id = pane.info.id.clone();
    registry.lock().panes.insert(pane.info.id.clone(), pane);
    let _ = app.emit("pty://panes-changed", ());
    info!(pane = %id, agent = %args.agent_type, pid, "pane spawned");
    Ok(id)
}

pub fn write_input(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    text: &str,
    append_newline: bool,
) -> Result<(), String> {
    let mut reg = registry.lock();
    let pane = reg
        .panes
        .get_mut(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;

    let agent = pane.info.agent_type.clone();

    if !text.is_empty() {
        pane.writer
            .write_all(text.as_bytes())
            .map_err(|e| format!("write_all: {e}"))?;
        pane.writer.flush().map_err(|e| format!("flush: {e}"))?;
        // Ink/React TUIs can drop a submitted prompt if the prompt text and
        // Enter arrive in the same instant. Raw terminal input must stay
        // immediate, so only pause for submit-style writes.
        if append_newline {
            thread::sleep(Duration::from_millis(50));
        }
    }

    if append_newline {
        write_enter_bytes(&mut pane.writer, &agent)?;
    }
    Ok(())
}

/// Send Enter to the PTY. Uses \r (xterm/ConPTY).
fn write_enter_bytes(writer: &mut Box<dyn Write + Send>, _agent_type: &str) -> Result<(), String> {
    writer
        .write_all(b"\r")
        .map_err(|e| format!("write enter: {e}"))?;
    writer.flush().map_err(|e| format!("flush enter: {e}"))?;
    Ok(())
}

pub fn read_buffer(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    lines: usize,
) -> Result<String, String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let text = pane.scrollback.lock().tail_text(lines);
    Ok(strip_ansi(&text))
}

pub fn read_snapshot(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<String, String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let snapshot = pane.screen.lock().screen().contents();
    Ok(snapshot)
}

pub fn read_raw_buffer(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    lines: usize,
) -> Result<Vec<u8>, String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let raw = pane.scrollback.lock().tail_raw_bytes(lines);
    Ok(raw)
}

pub fn resize(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    cols: u16,
    rows: u16,
) -> Result<bool, String> {
    let mut reg = registry.lock();
    let pane = reg
        .panes
        .get_mut(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    if pane.info.cols == cols && pane.info.rows == rows {
        return Ok(false);
    }
    pane.master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize: {e}"))?;
    // Keep the headless emulator's grid the same shape as the PTY so a later
    // screen reconstruction matches the live geometry.
    pane.screen.lock().screen_mut().set_size(rows, cols);
    pane.info.cols = cols;
    pane.info.rows = rows;
    Ok(true)
}

pub fn kill_pane(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<(), String> {
    registry.lock().kill(pane_id);
    Ok(())
}

pub fn kill_all(registry: &Mutex<PaneRegistry>) {
    registry.lock().kill_all();
}

#[allow(dead_code)]
pub fn set_project_path(registry: &Mutex<PaneRegistry>, path: String) {
    if let Ok(normalized) =
        crate::project_path::normalize_project_path(std::path::Path::new(&path))
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

#[derive(Debug, Clone, Serialize)]
struct PaneStatusEvent {
    pane_id: String,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct PaneExitEvent {
    pane_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct TerminalSnapshotEvent {
    pane_id: String,
    snapshot: String,
}

#[derive(Debug, Clone, Serialize)]
struct TerminalDataEvent {
    pane_id: String,
    data: Vec<u8>,
}
