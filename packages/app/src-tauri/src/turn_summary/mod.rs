//! Deterministic turn summaries (no LLM) and the "While you were away" digest.
//! See `docs/features/turn-summary-and-away-digest.md`.
//!
//! - `model`: `TurnSummary` and the one-line sentence.
//! - `tracker`: finds turns from pane status, keeps answered prompts, builds and stores summaries.
//! - `store`: last 100 per pane in memory plus a JSON ring file per project.
//! - `digest`, `presence`: the away digest and the idle/focus state machine.
//! - `http`: `GET /digest?since=` for the phone.
//!
//! This file is the glue to the running app (registry, checkpoints, Tauri events, SSE).

pub mod blame;
pub mod digest;
pub mod http;
pub mod model;
pub mod pending;
pub mod presence;
pub mod settle;
pub mod store;
pub mod tracker;
#[cfg(test)]
mod blame_tests;
#[cfg(test)]
mod focus_tests;
#[cfg(test)]
mod sentence_tests;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;
use tauri::Emitter;

pub use digest::{Digest, NeedsYou};
pub use model::{Approval, FileStats, ReceiptFacts, TurnSummary};
use tracker::{Env, PaneInfo, Tracker};

/// How long a finished turn waits for its "after" checkpoint before the summary is built anyway.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(25);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct LiveEnv;

impl Env for LiveEnv {
    fn now_ms(&self) -> u64 {
        now_ms()
    }

    fn pane(&self, pane_id: &str) -> Option<PaneInfo> {
        let facts = crate::bridge::checkpoint_pane_facts(pane_id)?;
        let profile = crate::tui::profile_for(&facts.agent_type);
        let label = profile
            .context()
            .map(|c| c.label.to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| facts.agent_type.clone());
        let account = crate::bridge::live_registry()
            .and_then(|registry| registry.lock().list().into_iter().find(|p| p.id == pane_id))
            .and_then(|info| info.account);
        Some(PaneInfo {
            cwd: facts.cwd,
            label,
            agent_id: facts.agent_type,
            account,
            has_turns: profile.features().kind != crate::tui::Kind::Shell,
        })
    }

    fn files(&self, pane_id: &str, started_at_ms: u64) -> Option<FileStats> {
        let changes = crate::checkpoints::live().turn_changes(pane_id, started_at_ms).ok().flatten()?;
        Some(FileStats::from_changes(&changes))
    }

    fn end_prompt(&self, pane_id: &str) -> Option<String> {
        let registry = crate::bridge::live_registry()?;
        describe_prompt(&registry, pane_id).map(|(_, text)| text)
    }

    fn receipt(&self, pane_id: &str) -> Option<ReceiptFacts> {
        let registry = crate::bridge::live_registry()?;
        let info = crate::agent_runs::pane_api::pane_run_info(&registry, pane_id).ok().flatten()?;
        let receipt = info.get("receipt")?;
        let text = |key: &str| receipt.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let (checks, outcome) = (text("checks"), text("outcome"));
        (!outcome.is_empty()).then_some(ReceiptFacts { checks, outcome })
    }
}

/// How long a run's pane waits, before it is closed, for the summary of its last turn to be built.
/// Normally the "after" checkpoint takes well under a second; past this the pane closes anyway and the
/// summary is still recorded from the facts captured at the rest point (without the live-only ones).
pub const DISPOSE_WAIT: Duration = Duration::from_secs(10);

/// Summaries being built right now, per pane.
static PENDING: Lazy<pending::Pending> = Lazy::new(pending::Pending::default);

/// Marks a summary of `pane_id` as in flight until the guard is dropped.
pub fn summary_in_flight(pane_id: &str) -> pending::InFlight {
    pending::InFlight::new(&PENDING, pane_id)
}

/// Blocks, at most `timeout`, until no summary of `pane_id` is being built. Call it before closing a
/// pane (see `agent_runs::pane_close`). Returns whether none is left.
pub fn wait_for_summary(pane_id: &str, timeout: Duration) -> bool {
    PENDING.wait(pane_id, timeout)
}

/// Test only: how many waits for `pane_id` went to sleep because a summary of it was in flight.
#[cfg(test)]
pub fn blocked_waits(pane_id: &str) -> u32 {
    PENDING.blocked_waits(pane_id)
}

static TRACKER: Lazy<Tracker> = Lazy::new(|| Tracker::new(Arc::new(LiveEnv)));

pub fn tracker() -> &'static Tracker {
    &TRACKER
}

/// The turn number for a checkpoint taken now (see `Tracker::turn_for_checkpoint`).
pub fn checkpoint_turn(pane_id: &str, before: bool) -> Option<u32> {
    tracker().turn_for_checkpoint(pane_id, before)
}

/// `(kind, short text)` of the prompt a pane shows right now: `command` for a command approval.
fn describe_prompt(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
) -> Option<(String, String)> {
    let prompt = crate::pane_prompt::detect(registry, pane_id, false)?;
    let text = |key: &str| prompt.get(key).and_then(Value::as_str).filter(|t| !t.trim().is_empty()).map(str::to_string);
    let kind = if text("command").is_some() { "command".to_string() } else { text("kind").unwrap_or_else(|| "request".into()) };
    let body = text("summary").or_else(|| text("text")).or_else(|| text("command")).unwrap_or_default();
    Some((kind, model::shorten(&body)))
}

// ---- turn boundaries ----

/// Tickets of the "after" checkpoint captures (the summary waits for its own).
static SETTLE: Lazy<settle::Settle> = Lazy::new(settle::Settle::default);

/// An "after" capture of `pane_id` is about to start (called on the status-event thread by the
/// checkpoints hook). The returned ticket goes to [`checkpoint_settled`] when it is done.
pub fn checkpoint_requested(pane_id: &str) -> u64 {
    SETTLE.request(pane_id)
}

/// Called by the checkpoints worker when the "after" capture behind `ticket` is done (or skipped).
pub fn checkpoint_settled(pane_id: &str, ticket: u64) {
    SETTLE.settle(pane_id, ticket);
}

/// A prompt was typed into `pane_id` (see `Tracker::note_dispatched`).
pub fn note_dispatched(pane_id: &str) {
    tracker().note_dispatched(pane_id);
}

/// Waits for the rest point's own checkpoint capture, then reads the facts and builds (and keeps) the
/// summary. Blocking; run on a worker.
fn summarize(
    tracker: &Tracker,
    settle: &settle::Settle,
    ended: tracker::Ended,
    ticket: u64,
    timeout: Duration,
) -> Option<TurnSummary> {
    settle.wait(&ended.pane_id, ticket, timeout);
    tracker.finish(ended)
}

/// Hook for `bridge/sse.rs::push_pane_status_sse` (called after the checkpoints hook). Cheap on
/// the caller's thread: the summary is built on a short-lived worker.
pub fn on_pane_status(pane_id: &str, status: &str) {
    let Some(ended) = tracker().on_status(pane_id, status) else { return };
    // The checkpoints hook ran just before this one: the newest ticket is this rest point's own
    // capture (a later one only makes the wait longer, never shorter).
    let ticket = if ended.from_running { SETTLE.latest(pane_id) } else { 0 };
    // Taken here, on the status-event thread, so a pane disposal right after this event waits for it.
    let in_flight = summary_in_flight(&ended.pane_id);
    let spawned = std::thread::Builder::new().name("pm-turn-summary".into()).spawn(move || {
        let _in_flight = in_flight;
        if let Some(summary) = summarize(tracker(), &SETTLE, ended, ticket, SETTLE_TIMEOUT) {
            publish(&summary);
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start turn summary thread");
    }
}

/// `turn://summary` (Tauri) and `event: turn-summary` (SSE for the phone).
fn publish(summary: &TurnSummary) {
    if let Some(app) = crate::bridge::live_app() {
        let _ = app.emit("turn://summary", summary);
    }
    if let Ok(json) = serde_json::to_string(summary) {
        crate::bridge::push_sse(format!("event: turn-summary\ndata: {json}\n\n"));
    }
}

/// The approval entry for an answer. `shown` is what the pane displayed just before the answer was
/// sent (`prompt_before_answer`).
fn approval_for(prompt_id: &str, choice: &str, shown: Option<(String, String)>) -> Approval {
    let (kind, prompt) = shown.unwrap_or_else(|| ("request".into(), String::new()));
    let decision = match choice {
        "deny" | "reject" | "no" => "deny",
        _ if kind.contains("question") => "answer",
        _ => "allow",
    };
    Approval { at_ms: now_ms(), prompt_id: prompt_id.to_string(), prompt, kind, decision: decision.into() }
}

/// Record that the user answered the prompt on `pane_id`, for an answer that is already sent. Prefer
/// [`answering`]: noting an answer after its key was typed can lose it to a fast agent.
pub fn record_answer(pane_id: &str, prompt_id: &str, choice: &str, shown: Option<(String, String)>) {
    tracker().record_approval(pane_id, approval_for(prompt_id, choice, shown));
}

/// Sends an answer to the prompt on `pane_id` with `send` and notes it on the pane's open turn
/// BEFORE the key goes out (taken back if `delivered(&result)` is false). See `Tracker::answering`.
pub fn answering<T>(
    pane_id: &str,
    prompt_id: &str,
    choice: &str,
    shown: Option<(String, String)>,
    send: impl FnOnce() -> T,
    delivered: impl FnOnce(&T) -> bool,
) -> T {
    tracker().answering(pane_id, approval_for(prompt_id, choice, shown), send, delivered)
}

/// What the pane shows right now, captured before an answer is sent (the answer clears it).
pub fn prompt_before_answer(pane_id: &str) -> Option<(String, String)> {
    describe_prompt(&crate::bridge::live_registry()?, pane_id)
}

// ---- digest ----

/// Everything that needs the user right now. It is the attention list (`attention::`), so the
/// digest's "Needs you" has exactly as many lines as the Inbox, the tray badge and the phone show.
pub fn needs_you() -> Vec<NeedsYou> {
    needs_you_from(&crate::attention::snapshot_now())
}

/// The open blocking items of an attention snapshot, in its order, as digest lines.
pub fn needs_you_from(snapshot: &crate::attention::model::Snapshot) -> Vec<NeedsYou> {
    use crate::attention::model::{Kind, Lane, State};
    snapshot
        .items
        .iter()
        .filter(|item| item.state == State::Open && item.lane == Lane::Blocking)
        .map(|item| NeedsYou {
            id: Some(item.id.clone()),
            pane_id: item.pane.as_ref().map(|p| p.id.clone()).unwrap_or_default(),
            agent: item.pane.as_ref().and_then(|p| p.agent.clone()).unwrap_or_else(|| "An agent".to_string()),
            status: if item.kind == Kind::Error { "error" } else { "waiting_input" }.to_string(),
            prompt: item.sentence.clone().or_else(|| Some(item.title.clone())).filter(|t| !t.is_empty()),
        })
        .collect()
}

/// The digest for `since_ms..now` from the live tracker.
pub fn current_digest(since_ms: u64) -> Digest {
    let now = now_ms();
    digest::build(&tracker().since(since_ms), needs_you(), since_ms, now)
}

// ---- desktop presence ----

struct Desk {
    presence: presence::Presence,
    /// A digest waiting for the cockpit (shown when it is opened or focused).
    pending: Option<Digest>,
}

static DESK: Lazy<Mutex<Desk>> =
    Lazy::new(|| Mutex::new(Desk { presence: presence::Presence::new(presence::DEFAULT_AWAY_MS), pending: None }));

/// The setting `away_minutes` (public settings; default 5, 0 = off) in milliseconds.
fn away_threshold_ms() -> u64 {
    let minutes = crate::bridge::checkpoint_public_settings()
        .and_then(|v| v.get("away_minutes").and_then(Value::as_u64))
        .unwrap_or(presence::DEFAULT_AWAY_MS / 60_000);
    minutes.min(24 * 60) * 60_000
}

fn on_return(ret: presence::Return) {
    if let Some(digest) = away_digest(ret.since_ms, crate::attention::focus::is_on()) {
        offer(digest);
    }
}

/// The "While you were away" card for a return, or `None`. While Focus is on there is none: the Focus summary
/// replaces it (one summary, not two).
fn away_digest(since_ms: u64, focus_on: bool) -> Option<Digest> {
    if focus_on {
        return None;
    }
    Some(current_digest(since_ms)).filter(|digest| !digest.is_empty())
}

/// Park a digest for the cockpit and tell it.
fn offer(digest: Digest) {
    DESK.lock().pending = Some(digest.clone());
    if let Some(app) = crate::bridge::live_app() {
        let _ = app.emit("digest://ready", &digest);
    }
}

/// The digest of a finished Focus: the same per-agent lines the away digest shows (same redaction), for the
/// window the Focus ran, with the Focus summary on top. Shown even when no agent line exists (held updates, or
/// cards that asked for you).
pub fn offer_focus_digest(summary: crate::attention::focus::summary::FocusSummary) {
    offer(focus_digest(&summary, &tracker().since(summary.started_at_ms), needs_you()));
}

/// Pure part of `offer_focus_digest`: turns that ended after the Focus did are left out.
fn focus_digest(
    summary: &crate::attention::focus::summary::FocusSummary,
    turns: &[TurnSummary],
    needs_you: Vec<NeedsYou>,
) -> Digest {
    let within: Vec<TurnSummary> = turns.iter().filter(|t| t.ended_at_ms <= summary.ended_at_ms).cloned().collect();
    let mut digest = digest::build(&within, needs_you, summary.started_at_ms, summary.ended_at_ms);
    digest.focus = Some(digest::DigestFocus::from(summary));
    digest
}

/// Start the idle poller (call once from `setup`).
pub fn start_presence() {
    let spawned = std::thread::Builder::new().name("pm-presence".into()).spawn(|| loop {
        std::thread::sleep(Duration::from_secs(5));
        let Some(idle) = presence::system_idle_ms() else { continue };
        let threshold = away_threshold_ms();
        let ret = {
            let mut desk = DESK.lock();
            desk.presence.set_threshold(threshold);
            desk.presence.tick(now_ms(), idle)
        };
        if let Some(ret) = ret {
            on_return(ret);
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start presence thread");
    }
}

/// The cockpit window gained or lost focus (or was just opened: `true`).
pub fn cockpit_focus(focused: bool) {
    let ret = DESK.lock().presence.set_focus(focused);
    if let Some(ret) = ret {
        on_return(ret);
    }
}

/// The digest waiting for the cockpit, if any (a cockpit that opens after the return fetches it).
pub fn pending_digest() -> Option<Digest> {
    DESK.lock().pending.clone()
}

pub fn dismiss_digest() {
    DESK.lock().pending = None;
}

// ---- phone: per-device "last seen" ----

static LAST_SEEN: Lazy<Mutex<digest::LastSeen>> = Lazy::new(|| {
    let path: PathBuf = crate::app_paths::app_data_dir().join("digest-last-seen.json");
    Mutex::new(digest::LastSeen::load(&path))
});

pub fn last_seen() -> &'static Mutex<digest::LastSeen> {
    &LAST_SEEN
}
