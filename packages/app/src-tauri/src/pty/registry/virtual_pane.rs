//! A pane with no terminal behind it: the ACP lane's chat session (`crate::acp::chat`). It stands in the registry like
//! any pane (so the pane list, the sidebar, `wait_agents` and the phone know it by its id, its folder and its
//! status), but there is no process, no PTY and no reader thread. Its status comes from the ACP session's snapshot
//! (`VirtualPaneHandles`), its input is routed to the session by `write_input` (`crate::acp::chat::terminal_input`),
//! and closing it stops the session (`on_close`).
//!
//! The "terminal" is a null object: it reads nothing, discards writes and accepts any size.

use super::*;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty};
use std::sync::atomic::{AtomicBool, Ordering};

/// Size the pane reports; nothing is drawn at it.
const COLS: u16 = 100;
const ROWS: u16 = 30;

/// The one line the screen of such a pane shows, for a tool that reads the screen.
pub const SCREEN_NOTE: &str = "Chat session (no terminal). Read it with the pane's chat.\r\n";

pub struct VirtualPaneSpec {
    pub pane_id: String,
    pub agent_type: String,
    pub cwd: String,
    /// `PaneInfo.lane` (`"acp"`).
    pub lane: String,
}

/// What the owner of a virtual pane updates as the session changes.
#[derive(Clone)]
pub struct VirtualPaneHandles {
    pub status: Arc<Mutex<PaneStatus>>,
    pub last_output: Arc<Mutex<Instant>>,
    pub exited: Arc<Mutex<bool>>,
}

struct NullMaster;

impl MasterPty for NullMaster {
    fn resize(&self, _size: PtySize) -> Result<(), anyhow::Error> {
        Ok(())
    }
    fn get_size(&self) -> Result<PtySize, anyhow::Error> {
        Ok(PtySize { rows: ROWS, cols: COLS, pixel_width: 0, pixel_height: 0 })
    }
    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>, anyhow::Error> {
        Ok(Box::new(std::io::empty()))
    }
    fn take_writer(&self) -> Result<Box<dyn Write + Send>, anyhow::Error> {
        Ok(Box::new(std::io::sink()))
    }
    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<i32> {
        None
    }
    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        None
    }
}

/// The "process" of a virtual pane: killing it stops the session, once.
#[derive(Clone)]
struct NullChild {
    on_close: Arc<dyn Fn() + Send + Sync>,
    closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for NullChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NullChild").field("closed", &self.closed.load(Ordering::SeqCst)).finish()
    }
}

impl ChildKiller for NullChild {
    fn kill(&mut self) -> std::io::Result<()> {
        if !self.closed.swap(true, Ordering::SeqCst) {
            (self.on_close)();
        }
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}

impl Child for NullChild {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        Ok(self.closed.load(Ordering::SeqCst).then(|| ExitStatus::with_exit_code(0)))
    }
    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        Ok(ExitStatus::with_exit_code(0))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

/// Puts a pane with no terminal in the registry. `on_close` runs once when the pane is closed (the session stops).
pub fn insert_virtual_pane(registry: &Mutex<PaneRegistry>, spec: VirtualPaneSpec, on_close: Arc<dyn Fn() + Send + Sync>) -> VirtualPaneHandles {
    let status = Arc::new(Mutex::new(PaneStatus::Idle));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let exited = Arc::new(Mutex::new(false));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, SCROLLBACK_CAP)));
    screen.lock().process(SCREEN_NOTE.as_bytes());
    let info = PaneInfo {
        id: spec.pane_id.clone(),
        agent_type: spec.agent_type,
        pid: 0,
        status: PaneStatus::Idle.as_str().to_string(),
        created_at: chrono_now_ms(),
        last_output_at: None,
        cwd: spec.cwd,
        cols: COLS,
        rows: ROWS,
        viewer_held: false,
        reprints_on_resize: false,
        account: None,
        account_label: None,
        title: None,
        parent_pane_id: None,
        place: None,
        lane: Some(spec.lane),
    };
    let pane = PaneState {
        info,
        scrollback: Arc::new(Mutex::new(Scrollback::new(SCROLLBACK_CAP))),
        screen,
        status: status.clone(),
        last_output: last_output.clone(),
        master: Arc::new(Mutex::new(Box::new(NullMaster))),
        writer: Arc::new(Mutex::new(Box::new(std::io::sink()))),
        child: Box::new(NullChild { on_close, closed: Arc::new(AtomicBool::new(false)) }),
        exited: exited.clone(),
        opencode: None,
        opencode_key_event: None,
    };
    registry.lock().panes.insert(spec.pane_id.clone(), pane);
    VirtualPaneHandles { status, last_output, exited }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str) -> VirtualPaneSpec {
        VirtualPaneSpec { pane_id: id.into(), agent_type: "codex".into(), cwd: "C:\\work".into(), lane: "acp".into() }
    }

    #[test]
    fn a_virtual_pane_is_listed_like_any_pane_and_says_it_is_a_chat() {
        let registry = Mutex::new(PaneRegistry::new());
        insert_virtual_pane(&registry, spec("v1"), Arc::new(|| {}));
        let listed = registry.lock().list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].lane.as_deref(), Some("acp"));
        assert_eq!(listed[0].agent_type, "codex");
        let json = serde_json::to_value(&listed[0]).unwrap();
        assert_eq!(json["lane"], "acp");
        assert_eq!(json["pid"], 0);
    }

    #[test]
    fn a_pane_of_a_terminal_leaves_the_lane_out_of_its_json() {
        // The new field must not change the JSON of any other pane.
        let info = PaneInfo {
            id: "t".into(),
            agent_type: "shell".into(),
            pid: 1,
            status: "idle".into(),
            created_at: 0,
            last_output_at: None,
            cwd: String::new(),
            cols: 80,
            rows: 24,
            viewer_held: false,
            reprints_on_resize: false,
            account: None,
            account_label: None,
            title: None,
            parent_pane_id: None,
            place: None,
            lane: None,
        };
        assert!(serde_json::to_value(&info).unwrap().get("lane").is_none());
    }

    #[test]
    fn the_status_the_owner_sets_is_what_the_pane_reports() {
        let registry = Mutex::new(PaneRegistry::new());
        let handles = insert_virtual_pane(&registry, spec("v2"), Arc::new(|| {}));
        *handles.status.lock() = PaneStatus::WaitingInput;
        assert_eq!(registry.lock().get("v2").unwrap().info().status, "waiting_input");
    }

    #[test]
    fn closing_the_pane_stops_the_session_once() {
        let registry = Mutex::new(PaneRegistry::new());
        let stops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = stops.clone();
        insert_virtual_pane(&registry, spec("v3"), Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let pane = registry.lock().take("v3").unwrap();
        shutdown_pane(pane);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_null_terminal_reads_nothing_and_takes_any_size() {
        let master = NullMaster;
        assert!(master.resize(PtySize { rows: 5, cols: 5, pixel_width: 0, pixel_height: 0 }).is_ok());
        let mut text = String::new();
        master.try_clone_reader().unwrap().read_to_string(&mut text).unwrap();
        assert!(text.is_empty());
        master.take_writer().unwrap().write_all(b"anything").unwrap();
    }
}
