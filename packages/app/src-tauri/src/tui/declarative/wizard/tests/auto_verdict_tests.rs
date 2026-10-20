//! "Finish without me" (SPEC 11.19), the capture engine's side: what the drafter AI's one-word look at a screen the
//! wizard could not read may cause. Driven with scripted screens and a clock, no pane, no thread.
//!
//! The invariants under test: a verdict can only press "It is ready" for the screen it was given, hand a question to
//! the SAFE answerer (which still refuses every broad option), wait, stop the run (sign-in, payment) or keep what was
//! captured (error). Nothing a verdict says ever types or presses anything else.

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::answer::AnswerKey;
use crate::tui::declarative::wizard::visible::capture::{message_task, Action, Capture, CaptureAction, Config, StepId, StepStatus};
use crate::tui::declarative::wizard::visible::host::Look;
use crate::tui::declarative::wizard::visible::screen::{classify, ScreenKind};
use crate::tui::declarative::wizard::visible::verdict::{Decision, Verdict};

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

/// An input box next to a word that belongs to a question: the classifier says "unknown" and asks the person.
const UNSURE_READY: &str = "NIMBUS 0.4\n\nTip: ask me to approve a plan before I start.\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";
/// A question the classifier does not know (no choice words), with one plain yes and one no.
const UNKNOWN_QUESTION: &str = "NIMBUS\n\nLet nimbus read and edit the files in this folder?\n\n\u{25CF} Yes\n  No\n";
/// The same kind of question where the only yes is a broad one.
const ALWAYS_ONLY: &str = "NIMBUS\n\nLet nimbus read and edit the files in this folder?\n\n\u{25CF} Yes - always\n  No\n";
const SIGN_IN: &str = "NIMBUS\n\nVisit https://example.test/device and enter the code ABCD-1234.\nWaiting for you to finish...\n";
const FOLDER_TRUST: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n  1. Yes, trust this folder\n  2. No, exit\n";
const ERROR_SCREEN: &str = "NIMBUS 0.4\n\nError: 429 rate limit reached, try again later\n";

struct Drive {
    c: Capture,
    now: u64,
}

impl Drive {
    fn new(auto: bool) -> Self {
        Self { c: Capture::new(words(), (120, 40), Config::default()).with_auto_answer(auto), now: 0 }
    }

    fn show(&mut self, screen: &str, ms: u64) -> Vec<Action> {
        let mut got = Vec::new();
        let end = self.now + ms;
        while self.now < end {
            self.now += 250;
            let look = Look { screen: screen.into(), watch: screen.into(), status: "idle".into(), alive: true, output_pos: None, hello_written: false, grid: Some((120, 40)), history: String::new(), styled: None };
            got.extend(self.c.tick(&look, self.now));
        }
        got
    }

    fn verdict(&mut self, v: Verdict) -> Decision {
        self.c.apply_verdict(v, self.now)
    }
}

fn pressed(actions: &[Action]) -> Vec<AnswerKey> {
    actions.iter().flat_map(|a| if let Action::Answer { keys, .. } = a { keys.clone() } else { Vec::new() }).collect()
}

fn delivered(actions: &[Action]) -> Vec<String> {
    actions.iter().filter_map(|a| if let Action::Deliver { text } = a { Some(text.clone()) } else { None }).collect()
}

#[test]
fn the_screens_used_here_are_the_ones_the_classifier_cannot_read() {
    assert_eq!(classify(UNSURE_READY), ScreenKind::Unknown);
    assert_eq!(classify(UNKNOWN_QUESTION), ScreenKind::Unknown);
    assert_eq!(classify(ALWAYS_ONLY), ScreenKind::Unknown);
}

#[test]
fn an_unsure_ready_screen_waits_for_the_person_until_the_ai_look_says_ready() {
    let mut d = Drive::new(false);
    let got = d.show(UNSURE_READY, 30_000);
    assert!(delivered(&got).is_empty(), "nothing is typed into a screen the wizard cannot read");
    assert_eq!(d.c.needs_you().map(|n| n.kind.as_str()), Some("unsure"));
    // the AI look: "ready for input" is the same as the person's "It is ready" for this exact screen
    assert_eq!(d.verdict(Verdict::ReadyForInput), Decision::PressedReady);
    assert!(d.c.needs_you().is_none());
    let got = d.show(UNSURE_READY, 4000);
    assert_eq!(delivered(&got), vec![message_task(&words())], "the test message goes in once");
}

#[test]
fn a_ready_verdict_is_refused_for_a_question_on_the_screen() {
    let mut d = Drive::new(false);
    d.show(FOLDER_TRUST, 5000);
    assert!(matches!(d.verdict(Verdict::ReadyForInput), Decision::Refused(_)));
    let got = d.show(FOLDER_TRUST, 30_000);
    assert!(delivered(&got).is_empty() && pressed(&got).is_empty());
}

#[test]
fn a_ready_verdict_is_bound_to_the_screen_it_was_given() {
    let mut d = Drive::new(false);
    d.show(UNSURE_READY, 10_000);
    assert_eq!(d.verdict(Verdict::ReadyForInput), Decision::PressedReady);
    // a different screen arrives before the wizard types: the earlier "ready" does not cover it
    let other = "NIMBUS 0.4

Something new happened.
Please approve the new terms before going on.

+------------------------------+
| > Type a task...             |
+------------------------------+
";
    let got = d.show(other, 20_000);
    assert!(delivered(&got).is_empty(), "{got:?}");
}

#[test]
fn a_question_the_classifier_does_not_know_is_answered_only_after_the_ai_look_and_only_with_the_plain_yes() {
    let mut d = Drive::new(true);
    d.show("", 500);
    let got = d.show(UNKNOWN_QUESTION, 8000);
    assert!(pressed(&got).is_empty(), "without the AI look the wizard does not know it is a question: {got:?}");
    assert_eq!(d.verdict(Verdict::QuestionTrustOrOnce), Decision::SafeAnswerer);
    let got = d.show(UNKNOWN_QUESTION, 4000);
    assert_eq!(pressed(&got), vec![AnswerKey::Enter], "the highlight is on the plain yes: only Enter");
}

#[test]
fn the_safe_answerer_still_refuses_a_broad_option_when_the_ai_calls_it_a_trust_question() {
    let mut d = Drive::new(true);
    d.show("", 500);
    d.show(ALWAYS_ONLY, 2000);
    assert_eq!(d.verdict(Verdict::QuestionTrustOrOnce), Decision::SafeAnswerer);
    let got = d.show(ALWAYS_ONLY, 20_000);
    assert!(pressed(&got).is_empty(), "never an always-allow option: {got:?}");
    assert!(delivered(&got).is_empty());
    assert!(d.c.into_set().answered.is_empty());
}

#[test]
fn the_ai_look_cannot_answer_setup_questions_when_the_session_has_the_answerer_off() {
    let mut d = Drive::new(false);
    d.show(UNKNOWN_QUESTION, 3000);
    assert!(matches!(d.verdict(Verdict::QuestionTrustOrOnce), Decision::Refused(_)));
    assert!(pressed(&d.show(UNKNOWN_QUESTION, 20_000)).is_empty());
}

#[test]
fn a_sign_in_verdict_stops_the_run_and_nothing_more_is_typed_or_pressed() {
    let mut d = Drive::new(true);
    d.show(SIGN_IN, 4000);
    let decision = d.verdict(Verdict::SignInOrPayment);
    assert_eq!(decision.stop_code(), Some("WIZARD_SIGN_IN_NEEDED"), "{decision:?}");
    assert_eq!(d.c.status_json(d.now)["halted"], "sign_in");
    // even an input box and a question in the minutes that follow get nothing
    let mut all = d.show(UNSURE_READY, 60_000);
    all.extend(d.show(UNKNOWN_QUESTION, 30_000));
    assert!(all.is_empty(), "{all:?}");
    // "Retry" starts the step again
    d.c.act(Some(StepId::Start), CaptureAction::Retry, d.now).unwrap();
    assert!(d.c.status_json(d.now)["halted"].is_null());
}

#[test]
fn a_working_verdict_keeps_waiting_and_an_unknown_one_waits_then_skips_without_pressing_anything() {
    let mut d = Drive::new(true);
    d.show(UNSURE_READY, 10_000);
    assert_eq!(d.verdict(Verdict::Working), Decision::Wait);
    assert_eq!(d.verdict(Verdict::QuestionOther), Decision::WaitThenSkip);
    assert_eq!(d.verdict(Verdict::Unknown), Decision::WaitThenSkip);
    let got = d.show(UNSURE_READY, 10_000);
    assert!(got.is_empty(), "{got:?}");
    // the front end then skips the step: the engine moves on and records it
    d.c.act(None, CaptureAction::Skip, d.now).unwrap();
    assert_eq!(d.c.step(StepId::Ready).status, StepStatus::Skipped);
}

#[test]
fn an_error_verdict_keeps_what_was_captured_and_notes_the_error_for_the_drafter() {
    let mut d = Drive::new(true);
    d.show(ERROR_SCREEN, 5000);
    assert_eq!(d.verdict(Verdict::Error), Decision::MarkedStuck);
    let set = d.c.into_set();
    assert!(set.partial.iter().any(|p| p == "ready"), "{:?}", set.partial);
    assert!(set.notes.iter().any(|n| n.contains("showed an error") && n.contains("429")), "{:?}", set.notes);
}

#[test]
fn a_verdict_word_is_one_of_exactly_seven() {
    for v in Verdict::ALL {
        assert_eq!(Verdict::parse(v.as_str()), Some(v));
    }
    assert_eq!(Verdict::ALL.len(), 7);
    assert_eq!(Verdict::parse("press enter"), None);
    assert_eq!(Verdict::parse("ready_for_input please"), None);
}

#[test]
fn a_trust_look_on_a_screen_that_does_not_talk_about_trust_or_permission_is_refused() {
    let update = "NIMBUS\n\nA new version is available. Update now?\n\n\u{25CF} Yes\n  No\n";
    assert_eq!(classify(update), ScreenKind::Unknown);
    let mut d = Drive::new(true);
    d.show("", 500);
    d.show(update, 3000);
    assert!(matches!(d.verdict(Verdict::QuestionTrustOrOnce), Decision::Refused(_)), "an update prompt is never the safe answerer's");
    assert!(pressed(&d.show(update, 20_000)).is_empty());
}

#[test]
fn an_error_look_at_an_account_or_status_row_shown_since_startup_is_refused_but_a_real_error_row_still_counts() {
    // the person's address and the plan in parentheses, drawn since startup: the AI called it an error; Rust does not
    let account = "NIMBUS 0.4\n\nsomeone@example.com (Nimbus Starter Quota)\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";
    let mut d = Drive::new(true);
    d.show(account, 5000);
    assert!(matches!(d.verdict(Verdict::Error), Decision::Refused(_)), "an account row is no error");
    assert!(!d.c.into_set().notes.iter().any(|n| n.contains("showed an error")));
    // the same row beside a real new error row: the look stands
    let mut d = Drive::new(true);
    d.show(account, 5000);
    d.show(&format!("{account}Error: 429 rate limit reached\n"), 5000);
    assert_eq!(d.verdict(Verdict::Error), Decision::MarkedStuck);
    // no row at all that reads like a problem: no say either way, the look stands
    let mut d = Drive::new(true);
    d.show("NIMBUS 0.4\n\nsomething broke in an odd way\n", 5000);
    assert_eq!(d.verdict(Verdict::Error), Decision::MarkedStuck);
}
