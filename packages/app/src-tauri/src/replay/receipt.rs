//! Replays a pane's status history through the turn tracker and the "wait for my own checkpoint"
//! settle, on virtual time, to decide what the end-of-turn summary (the Receipt, "Last turn") says.
//!
//! The real chain is `checkpoints::on_pane_status` (takes a ticket, captures on a worker thread)
//! and `turn_summary::on_pane_status` (waits for that ticket, then builds the summary). Both hang
//! off global statics and real threads, so this module drives the same pieces
//! (`turn_summary::tracker::Tracker`, `turn_summary::settle::Settle`) with a model of the capture
//! worker whose timing is explicit and virtual:
//!
//! - a rest point that follows `running` takes a ticket at once (`Settle::request`);
//! - its capture starts when the debounce window since the previous capture allows and no other
//!   capture is writing, takes `capture_takes`, and snapshots the workspace as it is when it STARTS
//!   (the cast's `edit:` markers say what the agent had changed by then);
//! - when it finishes, its ticket is settled and the "after" facts the summary reads are replaced;
//! - a summary is built the moment its own ticket is settled.
//!
//! `checkpoints::Runtime::decide` (which rest points take a snapshot) is private to that module, so
//! its rule is repeated here: running to idle, waiting_input or error. "Before" snapshots are left
//! out: in a real turn they are older than the debounce window by the time the turn rests.

use super::cast::Kind;
use super::pane::{ReplayPane, Timeline};
use crate::pty::status::PaneStatus;
use crate::turn_summary::model::{Approval, FileStats, ReceiptFacts, TurnSummary};
use crate::turn_summary::settle::Settle;
use crate::turn_summary::tracker::{Ended, Env, PaneInfo, Tracker};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PANE: &str = "receipt-pane";

/// How the checkpoint worker behaves in the replay. The debounce is the app's
/// (`checkpoints::runtime::DEBOUNCE_MS`, 10 s); `capture_takes` is the scenario's to set.
#[derive(Debug, Clone, Copy)]
pub struct CaptureModel {
    pub debounce: Duration,
    pub capture_takes: Duration,
}

impl CaptureModel {
    /// Live VERIFY #5: the snapshot taken for the approval prompt was still running when the turn ended.
    pub fn slow_approval_capture() -> Self {
        Self { debounce: Duration::from_secs(10), capture_takes: Duration::from_secs(9) }
    }
}

struct SimEnv {
    now_ms: AtomicU64,
    cwd: std::path::PathBuf,
    files: Mutex<Option<FileStats>>,
}

impl Env for SimEnv {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }
    fn pane(&self, _pane_id: &str) -> Option<PaneInfo> {
        Some(PaneInfo { cwd: self.cwd.to_string_lossy().into_owned(), label: "Claude Code".into(), has_turns: true })
    }
    fn files(&self, _pane_id: &str, _started_at_ms: u64) -> Option<FileStats> {
        self.files.lock().clone()
    }
    fn end_prompt(&self, _pane_id: &str) -> Option<String> {
        None
    }
    fn receipt(&self, _pane_id: &str) -> Option<ReceiptFacts> {
        None
    }
}

struct Capture {
    ticket: u64,
    snapshot_files: u32,
    done_at: Duration,
}

struct Waiting {
    ended: Ended,
    ticket: u64,
}

enum Ev {
    Status(PaneStatus),
    Answer,
    Edit(String),
}

/// What the replay concluded.
pub struct ReceiptReplay {
    /// The summaries as the app keeps them (oldest first) after everything settled.
    pub kept: Vec<TurnSummary>,
    /// Every summary built: when, which turn, and how many files it counted.
    pub built: Vec<(Duration, u32, Option<u32>)>,
}

struct Sim<'a> {
    env: Arc<SimEnv>,
    tracker: Tracker,
    settle: Settle,
    captures: Vec<Capture>,
    waiting: Vec<Waiting>,
    built: Vec<(Duration, u32, Option<u32>)>,
    timeline: &'a mut Timeline,
}

impl Sim<'_> {
    /// Builds every summary whose ticket is settled (the summary worker's wait, polled on virtual time).
    fn release(&mut self, at: Duration) {
        self.env.now_ms.store(at.as_millis() as u64, Ordering::SeqCst);
        let mut still = Vec::new();
        for wait in std::mem::take(&mut self.waiting) {
            if !self.settle.wait(PANE, wait.ticket, Duration::ZERO) {
                still.push(wait);
                continue;
            }
            let (turn, end, ticket) = (wait.ended.turn, wait.ended.end_status.clone(), wait.ticket);
            if let Some(summary) = self.tracker.finish(wait.ended) {
                let files = summary.files.as_ref().map(|f| f.count);
                let shown = files.map_or("unknown".to_string(), |n| n.to_string());
                self.timeline.note(
                    at,
                    "summary",
                    format!("turn {turn} ({end}) built after ticket {ticket}: files={shown} approvals={}", summary.approvals.len()),
                );
                self.built.push((at, turn, files));
            }
        }
        self.waiting = still;
    }

    /// Finishes captures in completion order up to `until` (all of them when `None`).
    fn finish_captures(&mut self, until: Option<Duration>) {
        while let Some(index) = self
            .captures
            .iter()
            .enumerate()
            .filter(|(_, c)| until.is_none_or(|limit| c.done_at <= limit))
            .min_by_key(|(_, c)| c.done_at)
            .map(|(i, _)| i)
        {
            let capture = self.captures.remove(index);
            self.env.now_ms.store(capture.done_at.as_millis() as u64, Ordering::SeqCst);
            *self.env.files.lock() = Some(FileStats { count: capture.snapshot_files, added: 0, deleted: 0, paths: Vec::new() });
            self.settle.settle(PANE, capture.ticket);
            self.timeline.note(capture.done_at, "capture", format!("ticket {} done, snapshot holds {} file(s)", capture.ticket, capture.snapshot_files));
            self.release(capture.done_at);
        }
    }
}

pub fn replay_receipts(pane: &ReplayPane, model: CaptureModel, timeline: &mut Timeline) -> ReceiptReplay {
    let cwd = std::env::temp_dir().join(format!("pm-replay-receipt-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&cwd).expect("temp workspace");
    let env = Arc::new(SimEnv { now_ms: AtomicU64::new(0), cwd: cwd.clone(), files: Mutex::new(None) });
    let mut sim = Sim {
        env: env.clone(),
        tracker: Tracker::new(env.clone()),
        settle: Settle::default(),
        captures: Vec::new(),
        waiting: Vec::new(),
        built: Vec::new(),
        timeline,
    };

    // Everything that happened to the pane, in time order.
    let mut events: Vec<(Duration, Ev)> = Vec::new();
    for (at, status) in pane.transitions.iter().skip(1) {
        events.push((*at, Ev::Status(*status)));
    }
    for event in &pane.cast().events {
        if let Kind::Input(_) = event.kind {
            events.push((event.at, Ev::Answer));
        }
    }
    for (at, path) in pane.cast().edits() {
        events.push((at, Ev::Edit(path)));
    }
    events.sort_by_key(|(at, _)| *at);

    let mut edits: Vec<Duration> = Vec::new();
    let mut last_start: Option<Duration> = None;
    let mut last_done = Duration::ZERO;
    let mut current = pane.transitions.first().map_or(PaneStatus::Running, |(_, s)| *s);
    let mut approvals = 0;

    for (at, event) in events {
        sim.finish_captures(Some(at));
        env.now_ms.store(at.as_millis() as u64, Ordering::SeqCst);
        match event {
            Ev::Edit(path) => {
                edits.push(at);
                sim.timeline.note(at, "workspace", format!("agent edited {path}"));
            }
            Ev::Answer => {
                // A recorded input while a prompt is up answers it.
                if current == PaneStatus::WaitingInput {
                    approvals += 1;
                    let approval = Approval { at_ms: at.as_millis() as u64, prompt_id: format!("a{approvals}"), prompt: String::new(), kind: "request".into(), decision: "allow".into() };
                    sim.tracker.record_approval(PANE, approval);
                    sim.timeline.note(at, "approval", format!("answered #{approvals}"));
                }
            }
            Ev::Status(status) => {
                let was = std::mem::replace(&mut current, status);
                let ended = sim.tracker.on_status(PANE, status.as_str());
                let rest = matches!(status, PaneStatus::Idle | PaneStatus::WaitingInput | PaneStatus::Error);
                sim.timeline.note(at, "tracker", format!("{} -> {}{}", was.as_str(), status.as_str(), if ended.is_some() { ", the turn came to rest" } else { "" }));
                // `checkpoints::on_pane_status` takes the ticket on the status-event thread, before
                // the summary hook reads `Settle::latest`.
                if was == PaneStatus::Running && rest {
                    let ticket = sim.settle.request(PANE);
                    let candidates = [Some(at), last_start.map(|s| s + model.debounce), Some(last_done)];
                    let start = candidates.into_iter().flatten().max().unwrap_or(at);
                    let done_at = start + model.capture_takes;
                    (last_start, last_done) = (Some(start), done_at);
                    let snapshot_files = edits.iter().filter(|e| **e <= start).count() as u32;
                    sim.timeline.note(at, "capture", format!("ticket {ticket} requested, runs {} ms to {} ms", start.as_millis(), done_at.as_millis()));
                    sim.captures.push(Capture { ticket, snapshot_files, done_at });
                }
                if let Some(ended) = ended {
                    // `turn_summary::on_pane_status`: a turn that was running waits for the newest ticket.
                    let ticket = if ended.from_running { sim.settle.latest(PANE) } else { 0 };
                    sim.timeline.note(at, "summary", format!("turn {} waits for ticket {ticket}", ended.turn));
                    sim.waiting.push(Waiting { ended, ticket });
                    sim.release(at);
                }
            }
        }
    }
    sim.finish_captures(None);
    let kept = sim.tracker.summaries(PANE);
    let built = std::mem::take(&mut sim.built);
    let _ = std::fs::remove_dir_all(&cwd);
    ReceiptReplay { kept, built }
}
