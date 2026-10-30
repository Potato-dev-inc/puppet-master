//! Timeline probe for "I typed a message in Chat and it took ten seconds to show in the terminal".
//!
//! It runs the real agent CLI on a ConPTY (like the other live checks), keeps the pane status the
//! way the app's PTY reader and idle watcher do (`LiveStatus`: the screen-change clock, the profile's
//! `busy_marker`, `idle_window`), and drives the real pane outbox (`pane_outbox::Outbox`, default
//! `Timing`, the real `decide`) over a host that looks at that pane and types with the real
//! `deliver_text`. Every step is stamped on one timeline:
//!
//!   ready (first Ready verdict) -> status idle -> message enqueued -> outbox decides Deliver ->
//!   first key written -> text visible -> Enter written -> delivery returns
//!
//! `PM_LIVE_CWD=<a scratch folder> cargo test --lib live_send_ -- --ignored --nocapture --test-threads=1`
//! Env: `PM_SEND_SOAK_SECS` (watch the idle pane that long before sending), `PM_SEND_TOOL` (the last message asks for a shell command),
//! `PM_LIVE_CWD` (default: a folder under the system temp dir), `PM_SEND_AGENT` (`codex`, the default,
//! or `claude`), `PM_SEND_TURNS` (messages sent one right after the other; default 2, each is one tiny turn).

use super::*;
use crate::pane_outbox::{decide, Attempt, Host, Look, Outbox, OutboxMessage, PaneKind, State, Timing};
use crate::pty::status::{self, PaneStatus};
use std::sync::atomic::AtomicBool;

/// One timeline shared by the status tracker, the host and the test body.
#[derive(Clone)]
pub(super) struct Timeline {
    t0: Instant,
    lines: Arc<Mutex<Vec<(Duration, String)>>>,
}

impl Timeline {
    fn new() -> Self {
        Self { t0: Instant::now(), lines: Arc::new(Mutex::new(Vec::new())) }
    }

    fn note(&self, what: impl Into<String>) {
        let what = what.into();
        let at = self.t0.elapsed();
        println!("[{:>7.2}s] {what}", at.as_secs_f64());
        self.lines.lock().push((at, what));
    }

    /// When the first line starting with `prefix` was stamped at or after `after`.
    fn first_after(&self, after: Duration, prefix: &str) -> Option<Duration> {
        self.lines.lock().iter().find(|(at, what)| *at >= after && what.starts_with(prefix)).map(|(at, _)| *at)
    }
}

struct Inner {
    status: PaneStatus,
    last_snapshot: String,
    last_change: Instant,
    busy: Option<bool>,
    gate: bool,
    /// Chunks (and bytes) that arrived without changing a visible cell since the last screen change.
    quiet_chunks: u32,
    quiet_bytes: usize,
    /// Screen changes that showed work (`busy_marker` = Some(true)).
    work_frames: u32,
}

/// What the PTY reader (`pty::registry::spawn`) and the idle watcher keep for a pane's status.
pub(super) struct LiveStatus {
    agent: &'static str,
    timeline: Timeline,
    inner: Mutex<Inner>,
    stop: AtomicBool,
}

impl LiveStatus {
    pub(super) fn start(agent: &'static str) -> Arc<Self> {
        let me = Arc::new(Self {
            agent,
            timeline: Timeline::new(),
            inner: Mutex::new(Inner {
                status: PaneStatus::Running,
                last_snapshot: String::new(),
                last_change: Instant::now(),
                busy: None,
                gate: false,
                quiet_chunks: 0,
                quiet_bytes: 0,
                work_frames: 0,
            }),
            stop: AtomicBool::new(false),
        });
        let watcher = Arc::clone(&me);
        thread::spawn(move || {
            let profile = profile_for(watcher.agent);
            let (base, quiet) = (profile.idle_after(), profile.quiet_idle_after());
            while !watcher.stop.load(Ordering::Relaxed) {
                thread::sleep(super::super::idle::IDLE_POLL);
                let mut inner = watcher.inner.lock();
                if inner.gate {
                    continue;
                }
                let window = super::super::idle::idle_window(inner.busy, base, quiet);
                if inner.status == PaneStatus::Running && inner.last_change.elapsed() >= window {
                    inner.status = PaneStatus::Idle;
                    let (chunks, bytes) = (inner.quiet_chunks, inner.quiet_bytes);
                    drop(inner);
                    watcher.timeline.note(format!(
                        "status -> idle (screen still for {window:?}; {chunks} invisible chunks / {bytes} bytes since the last change)"
                    ));
                }
            }
        });
        me
    }

    pub(super) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The reader thread's per-chunk work for `n` bytes that left the screen text as `snapshot`.
    pub(super) fn feed(&self, n: usize, snapshot: &str) {
        let mut inner = self.inner.lock();
        if snapshot == inner.last_snapshot {
            inner.quiet_chunks += 1;
            inner.quiet_bytes += n;
            return;
        }
        let line = first_difference(&inner.last_snapshot, snapshot);
        inner.last_change = Instant::now();
        inner.gate = status::startup_gate_visible(self.agent, snapshot);
        let busy = if inner.gate { None } else { status::busy_marker(self.agent, snapshot) };
        inner.busy = busy;
        if busy == Some(true) {
            inner.work_frames += 1;
        }
        let observed = status::classify_agent_observation(self.agent, snapshot, "");
        let settled = status::settle_status_after_resize(inner.status, observed, busy, false);
        let (chunks, bytes) = (inner.quiet_chunks, inner.quiet_bytes);
        inner.quiet_chunks = 0;
        inner.quiet_bytes = 0;
        inner.last_snapshot = snapshot.to_string();
        let flipped = settled != inner.status;
        inner.status = settled;
        drop(inner);
        self.timeline.note(format!(
            "screen change (busy={busy:?}, +{chunks} invisible chunks/{bytes}B before it): {line}{}",
            if flipped { format!("  => status {}", settled.as_str()) } else { String::new() }
        ));
    }

    pub(super) fn current(&self) -> PaneStatus {
        self.inner.lock().status
    }

    /// Work frames seen so far, and whether the screen shows no work now.
    fn work(&self) -> (u32, Option<bool>) {
        let inner = self.inner.lock();
        (inner.work_frames, inner.busy)
    }
}

/// The first row that differs between two screens, shortened for the log.
fn first_difference(before: &str, after: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    for row in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(row).copied().unwrap_or(""), b.get(row).copied().unwrap_or(""));
        if x != y {
            let short = |s: &str| s.trim().chars().take(70).collect::<String>();
            return format!("row {row}: {:?} -> {:?}", short(x), short(y));
        }
    }
    "(identical rows)".into()
}

/// The pane as `DeliveryIo`, with the pane status the app would hold, and every key stamped.
struct ProbeIo<'a> {
    pane: &'a mut LivePane,
    timeline: &'a Timeline,
    typed_at: Option<Duration>,
    shown: bool,
    marker: &'a str,
}

impl DeliveryIo for ProbeIo<'_> {
    fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
        let wrote = DeliveryIo::write(&mut *self.pane, origin, text, enter);
        let what = match (text.is_empty(), enter) {
            (false, _) => format!("keys written: {} chars of text", text.chars().count()),
            (true, true) => "Enter written".to_string(),
            (true, false) => "empty write".to_string(),
        };
        self.timeline.note(format!("{what} ({})", if wrote.is_ok() { "accepted" } else { "refused" }));
        if !text.is_empty() && wrote.is_ok() {
            self.typed_at = Some(self.timeline.t0.elapsed());
        }
        wrote
    }
    fn screen(&mut self) -> String {
        self.pane.text()
    }
    fn observe(&mut self) -> Observation {
        let observation = DeliveryIo::observe(&mut *self.pane);
        if self.typed_at.is_some() && !self.shown && observation.screen.contains(self.marker) {
            self.shown = true;
            self.timeline.note("message text visible on screen");
        }
        observation
    }
    /// The real `PaneIo::busy`: the pane status is not idle.
    fn busy(&mut self) -> bool {
        self.pane.status.current() != PaneStatus::Idle
    }
    fn pause(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
    fn enter_settle(&mut self) -> Duration {
        DeliveryIo::enter_settle(&mut *self.pane)
    }
    fn profile(&mut self) -> Arc<dyn TuiProfile> {
        self.pane.profile_arc()
    }
}

/// The outbox's host over one live pane.
struct ProbeHost {
    pane: Arc<Mutex<LivePane>>,
    timeline: Timeline,
    last_look: Mutex<String>,
    marker: String,
}

impl Host for ProbeHost {
    fn pane_kind(&self, _pane_id: &str) -> Option<PaneKind> {
        Some(PaneKind::Agent)
    }
    fn run_controls(&self, _pane_id: &str) -> bool {
        false
    }
    fn look(&self, _pane_id: &str) -> Option<Look> {
        let mut pane = self.pane.lock();
        let profile = pane.profile_arc();
        let verdict = readiness::assess(profile.as_ref(), DeliveryIo::observe(&mut *pane));
        let status = pane.status.current();
        let look = Look::from_verdict(&verdict, status == PaneStatus::Idle).waiting(status == PaneStatus::WaitingInput);
        let step = decide(&look);
        let now = format!("{:?}/{:?}/status={}/{:?}", look.readiness_label(), look.composer, status.as_str(), step);
        let mut last = self.last_look.lock();
        if *last != now {
            *last = now.clone();
            self.timeline.note(format!("outbox look: {now}"));
        }
        Some(look)
    }
    fn type_message(&self, _pane_id: &str, text: &str) -> Attempt {
        self.timeline.note("outbox: delivering (type_message starts)");
        let mut pane = self.pane.lock();
        let profile = pane.profile_arc();
        let mut trace = Vec::new();
        let outcome = {
            let mut io = ProbeIo { pane: &mut pane, timeline: &self.timeline, typed_at: None, shown: false, marker: &self.marker };
            delivery::deliver_text(&mut io, profile.as_ref(), None, text, &mut trace)
        };
        self.timeline.note(format!("outbox: delivery returned {outcome:?}"));
        println!("--- delivery trace ---\n{}\n----------------------", trace.join("\n"));
        match outcome {
            Delivery::Started | Delivery::SubmittedUnconfirmed => Attempt::Sent,
            Delivery::NotReady { .. } => Attempt::NotTyped,
            other => Attempt::Failed(format!("{other:?}")),
        }
    }
    fn publish(&self, _pane_id: &str, messages: &[OutboxMessage]) {
        if let Some(last) = messages.last() {
            self.timeline.note(format!("outbox message state: {:?} {}", last.state, last.waiting_for.as_deref().unwrap_or("")));
        }
    }
}

trait LookLabel {
    fn readiness_label(&self) -> String;
}

impl LookLabel for Look {
    fn readiness_label(&self) -> String {
        match &self.readiness {
            readiness::Readiness::Gate(gate) => format!("Gate({})", gate.id),
            other => format!("{other:?}").chars().take(20).collect(),
        }
    }
}

fn probe_cwd() -> String {
    let dir = match std::env::var("PM_LIVE_CWD") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => std::env::temp_dir().join("pm-live-send-probe"),
    };
    std::fs::create_dir_all(&dir).expect("create the probe folder");
    dir.to_string_lossy().into_owned()
}

/// Codex asks before it works in an unknown folder; the probe trusts its own scratch folder for this
/// one process (`-c`), without writing the person's config.
fn trust_args(agent: &str, cwd: &str) -> Vec<String> {
    if agent != "codex" {
        return Vec::new();
    }
    let toml_path = cwd.replace('/', "\\").replace('\\', "\\\\");
    vec!["-c".into(), format!("projects.\"{toml_path}\".trust_level=\"trusted\"")]
}

/// Waits (polling 50 ms) until `done`, or panics with the screen after `secs`.
fn wait_for(pane: &Arc<Mutex<LivePane>>, secs: u64, what: &str, done: impl Fn(&mut LivePane) -> bool) -> Duration {
    let started = Instant::now();
    loop {
        {
            let mut guard = pane.lock();
            if done(&mut guard) {
                return started.elapsed();
            }
            assert!(started.elapsed() < Duration::from_secs(secs), "timed out waiting for {what}:\n{}", guard.text());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
#[test]
#[ignore = "live: starts the real agent CLI (default codex) and spends PM_SEND_TURNS tiny turns (default 2)"]
fn live_send_timeline_from_chat_to_terminal() {
    let agent: &'static str = if std::env::var("PM_SEND_AGENT").as_deref() == Ok("claude") { "claude" } else { "codex" };
    let kind = if agent == "claude" { AgentType::Claude } else { AgentType::Codex };
    let turns: usize = std::env::var("PM_SEND_TURNS").ok().and_then(|n| n.parse().ok()).unwrap_or(2).clamp(1, 4);
    let cwd = probe_cwd();
    let pane = LivePane::start_in(agent, kind, &cwd, &trust_args(agent, &cwd));
    let timeline = pane.status.timeline.clone();
    timeline.note(format!("{agent} pane started"));
    let status = Arc::clone(&pane.status);
    let pane = Arc::new(Mutex::new(pane));

    // (a) How long after the pane is visibly ready does its status turn idle?
    // A release with a pending update draws its notice about a second in; the startup pass closes it (Esc),
    // as every delivery does. Waiting out that second first keeps the "ready" stamp honest.
    wait_for(&pane, 90, "the composer or a notice", |p| {
        let v = p.verdict();
        v.is_ready() || v.gate().is_some()
    });
    thread::sleep(Duration::from_millis(2500));
    {
        let mut guard = pane.lock();
        let profile = guard.profile_arc();
        if guard.verdict().gate().is_some() {
            let report = crate::bridge::startup_gate::dismiss_startup_gates(&mut *guard, profile.as_ref());
            timeline.note(format!("startup notice pass: {report:?}"));
        }
    }
    let ready_after = wait_for(&pane, 90, "the composer", |p| p.verdict().is_ready());
    let ready_at = timeline.t0.elapsed();
    timeline.note(format!("READY: first Ready verdict ({ready_after:?} after start)"));
    let idle_wait = wait_for(&pane, 60, "status idle", |_| status.current() == PaneStatus::Idle);
    println!("\n>>> (a) Ready -> status idle: {idle_wait:?}\n");
    if let Some(soak) = std::env::var("PM_SEND_SOAK_SECS").ok().and_then(|n| n.parse::<u64>().ok()) {
        timeline.note(format!("soak: watching the idle pane for {soak}s (a line below is the screen changing while idle)"));
        thread::sleep(Duration::from_secs(soak.min(120)));
        timeline.note("soak over");
    }

    let host = Arc::new(ProbeHost {
        pane: Arc::clone(&pane),
        timeline: timeline.clone(),
        last_look: Mutex::new(String::new()),
        marker: "single word".into(),
    });
    let outbox = Outbox::with_timing(host, Timing::default());
    let words = ["ok", "yes", "fine", "good"];
    let mut summary = Vec::new();
    for (n, word) in words.iter().take(turns).enumerate() {
        // (c) for n > 0 the message goes in the moment the previous turn is over.
        let tool_turn = std::env::var("PM_SEND_TOOL").is_ok() && n + 1 == turns;
        let text = if tool_turn {
            format!("Run the shell command: echo {word}. Then reply with the single word {word}.")
        } else {
            format!("Reply with the single word {word}. Do not use any tools.")
        };
        timeline.note(format!("=== message {} enqueued (the Chat send) ===", n + 1));
        let enqueued = timeline.t0.elapsed();
        let frames_before = status.work().0;
        let message = outbox.send("probe", &text).expect("queue the message");
        let sent_after = wait_for(&pane, 120, "the message to be sent", |_| {
            outbox.list("probe").iter().any(|m| m.id == message.id && m.state == State::Sent)
        });
        let delivered = timeline.first_after(enqueued, "outbox: delivering").unwrap_or_default();
        let first_key = timeline.first_after(enqueued, "keys written").unwrap_or_default();
        let visible = timeline.first_after(enqueued, "message text visible").unwrap_or_default();
        let enter = timeline.first_after(enqueued, "Enter written").unwrap_or_default();
        let secs = |at: Duration| at.saturating_sub(enqueued).as_secs_f64();
        summary.push(format!(
            "message {}: decide->Deliver {:.2}s | first key {:.2}s | text visible {:.2}s | Enter {:.2}s | Sent {:.2}s (after enqueue)",
            n + 1,
            secs(delivered),
            secs(first_key),
            secs(visible),
            secs(enter),
            sent_after.as_secs_f64(),
        ));
        // The turn ends: work was shown since the send and no work row is left.
        wait_for(&pane, 120, "the turn to end", |_| {
            let (frames, busy) = status.work();
            frames > frames_before && busy == Some(false)
        });
        timeline.note(format!("turn {} is over (answer on screen)", n + 1));
    }
    println!("\n================ SUMMARY ({agent}) ================");
    println!("(a) visibly Ready -> status idle: {idle_wait:?}");
    println!("ready at {ready_at:?} after start");
    for line in &summary {
        println!("{line}");
    }
    pane.lock().capture("send_probe_final.txt");
}
