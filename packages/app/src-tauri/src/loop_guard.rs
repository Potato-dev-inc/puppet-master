//! Loop and burn detector: "this agent seems stuck".
//!
//! Consumes what the activity ticker (`pane_activity`) already reads once a second: the visible
//! screen tail of a pane whose screen or status changed, plus its status. Nothing runs in the PTY
//! reader and nothing runs for a silent pane. From the stream of tails it derives per-turn
//! records (a turn is `running` -> `idle`/`error`) and looks for four agent-agnostic signals:
//!
//! * `repeated_error`: the same error-like line (near-duplicate, numbers/paths/timestamps ignored)
//!   in N consecutive turns;
//! * `repeated_action`: the same tool/action lines in the same order for N consecutive turns,
//!   with no progress marker anywhere in them;
//! * `no_progress`: 2N turns inside ten minutes, each doing tool work or hitting errors, none
//!   showing a progress marker;
//! * `stalled`: a pane `running` continuously for X minutes without a single new distinct line.
//!
//! Only lines that are NEW since the previous observation count (a multiset difference of
//! normalised lines), so an old error that merely stays on screen never repeats. A signal is
//! raised once per episode (hysteresis): it re-arms only after the detection has cleared.
//! Generic code never matches agent names; error-likeness comes from the profile
//! (`TuiProfile::is_error_line`: a generic default plus TOML `[tui] error_regex`).
//!
//! Near-duplicate matching uses the `strsim` crate (MIT).

use crate::pty::status::PaneStatus;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Turn records kept per pane.
const MAX_TURNS: usize = 16;
/// Window for the `no_progress` signal.
const NO_PROGRESS_WINDOW_MS: u64 = 10 * 60 * 1000;
/// Two normalised error lines at least this similar are the same error.
const SIMILARITY: f64 = 0.85;
/// Evidence lines shown on the card.
const MAX_EVIDENCE: usize = 3;
/// Longest evidence line kept.
const MAX_LINE: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    RepeatedError,
    RepeatedAction,
    NoProgress,
    Stalled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoopSignal {
    pub pane_id: String,
    pub kind: LoopKind,
    /// At most three screen lines that show the problem.
    pub evidence_lines: Vec<String>,
    /// Turns involved (for `stalled`: minutes without new output).
    pub turns: u32,
    /// Epoch milliseconds when the pattern started.
    pub since: u64,
    /// Plain sentence tail for the card: "the same error 4 times".
    pub summary: String,
}

/// What the guard tells the outside world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Raised(LoopSignal),
    Cleared(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Consecutive turns before a repeat counts (conservative default 4; clamped 3..=10).
    pub turns: u32,
    /// Minutes of `running` without new output before `stalled` fires (clamped 5..=240).
    pub stalled_minutes: u32,
    /// Register the global panic shortcut (Ctrl+Alt+Shift+P). Off by default.
    pub panic_shortcut: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, turns: 4, stalled_minutes: 20, panic_shortcut: false }
    }
}

impl Settings {
    pub fn clamped(mut self) -> Self {
        self.turns = self.turns.clamp(3, 10);
        self.stalled_minutes = self.stalled_minutes.clamp(5, 240);
        self
    }
}

// ---------------------------------------------------------------------------------------------
// Line classification

/// Generic error-likeness; profiles extend it (`TuiProfile::is_error_line`).
static ERROR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(error|errors|failed|failure|exception|traceback|panicked|fatal|cannot|can't|could not|couldn't|unable to|not found|undefined|permission denied|access denied|timed out|timeout|enoent|eacces|segmentation fault|unresolved|unexpected)\b|\bTS\d{4}\b|\bE\d{4}\b",
    )
    .expect("error regex")
});
/// Lines that mention errors without being one ("0 failed", "no errors", "fixed the error").
static NOT_ERROR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b0 (errors?|failed|failures?)\b|\bno (errors?|failures?|issues)\b|\bwithout (errors?|failures?)\b|\b(fixed|resolved|fixes|fixing)\b")
        .expect("not error regex")
});
static PROGRESS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(passed|passing|succe(ss|eded|ssful|ssfully)|compiled|built|created|wrote|written|saved|updated|edited|added|installed|committed|merged|done|completed|all tests)\b|[\u{2713}\u{2714}]",
    )
    .expect("progress regex")
});
/// A tool row printed by an agent: a leading bullet plus a verb. No agent names.
static TOOL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^[\s\u{25cf}\u{23fa}\u{2022}>*\u{b7}\u{2713}-]*(bash|read|edit|write|update|grep|search|list|glob|ran|edited|wrote|updated|searched|created|running|executing|fetch|fetched)\b[\s(:].{2,}",
    )
    .expect("tool regex")
});

/// The generic error-like test every profile starts from.
pub fn default_error_line(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && line.chars().count() <= 400 && ERROR_RE.is_match(line) && !NOT_ERROR_RE.is_match(line)
}

fn default_tool_line(line: &str) -> bool {
    TOOL_RE.is_match(line)
}

/// How one agent's lines are classified (profile-backed in production, generic in tests).
pub trait LineKinds {
    fn is_error(&self, line: &str) -> bool;
    fn is_tool(&self, line: &str) -> bool;
    fn is_chrome(&self, line: &str) -> bool;
}

/// Generic classification with no agent knowledge.
pub struct GenericKinds;

impl LineKinds for GenericKinds {
    fn is_error(&self, line: &str) -> bool {
        default_error_line(line)
    }
    fn is_tool(&self, line: &str) -> bool {
        default_tool_line(line)
    }
    fn is_chrome(&self, _line: &str) -> bool {
        false
    }
}

/// Classification through a pane's `TuiProfile`.
pub struct ProfileKinds(pub std::sync::Arc<dyn crate::tui::TuiProfile>);

impl LineKinds for ProfileKinds {
    fn is_error(&self, line: &str) -> bool {
        self.0.is_error_line(line)
    }
    fn is_tool(&self, line: &str) -> bool {
        self.0.is_tool_activity_line(&line.to_lowercase()) || default_tool_line(line)
    }
    fn is_chrome(&self, line: &str) -> bool {
        self.0.is_chrome_line(&line.to_lowercase())
    }
}

/// Strips what changes between two occurrences of the same message: numbers, timestamps, hashes,
/// spinner glyphs, punctuation and (unless `keep_paths`) file paths. Lowercased words only.
pub fn normalize(line: &str, keep_paths: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    for tok in line.split_whitespace() {
        if tok.contains('/') || tok.contains('\\') {
            if keep_paths {
                let kept: String = tok
                    .chars()
                    .filter(|c| c.is_alphabetic() || matches!(c, '/' | '\\' | '.' | '_' | '-'))
                    .flat_map(char::to_lowercase)
                    .collect();
                let kept = kept.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
                if !kept.is_empty() {
                    out.push(kept);
                }
            }
            continue;
        }
        let bare = tok.trim_matches(|c: char| !c.is_alphanumeric());
        let digits = bare.chars().take_while(|c| c.is_ascii_digit() || *c == '.').count();
        if digits > 0 && bare.chars().skip(digits).count() <= 2 && bare.chars().skip(digits).all(|c| c.is_alphabetic()) {
            continue; // numbers and durations: 12, 3.4s, 250ms, 5m
        }
        let has_digit = tok.chars().any(|c| c.is_ascii_digit());
        let has_letter = tok.chars().any(|c| c.is_alphabetic());
        if has_digit && has_letter && tok.chars().count() >= 6 {
            continue; // ids, hashes, error codes with digits
        }
        let cleaned: String = tok.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
        if !cleaned.is_empty() {
            out.push(cleaned);
        }
    }
    out.join(" ")
}

/// Same error after normalisation: equal, or (for non-trivial lines) near-duplicate.
fn same_error(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    a.chars().count() >= 8 && b.chars().count() >= 8 && strsim::normalized_levenshtein(a, b) >= SIMILARITY
}

fn clip(line: &str) -> String {
    let line = line.trim();
    if line.chars().count() <= MAX_LINE {
        line.to_string()
    } else {
        let cut: String = line.chars().take(MAX_LINE - 1).collect();
        format!("{cut}\u{2026}")
    }
}

// ---------------------------------------------------------------------------------------------
// Per-pane tracking

#[derive(Clone, Default)]
struct Turn {
    started: u64,
    /// (normalised key, original line), one per distinct key.
    errors: Vec<(String, String)>,
    /// (normalised key with paths, original line), consecutive repeats collapsed.
    actions: Vec<(String, String)>,
    progress: bool,
}

#[derive(Clone)]
struct TurnRecord {
    turn: Turn,
    ended: u64,
}

struct Active {
    signal: LoopSignal,
    dismissed: bool,
}

#[derive(Default)]
struct PaneTrack {
    seeded: bool,
    prev: HashMap<String, usize>,
    status: Option<PaneStatus>,
    turn: Turn,
    turns: VecDeque<TurnRecord>,
    running_since: Option<u64>,
    last_new_at: u64,
    seen_this_run: HashSet<String>,
    last_lines: Vec<String>,
    active: Option<Active>,
}

struct Detection {
    kind: LoopKind,
    evidence: Vec<String>,
    turns: u32,
    since: u64,
    summary: String,
}

/// The detector. Pure: time is passed in, so tests drive it with a fake clock.
#[derive(Default)]
pub struct Guard {
    pub settings: Settings,
    panes: HashMap<String, PaneTrack>,
}

impl Guard {
    pub fn new(settings: Settings) -> Self {
        Self { settings: settings.clamped(), panes: HashMap::new() }
    }

    pub fn set_settings(&mut self, settings: Settings) -> Vec<Event> {
        self.settings = settings.clamped();
        if self.settings.enabled {
            return Vec::new();
        }
        // Turning it off clears every card.
        let mut events = Vec::new();
        for (id, track) in self.panes.iter_mut() {
            if track.active.take().is_some() {
                events.push(Event::Cleared(id.clone()));
            }
        }
        events
    }

    /// Feed one observation of a pane: its visible tail (ANSI stripped) and status.
    pub fn observe(&mut self, pane_id: &str, tail: &str, status: PaneStatus, kinds: &dyn LineKinds, now: u64) -> Vec<Event> {
        if !self.settings.enabled {
            return Vec::new();
        }
        let settings = self.settings;
        let track = self.panes.entry(pane_id.to_string()).or_default();

        // New lines since the previous observation.
        let lines: Vec<&str> = tail.lines().map(str::trim).filter(|l| !l.is_empty() && !kinds.is_chrome(l)).collect();
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut fresh: Vec<&str> = Vec::new();
        let mut remaining = track.prev.clone();
        for line in &lines {
            let key = normalize(line, true);
            if key.is_empty() {
                continue;
            }
            *counts.entry(key.clone()).or_insert(0) += 1;
            match remaining.get_mut(&key) {
                Some(n) if *n > 0 => *n -= 1,
                _ => {
                    if track.seeded {
                        fresh.push(line);
                    }
                }
            }
        }
        track.prev = counts;
        track.seeded = true;
        track.last_lines = lines.iter().rev().take(MAX_EVIDENCE).rev().map(|l| clip(l)).collect();

        // A turn starts when the pane starts running.
        let prev_status = track.status;
        if status == PaneStatus::Running && prev_status != Some(PaneStatus::Running) && prev_status != Some(PaneStatus::WaitingInput) {
            track.turn = Turn { started: now, ..Turn::default() };
            track.running_since = Some(now);
            track.last_new_at = now;
            track.seen_this_run.clear();
        }

        for line in fresh {
            if kinds.is_error(line) {
                let key = normalize(line, false);
                if !key.is_empty() && !track.turn.errors.iter().any(|(k, _)| *k == key) {
                    track.turn.errors.push((key, clip(line)));
                }
            } else {
                if kinds.is_tool(line) {
                    let key = normalize(line, true);
                    if track.turn.actions.last().map(|(k, _)| k != &key).unwrap_or(true) {
                        track.turn.actions.push((key, clip(line)));
                    }
                }
                if PROGRESS_RE.is_match(line) {
                    track.turn.progress = true;
                }
            }
            if status == PaneStatus::Running && track.seen_this_run.insert(normalize(line, true)) {
                track.last_new_at = now;
            }
        }

        // A turn ends when a running pane settles.
        let turn_ended = prev_status == Some(PaneStatus::Running) && matches!(status, PaneStatus::Idle | PaneStatus::Error);
        if turn_ended {
            let record = TurnRecord { turn: std::mem::take(&mut track.turn), ended: now };
            track.turns.push_back(record);
            while track.turns.len() > MAX_TURNS {
                track.turns.pop_front();
            }
        }
        if status != PaneStatus::Running && status != PaneStatus::WaitingInput {
            track.running_since = None;
        }
        track.status = Some(status);

        Self::settle(pane_id, track, &settings, now)
    }

    /// Time-based pass for panes that went quiet (the `stalled` signal). Cheap; call about once a second.
    pub fn tick(&mut self, now: u64) -> Vec<Event> {
        if !self.settings.enabled {
            return Vec::new();
        }
        let settings = self.settings;
        let mut events = Vec::new();
        for (id, track) in self.panes.iter_mut() {
            if track.status == Some(PaneStatus::Running) {
                events.extend(Self::settle(id, track, &settings, now));
            }
        }
        events
    }

    /// Forget panes that no longer exist.
    pub fn retain(&mut self, alive: &dyn Fn(&str) -> bool) -> Vec<Event> {
        let mut events = Vec::new();
        self.panes.retain(|id, track| {
            let keep = alive(id);
            if !keep && track.active.is_some() {
                events.push(Event::Cleared(id.clone()));
            }
            keep
        });
        events
    }

    /// The signal currently shown for a pane (not dismissed).
    pub fn status(&self, pane_id: &str) -> Option<LoopSignal> {
        let active = self.panes.get(pane_id)?.active.as_ref()?;
        (!active.dismissed).then(|| active.signal.clone())
    }

    pub fn all_active(&self) -> Vec<LoopSignal> {
        self.panes.values().filter_map(|t| t.active.as_ref()).filter(|a| !a.dismissed).map(|a| a.signal.clone()).collect()
    }

    /// Suppress the current episode. A later episode (after the signal clears) still alerts.
    pub fn dismiss(&mut self, pane_id: &str) -> bool {
        match self.panes.get_mut(pane_id).and_then(|t| t.active.as_mut()) {
            Some(active) => {
                active.dismissed = true;
                true
            }
            None => false,
        }
    }

    fn settle(pane_id: &str, track: &mut PaneTrack, settings: &Settings, now: u64) -> Vec<Event> {
        match (Self::detect(track, settings, now), track.active.is_some()) {
            (Some(d), false) => {
                let signal = LoopSignal {
                    pane_id: pane_id.to_string(),
                    kind: d.kind,
                    evidence_lines: d.evidence,
                    turns: d.turns,
                    since: d.since,
                    summary: d.summary,
                };
                track.active = Some(Active { signal: signal.clone(), dismissed: false });
                vec![Event::Raised(signal)]
            }
            (None, true) => {
                track.active = None;
                vec![Event::Cleared(pane_id.to_string())]
            }
            _ => Vec::new(),
        }
    }

    fn detect(track: &PaneTrack, settings: &Settings, now: u64) -> Option<Detection> {
        let n = settings.turns as usize;
        let records: Vec<&TurnRecord> = track.turns.iter().collect();

        // (d) running for a long time without anything new.
        let stall_ms = settings.stalled_minutes as u64 * 60_000;
        if track.status == Some(PaneStatus::Running) {
            if let Some(since) = track.running_since {
                if now.saturating_sub(since) >= stall_ms && now.saturating_sub(track.last_new_at) >= stall_ms {
                    let minutes = (now.saturating_sub(track.last_new_at) / 60_000) as u32;
                    return Some(Detection {
                        kind: LoopKind::Stalled,
                        evidence: track.last_lines.clone(),
                        turns: minutes,
                        since: track.last_new_at,
                        summary: format!("no new output for {minutes} minutes"),
                    });
                }
            }
        }

        if records.len() >= n {
            let last = &records[records.len() - n..];
            // (a) the same error in each of the last N turns.
            if let Some(newest) = last.last() {
                let mut shared: Vec<String> = Vec::new();
                for (key, original) in &newest.turn.errors {
                    if last.iter().all(|r| r.turn.errors.iter().any(|(k, _)| same_error(k, key))) {
                        shared.push(original.clone());
                    }
                }
                if !shared.is_empty() {
                    shared.truncate(MAX_EVIDENCE);
                    let key = &newest.turn.errors.iter().find(|(_, o)| *o == shared[0]).map(|(k, _)| k.clone()).unwrap_or_default();
                    let mut count = 0u32;
                    for r in records.iter().rev() {
                        if r.turn.errors.iter().any(|(k, _)| same_error(k, key)) {
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    return Some(Detection {
                        kind: LoopKind::RepeatedError,
                        evidence: shared,
                        turns: count,
                        since: last[0].turn.started,
                        summary: format!("the same error {count} times"),
                    });
                }
            }
            // (b) the same actions in the same order, never any sign of progress.
            let sig = |r: &TurnRecord| r.turn.actions.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join("|");
            let first_sig = sig(last[0]);
            if !first_sig.is_empty()
                && last.iter().all(|r| sig(r) == first_sig && !r.turn.progress && r.turn.errors.is_empty())
            {
                let newest = last.last().expect("n >= 3");
                let evidence: Vec<String> = newest.turn.actions.iter().take(MAX_EVIDENCE).map(|(_, o)| o.clone()).collect();
                return Some(Detection {
                    kind: LoopKind::RepeatedAction,
                    evidence,
                    turns: n as u32,
                    since: last[0].turn.started,
                    summary: format!("repeating the same steps {n} times"),
                });
            }
        }

        // (c) many busy turns in a short window, none showing progress.
        let m = n * 2;
        if records.len() >= m {
            let last = &records[records.len() - m..];
            let span = last.last().map(|r| r.ended).unwrap_or(0).saturating_sub(last[0].turn.started);
            let busy = |r: &TurnRecord| !r.turn.actions.is_empty() || !r.turn.errors.is_empty();
            if span <= NO_PROGRESS_WINDOW_MS && last.iter().all(|r| !r.turn.progress && busy(r)) {
                let newest = last.last().expect("m >= 6");
                let mut evidence: Vec<String> = newest.turn.errors.iter().map(|(_, o)| o.clone()).collect();
                evidence.extend(newest.turn.actions.iter().map(|(_, o)| o.clone()));
                evidence.truncate(MAX_EVIDENCE);
                return Some(Detection {
                    kind: LoopKind::NoProgress,
                    evidence,
                    turns: m as u32,
                    since: last[0].turn.started,
                    summary: format!("{m} rounds in a few minutes without getting anywhere"),
                });
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------------------------
// Process-wide instance and wiring

type Sink = Box<dyn Fn(&Event) + Send + Sync>;

static GUARD: Lazy<Mutex<Guard>> = Lazy::new(|| Mutex::new(Guard::new(Settings::default())));
static SINK: Lazy<Mutex<Option<Sink>>> = Lazy::new(|| Mutex::new(None));

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn deliver(events: Vec<Event>) {
    if events.is_empty() {
        return;
    }
    if let Some(sink) = SINK.lock().as_ref() {
        for event in &events {
            sink(event);
        }
    }
}

/// Called by the activity ticker for each pane whose screen or status changed.
pub fn observe_pane(pane_id: &str, agent_type: &str, tail: &str, status: PaneStatus) {
    let kinds = ProfileKinds(crate::tui::profile_for(agent_type));
    let events = GUARD.lock().observe(pane_id, tail, status, &kinds, now_ms());
    deliver(events);
}

/// Called by the activity ticker once per tick.
pub fn tick(alive: &dyn Fn(&str) -> bool) {
    let events = {
        let mut guard = GUARD.lock();
        let mut events = guard.retain(alive);
        events.extend(guard.tick(now_ms()));
        events
    };
    deliver(events);
}

pub fn status(pane_id: &str) -> Option<LoopSignal> {
    GUARD.lock().status(pane_id)
}

pub fn all_active() -> Vec<LoopSignal> {
    GUARD.lock().all_active()
}

pub fn dismiss(pane_id: &str) -> bool {
    GUARD.lock().dismiss(pane_id)
}

pub fn settings() -> Settings {
    GUARD.lock().settings
}

pub fn set_settings(settings: Settings) -> Settings {
    let (events, applied) = {
        let mut guard = GUARD.lock();
        let events = guard.set_settings(settings);
        (events, guard.settings)
    };
    deliver(events);
    save_settings(&applied);
    applied
}

fn settings_path() -> std::path::PathBuf {
    crate::app_paths::app_data_dir().join("loop_guard.json")
}

fn save_settings(settings: &Settings) {
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(settings_path(), text);
    }
}

/// Load saved settings and route events to the UI (Tauri event + SSE). Call once at startup.
pub fn init(app: tauri::AppHandle) {
    if let Some(saved) = std::fs::read_to_string(settings_path()).ok().and_then(|t| serde_json::from_str::<Settings>(&t).ok()) {
        GUARD.lock().settings = saved.clamped();
    }
    *SINK.lock() = Some(Box::new(move |event| {
        use tauri::Emitter;
        match event {
            Event::Raised(signal) => {
                let _ = app.emit("loop://signal", signal);
                crate::bridge::push_loop_signal_sse(Some(signal), &signal.pane_id);
            }
            Event::Cleared(pane_id) => {
                let _ = app.emit("loop://clear", serde_json::json!({ "pane_id": pane_id }));
                crate::bridge::push_loop_signal_sse(None, pane_id);
            }
        }
    }));
}

#[cfg(test)]
mod tests;
