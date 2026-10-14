//! `ReplayPane`: a pane whose output comes from a cast and whose clock is virtual.
//!
//! It feeds the recorded bytes into the same screen model the app uses (`vt100` behind
//! `pty::stream_pos::StreamPos::feed`, read back through `pty::registry_read_snapshot_full`) and,
//! after every chunk, runs the same status steps the PTY reader runs (`busy_marker`,
//! `classify_agent_observation`, `settle_status_after_resize`, plus the idle flip of the idle
//! watcher). Time moves only through [`ReplayPane::run_to`] / [`DeliveryIo::pause`], so a replay of
//! a ten second run takes microseconds and the same cast always gives the same result.
//!
//! As a [`DeliveryIo`] it is a pane the delivery protocol can type into: what is written is
//! recorded, and a cast's reactions (`ro` events) answer it the way the real agent did.
//!
//! Everything the code under test decided is written to a [`Timeline`], the text the corpus tests
//! snapshot with `insta`.

use super::cast::{Cast, Kind, Reaction};
use crate::agent_runs::delivery::DeliveryIo;
use crate::agent_runs::readiness::Observation;
use crate::pty::agent_proc::AgentProcess;
use crate::pty::WriteOrigin;
use crate::pty::status::{self, PaneStatus};
use crate::pty::stream_pos::StreamPos;
use crate::pty::PaneRegistry;
use crate::tui::{profile_for, TuiProfile};
use parking_lot::Mutex;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

const PANE_ID: &str = "replay-pane";
/// How often the app's idle watcher looks (`pty::registry::idle::IDLE_POLL`).
const IDLE_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct Line {
    pub at: Duration,
    pub channel: &'static str,
    pub detail: String,
}

/// What was decided, in order, with the virtual time it happened at.
#[derive(Debug, Clone, Default)]
pub struct Timeline {
    pub lines: Vec<Line>,
}

impl Timeline {
    pub fn note(&mut self, at: Duration, channel: &'static str, detail: impl Into<String>) {
        self.lines.push(Line { at, channel, detail: detail.into() });
    }

    /// This timeline and `other` in time order (this one's line first at equal times).
    pub fn merged(&self, other: &Timeline) -> Timeline {
        let mut lines = self.lines.clone();
        lines.extend(other.lines.iter().cloned());
        lines.sort_by_key(|line| line.at);
        Timeline { lines }
    }

    /// One line per decision: `   600 ms  status   running`.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(&format!("{:>7} ms  {:<9} {}\n", line.at.as_millis(), line.channel, line.detail));
        }
        out
    }
}

/// An input the code under test sent to the pane.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub at: Duration,
    pub text: String,
    pub enter: bool,
    /// The startup screen the profile saw on the pane when this was written, if any.
    pub gate: Option<String>,
}

/// What the screen means right now, according to the code under test. Logged when it changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Verdicts {
    gate: String,
    prompt: String,
    typing: String,
    composer: String,
}

pub struct ReplayPane {
    agent: String,
    cast: Cast,
    next_event: usize,
    now: Duration,
    next_tick: Duration,
    registry: Arc<Mutex<PaneRegistry>>,
    parser: Arc<Mutex<vt100::Parser>>,
    pos: StreamPos,
    total: u64,
    last_snapshot: String,
    last_change: Duration,
    last_resize: Option<Duration>,
    busy: Option<bool>,
    status: PaneStatus,
    pending: Vec<(Duration, Vec<u8>)>,
    fired: Vec<bool>,
    verdicts: Verdicts,
    pub timeline: Timeline,
    pub inputs: Vec<Input>,
    /// Every status change with its time (the receipt replay reads these).
    pub transitions: Vec<(Duration, PaneStatus)>,
}

impl ReplayPane {
    pub fn new(cast: Cast) -> Self {
        let agent = cast.meta.agent.clone();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(cast.rows, cast.cols, 0)));
        let mut registry = PaneRegistry::new();
        let mut stub = PaneRegistry::test_pane_stub(PANE_ID);
        stub.info.agent_type = agent.clone();
        stub.screen = parser.clone();
        registry.panes.insert(PANE_ID.into(), stub);
        let status = if cast.meta.initial_status == "idle" { PaneStatus::Idle } else { PaneStatus::Running };
        let fired = vec![false; cast.reactions.len()];
        let mut pane = Self {
            agent,
            cast,
            next_event: 0,
            now: Duration::ZERO,
            next_tick: IDLE_POLL,
            registry: Arc::new(Mutex::new(registry)),
            parser,
            pos: StreamPos::default(),
            total: 0,
            last_snapshot: String::new(),
            last_change: Duration::ZERO,
            last_resize: None,
            busy: None,
            status,
            pending: Vec::new(),
            fired,
            verdicts: Verdicts::default(),
            timeline: Timeline::default(),
            inputs: Vec::new(),
            transitions: Vec::new(),
        };
        pane.timeline.note(Duration::ZERO, "status", format!("{} (start)", status.as_str()));
        pane.transitions.push((Duration::ZERO, status));
        pane
    }

    pub fn corpus(relative: &str) -> Self {
        Self::new(Cast::corpus(relative))
    }

    pub fn status(&self) -> PaneStatus {
        self.status
    }

    pub fn cast(&self) -> &Cast {
        &self.cast
    }

    /// The whole visible screen, read the way delivery reads it.
    pub fn screen_text(&self) -> String {
        crate::pty::registry_read_snapshot_full(self.registry.as_ref(), PANE_ID).unwrap_or_default()
    }

    pub fn note(&mut self, channel: &'static str, detail: impl Into<String>) {
        self.timeline.note(self.now, channel, detail);
    }

    /// Plays the cast up to `target` (virtual time), then stops there.
    pub fn run_to(&mut self, target: Duration) {
        loop {
            let event_at = self.cast.events.get(self.next_event).map(|e| e.at);
            let reaction_at = self.pending.iter().map(|(at, _)| *at).min();
            let due = [event_at, reaction_at].into_iter().flatten().min();
            // An event at the same instant as an idle poll goes first: the poll sees its result.
            if let Some(at) = due.filter(|at| *at <= target && *at <= self.next_tick) {
                self.now = self.now.max(at);
                if event_at == Some(at) {
                    let event = self.cast.events[self.next_event].clone();
                    self.next_event += 1;
                    self.apply(event.kind);
                } else if let Some(index) = self.pending.iter().position(|(due_at, _)| *due_at == at) {
                    let (_, bytes) = self.pending.remove(index);
                    self.output(&bytes);
                }
            } else if self.next_tick <= target {
                self.now = self.next_tick;
                self.next_tick += IDLE_POLL;
                self.idle_watch();
            } else {
                break;
            }
        }
        self.now = self.now.max(target);
    }

    pub fn advance(&mut self, by: Duration) {
        self.run_to(self.now + by);
    }

    /// Plays everything left in the cast and `extra` more virtual time after its last event.
    pub fn run_to_end(&mut self, extra: Duration) {
        let end = self.cast.end().max(self.now) + extra;
        self.run_to(end);
    }

    fn apply(&mut self, kind: Kind) {
        match kind {
            Kind::Output(bytes) => self.output(&bytes),
            Kind::Resize { cols, rows } => {
                // `pty::registry::resize_pty`: the emulator keeps the PTY's shape, and the resize
                // clock starts (`pty::resize_clock::note_resize`, here on virtual time).
                self.parser.lock().screen_mut().set_size(rows, cols);
                self.last_resize = Some(self.now);
                self.note("resize", format!("{cols}x{rows}"));
            }
            Kind::Input(text) => {
                // A recorded input: the user's or the app's at capture time. Replays do not re-send it.
                self.note("recorded-input", format!("{text:?}"));
            }
            Kind::Marker(text) => self.note("marker", text),
        }
    }

    fn recently_resized(&self) -> bool {
        self.last_resize.is_some_and(|at| self.now.saturating_sub(at) < crate::pty::resize_clock::SETTLE_WINDOW)
    }

    /// One chunk of PTY output: the PTY reader's per-chunk work.
    fn output(&mut self, bytes: &[u8]) {
        let snapshot = self.pos.feed(&self.parser, &mut self.total, bytes, |_| {});
        // Output that leaves every visible cell alone is not activity (the reader `continue`s).
        if snapshot == self.last_snapshot {
            return;
        }
        self.last_change = self.now;
        let busy = status::busy_marker(&self.agent, &snapshot);
        self.busy = busy;
        let observed = status::classify_agent_observation(&self.agent, &snapshot, "");
        let settled = status::settle_status_after_resize(self.status, observed, busy, self.recently_resized());
        if settled != self.status {
            self.status = settled;
            self.transitions.push((self.now, settled));
            let why = if observed != settled { format!(" (screen read {}, held back)", observed.as_str()) } else { String::new() };
            self.note("status", format!("{}{why}", settled.as_str()));
        } else if observed != settled {
            self.note("status", format!("{} (screen read {}, held back)", settled.as_str(), observed.as_str()));
        }
        self.last_snapshot = snapshot;
        self.observe();
    }

    /// The idle watcher thread: a running pane whose screen held still long enough is idle.
    fn idle_watch(&mut self) {
        if self.status != PaneStatus::Running {
            return;
        }
        let profile = profile_for(&self.agent);
        let (base, quiet) = (profile.idle_after(), profile.quiet_idle_after());
        // `pty::registry::idle::idle_window`
        let window = if self.busy == Some(false) { quiet.min(base) } else { base };
        if self.now.saturating_sub(self.last_change) >= window {
            self.status = PaneStatus::Idle;
            self.transitions.push((self.now, PaneStatus::Idle));
            self.note("status", "idle (screen held still)");
        }
    }

    /// Logs what the app's screen readers say about the current screen, when it changed.
    fn observe(&mut self) {
        let screen = self.screen_text();
        let profile = profile_for(&self.agent);
        let now = Verdicts {
            gate: profile.startup_gate(&screen).map(|g| g.id.to_string()).unwrap_or_default(),
            prompt: crate::pane_prompt::detect_in_screen(&self.agent, &screen, false)
                .and_then(|p| p.get("kind").and_then(|k| k.as_str()).map(str::to_string))
                .unwrap_or_default(),
            typing: crate::pane_prompt::dispatch_blocker(&self.agent, &screen).map(|e| e.code).unwrap_or_default(),
            composer: format!("{:?}", profile.composer_state(&screen)),
        };
        let was = std::mem::replace(&mut self.verdicts, now.clone());
        let show = |text: &str| if text.is_empty() { "-".to_string() } else { text.to_string() };
        if was.gate != now.gate {
            self.note("gate", show(&now.gate));
        }
        if was.prompt != now.prompt {
            self.note("prompt", show(&now.prompt));
        }
        if was.typing != now.typing {
            self.note("typing", if now.typing.is_empty() { "allowed".to_string() } else { format!("refused {}", now.typing) });
        }
        if was.composer != now.composer {
            self.note("composer", now.composer.clone());
        }
    }

    fn matching_reaction(&self, text: &str, enter: bool) -> Option<usize> {
        let screen = self.screen_text();
        self.cast.reactions.iter().enumerate().position(|(index, reaction)| {
            !self.fired[index]
                && reaction.on == text
                && reaction.enter.is_none_or(|want| want == enter)
                && reaction.while_screen_contains.as_ref().is_none_or(|needle| screen.contains(needle.as_str()))
        })
    }

    fn react(&mut self, index: usize) {
        self.fired[index] = true;
        let Reaction { delay, data, .. } = self.cast.reactions[index].clone();
        self.pending.push((self.now + delay, data));
    }
}

impl DeliveryIo for ReplayPane {
    fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
        self.run_to(self.now);
        // The live write path's permit check (`pty::registry::pane_io::write_input`): a permit whose
        // screen is no longer the latest is refused, so the replay types exactly what the app would.
        let emitted = Some(self.pos.emitted.load(Ordering::SeqCst));
        if let Err(refused) = origin.admit_terminal_write(PANE_ID, text, enter, emitted, || AgentProcess::Unknown) {
            self.note("input", format!("{text:?} refused: {refused}"));
            return Err(refused);
        }
        let gate = profile_for(&self.agent).startup_gate(&self.screen_text()).map(|g| g.id.to_string());
        self.inputs.push(Input { at: self.now, text: text.to_string(), enter, gate: gate.clone() });
        let over = gate.map(|id| format!("  [typed over '{id}']")).unwrap_or_default();
        self.note("input", format!("{text:?}{}{over}", if enter { " + Enter" } else { "" }));
        if let Some(index) = self.matching_reaction(text, enter) {
            self.react(index);
        }
        Ok(())
    }

    fn screen(&mut self) -> String {
        self.run_to(self.now);
        self.screen_text()
    }

    /// `agent_runs::delivery::PaneIo::busy`: the pane is not idle.
    fn busy(&mut self) -> bool {
        self.status != PaneStatus::Idle
    }

    fn pause(&mut self, duration: Duration) {
        self.advance(duration);
    }

    fn enter_settle(&mut self) -> Duration {
        profile_for(&self.agent).submit_sequence().settle
    }

    /// The screen with the stream position it was read at (`agent_runs::delivery::observe_pane`).
    /// The replay has no process to look at.
    fn observe(&mut self) -> Observation {
        self.run_to(self.now);
        Observation {
            pane_id: PANE_ID.to_string(),
            screen: self.screen_text(),
            output_pos: Some(self.pos.processed.load(Ordering::SeqCst)),
            agent: AgentProcess::Unknown,
        }
    }

    fn profile(&mut self) -> Arc<dyn TuiProfile> {
        profile_for(&self.agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replay mirrors the PTY reader's status steps; this fails when the reader stops calling
    /// them in this order, which is the cue to update `ReplayPane::output`.
    #[test]
    fn the_status_steps_match_the_pty_reader() {
        let reader = include_str!("../pty/registry/spawn.rs");
        let order = ["status::busy_marker(", "status::classify_agent_observation(", "status::settle_status_after_resize(", "resize_clock::recently_resized("];
        let mut from = 0;
        for call in order {
            let at = reader[from..].find(call).unwrap_or_else(|| panic!("spawn.rs no longer calls {call} in the replayed order"));
            from += at + call.len();
        }
        assert!(reader.contains("idle_clock.idle_due("), "the idle watcher changed");
    }

    #[test]
    fn time_is_virtual_and_output_follows_the_cast() {
        let text = concat!(
            r#"{"version":2,"width":40,"height":5,"pm":{"agent":"powershell"}}"#,
            "\n",
            r#"[0.5,"o","PS C:\\> "]"#,
            "\n",
        );
        let mut pane = ReplayPane::new(Cast::parse("t", text).unwrap());
        pane.run_to(Duration::from_millis(499));
        assert!(pane.screen_text().is_empty());
        pane.run_to(Duration::from_millis(500));
        assert_eq!(pane.screen_text().trim(), "PS C:\\>");
        assert_eq!(pane.status(), PaneStatus::Idle, "a shell at its prompt is idle at once");
        let wall = std::time::Instant::now();
        pane.run_to(Duration::from_secs(3600));
        assert!(wall.elapsed() < Duration::from_secs(2), "an hour of virtual time costs no real time");
    }
}
