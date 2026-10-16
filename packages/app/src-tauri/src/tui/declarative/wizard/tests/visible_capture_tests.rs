//! The guided capture engine (SPEC 11.5), driven with scripted screens and a clock. No pane, no thread.
//! The invariants under test: the engine types only through its one door (`may_type`), never into a
//! busy pane or a dialog, never answers anything, and keeps the right screens.

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::capture::{
    may_type, message_task, permission_task, reply_rows, Action, Capture, CaptureAction, Config, Gate, Refusal, StepId, StepStatus, MAX_DELIVERIES,
};
use crate::tui::declarative::wizard::visible::host::Look;
use crate::tui::declarative::wizard::visible::screen::{Flavour, ScreenKind};

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";
const TRUST: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n  1. Yes, trust this folder\n  2. No, exit\n";
const PERMISSION: &str = "NIMBUS\n\nAllow nimbus to create hello.txt? [y/n]\n";
const LOGIN: &str = "NIMBUS\n\nPlease sign in to continue.\n\n  1. Yes, open the browser\n  2. No, quit\n";

fn working(n: usize) -> String {
    let spinner = ["⠋", "⠙", "⠹", "⠸"][n % 4];
    format!("NIMBUS 0.4\n\n> Reply with exactly the word: pong-aaaaaa\n\n{spinner} Thinking...  ctrl+c to stop\n\n| > Type a task...             |\n")
}

fn reply(word: &str) -> String {
    format!("NIMBUS 0.4\n\n> Reply with exactly the word: {word}\nreply: {word}\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n")
}

struct Drive {
    c: Capture,
    now: u64,
    alive: bool,
    hello: bool,
    grid: Option<(u16, u16)>,
    actions: Vec<Action>,
}

impl Drive {
    fn new() -> Self {
        Self { c: Capture::new(words(), (120, 40), Config::default()), now: 0, alive: true, hello: false, grid: Some((120, 40)), actions: Vec::new() }
    }

    fn look(&self, screen: &str) -> Look {
        Look { screen: screen.into(), watch: screen.into(), status: "idle".into(), alive: self.alive, output_pos: None, hello_written: self.hello, grid: self.grid }
    }

    /// Show `screen` for `ms` milliseconds, one look every 250 ms. Deliveries are not answered here.
    fn show(&mut self, screen: &str, ms: u64) -> Vec<Action> {
        let mut got = Vec::new();
        let end = self.now + ms;
        while self.now < end {
            self.now += 250;
            let look = self.look(screen);
            got.extend(self.c.tick(&look, self.now));
        }
        self.actions.extend(got.clone());
        got
    }

    /// Show a different frame on every look (a spinner).
    fn animate(&mut self, ms: u64) -> Vec<Action> {
        let mut got = Vec::new();
        let end = self.now + ms;
        let mut n = 0;
        while self.now < end {
            self.now += 250;
            n += 1;
            let look = self.look(&working(n));
            got.extend(self.c.tick(&look, self.now));
        }
        got
    }

    fn delivered(&mut self, ok: bool, why: &str) {
        self.c.on_delivered(ok, why, self.now);
    }

    fn status(&self, id: StepId) -> StepStatus {
        self.c.step(id).status
    }
}

fn only_deliver(actions: &[Action]) -> String {
    assert_eq!(actions.len(), 1, "{actions:?}");
    let Action::Deliver { text } = &actions[0];
    text.clone()
}

#[test]
fn the_whole_capture_on_a_made_up_agent() {
    let mut d = Drive::new();
    assert!(d.show("", 1000).is_empty(), "a blank pane gets nothing");
    // 1. the folder question: shown to the person, recorded, nothing typed
    assert!(d.show(TRUST, 12_000).is_empty());
    assert_eq!(d.status(StepId::Start), StepStatus::NeedsYou);
    let needs = d.c.needs_you().unwrap();
    assert_eq!((needs.step, needs.kind.as_str()), (StepId::Start, "trust"));
    assert!(needs.message.contains("presses nothing"), "{}", needs.message);
    assert!(d.c.frame("tui_trust").unwrap().text.contains("Do you trust"));
    // 2. the person answers; the agent shows its input box; the engine types the test message once
    let got = d.show(IDLE, 4000);
    assert_eq!(only_deliver(&got), message_task(&words()));
    assert_eq!(d.status(StepId::Start), StepStatus::Done);
    assert_eq!(d.status(StepId::Ready), StepStatus::Done);
    assert!(d.c.frame("tui_idle").unwrap().text.contains("Type a task"));
    // 3. while the delivery is in flight nothing else is typed
    assert!(d.show(IDLE, 3000).is_empty(), "no second delivery while one is in flight");
    d.delivered(true, "");
    // 4. working screens, then the reply
    assert!(d.animate(3000).is_empty());
    // 5. the answer is on screen and the box is quiet again: the permission task goes in
    let got = d.show(&reply("pong-aaaaaa"), 3500);
    assert_eq!(d.status(StepId::Message), StepStatus::Done);
    assert!(d.c.frame("tui_working").unwrap().text.contains("Thinking"));
    assert!(d.c.frame("tui_reply").unwrap().text.contains("reply: pong-aaaaaa"));
    assert_eq!(only_deliver(&got), permission_task(&words()));
    d.delivered(true, "");
    assert!(d.animate(1500).is_empty());
    // 6. the permission question: recorded, shown, never answered
    assert!(d.show(PERMISSION, 15_000).is_empty(), "no key goes into the question");
    assert_eq!(d.status(StepId::Permission), StepStatus::NeedsYou);
    assert_eq!(d.c.needs_you().unwrap().kind, "permission");
    assert!(d.c.frame("tui_permission").unwrap().text.contains("Allow nimbus"));
    // 7. the person answers; the box is back
    assert!(d.show(IDLE, 4000).is_empty());
    assert_eq!(d.status(StepId::Permission), StepStatus::Done);
    assert_eq!(d.c.current(), StepId::Exit);
    // 8. exit: nothing happens until the person asks
    assert!(d.show(IDLE, 6000).is_empty());
    d.c.act(None, CaptureAction::TryExit, d.now).unwrap();
    let got = d.show(IDLE, 1000);
    assert_eq!(only_deliver(&got), "/exit");
    d.delivered(true, "");
    d.alive = false;
    assert!(d.show("bye\n", 500).is_empty());
    assert!(d.c.settled());
    let set = d.c.into_set();
    assert_eq!(set.exit_method, "slash_exit");
    assert_eq!(set.grid, (120, 40));
    for name in ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_permission"] {
        assert!(set.has(name), "{name}");
    }
    assert!(set.partial.is_empty(), "{:?}", set.partial);
    assert_eq!(set.startup_dialogs, vec!["trust".to_string()]);
}

#[test]
fn a_trust_screen_and_a_permission_screen_get_zero_deliveries_over_a_long_run() {
    for screen in [TRUST, PERMISSION, LOGIN] {
        let mut d = Drive::new();
        let got = d.show(screen, 120_000);
        assert!(got.is_empty(), "{screen}: {got:?}");
        assert_eq!(d.c.deliveries(), 0);
        assert!(d.c.needs_you().is_some());
    }
}

#[test]
fn the_dialog_flavour_is_told_to_the_person() {
    for (screen, kind) in [(TRUST, "trust"), (PERMISSION, "permission"), (LOGIN, "login")] {
        let mut d = Drive::new();
        d.show(screen, 4000);
        assert_eq!(d.c.needs_you().unwrap().kind, kind, "{screen}");
        assert!(d.c.needs_you().unwrap().message.contains("pane"), "the message says where to answer");
    }
    // a startup question that is not trust or sign-in is recorded as a first-run screen
    let mut d = Drive::new();
    d.show("Update available.\n\n  1. Update now\n  2. Skip\n\nenter to continue\n", 4000);
    assert!(d.c.frame("tui_firstrun").is_some());
    assert!(d.c.frame("tui_trust").is_none());
}

#[test]
fn nothing_is_typed_while_the_screen_still_changes_or_the_agent_works() {
    let mut d = Drive::new();
    d.show(IDLE, 6000); // start and ready done, the message is typed
    let first = d.actions.len();
    assert_eq!(first, 1);
    d.delivered(true, "");
    // a second capture where the box is never quiet
    let mut d = Drive::new();
    assert!(d.animate(30_000).is_empty(), "a spinner screen is never typed into");
    assert_eq!(d.c.deliveries(), 0);
    assert_eq!(d.c.kind(), ScreenKind::Working);
}

#[test]
fn an_unclear_screen_waits_for_the_person_and_the_ready_button_is_bound_to_that_picture() {
    let mut d = Drive::new();
    let odd = "mystery agent v1\n\nloading models\nthings are ready\n";
    assert!(d.show(odd, 12_000).is_empty());
    assert_eq!(d.c.needs_you().unwrap().kind, "unsure");
    assert_eq!(d.status(StepId::Ready), StepStatus::NeedsYou);
    // the person says it is ready: this exact picture is accepted
    d.c.act(None, CaptureAction::Ready, d.now).unwrap();
    let got = d.show(odd, 1000);
    assert_eq!(d.status(StepId::Ready), StepStatus::Done);
    assert_eq!(only_deliver(&got), message_task(&words()));
    d.delivered(true, "");
    // a screen that later changes into a question is not covered by that answer
    let got = d.show(PERMISSION, 10_000);
    assert!(got.is_empty());
}

#[test]
fn the_ready_button_refuses_while_a_question_is_on_screen() {
    let mut d = Drive::new();
    d.show(TRUST, 4000);
    let err = d.c.act(None, CaptureAction::Ready, d.now).unwrap_err();
    assert!(err.contains("Answer it in the pane"), "{err}");
}

fn gate<'a>(kind: ScreenKind) -> Gate<'a> {
    Gate { alive: true, stable_ms: 3000, need_ms: 2500, kind, in_flight: false, deliveries: 0, ready_for: None, fingerprint: "fp" }
}

#[test]
fn the_typing_guard_refuses_every_unsafe_state() {
    assert_eq!(may_type(&gate(ScreenKind::Input)), Ok(()));
    assert_eq!(may_type(&Gate { alive: false, ..gate(ScreenKind::Input) }), Err(Refusal::Dead));
    assert_eq!(may_type(&Gate { in_flight: true, ..gate(ScreenKind::Input) }), Err(Refusal::InFlight));
    assert_eq!(may_type(&Gate { deliveries: MAX_DELIVERIES, ..gate(ScreenKind::Input) }), Err(Refusal::TooMany));
    assert_eq!(may_type(&Gate { stable_ms: 2499, ..gate(ScreenKind::Input) }), Err(Refusal::NotStable));
    assert_eq!(may_type(&gate(ScreenKind::Blank)), Err(Refusal::Blank));
    assert_eq!(may_type(&gate(ScreenKind::Shell)), Err(Refusal::Shell));
    assert_eq!(may_type(&gate(ScreenKind::Working)), Err(Refusal::Working));
    for flavour in [Flavour::Trust, Flavour::Login, Flavour::Permission, Flavour::Question] {
        assert_eq!(may_type(&gate(ScreenKind::Dialog(flavour))), Err(Refusal::Dialog));
        // the person's "ready" cannot override a dialog either
        let said = Gate { ready_for: Some("fp"), ..gate(ScreenKind::Dialog(flavour)) };
        assert_eq!(may_type(&said), Err(Refusal::Dialog));
    }
    // an unclear screen needs the person's word for exactly this picture
    assert_eq!(may_type(&gate(ScreenKind::Unknown)), Err(Refusal::NotInput));
    assert_eq!(may_type(&Gate { ready_for: Some("other"), ..gate(ScreenKind::Unknown) }), Err(Refusal::NotInput));
    assert_eq!(may_type(&Gate { ready_for: Some("fp"), ..gate(ScreenKind::Unknown) }), Ok(()));
    // even the person's word does not beat the busy and stable rules
    assert_eq!(may_type(&Gate { ready_for: Some("fp"), stable_ms: 10, ..gate(ScreenKind::Unknown) }), Err(Refusal::NotStable));
    assert_eq!(may_type(&Gate { ready_for: Some("fp"), ..gate(ScreenKind::Working) }), Err(Refusal::Working));
}

#[test]
fn a_failed_delivery_is_noted_and_asked_about_after_two_failures() {
    let mut d = Drive::new();
    let got = d.show(IDLE, 4000);
    assert_eq!(got.len(), 1);
    d.delivered(false, "the text did not show up");
    assert!(d.c.step(StepId::Message).note.contains("did not show up"));
    assert!(d.c.needs_you().is_none());
    let got = d.show(IDLE, 1000);
    assert_eq!(got.len(), 1, "one more try");
    d.delivered(false, "the text did not show up");
    assert_eq!(d.c.needs_you().unwrap().kind, "typing");
    assert!(d.show(IDLE, 20_000).is_empty(), "after two failures nothing more is typed");
    assert_eq!(d.c.deliveries(), 2);
}

#[test]
fn skip_marks_a_step_and_moves_on_without_typing() {
    let mut d = Drive::new();
    d.show(TRUST, 4000);
    d.c.act(Some(StepId::Start), CaptureAction::Skip, d.now).unwrap();
    assert_eq!(d.status(StepId::Start), StepStatus::Skipped);
    assert_eq!(d.c.current(), StepId::Ready);
    assert!(d.show(TRUST, 5000).is_empty(), "the question is still there: still nothing typed");
}

#[test]
fn stuck_keeps_what_was_captured_and_marks_the_step_partial() {
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.animate(2000);
    d.c.act(None, CaptureAction::Stuck, d.now).unwrap();
    assert_eq!(d.status(StepId::Message), StepStatus::Stuck);
    assert!(d.c.frame("tui_idle").is_some());
    let set = d.c.into_set();
    assert_eq!(set.partial, vec!["message".to_string()]);
}

#[test]
fn retry_clears_that_step_and_runs_it_again() {
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.animate(2000);
    d.show(&reply("pong-aaaaaa"), 4000); // the permission task goes in too
    d.delivered(true, "");
    assert!(d.c.frame("tui_reply").is_some());
    d.c.act(Some(StepId::Message), CaptureAction::Retry, d.now).unwrap();
    assert!(d.c.frame("tui_reply").is_none() && d.c.frame("tui_working").is_none());
    assert!(d.c.frame("tui_idle").is_some(), "earlier steps keep their captures");
    assert_eq!(d.c.current(), StepId::Message);
    let got = d.show(IDLE, 4000);
    assert_eq!(only_deliver(&got), message_task(&words()));
}

#[test]
fn an_agent_that_quits_fails_the_running_step_and_skips_the_ones_that_cannot_run() {
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.alive = false;
    d.show("", 1000);
    assert_eq!(d.status(StepId::Message), StepStatus::Failed);
    assert_eq!(d.status(StepId::Permission), StepStatus::Skipped);
    assert!(d.c.settled());
    assert!(d.c.into_set().partial.contains(&"message".to_string()));
}

#[test]
fn the_answer_word_must_be_on_a_row_that_is_not_the_instruction() {
    let w = "pong-aaaaaa";
    assert_eq!(reply_rows(&format!("> Reply with exactly the word: {w}\n"), w), 0);
    assert_eq!(reply_rows(&format!("> Reply with exactly the word: {w}\nreply: {w}\n"), w), 1);
    assert_eq!(reply_rows("nothing", w), 0);
}

#[test]
fn a_settled_screen_without_the_answer_asks_instead_of_guessing() {
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.animate(1500);
    let no_answer = "NIMBUS 0.4\n\n> Reply with exactly the word: pong-aaaaaa\nhmm\n\n| > Type a task...             |\n";
    d.show(no_answer, 12_000);
    assert_eq!(d.c.needs_you().unwrap().kind, "unsure_reply");
    assert_eq!(d.status(StepId::Message), StepStatus::NeedsYou);
    // the person says: take this one
    d.c.act(None, CaptureAction::Ready, d.now).unwrap();
    d.show(no_answer, 1000);
    assert_eq!(d.status(StepId::Message), StepStatus::Done);
}

#[test]
fn an_agent_that_writes_the_file_without_asking_ends_the_permission_step_with_a_note() {
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.animate(1500);
    let got = d.show(&reply("pong-aaaaaa"), 4000);
    assert_eq!(only_deliver(&got), permission_task(&words()));
    d.delivered(true, "");
    d.animate(1500);
    d.hello = true;
    d.show(IDLE, 4000);
    assert_eq!(d.status(StepId::Permission), StepStatus::Done);
    assert!(d.c.step(StepId::Permission).note.contains("did not ask"));
    assert!(d.c.frame("tui_permission").is_none());
    assert!(d.c.into_set().hello_written);
}

#[test]
fn the_exit_step_types_nothing_until_asked_and_closed_ends_it() {
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.c.act(Some(StepId::Message), CaptureAction::Skip, d.now).unwrap();
    d.c.act(Some(StepId::Permission), CaptureAction::Skip, d.now).unwrap();
    assert_eq!(d.c.current(), StepId::Exit);
    assert!(d.show(IDLE, 20_000).is_empty());
    d.c.act(None, CaptureAction::Closed, d.now).unwrap();
    assert_eq!(d.status(StepId::Exit), StepStatus::Done);
    assert_eq!(d.c.into_set().exit_method, "person");
}

#[test]
fn try_exit_is_refused_outside_the_exit_step_and_into_a_dialog() {
    let mut d = Drive::new();
    assert!(d.c.act(None, CaptureAction::TryExit, 0).is_err());
    let mut d = Drive::new();
    d.show(IDLE, 6000);
    d.delivered(true, "");
    d.c.act(Some(StepId::Message), CaptureAction::Skip, d.now).unwrap();
    d.c.act(Some(StepId::Permission), CaptureAction::Skip, d.now).unwrap();
    d.delivered(true, "");
    d.c.act(None, CaptureAction::TryExit, d.now).unwrap();
    let got = d.show(PERMISSION, 10_000);
    assert!(got.is_empty(), "/exit is not typed into a question");
}

#[test]
fn the_status_names_the_step_what_can_be_pressed_and_the_screen_kind() {
    let mut d = Drive::new();
    d.show(TRUST, 4000);
    let status = d.c.status_json(d.now);
    assert_eq!(status["step"], "start");
    assert_eq!(status["screen_kind"], "dialog");
    assert_eq!(status["needs_you"]["kind"], "trust");
    let can: Vec<&str> = status["can"].as_array().unwrap().iter().filter_map(|c| c.as_str()).collect();
    assert!(can.contains(&"skip") && can.contains(&"retry") && can.contains(&"stuck") && !can.contains(&"ready"), "{can:?}");
    assert_eq!(status["steps"].as_array().unwrap().len(), 5);
    assert_eq!(status["steps"][0]["status"], "needs_you");
    assert_eq!(status["steps"][1]["status"], "pending");
}

#[test]
fn captured_frames_are_cleaned_but_keep_the_real_word() {
    let mut d = Drive::new();
    d.show("C:\\Users\\someone\\project\n| > Type a task...\n", 6000);
    d.delivered(true, "");
    let idle = d.c.frame("tui_idle").unwrap();
    assert!(idle.text.contains("C:\\Users\\user\\project"), "{}", idle.text);
    assert!(!idle.text.contains("someone"));
    d.animate(1500);
    d.show(&reply("pong-aaaaaa"), 4000);
    assert!(d.c.frame("tui_reply").unwrap().text.contains("pong-aaaaaa"), "fixtures keep the real word");
}

#[test]
fn a_pane_that_changes_size_mid_capture_says_so_and_the_set_records_the_grid_it_was_asked_for() {
    let mut d = Drive::new();
    d.show(IDLE, 3000);
    assert!(d.c.into_set().notes.is_empty(), "the grid never changed");
    d.grid = Some((100, 30));
    d.show(IDLE, 1000);
    let set = d.c.into_set();
    assert_eq!(set.grid, (120, 40));
    assert_eq!(set.notes.len(), 1, "{:?}", set.notes);
    assert!(set.notes[0].contains("120x40 to 100x30"), "{}", set.notes[0]);
    d.show(IDLE, 1000);
    assert_eq!(d.c.into_set().notes.len(), 1, "said once");
}
