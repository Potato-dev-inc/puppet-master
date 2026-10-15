//! A fake native stream (bytes through a real VT parser) drives the status tracker, with time
//! passed in by the test. No app, no server, no windows.

use super::*;
use crate::turn_summary::tracker::{Env, PaneInfo, Tracker};
use crate::turn_summary::{FileStats, ReceiptFacts};
use std::sync::atomic::{AtomicU64, Ordering};

const IDLE_AFTER: Duration = Duration::from_secs(5);

/// One native pane's reader, minus the threads: bytes in, a VT screen, the tracker, and the
/// statuses that would have gone out as `pty://status`.
struct FakePane {
    screen: vt100::Parser,
    tracker: NativeStatus,
    status: Arc<Mutex<PaneStatus>>,
    now: Instant,
    published: Vec<PaneStatus>,
    resized_recently: bool,
}

impl FakePane {
    fn new() -> Self {
        let status = Arc::new(Mutex::new(PaneStatus::Running));
        let now = Instant::now();
        Self {
            screen: vt100::Parser::new(10, 60, 0),
            tracker: NativeStatus::new(status.clone(), now),
            status,
            now,
            published: Vec::new(),
            resized_recently: false,
        }
    }

    /// The TUI wrote `bytes`: what `spawn_reader_thread` does per chunk.
    fn output(&mut self, bytes: &[u8]) {
        self.screen.process(bytes);
        let visible = self.screen.screen().contents();
        if let Some(status) = self.tracker.on_output(&visible, self.now, self.resized_recently) {
            self.published.push(status);
        }
    }

    /// Wall time passes with no output; the watcher looks once at the end.
    fn wait(&mut self, by: Duration, server: ServerSession) {
        self.now += by;
        if let Some(status) = self.tracker.tick(self.now, IDLE_AFTER, &|| server) {
            self.published.push(status);
        }
    }

    fn status(&self) -> PaneStatus {
        *self.status.lock()
    }
}

#[test]
fn a_finished_turn_reads_idle_once_the_screen_holds_still() {
    // Live: composer idle and `finish: "stop"`, yet the pane read `running` for 374 s.
    let mut pane = FakePane::new();
    pane.output(b"\x1b[2J\x1b[Hask anything...\r\n");
    assert_eq!(pane.status(), PaneStatus::Running, "the TUI just painted");

    pane.output(b"OC-DONE\r\n");
    pane.wait(Duration::from_millis(4_900), ServerSession::NotBusy);
    assert_eq!(pane.status(), PaneStatus::Running, "not before the whole window");

    pane.wait(Duration::from_millis(200), ServerSession::NotBusy);
    assert_eq!(pane.status(), PaneStatus::Idle);
    assert_eq!(pane.published, vec![PaneStatus::Idle], "announced once, as the regular panes do");

    pane.wait(Duration::from_secs(60), ServerSession::NotBusy);
    assert_eq!(pane.published, vec![PaneStatus::Idle], "no repeat");
}

#[test]
fn output_that_changes_no_visible_cell_does_not_postpone_idle() {
    let mut pane = FakePane::new();
    pane.output(b"ready\r\n");
    pane.wait(Duration::from_secs(3), ServerSession::NotBusy);
    // Cursor moves and attribute resets: the TUI is chatty but nothing on screen changed.
    pane.output(b"\x1b[?25l\x1b[0m\x1b[?25h");
    pane.wait(Duration::from_millis(2_100), ServerSession::NotBusy);
    assert_eq!(pane.status(), PaneStatus::Idle);
}

#[test]
fn new_output_on_an_idle_pane_reads_running_again_and_is_announced() {
    let mut pane = FakePane::new();
    pane.output(b"ready\r\n");
    pane.wait(Duration::from_secs(6), ServerSession::NotBusy);
    assert_eq!(pane.published, vec![PaneStatus::Idle]);

    pane.output(b"thinking...\r\n");
    assert_eq!(pane.status(), PaneStatus::Running);
    assert_eq!(pane.published, vec![PaneStatus::Idle, PaneStatus::Running]);

    // More output while running is not another event.
    pane.output(b"still going\r\n");
    assert_eq!(pane.published, vec![PaneStatus::Idle, PaneStatus::Running]);

    pane.wait(Duration::from_secs(6), ServerSession::NotBusy);
    assert_eq!(pane.published, vec![PaneStatus::Idle, PaneStatus::Running, PaneStatus::Idle]);
}

#[test]
fn a_session_the_server_says_is_working_is_not_idle_however_still_the_screen_is() {
    // A long tool call: the TUI shows nothing new, the server knows the turn is still going.
    let mut pane = FakePane::new();
    pane.output(b"running tests\r\n");
    pane.wait(Duration::from_secs(30), ServerSession::Busy);
    assert_eq!(pane.status(), PaneStatus::Running);
    assert!(pane.published.is_empty());

    // The server's answer, not the clock, ends it.
    pane.wait(Duration::from_secs(1), ServerSession::NotBusy);
    assert_eq!(pane.status(), PaneStatus::Idle);
}

#[test]
fn a_busy_server_is_asked_again_at_a_gentle_rate() {
    let mut pane = FakePane::new();
    pane.output(b"running tests\r\n");
    pane.now += Duration::from_secs(6);
    let asked = AtomicU64::new(0);
    let busy = || {
        asked.fetch_add(1, Ordering::SeqCst);
        ServerSession::Busy
    };
    // The watcher looks every 100 ms.
    for _ in 0..30 {
        pane.now += Duration::from_millis(100);
        pane.tracker.tick(pane.now, IDLE_AFTER, &busy);
    }
    let asked = asked.load(Ordering::SeqCst);
    assert!((3..=4).contains(&asked), "3 s of looks asked the server {asked} times");
}

#[test]
fn a_server_that_cannot_be_asked_leaves_the_decision_to_the_screen() {
    let mut pane = FakePane::new();
    pane.output(b"ready\r\n");
    pane.wait(Duration::from_secs(6), ServerSession::Unknown);
    assert_eq!(pane.status(), PaneStatus::Idle);
}

#[test]
fn a_repaint_after_our_own_resize_does_not_wake_an_idle_pane() {
    let mut pane = FakePane::new();
    pane.output(b"ready\r\n");
    pane.wait(Duration::from_secs(6), ServerSession::NotBusy);
    assert_eq!(pane.status(), PaneStatus::Idle);

    pane.resized_recently = true;
    pane.output(b"\x1b[2J\x1b[Hready again, redrawn\r\n");
    assert_eq!(pane.status(), PaneStatus::Idle, "a reprint is not work");
    assert_eq!(pane.published, vec![PaneStatus::Idle]);
}

#[test]
fn an_exited_pane_is_never_turned_idle_or_woken() {
    let mut pane = FakePane::new();
    pane.output(b"ready\r\n");
    *pane.status.lock() = PaneStatus::Error;
    pane.wait(Duration::from_secs(60), ServerSession::NotBusy);
    pane.output(b"late bytes\r\n");
    assert_eq!(pane.status(), PaneStatus::Error);
    assert!(pane.published.is_empty());
}

#[test]
fn the_server_session_map_says_which_sessions_are_working() {
    use serde_json::json;
    let body = json!({ "ses-a": { "type": "busy" }, "ses-b": { "type": "retry", "attempt": 2 }, "ses-c": { "type": "idle" } });
    assert_eq!(client::session_busy_in(&body, "ses-a"), Some(true));
    assert_eq!(client::session_busy_in(&body, "ses-b"), Some(true), "waiting to retry is still working");
    assert_eq!(client::session_busy_in(&body, "ses-c"), Some(false));
    assert_eq!(client::session_busy_in(&body, "ses-gone"), Some(false), "idle sessions are left out");
    assert_eq!(client::session_busy_in(&json!({}), "ses-a"), Some(false));
    assert_eq!(client::session_busy_in(&json!([1]), "ses-a"), None, "not a map: unknown");
}

// ---- the idle reaches the consumers: turn summaries ----

struct Clock {
    now: AtomicU64,
    cwd: std::path::PathBuf,
}

impl Env for Clock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    fn pane(&self, _pane_id: &str) -> Option<PaneInfo> {
        // The same facts `turn_summary::LiveEnv` reads for a native pane: not a plain shell.
        Some(PaneInfo { cwd: self.cwd.to_string_lossy().into_owned(), label: "OpenCode".into(), has_turns: true })
    }
    fn files(&self, _pane_id: &str, _started_at_ms: u64) -> Option<FileStats> {
        None
    }
    fn end_prompt(&self, _pane_id: &str) -> Option<String> {
        None
    }
    fn receipt(&self, _pane_id: &str) -> Option<ReceiptFacts> {
        None
    }
}

#[test]
fn the_status_events_of_a_native_turn_end_it_for_the_turn_summary() {
    let cwd = std::env::temp_dir().join(format!("pm-native-idle-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&cwd).unwrap();
    let env = Arc::new(Clock { now: AtomicU64::new(10_000), cwd: cwd.clone() });
    let turns = Tracker::new(env.clone());

    let mut pane = FakePane::new();
    pane.output(b"ask anything\r\n");
    pane.wait(Duration::from_secs(6), ServerSession::NotBusy);
    // The first idle is the TUI settling after start-up: no turn is open, so no summary.
    for status in pane.published.drain(..) {
        assert!(turns.on_status("native", status.as_str()).is_none());
    }

    // A prompt arrives: the redraw reads running, the turn opens when that event is delivered.
    pane.output(b"> say OC-DONE
working
");
    assert_eq!(pane.published, vec![PaneStatus::Running]);
    for status in pane.published.drain(..) {
        assert!(turns.on_status("native", status.as_str()).is_none());
    }

    // The model works for 30 s, then the screen goes still and the idle event closes the turn.
    env.now.fetch_add(30_000, Ordering::SeqCst);
    pane.wait(Duration::from_secs(6), ServerSession::NotBusy);
    assert_eq!(pane.published, vec![PaneStatus::Idle]);
    let ended = pane.published.drain(..).find_map(|status| turns.on_status("native", status.as_str()));
    let ended = ended.expect("the idle event closes the turn");
    assert_eq!(ended.end_status, "idle");
    assert!(ended.from_running);
    assert_eq!(ended.turn, 1);
    let summary = turns.finish(ended).expect("a summary is built");
    assert_eq!(summary.pane_id, "native");
    let _ = std::fs::remove_dir_all(&cwd);
}
