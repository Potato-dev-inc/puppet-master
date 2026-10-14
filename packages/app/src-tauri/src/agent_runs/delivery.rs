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

use crate::tui::{ComposerState, StartupGate};
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

pub trait DeliveryIo {
    fn write(&mut self, text: &str, enter: bool) -> Result<(), String>;
    fn screen(&mut self) -> String;
    /// True while the pane reports it is busy (a turn is running).
    fn busy(&mut self) -> bool;
    fn pause(&mut self, duration: Duration);
    /// Least pause between the typed prompt and Enter (the profile's `submit_sequence().settle`).
    fn enter_settle(&mut self) -> Duration {
        Duration::ZERO
    }
    /// What the input box holds on `screen`, when the profile can read it.
    fn composer_state(&mut self, _screen: &str) -> ComposerState {
        ComposerState::Unknown
    }
    /// The startup screen (update notice, trust question, sign-in) `screen` shows, when the profile
    /// knows it. Nothing is typed over one: a dismissible notice is closed with its own keys, any
    /// other waits for the user (`Delivery::Blocked`).
    fn startup_gate(&mut self, _screen: &str) -> Option<StartupGate> {
        None
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
    /// A startup screen that only the user may pass (folder trust, sign-in), or a notice that
    /// ignored its dismiss keys, is on screen. Nothing was submitted.
    Blocked(StartupGate),
    WriteFailed(String),
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

/// Closes the startup screens on the pane, one at a time, before anything is typed: a dismissible
/// notice gets the keys its profile declares (and the screen must change and settle after them),
/// any other gate is handed back for the caller to refuse. Typing a task over the notice is what
/// lost a fresh Codex run (its update menu takes Enter as "Update now"). `rounds` is shared by the
/// whole delivery so a screen that keeps returning stops the loop.
fn close_startup_gates(
    io: &mut dyn DeliveryIo,
    budget: &mut u32,
    rounds: &mut u32,
    trace: &mut Vec<String>,
) -> Result<(), StartupGate> {
    loop {
        let before = io.screen();
        let Some(gate) = io.startup_gate(&before) else { return Ok(()) };
        let Some(keys) = gate.dismiss else {
            trace.push(format!("startup screen '{}' is on screen and only the user may pass it", gate.id));
            return Err(gate);
        };
        if *rounds >= MAX_DELIVERY_GATES {
            trace.push(format!("startup screen '{}' keeps coming back; not closing it again", gate.id));
            return Err(gate);
        }
        *rounds += 1;
        trace.push(format!("startup screen '{}' on screen: closing it with its own keys before typing", gate.id));
        if io.write(keys, false).is_err() {
            return Err(gate);
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
            return Err(gate);
        }
        // Let what replaced it finish drawing; the next lap checks that it is not another gate.
        wait_for_screen(io, budget);
    }
}

/// [`deliver_prompt`] that also records what it saw, so a delivery that claimed success without a
/// turn can be diagnosed from the operation transcript.
pub fn deliver_prompt_traced(
    io: &mut dyn DeliveryIo,
    clear: Option<&str>,
    prompt: &str,
    trace: &mut Vec<String>,
) -> Delivery {
    let mut visible = false;
    let mut ready_budget = READY_BUDGET_POLLS;
    let mut attempt = 0;
    let mut typed_any = false;
    let mut gate_rounds = 0;
    while attempt < 2 {
        if !wait_for_screen(io, &mut ready_budget) {
            trace.push("screen stayed empty: the TUI drew nothing within the startup budget".to_string());
            if typed_any {
                if let Some(clear) = clear {
                    let _ = io.write(clear, false);
                }
            }
            return Delivery::NotVisible;
        }
        // The settled screen may be a startup notice the readiness check could not have seen
        // (the TUI draws it a moment after it starts): close it, or refuse, before typing.
        if let Err(gate) = close_startup_gates(io, &mut ready_budget, &mut gate_rounds, trace) {
            return Delivery::Blocked(gate);
        }
        if let Some(clear) = clear {
            if let Err(error) = io.write(clear, false) {
                return Delivery::WriteFailed(error);
            }
            io.pause(CLEAR_PAUSE);
        }
        trace.push(format!("attempt {attempt}: typing; busy={} rows={}", io.busy(), trace_rows(&io.screen())));
        if let Err(error) = io.write(prompt, false) {
            return Delivery::WriteFailed(error);
        }
        typed_any = true;
        let mut late_gate = None;
        for poll in 0..VISIBLE_POLLS {
            io.pause(POLL_INTERVAL);
            let screen = io.screen();
            if let Some(gate) = io.startup_gate(&screen) {
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
            // Closed (or refused) at the top of the loop, then typed again; this attempt does not
            // count, the box never had the text.
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
    if !visible {
        if let Some(clear) = clear {
            let _ = io.write(clear, false);
        }
        trace.push(format!("not visible; bottom rows at give-up:
{}", bottom_rows(&io.screen(), 20)));
        return Delivery::NotVisible;
    }
    // Some TUIs read an Enter that follows the typed text too closely as a line break in a paste.
    let settle = io.enter_settle();
    if !settle.is_zero() {
        io.pause(settle);
    }
    let before = io.screen();
    if let Some(gate) = io.startup_gate(&before) {
        // Enter on a notice chooses one of its options ("Update now"): never press it there.
        trace.push(format!("startup screen '{}' is on screen; Enter was not pressed", gate.id));
        return Delivery::Blocked(gate);
    }
    let before_state = io.composer_state(&before);
    if let Err(error) = io.write("", true) {
        return Delivery::WriteFailed(error);
    }
    let mut resubmitted = false;
    for poll in 0..STARTED_POLLS {
        io.pause(POLL_INTERVAL);
        let busy = io.busy();
        let screen = io.screen();
        let state = io.composer_state(&screen);
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
            let _ = io.write("", true);
        }
    }
    Delivery::SubmittedUnconfirmed
}

/// [`DeliveryIo`] over a live pane.
pub struct PaneIo<'a> {
    pub registry: &'a std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    pub app: &'a tauri::AppHandle,
    pub pane_id: &'a str,
}

impl DeliveryIo for PaneIo<'_> {
    fn write(&mut self, text: &str, enter: bool) -> Result<(), String> {
        crate::pty::registry_write_input(self.registry, self.app, self.pane_id, text, enter, false, None)
    }

    fn screen(&mut self) -> String {
        crate::pty::registry_read_snapshot_full(self.registry.as_ref(), self.pane_id).unwrap_or_default()
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

    fn composer_state(&mut self, screen: &str) -> ComposerState {
        self.profile().composer_state(screen)
    }

    fn startup_gate(&mut self, screen: &str) -> Option<StartupGate> {
        self.profile().startup_gate(screen)
    }
}

impl PaneIo<'_> {
    /// The profile of the agent running in this pane.
    fn profile(&self) -> std::sync::Arc<dyn crate::tui::TuiProfile> {
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
    }

    impl DeliveryIo for FakePane {
        fn write(&mut self, text: &str, enter: bool) -> Result<(), String> {
            self.writes.push((text.to_string(), enter));
            let mut hook = std::mem::replace(&mut self.on_write, Box::new(|_, _, _| {}));
            hook(self, text, enter);
            self.on_write = hook;
            Ok(())
        }
        fn screen(&mut self) -> String {
            self.screen.clone()
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
        }
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
        // Blank pane: the typed text never renders.
        let mut pane = pane(|pane, _, _| pane.screen = String::new());
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
        fn write(&mut self, t: &str, e: bool) -> Result<(), String> {
            self.inner.write(t, e)
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
        assert_eq!(deliver_prompt_traced(&mut p, Some("\u{15}"), PROMPT, &mut trace), Delivery::NotVisible);
        assert!(p.writes.is_empty(), "{:?}", p.writes);
        assert!(trace.iter().any(|l| l.contains("screen stayed empty")), "{trace:?}");
        assert!(p.paused >= Duration::from_secs(19), "paused {:?}", p.paused);
    }
}
