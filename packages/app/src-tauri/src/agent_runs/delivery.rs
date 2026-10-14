//! Verified prompt delivery for visible TUI panes.
//!
//! Typing a prompt and pressing Enter blind lost turns on Cursor: the keystrokes raced the TUI,
//! the box kept (or regained) a stale prompt, and the run was later reported against the wrong
//! text. The protocol here, for profiles that opt in (`TuiProfile::verifies_prompt_delivery`):
//!
//! 1. clear the input box, type the prompt WITHOUT Enter;
//! 2. verify the prompt (or a distinctive prefix) is visible in the input area, otherwise clear
//!    and type it again once; if it still is not there, fail with `PROMPT_NOT_SUBMITTED` instead
//!    of pressing Enter over an unknown input box;
//! 3. press Enter and verify a new turn started (the pane left idle, or the prompt now appears
//!    in the transcript region); Enter is repeated once if nothing changed.
//!
//! A profile that can read its input box from the screen (`TuiProfile::composer_state`) sharpens
//! step 3: the turn started when the box emptied or a working marker appeared, and text still
//! waiting in the box shortly after Enter means the Enter was swallowed (Codex reads an Enter that
//! follows typed text too quickly as a line break), so Enter is repeated once at that point
//! instead of after the full wait. Such a profile also declares a `submit_sequence().settle`, the
//! pause between the typed text and Enter.
//!
//! The pane I/O is behind [`DeliveryIo`] so the sequences are unit-tested without a PTY.

use super::readiness::{self, AgentProcess, Basis, HostShell, Observation, Readiness, TypingPermit, Verdict};
use crate::pty::WriteOrigin;
use crate::tui::{ComposerState, StartupGate, TuiProfile};
use std::sync::Arc;
use std::time::Duration;

/// Longest prompt prefix compared against the screen.
const HEAD_CHARS: usize = 40;
const TAIL_CHARS: usize = 30;
/// Prompts longer than this may scroll inside the box, so only their opening is compared.
const TAIL_CHECK_MAX: usize = 300;
/// Prompts longer than this may be collapsed into a paste placeholder by the TUI.
const LARGE_PROMPT_CHARS: usize = 200;
/// Shortest prefix that is distinctive enough to trust.
const MIN_HEAD_CHARS: usize = 8;
/// Bottom screen rows that count as "the input area" (box, hint row, footer).
const INPUT_AREA_ROWS: usize = 16;

/// The windows below are time budgets (polls x interval), kept at 2s to type, 3s to start and a
/// second Enter at 1.5s; only the interval is short so a healthy pane is confirmed within ~100ms
/// of the screen settling instead of up to 250ms later.
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const VISIBLE_POLLS: u32 = 25;
const STARTED_POLLS: u32 = 38;
const RESUBMIT_AT_POLL: u32 = 19;
/// When the profile can read its input box, text still sitting in it this many polls (about
/// 0.4s) after Enter was not submitted: the TUI empties the box at once when it takes a prompt.
const RESUBMIT_IF_TEXT_AT_POLL: u32 = 5;
/// Longest total wait for a slow-starting TUI to draw anything (cold kilo can take >5s). The
/// budget is shared by the wait before the first keystroke and any later blank-screen waits.
const READY_BUDGET_POLLS: u32 = 250;
/// Consecutive identical non-blank polls that count as "the TUI has drawn and settled".
const READY_STABLE_POLLS: u32 = 2;
/// Pause after the clear sequence so the TUI has processed it before the text arrives.
const CLEAR_PAUSE: Duration = Duration::from_millis(120);
/// Startup screens (an update notice, ...) closed during one delivery. More than this is a screen
/// that keeps coming back, not a startup sequence.
const MAX_DELIVERY_GATES: u32 = 4;
/// How long a startup screen may take to react to its dismiss keys, as polls of [`POLL_INTERVAL`].
const GATE_CHANGE_POLLS: u32 = 40;
/// How long one readiness look may wait for the pane to read `Ready` (polls of [`POLL_INTERVAL`],
/// about 10 s) before delivery stops and the run is parked with the screen.
const READY_POLLS: u32 = 125;
/// Polls a screen gets to settle after a startup screen was closed.
const GATE_SETTLE_POLLS: u32 = 25;
/// Pause after the clear sequence when typing without verification (the old blind path's pause).
const UNVERIFIED_CLEAR_PAUSE: Duration = Duration::from_millis(250);
/// How many times a look may be repeated because the pane printed between the look and the write.
const MAX_STALE_LOOKS: u32 = 20;

pub trait DeliveryIo {
    /// Writes run input under the permit it was issued with (`WriteOrigin::Task` for the clear
    /// sequence, the task text and its Enter; `WriteOrigin::Gate` for a startup screen's own keys).
    /// The live pane's write path refuses a stale permit with an error starting `PERMIT_STALE`.
    fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String>;
    fn screen(&mut self) -> String;
    /// The screen together with the pane's output position and agent process, for a readiness
    /// verdict. A fake that knows neither gets the screen alone.
    fn observe(&mut self) -> Observation {
        Observation::screen_only(self.screen())
    }
    /// True while the pane reports it is busy (a turn is running).
    fn busy(&mut self) -> bool;
    fn pause(&mut self, duration: Duration);
    /// Least pause between the typed prompt and Enter (the profile's `submit_sequence().settle`).
    fn enter_settle(&mut self) -> Duration {
        Duration::ZERO
    }
    /// The profile of the agent in the pane: its readiness recogniser, startup screens and input
    /// box reader. Default: the generic profile (no recogniser, no gates).
    fn profile(&mut self) -> Arc<dyn TuiProfile> {
        crate::tui::profile_for("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The prompt was typed, verified, submitted, and a new turn was observed starting.
    Started,
    /// Typed, verified and submitted, but no sign of a new turn yet; the turn watch keeps looking.
    SubmittedUnconfirmed,
    /// The prompt never appeared in the input box, so Enter was not pressed.
    NotVisible,
    /// A startup screen that only the user may pass (folder trust, sign-in), a dialog, or a notice
    /// that ignored its dismiss keys, is on screen. Nothing was submitted.
    Blocked(StartupGate),
    /// The agent is not running in the pane (it exited, or its command was not found). Nothing was
    /// typed into the shell that is left.
    AgentGone(HostShell),
    /// The pane never showed a screen that takes a task within the readiness window (blank, a
    /// splash, a screen no rule recognises, a turn still running). Nothing was typed. The caller
    /// parks the run with `why` and `screen`; it never falls back to a background run for this.
    NotReady { why: String, screen: String },
    WriteFailed(String),
}

/// One short line for the run's transcript: what delivering the task did, in plain words. The
/// trace it is made from holds raw screen rows (footers, model names, the folder), which are
/// diagnostics: they go to the `delivery_trace` stream, which readers only get on request.
pub fn delivery_summary(outcome: &Delivery, trace: &[String]) -> String {
    let attempts = trace.iter().filter(|line| line.starts_with("attempt ") && line.contains(": typing")).count();
    let closed_notice = trace.iter().any(|line| line.contains("closing it with its own keys"));
    let enter_again = trace.iter().any(|line| line.contains("Enter sent once more"));
    let composer_cleared = trace
        .iter()
        .rev()
        .find(|line| line.starts_with("enter poll "))
        .is_some_and(|line| line.contains("started=true") && line.contains("composer=Empty"));
    match outcome {
        Delivery::Started | Delivery::SubmittedUnconfirmed => {
            let mut text = String::from("Sent.");
            if closed_notice {
                text.push_str(" Closed a startup notice first.");
            }
            if attempts > 1 {
                text.push_str(" Typed it again after the first try was lost.");
            }
            if enter_again {
                text.push_str(" Enter was pressed twice.");
            }
            match outcome {
                Delivery::Started if composer_cleared => text.push_str(" Composer cleared."),
                Delivery::Started => text.push_str(" The agent started."),
                _ => text.push_str(" The agent has not shown it started yet; still watching."),
            }
            text
        }
        Delivery::NotVisible => {
            "Not sent. The text never showed up in the input box, so Enter was not pressed. The box was cleared; retrying is safe.".to_string()
        }
        Delivery::Blocked(gate) => format!(
            "Not sent. {}; nothing was typed or submitted. Deal with it in the pane, then retry.",
            gate.summary.trim_end_matches('.')
        ),
        Delivery::AgentGone(HostShell::AgentExited) => {
            "Not sent. The agent in the pane has exited; nothing was typed into the shell that is left.".to_string()
        }
        Delivery::AgentGone(HostShell::NotInstalled(command)) => format!(
            "Not sent. The terminal could not find the agent's command `{command}`; nothing was typed."
        ),
        Delivery::NotReady { why, .. } => format!(
            "Not sent yet. Waiting for the agent's input box: {why}. Nothing was typed."
        ),
        Delivery::WriteFailed(error) => format!("Not sent. Writing to the pane failed: {error}"),
    }
}

/// Writes a delivery to the run's transcript: the one-line summary as `delivery` (what people read),
/// the raw trace as `delivery_trace` (details on request). `again` is why the supervisor delivered
/// the task a second time.
pub fn record_delivery(project: &str, operation_id: &str, again: Option<&str>, outcome: &Delivery, trace: &[String]) {
    let mut summary = delivery_summary(outcome, trace);
    if let Some(why) = again {
        summary = format!("Sent again ({why}). {summary}");
    }
    super::transcript::append(project, operation_id, "delivery", &crate::redact::text(&summary));
    super::transcript::append(
        project,
        operation_id,
        super::transcript::TRACE_STREAM,
        &crate::redact::text(&format!("{outcome:?}\n{}", trace.join("\n"))),
    );
}

/// What a message should say when delivery did not happen.
pub const NOT_VISIBLE_MESSAGE: &str = "the prompt text never appeared in the pane's input box after two attempts, so it was not submitted; the input box was cleared, retrying is safe";

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn head_of(prompt: &str) -> Option<String> {
    let head: String = normalize(prompt).chars().take(HEAD_CHARS).collect();
    let head = head.trim_end().to_string();
    (head.chars().count() >= MIN_HEAD_CHARS).then_some(head)
}

fn bottom_rows(screen: &str, rows: usize) -> String {
    let lines: Vec<&str> = screen.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(rows)..].join("\n")
}

/// Whether the prompt (a distinctive prefix of it) is visible in the input area at the bottom of
/// the screen. A long or multi-line paste may be shown as a `[Pasted text ...]` placeholder, which
/// also counts. Prompts too short to be distinctive are accepted by a non-empty input area check
/// being impossible, so they fall back to "the whole prompt is visible".
pub fn prompt_visible_in_input(screen: &str, prompt: &str) -> bool {
    let area = normalize(&bottom_rows(screen, INPUT_AREA_ROWS));
    if let Some(head) = head_of(prompt) {
        // A multi-line or large paste may be shown as a placeholder instead of its text:
        // `[Pasted text #1 +5 lines]`, or Codex's `[Pasted Content 1889 chars]`.
        if (prompt.contains('\n') || prompt.chars().count() > LARGE_PROMPT_CHARS)
            && (area.contains("pasted text") || area.contains("pasted content"))
        {
            return true;
        }
        // A follow-up often starts like the previous prompt, so for prompts that fit the box
        // also require the end of the text, not just the shared opening.
        let norm = normalize(prompt);
        let len = norm.chars().count();
        let tail_ok = len <= HEAD_CHARS || len > TAIL_CHECK_MAX || {
            let tail: String = norm.chars().skip(len - TAIL_CHARS).collect();
            area.contains(tail.trim_start())
        };
        return area.contains(&head) && tail_ok;
    }
    let whole = normalize(prompt);
    !whole.is_empty() && area.contains(&whole)
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

/// Whether a new turn started after Enter: the pane is busy, or the prompt text now appears more
/// often on screen than before Enter (the TUI echoed it into the transcript), or the input area
/// no longer holds it while the screen changed.
pub fn turn_started(before: &str, now: &str, prompt: &str, busy: bool) -> bool {
    // Nothing on screen moved since Enter: the pane's "busy" flag is left over from typing the
    // prompt (any output flips an idle pane to running) or from the previous turn repainting.
    if before == now {
        return false;
    }
    if busy {
        return true;
    }
    let Some(head) = head_of(prompt) else {
        return normalize(before) != normalize(now);
    };
    let (before_n, now_n) = (normalize(before), normalize(now));
    if occurrences(&now_n, &head) > occurrences(&before_n, &head) {
        return true;
    }
    !prompt_visible_in_input(now, prompt) && before_n != now_n
}

/// What the input box says about a submit, for a profile that can read it. `Some(true)`: the turn
/// started (a working marker appeared, or the box emptied and the screen moved). `Some(false)`:
/// the text is still waiting, however busy the pane's flag looks (typing flips an idle pane to
/// running, so the flag proves nothing here). `None`: the profile cannot tell; use the generic
/// checks.
pub fn submit_confirmed(
    state: ComposerState,
    before_state: ComposerState,
    before: &str,
    now: &str,
) -> Option<bool> {
    match state {
        ComposerState::Unknown => None,
        ComposerState::HasText => Some(false),
        // A marker that was already up before Enter proves nothing about this submit.
        ComposerState::Working if before_state == ComposerState::Working => None,
        ComposerState::Working => Some(true),
        ComposerState::Empty => Some(before != now),
    }
}

/// Whether the supervisor must deliver the turn's prompt itself: the profile verifies delivery,
/// the run is already `running`, no dispatch closure is typing right now, the stage log shows the
/// dispatch never submitted the prompt, and this has not been tried yet.
pub fn needs_redelivery(
    profile_verifies: bool,
    running: bool,
    dispatch_in_flight: bool,
    prompt_dispatched: bool,
    already_redelivered: bool,
) -> bool {
    profile_verifies && running && !dispatch_in_flight && !prompt_dispatched && !already_redelivered
}

#[cfg(test)]
pub fn deliver_prompt(io: &mut dyn DeliveryIo, clear: Option<&str>, prompt: &str) -> Delivery {
    deliver_prompt_traced(io, clear, prompt, &mut Vec::new())
}

/// The few non-empty rows at the bottom of the screen, for a delivery trace line.
fn trace_rows(screen: &str) -> String {
    bottom_rows(screen, 5)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Polls until the screen has non-whitespace content that held still for a few polls. Keys typed
/// before a TUI's input handler exists are lost, so nothing is typed while the pane is blank.
/// Spends from `budget`; returns false when the budget ran out with the screen still blank.
fn wait_for_screen(io: &mut dyn DeliveryIo, budget: &mut u32) -> bool {
    let mut last = String::new();
    let mut stable = 0;
    while *budget > 0 {
        let screen = io.screen();
        if screen.trim().is_empty() {
            stable = 0;
        } else {
            if screen == last {
                stable += 1;
            } else {
                stable = 1;
            }
            if stable >= READY_STABLE_POLLS {
                return true;
            }
        }
        last = screen;
        *budget -= 1;
        io.pause(POLL_INTERVAL);
    }
    false
}

/// Why delivery stopped before typing anything.
enum Stop {
    Blocked(StartupGate),
    Gone(HostShell),
    NotReady { why: String, screen: String },
}

impl From<Stop> for Delivery {
    fn from(stop: Stop) -> Self {
        match stop {
            Stop::Blocked(gate) => Delivery::Blocked(gate),
            Stop::Gone(shell) => Delivery::AgentGone(shell),
            Stop::NotReady { why, screen } => Delivery::NotReady { why, screen },
        }
    }
}

/// Whether a write error is the pane saying the permit's screen is no longer the latest.
fn is_stale(error: &str) -> bool {
    error.starts_with(readiness::PERMIT_STALE)
}

/// Puts what delivery saw on the pane's flight recording as a mark (`pm:readiness=Gate(update_available)`),
/// once per change, so a recording of a run that got stuck shows what was on the pane and when.
/// A pane with no id (a test double) or no recording is skipped.
fn note_readiness(verdict: &Verdict, last: &mut Option<String>) {
    let label = match verdict.readiness() {
        Readiness::Ready => "Ready".to_string(),
        Readiness::Busy => "Busy".to_string(),
        Readiness::Gate(gate) => format!("Gate({})", gate.id),
        Readiness::HostShell(_) => "HostShell".to_string(),
        Readiness::Unknown => "Unknown".to_string(),
    };
    if last.as_deref() == Some(label.as_str()) {
        return;
    }
    if !verdict.pane_id().is_empty() {
        crate::flight::mark(verdict.pane_id(), &format!("readiness={label}"));
    }
    *last = Some(label);
}

/// Looks at the pane until it reads `Ready` and returns the permit for that look; a fresh verdict on
/// every poll. On the way, a startup screen that declares its own keys is closed with a gate permit
/// (and the screen must change and settle after them); any other gate, a dialog, or the agent gone
/// stops delivery. A screen that stays `Unknown` or `Busy` for the whole window stops it too, with
/// the reason and the screen, so the run is parked rather than typed into. Text already in a box the
/// profile can clear is accepted only when the window ends without an empty box (a menu that is
/// still being drawn reads as text). `when` names the moment for the trace.
fn await_ready(
    io: &mut dyn DeliveryIo,
    profile: &dyn TuiProfile,
    rounds: &mut u32,
    trace: &mut Vec<String>,
    when: &str,
) -> Result<TypingPermit, Stop> {
    let mut last: Option<Verdict> = None;
    let mut marked: Option<String> = None;
    for poll in 0..READY_POLLS {
        let verdict = readiness::assess(profile, io.observe());
        note_readiness(&verdict, &mut marked);
        match verdict.readiness().clone() {
            Readiness::Ready => {
                let last_poll = poll + 1 == READY_POLLS;
                if verdict.composer() == ComposerState::HasText && !last_poll {
                    last = Some(verdict);
                    io.pause(POLL_INTERVAL);
                    continue;
                }
                if poll > 0 || verdict.basis() == Basis::Inferred {
                    trace.push(format!(
                        "ready {when} after {poll} look(s) ({:?}, input box {:?})",
                        verdict.basis(),
                        verdict.composer()
                    ));
                }
                return Ok(verdict.typing_permit().expect("a Ready verdict issues a permit"));
            }
            Readiness::Gate(gate) => {
                let Some(permit) = verdict.gate_permit() else {
                    trace.push(format!("startup screen '{}' is on screen {when} and only the user may pass it", gate.id));
                    return Err(Stop::Blocked(gate));
                };
                if *rounds >= MAX_DELIVERY_GATES {
                    trace.push(format!("startup screen '{}' keeps coming back; not closing it again", gate.id));
                    return Err(Stop::Blocked(gate));
                }
                *rounds += 1;
                trace.push(format!(
                    "startup screen '{}' appeared {when}; not typing over it, closing it with its own keys",
                    gate.id
                ));
                let before = verdict.screen().to_string();
                match io.write(WriteOrigin::Gate(&permit), permit.keys(), false) {
                    // Audit: Puppet Master pressed keys on the person's behalf (which screen, which keys).
                    Ok(()) => crate::audit::note_gate_dismissal(gate.id, gate.summary, permit.keys()),
                    Err(error) if is_stale(&error) => continue,
                    Err(_) => return Err(Stop::Blocked(gate)),
                }
                let mut changed = false;
                for _ in 0..GATE_CHANGE_POLLS {
                    io.pause(POLL_INTERVAL);
                    if io.screen() != before {
                        changed = true;
                        break;
                    }
                }
                if !changed {
                    trace.push(format!("startup screen '{}' did not react to its keys", gate.id));
                    return Err(Stop::Blocked(gate));
                }
                // Let what replaced it finish drawing; the next look checks it is not another gate.
                let mut settle = GATE_SETTLE_POLLS;
                wait_for_screen(io, &mut settle);
            }
            Readiness::HostShell(shell) => {
                trace.push(format!("{} {when}; nothing is typed into the shell", verdict.why()));
                return Err(Stop::Gone(shell));
            }
            Readiness::Busy | Readiness::Unknown => {
                last = Some(verdict);
                io.pause(POLL_INTERVAL);
            }
        }
    }
    let (why, screen) = last
        .map(|verdict| (verdict.why().to_string(), verdict.screen().to_string()))
        .unwrap_or_else(|| ("the pane never showed the agent's input box".into(), io.screen()));
    trace.push(format!("not ready {when} within the readiness window: {why}"));
    Err(Stop::NotReady { why, screen })
}

/// [`deliver_prompt`] that also records what it saw, so a delivery that claimed success without a
/// turn can be diagnosed from the operation transcript.
///
/// Every write goes out under a permit (`agent_runs::readiness`): the clear sequence and the task
/// text each need a `Ready` verdict from a look taken right before them (the pane refuses the write
/// if anything was printed since, and delivery looks again), Enter rides on the permit of the text
/// it submits, and a startup screen's keys need that screen's gate permit.
pub fn deliver_prompt_traced(
    io: &mut dyn DeliveryIo,
    clear: Option<&str>,
    prompt: &str,
    trace: &mut Vec<String>,
) -> Delivery {
    let profile = io.profile();
    let profile = profile.as_ref();
    let mut visible = false;
    let mut ready_budget = READY_BUDGET_POLLS;
    let mut attempt = 0;
    let mut typed_any = false;
    let mut gate_rounds = 0;
    let mut stale_left = MAX_STALE_LOOKS;
    // The permit the task text went in under: Enter (and the clean-up clear) ride on it.
    let mut typed_under: Option<TypingPermit> = None;
    while attempt < 2 {
        if !wait_for_screen(io, &mut ready_budget) {
            trace.push("screen stayed empty: the TUI drew nothing within the startup budget".to_string());
            if typed_any {
                if let (Some(clear), Some(permit)) = (clear, typed_under.as_ref()) {
                    let _ = io.write(WriteOrigin::Task(permit), clear, false);
                }
                return Delivery::NotVisible;
            }
            return Delivery::NotReady {
                why: "the pane stayed blank".into(),
                screen: io.screen(),
            };
        }
        let mut permit = match await_ready(io, profile, &mut gate_rounds, trace, "before typing") {
            Ok(permit) => permit,
            Err(stop) => return stop.into(),
        };
        if let Some(clear) = clear {
            match io.write(WriteOrigin::Task(&permit), clear, false) {
                Ok(()) => {}
                Err(error) if is_stale(&error) && stale_left > 0 => {
                    stale_left -= 1;
                    trace.push(format!("the pane printed output after the readiness look; looking again ({error})"));
                    continue;
                }
                Err(error) => return Delivery::WriteFailed(error),
            }
            io.pause(CLEAR_PAUSE);
            // The settle and the clear pause took time, and a notice can draw in that time (live,
            // Codex 0.159.3: the look before saw no menu, 120 ms later the whole menu was up). The
            // text gets its own look and its own permit, right before it goes in.
            permit = match await_ready(io, profile, &mut gate_rounds, trace, "just before typing") {
                Ok(permit) => permit,
                Err(stop) => return stop.into(),
            };
        }
        let screen_now = io.screen();
        trace.push(format!("attempt {attempt}: typing; busy={} rows={}", io.busy(), trace_rows(&screen_now)));
        match io.write(WriteOrigin::Task(&permit), prompt, false) {
            Ok(()) => {}
            Err(error) if is_stale(&error) && stale_left > 0 => {
                stale_left -= 1;
                trace.push(format!("the pane printed output after the readiness look; looking again ({error})"));
                continue;
            }
            Err(error) => return Delivery::WriteFailed(error),
        }
        typed_any = true;
        typed_under = Some(permit);
        let mut late_gate = None;
        for poll in 0..VISIBLE_POLLS {
            io.pause(POLL_INTERVAL);
            let verdict = readiness::assess(profile, io.observe());
            if let Readiness::HostShell(shell) = verdict.readiness() {
                // The agent went away after the text went in: nothing more is typed, no Enter.
                trace.push(format!("typed poll {poll}: {}; Enter was not pressed", verdict.why()));
                return Delivery::AgentGone(shell.clone());
            }
            let screen = verdict.screen().to_string();
            if let Some(gate) = profile.startup_gate(&screen) {
                // A notice drew over the box after the keys went in: they went into the notice.
                trace.push(format!("typed poll {poll}: a startup screen appeared while typing"));
                late_gate = Some(gate);
                break;
            }
            let seen = prompt_visible_in_input(&screen, prompt);
            trace.push(format!("typed poll {poll}: visible={seen} busy={} rows={}", io.busy(), trace_rows(&screen)));
            if seen {
                visible = true;
                break;
            }
        }
        if visible {
            break;
        }
        if let Some(gate) = late_gate {
            // Closed (or refused) by the next look, then typed again; this attempt does not count,
            // the box never had the text.
            gate_rounds += 1;
            if gate_rounds > MAX_DELIVERY_GATES {
                return Delivery::Blocked(gate);
            }
            continue;
        }
        if io.screen().trim().is_empty() && ready_budget > 0 {
            // The TUI had not drawn when the keys went in (they are lost), so this attempt does
            // not count: wait for the screen and type again.
            trace.push(format!("attempt {attempt}: screen blank after typing, waiting for the TUI and retyping"));
            continue;
        }
        attempt += 1;
    }
    let Some(permit) = typed_under else {
        return Delivery::NotVisible;
    };
    if !visible {
        if let Some(clear) = clear {
            let _ = io.write(WriteOrigin::Task(&permit), clear, false);
        }
        trace.push(format!("not visible; bottom rows at give-up:\n{}", bottom_rows(&io.screen(), 20)));
        return Delivery::NotVisible;
    }
    // Some TUIs read an Enter that follows the typed text too closely as a line break in a paste.
    let settle = io.enter_settle();
    if !settle.is_zero() {
        io.pause(settle);
    }
    let verdict = readiness::assess(profile, io.observe());
    if let Readiness::HostShell(shell) = verdict.readiness() {
        trace.push(format!("{}; Enter was not pressed", verdict.why()));
        return Delivery::AgentGone(shell.clone());
    }
    let before = verdict.screen().to_string();
    if let Some(gate) = profile.startup_gate(&before) {
        // Enter on a notice chooses one of its options ("Update now"): never press it there.
        trace.push(format!("startup screen '{}' is on screen; Enter was not pressed", gate.id));
        return Delivery::Blocked(gate);
    }
    let before_state = profile.composer_state(&before);
    if let Err(error) = io.write(WriteOrigin::Task(&permit), "", true) {
        return Delivery::WriteFailed(error);
    }
    let mut resubmitted = false;
    for poll in 0..STARTED_POLLS {
        io.pause(POLL_INTERVAL);
        let busy = io.busy();
        let screen = io.screen();
        let state = profile.composer_state(&screen);
        let started = submit_confirmed(state, before_state, &before, &screen)
            .unwrap_or_else(|| turn_started(&before, &screen, prompt, busy));
        trace.push(format!("enter poll {poll}: started={started} busy={busy} composer={state:?} rows={}", trace_rows(&screen)));
        if started {
            return Delivery::Started;
        }
        // The typed text is verified present, so a second Enter cannot send anything else. A
        // profile that reads its box sees the swallowed Enter early; the others wait it out.
        let swallowed = state == ComposerState::HasText && poll == RESUBMIT_IF_TEXT_AT_POLL;
        if (swallowed || poll == RESUBMIT_AT_POLL) && !resubmitted {
            resubmitted = true;
            trace.push(format!("enter poll {poll}: Enter sent once more (composer={state:?})"));
            let _ = io.write(WriteOrigin::Task(&permit), "", true);
        }
    }
    Delivery::SubmittedUnconfirmed
}

/// Typing a task into a profile that cannot verify delivery (its input box is not readable enough to
/// confirm the text): still only under a permit. Waits for a drawn, settled screen and a `Ready`
/// verdict, clears the box with its own permit when the profile has a clear sequence, then types the
/// task and Enter under a fresh permit from a look taken right before. `Started` means typed and
/// submitted (nothing is verified); every other outcome typed nothing.
pub fn type_unverified(
    io: &mut dyn DeliveryIo,
    clear: Option<&str>,
    prompt: &str,
    trace: &mut Vec<String>,
) -> Delivery {
    let profile = io.profile();
    let profile = profile.as_ref();
    let mut ready_budget = READY_BUDGET_POLLS;
    let mut gate_rounds = 0;
    for _ in 0..=MAX_STALE_LOOKS {
        if !wait_for_screen(io, &mut ready_budget) {
            trace.push("screen stayed empty: the TUI drew nothing within the startup budget".to_string());
            return Delivery::NotReady { why: "the pane stayed blank".into(), screen: io.screen() };
        }
        let mut permit = match await_ready(io, profile, &mut gate_rounds, trace, "before typing") {
            Ok(permit) => permit,
            Err(stop) => return stop.into(),
        };
        if let Some(clear) = clear {
            match io.write(WriteOrigin::Task(&permit), clear, false) {
                Ok(()) => {}
                Err(error) if is_stale(&error) => continue,
                Err(error) => return Delivery::WriteFailed(error),
            }
            io.pause(UNVERIFIED_CLEAR_PAUSE);
            permit = match await_ready(io, profile, &mut gate_rounds, trace, "just before typing") {
                Ok(permit) => permit,
                Err(stop) => return stop.into(),
            };
        }
        match io.write(WriteOrigin::Task(&permit), prompt, true) {
            Ok(()) => return Delivery::Started,
            Err(error) if is_stale(&error) => {
                trace.push(format!("the pane printed output after the readiness look; looking again ({error})"));
                continue;
            }
            Err(error) => return Delivery::WriteFailed(error),
        }
    }
    Delivery::NotReady {
        why: "the pane kept printing output between every look and the keystroke".into(),
        screen: io.screen(),
    }
}

/// One look at a live pane: its uncropped screen with the output position it was read at, and what
/// the process tree says about the agent (`pty::agent_proc`). A pane that exited reads `Exited`.
pub fn observe_pane(registry: &std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>, pane_id: &str) -> Observation {
    let (screen, output_pos) = crate::pty::registry_read_snapshot_full_at(registry.as_ref(), pane_id).unwrap_or_default();
    let (pid, exited) = registry
        .lock()
        .panes
        .get(pane_id)
        .map(|pane| (pane.info.pid, *pane.exited.lock()))
        .unwrap_or((0, true));
    let agent = if exited {
        AgentProcess::Exited
    } else {
        crate::pty::agent_proc::state(pane_id, pid)
    };
    Observation {
        pane_id: pane_id.to_string(),
        screen,
        output_pos,
        agent,
    }
}

/// [`DeliveryIo`] over a live pane.
pub struct PaneIo<'a> {
    pub registry: &'a std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    pub app: &'a tauri::AppHandle,
    pub pane_id: &'a str,
}

impl DeliveryIo for PaneIo<'_> {
    fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
        crate::pty::registry_write_input(self.registry, self.app, self.pane_id, text, enter, false, None, origin)
    }

    fn screen(&mut self) -> String {
        crate::pty::registry_read_snapshot_full(self.registry.as_ref(), self.pane_id).unwrap_or_default()
    }

    fn observe(&mut self) -> Observation {
        observe_pane(self.registry, self.pane_id)
    }

    fn busy(&mut self) -> bool {
        self.registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == self.pane_id)
            .is_some_and(|pane| pane.status != "idle")
    }

    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn enter_settle(&mut self) -> Duration {
        self.profile().submit_sequence().settle
    }

    /// The profile of the agent running in this pane.
    fn profile(&mut self) -> Arc<dyn TuiProfile> {
        let agent_type = self
            .registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == self.pane_id)
            .map(|pane| pane.agent_type)
            .unwrap_or_default();
        crate::tui::profile_for(&agent_type)
    }
}
/// The text typed into the pane for a task: the task, plus the worker's completion-reporting
/// instruction when the worker has MCP tools.
pub fn dispatch_prompt(
    task: &str,
    has_mcp_tools: bool,
    task_id: &str,
    agent_id: &str,
    project_path: &str,
) -> String {
    let mut prompt = task.to_string();
    if has_mcp_tools {
        prompt.push_str(&format!("\nWhen complete call complete_task with task_id={task_id}, agent_id={agent_id}, project_path={project_path} and evidence. If blocked call report_task_status with status=blocked and reason."));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted pane: `on_write` decides what the screen shows after each write.
    struct FakePane {
        screen: String,
        busy: bool,
        writes: Vec<(String, bool)>,
        paused: Duration,
        on_write: Box<dyn FnMut(&mut FakePane, &str, bool)>,
        /// The pane's id as a readiness look reports it (empty: a double with no recording).
        pane_id: String,
    }

    impl DeliveryIo for FakePane {
        fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
            // The invariant, checked on every write a delivery makes: run input carries a permit.
            assert!(matches!(origin, WriteOrigin::Task(_) | WriteOrigin::Gate(_)), "unpermitted write {text:?}");
            origin.admit_terminal_write(&self.pane_id, text, enter, None, || AgentProcess::Unknown)?;
            self.writes.push((text.to_string(), enter));
            let mut hook = std::mem::replace(&mut self.on_write, Box::new(|_, _, _| {}));
            hook(self, text, enter);
            self.on_write = hook;
            Ok(())
        }
        fn screen(&mut self) -> String {
            self.screen.clone()
        }
        fn observe(&mut self) -> Observation {
            Observation { pane_id: self.pane_id.clone(), screen: self.screen.clone(), output_pos: None, agent: AgentProcess::Unknown }
        }
        fn busy(&mut self) -> bool {
            self.busy
        }
        fn pause(&mut self, duration: Duration) {
            self.paused += duration;
        }
    }

    const PROMPT: &str = "Summarize the failing test in tests/login.rs and propose a fix";

    fn pane(on_write: impl FnMut(&mut FakePane, &str, bool) + 'static) -> FakePane {
        FakePane {
            screen: "Composer 2.5\n→ Plan, search, build anything".into(),
            busy: false,
            writes: Vec::new(),
            paused: Duration::ZERO,
            on_write: Box::new(on_write),
            pane_id: String::new(),
        }
    }

    #[test]
    fn delivery_records_what_it_saw_on_the_panes_flight_recording() {
        let id = format!("delivery-flight-{}", uuid::Uuid::new_v4());
        crate::flight::start(&id, 80, 24);
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\n\nworking\n→ Add a follow-up");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │\nComposer 2.5");
            }
        });
        pane.pane_id = id.clone();
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), PROMPT), Delivery::Started);
        let (cast, _) = crate::flight::freeze::export(&id, "test").unwrap();
        let marks = cast.as_str().matches("pm:readiness=Ready").count();
        // One per readiness look (before the clear sequence and before the text), not one per poll.
        assert!((1..=2).contains(&marks), "{marks} marks: {}", cast.as_str());
    }

    #[test]
    fn healthy_pane_is_typed_verified_then_submitted() {
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\n\nworking\n→ Add a follow-up");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │\nComposer 2.5");
            }
        });
        let delivery = deliver_prompt(&mut pane, Some("\u{15}"), PROMPT);
        assert_eq!(delivery, Delivery::Started);
        // clear, text without Enter, then a lone Enter.
        assert_eq!(pane.writes[0], ("\u{15}".to_string(), false));
        assert_eq!(pane.writes[1], (PROMPT.to_string(), false));
        assert_eq!(pane.writes[2], (String::new(), true));
    }

    #[test]
    fn enter_is_never_pressed_when_the_prompt_does_not_show_up() {
        // The typed text wipes the pane (it never renders).
        let mut pane = pane(|pane, text, _| {
            if text == PROMPT {
                pane.screen = String::new();
            }
        });
        let delivery = deliver_prompt(&mut pane, Some("\u{15}"), PROMPT);
        assert_eq!(delivery, Delivery::NotVisible);
        assert!(
            pane.writes.iter().all(|(_, enter)| !enter),
            "Enter must not be sent over an unverified input box: {:?}",
            pane.writes
        );
        // Blank after typing: retyping only while the startup budget lasts; box left cleared.
        assert!(pane.writes.iter().filter(|(text, _)| text == PROMPT).count() >= 1);
        assert_eq!(pane.writes.last().unwrap().0, "\u{15}");
    }

    #[test]
    fn a_dropped_first_attempt_is_retried_once_automatically() {
        let mut typed = 0;
        let mut pane = pane(move |pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\nworking");
                pane.busy = true;
            } else if text == PROMPT {
                typed += 1;
                if typed >= 2 {
                    pane.screen = format!("│ {PROMPT} │");
                }
            }
        });
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), PROMPT), Delivery::Started);
        assert_eq!(pane.writes.iter().filter(|(text, _)| text == PROMPT).count(), 2);
    }

    #[test]
    fn stale_previous_prompt_in_the_box_is_not_the_new_prompt() {
        // The box keeps showing the previous turn's prompt whatever is typed.
        let previous = "List the files in the repository root directory";
        let mut pane = pane(move |pane, _, _| pane.screen = format!("│ {previous} │"));
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), PROMPT), Delivery::NotVisible);
        assert!(pane.writes.iter().all(|(_, enter)| !enter));
    }

    #[test]
    fn enter_is_repeated_once_when_nothing_changes() {
        let mut pane = pane(|pane, text, enter| {
            if !enter && text == PROMPT {
                pane.screen = format!("│ {PROMPT} │");
            }
        });
        let delivery = deliver_prompt(&mut pane, None, PROMPT);
        assert_eq!(delivery, Delivery::SubmittedUnconfirmed);
        assert_eq!(pane.writes.iter().filter(|(_, enter)| *enter).count(), 2);
    }

    #[test]
    fn multiline_prompt_shown_as_paste_placeholder_is_visible() {
        let prompt = "Do this\nthen that\nand finally the other thing";
        assert!(prompt_visible_in_input("│ [Pasted text #1 +3 lines] │", prompt));
        assert!(!prompt_visible_in_input("│ [Pasted text #1 +3 lines] │", "single line only"));
    }

    #[test]
    fn a_stale_prompt_sharing_the_opening_is_not_the_new_prompt() {
        let previous = "Reply with exactly the word ALPHA and nothing else at all";
        let next = "Reply with exactly the word BRAVO and nothing else at all please";
        assert!(prompt_visible_in_input(&format!("│ {previous} │\nComposer 2.5"), previous));
        assert!(!prompt_visible_in_input(&format!("│ {previous} │\nComposer 2.5"), next));
        assert!(prompt_visible_in_input(&format!("│ {next} │\nComposer 2.5"), next));
    }

    #[test]
    fn prompt_in_the_transcript_region_is_not_input_visibility() {
        let mut screen = format!("→ {PROMPT}\n");
        for n in 0..20 {
            screen.push_str(&format!("reply line {n}\n"));
        }
        screen.push_str("→ Add a follow-up\nComposer 2.5 · 3%");
        assert!(!prompt_visible_in_input(&screen, PROMPT));
    }

    #[test]
    fn redelivery_only_when_a_running_turn_never_had_its_prompt_dispatched() {
        // profile, running, in_flight, dispatched, already
        assert!(needs_redelivery(true, true, false, false, false));
        assert!(!needs_redelivery(true, true, false, true, false), "prompt was dispatched");
        assert!(!needs_redelivery(true, true, true, false, false), "dispatch still typing");
        assert!(!needs_redelivery(true, true, false, false, true), "already retried once");
        assert!(!needs_redelivery(true, false, false, false, false), "not running yet");
        assert!(!needs_redelivery(false, true, false, false, false), "profile cannot verify");
    }

    #[test]
    fn busy_left_over_from_typing_is_not_a_started_turn() {
        // Typing flips an idle pane to running for the next IDLE_AFTER seconds, so "busy" right
        // after Enter says nothing unless the screen also moved.
        let typed = format!("│ {PROMPT} │
Composer 2.5");
        assert!(!turn_started(&typed, &typed, PROMPT, true));
        assert!(turn_started(&typed, "→ Add a follow-up   ctrl+c to stop
Composer 2.5", PROMPT, true));
    }

    #[test]
    fn a_swallowed_enter_with_a_stale_busy_flag_is_not_reported_as_started() {
        let mut pane = pane(|pane, text, enter| {
            if !enter && text == PROMPT {
                pane.screen = format!("│ {PROMPT} │");
                pane.busy = true;
            }
        });
        assert_eq!(deliver_prompt(&mut pane, None, PROMPT), Delivery::SubmittedUnconfirmed);
    }

    #[test]
    fn the_trace_records_what_each_step_saw() {
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}
working
→ Add a follow-up");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │");
            }
        });
        let mut trace = Vec::new();
        assert_eq!(
            deliver_prompt_traced(&mut pane, None, PROMPT, &mut trace),
            Delivery::Started
        );
        assert!(trace.iter().any(|line| line.starts_with("typed poll 0: visible=true")));
        assert!(trace.iter().any(|line| line.starts_with("enter poll 0: started=true")));
    }

    #[test]
    fn dispatch_prompt_adds_the_completion_instruction_only_for_mcp_workers() {
        assert_eq!(dispatch_prompt("t", false, "id", "pane", "C:/p"), "t");
        let with = dispatch_prompt("t", true, "id", "pane", "C:/p");
        assert!(with.starts_with("t\nWhen complete call complete_task with task_id=id, agent_id=pane"));
    }

    #[test]
    fn a_healthy_delivery_costs_well_under_a_second_of_waiting() {
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\n\nworking\n→ Add a follow-up");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │\nComposer 2.5");
            }
        });
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), PROMPT), Delivery::Started);
        assert!(pane.paused <= Duration::from_millis(400), "paused {:?}", pane.paused);
    }

    #[test]
    fn polling_faster_does_not_shrink_the_time_given_to_a_slow_tui() {
        // Never shows the typed text (TUI drawn): each of the two attempts waits about 2s for it.
        let mut blank = pane(|pane, _, _| pane.screen = "Composer 2.5\n→ Plan".into());
        assert_eq!(deliver_prompt(&mut blank, Some("\u{15}"), PROMPT), Delivery::NotVisible);
        assert!(blank.paused >= Duration::from_secs(4), "paused {:?}", blank.paused);
        // Types fine, but the turn never starts: about 3s of polling before giving up.
        let mut stuck = pane(|pane, text, enter| {
            if !enter && text == PROMPT {
                pane.screen = format!("│ {PROMPT} │\nComposer 2.5");
            }
        });
        assert_eq!(deliver_prompt(&mut stuck, Some("\u{15}"), PROMPT), Delivery::SubmittedUnconfirmed);
        assert!(stuck.paused >= Duration::from_secs(3), "paused {:?}", stuck.paused);
        // The second Enter still goes out, once.
        let enters = stuck.writes.iter().filter(|(_, enter)| *enter).count();
        assert_eq!(enters, 2);
    }
    // ---- regression: kilo home screen on a 121-column pane (visible run_agent, PROMPT_NOT_SUBMITTED) ----

    const KILO_HOME_TYPED: &str = include_str!("fixtures/kilo_home_typed_121.txt");
    const KILO_TASK: &str = "Demo for a video: in two short sentences, say what Puppet Master is based on the README in this folder. Do not edit any files.";

    /// The fixture painted into a real 121x36 terminal emulator, then read the way the app reads panes.
    fn painted(keep_cols: Option<u16>) -> String {
        let body = KILO_HOME_TYPED.split_once('\n').unwrap().1.split_once('\n').unwrap().1;
        let mut parser = vt100::Parser::new(36, 121, 0);
        parser.process(body.replace('\n', "\r\n").as_bytes());
        crate::pty::registry::snapshot_text(parser.screen(), keep_cols)
    }

    #[test]
    fn kilo_home_screen_prompt_is_visible_on_the_uncropped_screen() {
        assert!(prompt_visible_in_input(&painted(None), KILO_TASK));
    }

    #[test]
    fn kilo_home_screen_sidebar_crop_hides_the_prompt_tail() {
        // Why delivery must not read the cropped snapshot: kilo's home screen has no sidebar, so the
        // 42-column crop of a 121-column pane cut the box and the end of the prompt was lost.
        assert!(!prompt_visible_in_input(&painted(Some(121 - 42)), KILO_TASK));
    }

    #[test]
    fn kilo_home_screen_delivers_with_the_notice_under_the_box() {
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = "Thinking\n  esc interrupt".into();
                pane.busy = true;
            } else if text == KILO_TASK {
                pane.screen = painted(None);
            }
        });
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), KILO_TASK), Delivery::Started);
    }

    // ---- regression: slow-starting TUI (kilo cold start) must not burn the typing attempts ----

    struct Late {
        inner: FakePane,
        blank_polls: u32,
    }

    impl DeliveryIo for Late {
        fn write(&mut self, o: WriteOrigin<'_>, t: &str, e: bool) -> Result<(), String> {
            self.inner.write(o, t, e)
        }
        fn screen(&mut self) -> String {
            if self.blank_polls > 0 {
                self.blank_polls -= 1;
                return String::new();
            }
            self.inner.screen()
        }
        fn busy(&mut self) -> bool {
            self.inner.busy()
        }
        fn pause(&mut self, d: Duration) {
            self.inner.pause(d)
        }
    }

    #[test]
    fn a_tui_that_draws_late_gets_the_prompt_after_it_appears() {
        // Blank for 200 polls (16s), far longer than the two old 2s attempts, then the idle box.
        let mut inner = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\nworking");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │\nComposer 2.5");
            }
        });
        inner.screen = "Composer 2.5\n→ Plan, search, build anything".into();
        let mut late = Late { inner, blank_polls: 200 };
        let mut trace = Vec::new();
        assert_eq!(deliver_prompt_traced(&mut late, Some("\u{15}"), PROMPT, &mut trace), Delivery::Started);
        // Nothing was typed into the blank pane: the first write is the clear, after it drew.
        assert_eq!(late.inner.writes[0], ("\u{15}".to_string(), false));
        assert_eq!(late.inner.writes.iter().filter(|(t, _)| t == PROMPT).count(), 1);
        assert!(late.inner.paused >= Duration::from_secs(10), "paused {:?}", late.inner.paused);
    }

    #[test]
    fn a_pane_that_stays_blank_is_not_typed_into_and_says_why() {
        let mut p = pane(|_, _, _| {});
        p.screen = "  \n \n".into();
        let mut trace = Vec::new();
        // Blank is `Unknown`: the run is parked with the reason, never typed into, never sent
        // headless (before the permit: `NotVisible`, which fell back to a background run).
        assert_eq!(
            deliver_prompt_traced(&mut p, Some("\u{15}"), PROMPT, &mut trace),
            Delivery::NotReady { why: "the pane stayed blank".into(), screen: "  \n \n".into() }
        );
        assert!(p.writes.is_empty(), "{:?}", p.writes);
        assert!(trace.iter().any(|l| l.contains("screen stayed empty")), "{trace:?}");
        assert!(p.paused >= Duration::from_secs(19), "paused {:?}", p.paused);
    }

    // ---- the permit on real captures (the same scenarios fail on the base: `fail-before-base.txt`) ----

    /// A pane of a real profile showing one captured screen; `process` is what the process tree says.
    struct Captured {
        inner: FakePane,
        agent: &'static str,
        process: AgentProcess,
    }

    impl DeliveryIo for Captured {
        fn write(&mut self, o: WriteOrigin<'_>, t: &str, e: bool) -> Result<(), String> {
            let process = self.process;
            o.admit_terminal_write("", t, e, None, || process)?;
            self.inner.write(o, t, e)
        }
        fn screen(&mut self) -> String {
            self.inner.screen()
        }
        fn observe(&mut self) -> Observation {
            Observation { pane_id: String::new(), screen: self.inner.screen(), output_pos: None, agent: self.process }
        }
        fn busy(&mut self) -> bool {
            self.inner.busy
        }
        fn pause(&mut self, d: Duration) {
            self.inner.pause(d)
        }
        fn profile(&mut self) -> Arc<dyn TuiProfile> {
            crate::tui::profile_for(self.agent)
        }
    }

    fn typed_on(agent: &'static str, screen: &str, process: AgentProcess) -> (Delivery, Vec<(String, bool)>) {
        let mut inner = pane(|_, _, _| {});
        inner.screen = screen.to_string();
        let mut io = Captured { inner, agent, process };
        let clear = crate::tui::profile_for(agent).clear_input_sequence();
        let outcome = deliver_prompt(&mut io, clear, PROMPT);
        (outcome, io.inner.writes)
    }

    #[test]
    fn a_killed_agent_whose_last_frame_shows_its_box_gets_nothing_typed() {
        let screen = include_str!("../tui/claude/fixtures/killed_host_shell_2_1_299_live.txt");
        let (outcome, writes) = typed_on("claude", screen, AgentProcess::Exited);
        assert_eq!(outcome, Delivery::AgentGone(HostShell::AgentExited));
        assert!(writes.is_empty(), "{writes:?}");
    }

    #[test]
    fn a_claude_box_holding_someones_draft_is_not_typed_into() {
        let screen = include_str!("../tui/claude/fixtures/draft_2_1_299_live.txt");
        let (outcome, writes) = typed_on("claude", screen, AgentProcess::Alive);
        assert!(matches!(&outcome, Delivery::NotReady { why, .. } if why.contains("already holds text")), "{outcome:?}");
        assert!(writes.is_empty(), "{writes:?}");
    }

    #[test]
    fn codex_still_loading_is_not_typed_into() {
        let screen = include_str!("../tui/codex/fixtures/loading_0_162_1_live.txt");
        let (outcome, writes) = typed_on("codex", screen, AgentProcess::Alive);
        assert!(matches!(outcome, Delivery::NotReady { .. }), "{outcome:?}");
        assert!(writes.is_empty(), "{writes:?}");
    }

    #[test]
    fn an_agent_that_is_not_installed_is_its_own_outcome() {
        let screen = include_str!("../tui/shell/fixtures/command_not_found_powershell_live.txt");
        let (outcome, writes) = typed_on("claude", screen, AgentProcess::Starting);
        assert_eq!(outcome, Delivery::AgentGone(HostShell::NotInstalled("pm-agent-not-installed-7f3".into())));
        assert!(writes.is_empty(), "{writes:?}");
    }

    #[test]
    fn the_same_screens_with_a_live_agent_still_deliver() {
        // The checks above refuse for the right reason: Claude at its fresh box takes the task.
        let screen = include_str!("../tui/claude/fixtures/ready_fresh_2_1_299_live.txt");
        let mut inner = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("\u{276f} {PROMPT}\n\u{2722} Generating\u{2026}\n");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = pane.screen.replace("Try \"edit <filepath> to...\"", PROMPT);
            }
        });
        inner.screen = screen.to_string();
        let mut io = Captured { inner, agent: "claude", process: AgentProcess::Alive };
        assert_eq!(deliver_prompt(&mut io, None, PROMPT), Delivery::Started);
        assert_eq!(io.inner.writes, vec![(PROMPT.to_string(), false), (String::new(), true)]);
    }

    #[test]
    fn a_menu_that_draws_between_the_look_and_the_keystroke_is_never_typed_into() {
        // The pane's write path refuses a permit whose screen is no longer the latest; the stale
        // refusal sends delivery back to look, and the late menu is closed with its own keys.
        struct Racy {
            inner: FakePane,
            pos: u64,
            menu_drawn: bool,
        }
        impl DeliveryIo for Racy {
            fn write(&mut self, o: WriteOrigin<'_>, t: &str, e: bool) -> Result<(), String> {
                if !self.menu_drawn && t == PROMPT {
                    // Codex draws its update menu right after the look that issued this permit.
                    self.menu_drawn = true;
                    self.inner.screen = include_str!("../tui/codex/fixtures/update_prompt_live.txt").to_string();
                    self.pos += 900;
                }
                o.admit_terminal_write("", t, e, Some(self.pos), || AgentProcess::Alive)?;
                if t == "\u{1b}" {
                    self.inner.screen = include_str!("../tui/codex/fixtures/standby_ready_live.txt").to_string();
                    self.pos += 500;
                    return Ok(());
                }
                self.inner.write(o, t, e)
            }
            fn screen(&mut self) -> String {
                self.inner.screen()
            }
            fn observe(&mut self) -> Observation {
                Observation { pane_id: String::new(), screen: self.inner.screen(), output_pos: Some(self.pos), agent: AgentProcess::Alive }
            }
            fn busy(&mut self) -> bool {
                false
            }
            fn pause(&mut self, d: Duration) {
                self.inner.pause(d)
            }
            fn profile(&mut self) -> Arc<dyn TuiProfile> {
                crate::tui::profile_for("codex")
            }
        }
        let mut inner = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("\u{203a} {PROMPT}\n\n  esc to interrupt\n");
            } else if text == PROMPT {
                pane.screen = pane.screen.replace("\u{203a} Ask Codex to do anything", &format!("\u{203a} {PROMPT}"));
            }
        });
        inner.screen = include_str!("../tui/codex/fixtures/standby_ready_live.txt").to_string();
        let mut io = Racy { inner, pos: 1_000, menu_drawn: false };
        let mut trace = Vec::new();
        let outcome = deliver_prompt_traced(&mut io, None, PROMPT, &mut trace);
        assert!(trace.iter().any(|l| l.contains("PERMIT_STALE")), "{trace:#?}");
        assert!(trace.iter().any(|l| l.contains("update_available")), "{trace:#?}");
        assert_eq!(outcome, Delivery::Started, "{trace:#?}");
        assert_eq!(io.inner.writes.iter().filter(|(t, _)| t == PROMPT).count(), 1, "typed once, on the composer");
    }
}
