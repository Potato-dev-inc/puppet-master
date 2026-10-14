//! Visible Codex pane, from a live run of Codex 0.159.3 on Windows (screens are captures under
//! `tui/codex/fixtures/`, see `docs/features/fix-live-codex.md`):
//!
//! - Bug #3: the task text appeared in the composer, the Enter that followed 50 ms later became a
//!   line break, and the pane sat idle with the task unsent. Codex reads characters that arrive in
//!   a burst as a paste and an Enter inside the burst's window as part of it.
//! - Bug #4: a run completed with Codex's "Update available" menu as its answer.
//!
//! The pane here is a small model of that behaviour driven by the real captures and the real
//! Codex profile, so the delivery protocol and the completion watch run against what Codex does.

use super::completion::{observe_tui_turn, TuiTurnDecision, TuiTurnWatch};
use super::delivery::{deliver_prompt_traced, prompt_visible_in_input, Delivery, DeliveryIo};
use crate::tui::{profile_for, ComposerState, StartupGate};
use std::time::Duration;

const PROMPT: &str = "Reply with the single word FIVE. Do not use any tools.";

/// Codex 0.159.3 with history above the composer: the typed prompt waiting unsent, the turn done.
const READY: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");
const UNSENT: &str = include_str!("../tui/codex/fixtures/composer_unsent_glued_live.txt");
const SUBMITTED: &str = include_str!("../tui/codex/fixtures/submitted_live.txt");
const UPDATE_MENU: &str = include_str!("../tui/codex/fixtures/update_prompt_live.txt");

/// Live: Enter 0 ms and 50 ms after the text stayed unsent, 100 ms (plus HTTP latency) submitted.
/// Codex's own window is about this long.
const BURST_WINDOW: Duration = Duration::from_millis(120);

/// A Codex pane. A virtual clock advances only through `pause`.
struct CodexPane {
    clock: Duration,
    last_char_at: Option<Duration>,
    screen: String,
    busy: bool,
    enters: usize,
    /// Pretend the profile asked for this settle instead of its real one.
    settle: Option<Duration>,
    /// Every Enter is read as a line break (a worst case: the text can never be submitted).
    swallow_every_enter: bool,
    /// Startup model (live run 7bef62a6): blank until `menu_at`, then the update menu until a key
    /// closes it. `None`: the pane is already at its composer.
    menu_at: Option<Duration>,
    menu_up: bool,
    /// Whether Esc closes the menu (it does on the real Codex; false is a worst case).
    esc_closes_menu: bool,
    /// Keys the menu received that were not its own dismiss key (the bug: a task typed into it).
    typed_into_menu: Vec<String>,
    esc_sent: usize,
}

impl CodexPane {
    fn new() -> Self {
        Self {
            clock: Duration::from_secs(100),
            last_char_at: None,
            screen: READY.to_string(),
            busy: false,
            enters: 0,
            settle: None,
            swallow_every_enter: false,
            menu_at: None,
            menu_up: false,
            esc_closes_menu: true,
            typed_into_menu: Vec::new(),
            esc_sent: 0,
        }
    }

    /// A fresh pane: nothing drawn for `menu_after`, then Codex's update menu.
    fn fresh(menu_after: Duration) -> Self {
        let mut pane = Self::new();
        pane.clock = Duration::ZERO;
        pane.screen = String::new();
        pane.menu_at = Some(menu_after);
        pane
    }

    fn draw_menu_when_due(&mut self) {
        if self.menu_at.is_some_and(|at| self.clock >= at) && !self.menu_up {
            self.menu_at = None;
            self.menu_up = true;
            self.screen = UPDATE_MENU.to_string();
        }
    }
}

impl DeliveryIo for CodexPane {
    fn write(&mut self, text: &str, enter: bool) -> Result<(), String> {
        self.draw_menu_when_due();
        if self.menu_up {
            if text == "\u{1b}" && !enter {
                self.esc_sent += 1;
                if self.esc_closes_menu {
                    self.menu_up = false;
                    self.screen = READY.to_string();
                }
            } else {
                self.typed_into_menu.push(format!("{text:?} enter={enter}"));
            }
            return Ok(());
        }
        if self.menu_at.is_some() {
            // Not drawn yet: keys are lost.
            return Ok(());
        }
        if !enter {
            if text.chars().all(|ch| ch == '\u{15}') {
                // The clear sequence: an empty composer stays as it is.
                return Ok(());
            }
            self.screen = UNSENT.to_string();
            self.last_char_at = Some(self.clock);
            // Typing flips an idle pane to "running" (any output does), so busy proves nothing.
            self.busy = true;
            return Ok(());
        }
        self.enters += 1;
        let inside_burst = self
            .last_char_at
            .is_some_and(|at| self.clock.saturating_sub(at) < BURST_WINDOW);
        if inside_burst || self.swallow_every_enter {
            // The line break: the composer still holds the text (the capture of that state).
            self.screen = UNSENT.to_string();
        } else {
            self.screen = SUBMITTED.to_string();
        }
        Ok(())
    }
    fn screen(&mut self) -> String {
        self.draw_menu_when_due();
        self.screen.clone()
    }
    fn busy(&mut self) -> bool {
        self.busy
    }
    fn pause(&mut self, duration: Duration) {
        self.clock += duration;
    }
    fn startup_gate(&mut self, screen: &str) -> Option<StartupGate> {
        profile_for("codex").startup_gate(screen)
    }
    fn enter_settle(&mut self) -> Duration {
        self.settle
            .unwrap_or_else(|| profile_for("codex").submit_sequence().settle)
    }
    fn composer_state(&mut self, screen: &str) -> ComposerState {
        profile_for("codex").composer_state(screen)
    }
}

fn deliver(pane: &mut CodexPane) -> (Delivery, Vec<String>) {
    let mut trace = Vec::new();
    let clear = profile_for("codex").clear_input_sequence();
    let delivery = deliver_prompt_traced(pane, clear, PROMPT, &mut trace);
    (delivery, trace)
}

#[test]
fn the_captures_are_what_the_model_says_they_are() {
    let codex = profile_for("codex");
    assert_eq!(codex.composer_state(READY), ComposerState::Empty);
    assert_eq!(codex.composer_state(UNSENT), ComposerState::HasText);
    assert_eq!(codex.composer_state(SUBMITTED), ComposerState::Empty);
    assert!(prompt_visible_in_input(UNSENT, PROMPT));
}

#[test]
fn the_profile_waits_out_the_paste_burst_so_one_enter_submits() {
    let mut pane = CodexPane::new();
    let (delivery, trace) = deliver(&mut pane);
    assert_eq!(delivery, Delivery::Started, "{trace:#?}");
    assert_eq!(pane.enters, 1, "no blind second Enter was needed");
}

#[test]
fn an_enter_swallowed_as_a_line_break_is_noticed_and_repeated_once() {
    // The old behaviour: Enter right after the text (no settle). The text is still in the box, so
    // delivery presses Enter once more, after the burst window, and the turn starts.
    let mut pane = CodexPane::new();
    pane.settle = Some(Duration::ZERO);
    let (delivery, trace) = deliver(&mut pane);
    assert_eq!(delivery, Delivery::Started, "{trace:#?}");
    assert_eq!(pane.enters, 2);
    assert!(
        trace.iter().any(|line| line.contains("Enter sent once more")),
        "the retry is recorded: {trace:#?}"
    );
}

#[test]
fn text_still_waiting_in_a_busy_looking_pane_is_never_reported_as_started() {
    // Typing made the pane read "running", so the generic check would have called this a started
    // turn. The composer says the text was not taken.
    let mut pane = CodexPane::new();
    pane.swallow_every_enter = true;
    let (delivery, trace) = deliver(&mut pane);
    assert_eq!(delivery, Delivery::SubmittedUnconfirmed, "{trace:#?}");
    assert_eq!(pane.enters, 2, "Enter is repeated once, not mashed");
}

#[test]
fn a_pasted_content_placeholder_is_the_typed_prompt() {
    // Codex collapses a paste over 1000 characters into "[Pasted Content N chars]".
    let long = "Summarize in one word: lorem ipsum dolor sit amet consectetur. ".repeat(30);
    let placeholder = include_str!("../tui/codex/fixtures/composer_pasted_live.txt");
    assert!(prompt_visible_in_input(placeholder, long.trim()));
    // A short prompt is not excused by a placeholder from an earlier paste.
    assert!(!prompt_visible_in_input(placeholder, PROMPT));
}

// ---- fresh visible run: the update menu is drawn after readiness was judged (VERIFY #4) --------
//
// Live (ops 7bef62a6, adfc0d1e; dev log 03:26:19 and 03:29:33): the pane counted as ready the moment
// it was spawned, the dismissal ran on a blank screen, the menu was drawn about a second later, and
// the task was typed into it. The menu takes Enter as "Update now", Codex quit, delivery logged
// NotVisible and the run fell back to headless.

#[test]
fn a_menu_drawn_after_the_pane_counted_as_ready_is_closed_before_the_task_is_typed() {
    let mut pane = CodexPane::fresh(Duration::from_millis(1_100));
    let (delivery, trace) = deliver(&mut pane);
    assert_eq!(delivery, Delivery::Started, "{trace:#?}");
    assert_eq!(pane.esc_sent, 1, "the menu's own skip key, once: {trace:#?}");
    assert!(pane.typed_into_menu.is_empty(), "nothing typed into the menu: {:?}", pane.typed_into_menu);
    assert_eq!(pane.enters, 1, "one Enter, on the composer");
}

#[test]
fn a_menu_that_draws_while_the_task_is_being_typed_is_closed_and_the_task_typed_again() {
    // The first frame is not a gate (a splash), so typing starts; the menu lands during the
    // verification polls. Esc, then the text goes in again on the composer.
    let mut pane = CodexPane::fresh(Duration::from_millis(400));
    pane.screen = "  >_ OpenAI Codex (v0.159.3)\n\n  starting...\n".to_string();
    pane.menu_at = Some(Duration::from_millis(400));
    pane.menu_up = false;
    let (delivery, trace) = deliver(&mut pane);
    assert_eq!(delivery, Delivery::Started, "{trace:#?}");
    assert!(pane.esc_sent >= 1, "{trace:#?}");
    assert!(
        pane.typed_into_menu.iter().all(|key| key.contains("\u{15}")),
        "only the clear sequence may have reached the menu, never the task: {:?}",
        pane.typed_into_menu
    );
    assert!(trace.iter().any(|line| line.contains("startup screen")), "{trace:#?}");
}

#[test]
fn a_menu_drawn_during_the_clear_pause_is_seen_by_the_last_look_before_typing() {
    // Live (codex_live_fresh_pane_gets_its_task_...): the settle check saw no menu, 120 ms later
    // (the clear sequence's pause) the whole menu was up, and the task text was typed into it.
    let mut pane = CodexPane::new();
    pane.menu_at = Some(pane.clock + Duration::from_millis(100));
    let (delivery, trace) = deliver(&mut pane);
    assert_eq!(delivery, Delivery::Started, "{trace:#?}");
    assert_eq!(pane.esc_sent, 1, "{trace:#?}");
    assert!(
        pane.typed_into_menu.iter().all(|key| key.contains("\u{15}")),
        "the task was never typed into the menu: {:?}",
        pane.typed_into_menu
    );
    assert!(trace.iter().any(|line| line.contains("just before typing")), "{trace:#?}");
}

#[test]
fn a_menu_that_ignores_esc_blocks_the_run_and_nothing_is_typed_or_submitted() {
    let mut pane = CodexPane::fresh(Duration::from_millis(300));
    pane.esc_closes_menu = false;
    let (delivery, trace) = deliver(&mut pane);
    assert!(matches!(delivery, Delivery::Blocked(gate) if gate.id == "update_available"), "{delivery:?} {trace:#?}");
    assert_eq!(pane.esc_sent, 1, "one attempt, no key mashing");
    assert!(pane.typed_into_menu.is_empty(), "{:?}", pane.typed_into_menu);
    assert_eq!(pane.enters, 0);
}

#[test]
fn a_screen_only_the_user_may_pass_is_never_answered_and_nothing_is_typed() {
    // Codex's folder-trust question (hand-written: no live capture exists): no dismiss keys.
    const TRUST: &str = "> You are running Codex in ~/sandbox\n\n  Do you trust the files in this folder?\n\n\u{203a} 1. Yes, continue\n  2. No, quit\n\n  Press enter to continue\n";
    let mut pane = CodexPane::new();
    pane.screen = TRUST.to_string();
    let (delivery, trace) = deliver(&mut pane);
    assert!(matches!(delivery, Delivery::Blocked(gate) if gate.dismiss.is_none()), "{delivery:?} {trace:#?}");
    assert_eq!(pane.esc_sent, 0);
    assert_eq!(pane.enters, 0);
}

// ---- completion: a startup screen is never a turn's answer (bug #4) ----------------------------

const TASK: &str = "Read README.md and suggest one improvement. Do not edit any files.";

#[test]
fn the_update_menu_never_completes_a_run() {
    // Live run 5c7dab6f: the pane read "running" while the task was typed into the menu, then
    // "idle" with the menu still up, and the run completed with the menu text as its answer.
    let mut watch = TuiTurnWatch::from_baseline("").with_task(TASK);
    let mut decisions = vec![observe_tui_turn(&mut watch, "running", UPDATE_MENU, false, "codex")];
    for _ in 0..80 {
        decisions.push(observe_tui_turn(&mut watch, "idle", UPDATE_MENU, false, "codex"));
    }
    assert!(
        !decisions
            .iter()
            .any(|decision| matches!(decision, TuiTurnDecision::Complete { .. })),
        "the update menu must not become an answer: {:?}",
        decisions.iter().find(|d| matches!(d, TuiTurnDecision::Complete { .. }))
    );
    // It is not a failure either: the turn simply has not begun, and nothing times it out.
    assert!(decisions.iter().all(|d| *d == TuiTurnDecision::WaitingInput || *d == TuiTurnDecision::Continue));
}

#[test]
fn the_same_screen_for_another_agent_is_unaffected() {
    // The gate is the Codex profile's; a profile without one keeps its behaviour.
    let mut watch = TuiTurnWatch::from_baseline("").with_task(TASK);
    let first = observe_tui_turn(&mut watch, "idle", UPDATE_MENU, false, "claude");
    assert_ne!(first, TuiTurnDecision::WaitingInput);
}
