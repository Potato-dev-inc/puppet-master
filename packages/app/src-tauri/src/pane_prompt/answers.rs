//! One answer per prompt occurrence, and the person's click behind every desktop answer.
//!
//! Live v0.2.0 hunt (audit entries 7-11): five "trust" answers for one Codex folder-trust card
//! within 400 ms, all recorded `who: desktop`. Three gaps let that happen, closed here and in the
//! callers:
//!
//! 1. **No dedupe on the bare-pane path.** `pane_prompt::answer` pressed the key every time it was
//!    called while the dialog was still drawn (a TUI redraws a few hundred ms after the key), and a
//!    worktree run's pane was answered through that path (`/agents/answer` looked for the run in the
//!    project and the worktree folder, never the worktree's origin project). Every key-pressing answer
//!    path now claims a slot here first: the same prompt on the same pane, in the same occurrence
//!    (the prompt has not left the screen and come back, `note_prompt_event`), within
//!    [`DEDUPE_WINDOW_MS`], is a duplicate. A duplicate presses nothing and writes no audit line.
//! 2. **No proof of a person.** The desktop command (`answer_pane_prompt`) took any call. Now an
//!    answer from the desktop UI carries a [`UserGesture`] minted by the click handler (a trusted
//!    DOM event, fresh, used once). The only answer allowed without one is a refusal of a prompt that
//!    is not a startup gate (Simple mode's own No to a high-risk command). Folder trust and every
//!    accepting answer need the click.
//! 3. **Wrong attribution.** Any bridge caller without an MCP session was written `who: desktop`.
//!    It is now `local` (a program on this computer) or `device` (a paired phone); only the desktop
//!    command writes `desktop` (`agent_authority::gate::AnsweredBy`).

use crate::operations::OperationError;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};

/// How long after a delivered answer the same prompt, still in the same occurrence, counts as
/// already answered (a TUI redraws its dialog away within a second; five clicks in 400 ms must not
/// become five keys). Matches the run ledger's replay window (`agent_runs::answer_guard`).
pub const DEDUPE_WINDOW_MS: u64 = 5_000;
/// How old a gesture may be when the command runs. The click and the command are on one machine,
/// a few ms apart; ten seconds is slack for a busy UI thread, not for a replay.
pub const GESTURE_MAX_AGE_MS: u64 = 10_000;
/// How far in the future a gesture's time may be (clock reads in two processes).
const GESTURE_FUTURE_SLACK_MS: u64 = 2_000;
/// Gesture ids remembered (each is used once).
const USED_GESTURES_CAP: usize = 1_024;

/// What the desktop UI's click handler sends with an answer (`lib/user-gesture.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserGesture {
    /// Fresh per event (a random id), so one gesture answers once.
    pub id: String,
    /// `Event.isTrusted`: the browser made the event from real input, not `element.click()`.
    pub trusted: bool,
    /// `Date.now()` in the handler.
    pub at_ms: u64,
    /// The event type (`click`, `keydown`, ...).
    pub kind: String,
}

/// A refusal: the answer that leaves things as they are. The only kind of answer allowed without a
/// gesture, and never for a startup gate.
pub fn is_refusal(choice: &str) -> bool {
    matches!(
        choice.trim().to_ascii_lowercase().as_str(),
        "deny" | "reject" | "no" | "cancel" | "decline"
    )
}

fn is_startup_gate(kind: Option<&str>) -> bool {
    kind.is_some_and(|kind| super::STARTUP_GATE_KINDS.contains(&kind))
}

static USED_GESTURES: Lazy<Mutex<VecDeque<String>>> = Lazy::new(|| Mutex::new(VecDeque::new()));

/// The rule for an answer from the desktop UI. `prompt_kind` is the kind of the prompt on screen
/// (`workspace_trust`, `command_approval`, ...), `None` when none is detected (the answer then fails
/// later as stale anyway). `Ok` consumes the gesture.
pub fn check_desktop_answer(
    gesture: Option<&UserGesture>,
    prompt_kind: Option<&str>,
    choice: &str,
    now_ms: u64,
) -> Result<(), OperationError> {
    let Some(gesture) = gesture else {
        if is_refusal(choice) && !is_startup_gate(prompt_kind) {
            return Ok(());
        }
        return Err(needs_a_person(
            "no click came with this answer",
            if is_startup_gate(prompt_kind) { "a startup question such as folder trust" } else { "an answer that says yes" },
        ));
    };
    if !gesture.trusted || gesture.id.trim().is_empty() || gesture.kind.trim().is_empty() {
        return Err(needs_a_person("the click did not come from real input", "this answer"));
    }
    if gesture.at_ms > now_ms.saturating_add(GESTURE_FUTURE_SLACK_MS) || now_ms.saturating_sub(gesture.at_ms) > GESTURE_MAX_AGE_MS {
        return Err(needs_a_person("the click is too old", "this answer"));
    }
    let mut used = USED_GESTURES.lock();
    if used.iter().any(|id| id == &gesture.id) {
        return Err(needs_a_person("that click was already used for an answer", "this answer"));
    }
    if used.len() >= USED_GESTURES_CAP {
        used.pop_front();
    }
    used.push_back(gesture.id.clone());
    Ok(())
}

fn needs_a_person(why: &str, what: &str) -> OperationError {
    OperationError::new(
        "ANSWER_NEEDS_A_PERSON",
        format!("Nothing was sent: {what} is only sent when a person clicks it ({why})."),
        false,
    )
}

/// One delivered answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub prompt_id: String,
    pub choice: String,
    pub at_ms: u64,
    pub occurrence: u64,
}

/// The outcome of [`answer_once`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot<T> {
    /// The answer went out: `send` ran and its result is here.
    Sent(T),
    /// The same prompt, in the same occurrence, was answered `first.at_ms`: nothing was sent.
    Duplicate(Delivered),
}

#[derive(Default)]
struct Ledger {
    /// Per pane: how many times the prompt tracker saw its prompt appear, change or clear.
    occurrence: HashMap<String, u64>,
    /// Per pane: the last delivered answer.
    delivered: HashMap<String, Delivered>,
}

static LEDGER: Lazy<Mutex<Ledger>> = Lazy::new(|| Mutex::new(Ledger::default()));
/// Held across claim, send and record, so two concurrent answers cannot both pass the claim.
static SERIAL: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// The prompt tracker (`pty://prompt`) saw the pane's prompt appear, change or clear: the next
/// prompt shown is a new occurrence, even with the same id (the same command asked again).
pub fn note_prompt_event(pane_id: &str) {
    *LEDGER.lock().occurrence.entry(pane_id.to_string()).or_insert(0) += 1;
}

fn occurrence(ledger: &Ledger, pane_id: &str) -> u64 {
    ledger.occurrence.get(pane_id).copied().unwrap_or(0)
}

/// The earlier answer this one repeats, if any.
fn duplicate_of(pane_id: &str, prompt_id: &str, now_ms: u64) -> Option<Delivered> {
    let ledger = LEDGER.lock();
    let last = ledger.delivered.get(pane_id)?;
    (last.prompt_id == prompt_id
        && last.occurrence == occurrence(&ledger, pane_id)
        && now_ms.saturating_sub(last.at_ms) < DEDUPE_WINDOW_MS)
        .then(|| last.clone())
}

/// Press an answer's key at most once per prompt occurrence: `send` runs only when this pane's
/// prompt `prompt_id` was not answered in its current occurrence within [`DEDUPE_WINDOW_MS`]. Every
/// path that presses a key to answer a prompt goes through here (`pane_prompt::answer`, the run path
/// in `agent_runs::routes::answer`).
pub fn answer_once<T, E>(
    pane_id: &str,
    prompt_id: &str,
    choice: &str,
    now_ms: u64,
    send: impl FnOnce() -> Result<T, E>,
) -> Result<Slot<T>, E> {
    let _serial = SERIAL.lock();
    if let Some(first) = duplicate_of(pane_id, prompt_id, now_ms) {
        tracing::info!(pane_id, prompt_id, choice, first_choice = %first.choice, since_ms = now_ms.saturating_sub(first.at_ms), "duplicate answer ignored: nothing was sent");
        return Ok(Slot::Duplicate(first));
    }
    let sent = send()?;
    let mut ledger = LEDGER.lock();
    let occurrence = occurrence(&ledger, pane_id);
    ledger.delivered.insert(
        pane_id.to_string(),
        Delivered { prompt_id: prompt_id.to_string(), choice: choice.to_string(), at_ms: now_ms, occurrence },
    );
    Ok(Slot::Sent(sent))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "answers_tests.rs"]
mod tests;
