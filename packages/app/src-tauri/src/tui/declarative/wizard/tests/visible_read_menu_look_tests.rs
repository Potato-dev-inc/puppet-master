//! A permission question whose menu the parser READ but in which no option is a plain yes (SPEC 11.22, found by the
//! founder's run: the second question of one capture step stopped the wizard with "none of the options plainly means
//! yes"). The checked AI look is asked for that too, Rust's validation stays the guard, the wording of a one-time yes is
//! broader, the decision log says how the menu was read, and a run that stops leaves a redacted debug file.
//!
//! Scripted screens through the capture engine (`Capture`), no pane and no thread; the debug file through the real
//! operations over the scripted pane host.

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::answer::{can_look, is_unreadable, AnswerKey};
use crate::tui::declarative::wizard::visible::capture::{Action, Capture, Config};
use crate::tui::declarative::wizard::visible::host::Look;
use crate::tui::declarative::wizard::visible::menu::{classify_option, describe_menu, parse_menu, OptionClass, NO_PLAIN_YES};

use super::fake_host::{screens, wait_for};
use super::support::{ok, Tree};

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";

/// The yes row is drawn one column to the left of the other rows (a different indent), so the parser reads a menu made of
/// the "Always" and "No" rows only; the cursor is on "Always".
const YES_MISSED: &str = "Allow nimbus to run `ls`?\n\n Yes, run command\n   > Always run in this project\n     No\n";
/// The same question after one Up key: the cursor is on the yes row.
const YES_MISSED_UP: &str = "Allow nimbus to run `ls`?\n\n > Yes, run command\n     Always run in this project\n     No\n";
/// "Run command / Always run / Cancel", numbered, cursor on the first row.
const RUN_ALWAYS_CANCEL: &str = "Allow nimbus to run `ls`?\n\n> 1. Run command\n  2. Always run\n  3. Cancel\n";
/// Only broad options and a refusal.
const BROAD_ONLY: &str = "Allow nimbus to run `ls`?\n\n> 1. Always run in this project\n  2. Cancel\n";

struct Drive {
    c: Capture,
    now: u64,
}

impl Drive {
    fn new() -> Self {
        Self { c: Capture::new(words(), (120, 40), Config::default()).with_auto_answer(true), now: 0 }
    }

    fn show(&mut self, screen: &str, ms: u64) -> Vec<AnswerKey> {
        let mut keys = Vec::new();
        let end = self.now + ms;
        while self.now < end {
            self.now += 250;
            let look = Look {
                screen: screen.into(),
                watch: screen.into(),
                status: "idle".into(),
                alive: true,
                output_pos: None,
                hello_written: false,
                grid: Some((120, 40)),
                history: String::new(),
                styled: None,
            };
            for action in self.c.tick(&look, self.now) {
                if let Action::Answer { keys: k, .. } = action {
                    keys.extend(k);
                }
            }
        }
        keys
    }
}

// ---- the wording of a one-time yes -----------------------------------------------------------------------

#[test]
fn every_one_time_affirmative_wording_is_a_plain_yes() {
    for text in [
        "Yes",
        "Yes, run command",
        "Yes, proceed",
        "Yes, I trust this folder",
        "Yes, write it",
        "Allow",
        "Allow once",
        "Allow this time",
        "Run",
        "Run command",
        "Run once",
        "Proceed",
        "Continue",
        "Approve",
        "Approve once",
        "Accept",
        "OK",
        "Okay",
        "Confirm",
        "Go ahead",
        "Execute",
        "Trust this folder",
    ] {
        assert_eq!(classify_option(text), OptionClass::SafeYes, "{text}");
    }
}

#[test]
fn anything_broad_is_never_a_plain_yes_whatever_else_it_says() {
    for text in [
        "Always allow",
        "Always run",
        "Yes, always run in this project",
        "Yes, and don't ask again for: ls *",
        "Allow and do not ask again",
        "Allow for this session",
        "Yes, allow all edits this session",
        "Allow all",
        "Run all commands",
        "Run every time",
        "Run each time and remember my choice",
        "Remember this choice",
        "Trust all folders",
        "Yes, for this project",
        "Allow permanently",
        "Approve forever",
        "Allow for the project",
        "Yes, from now on",
        "Add to the allow list",
        "Continue without asking",
        "Proceed anyway",
        "Accept any command",
        "Run with full access",
        "Yes, bypass permissions",
    ] {
        assert_eq!(classify_option(text), OptionClass::Risky, "{text}");
    }
    // a refusal is not a yes either
    for text in ["No", "Deny", "Cancel", "No, exit", "Reject", "Decline"] {
        assert_ne!(classify_option(text), OptionClass::SafeYes, "{text}");
    }
}

#[test]
fn the_menu_as_read_is_written_with_the_class_of_every_option() {
    let menu = parse_menu(RUN_ALWAYS_CANCEL).expect("a menu");
    assert_eq!(describe_menu(&menu), r#"numbered rows: "1. Run command" (plain yes, highlighted); "2. Always run" (broad); "3. Cancel" (no)"#);
}

#[test]
fn a_menu_that_was_read_without_a_plain_yes_may_be_looked_at_but_other_reasons_may_not() {
    assert!(can_look(NO_PLAIN_YES));
    assert!(can_look("no menu or yes/no question could be read from the screen"));
    assert!(!is_unreadable(NO_PLAIN_YES), "it is not 'unreadable': the menu was read");
    // a question the wizard never answers, a menu with two plain yeses and a menu that moved are nobody's to help with
    assert!(!can_look("it is a login question, which only you answer"));
    assert!(!can_look("more than one option could be the safe one"));
    assert!(!can_look("the menu changed while it was being answered"));
    assert!(!can_look("the question mentions \"model\", and sign-in, payment, model and telemetry questions are only yours to answer"));
}

// ---- (b) other wording answered by the rules ---------------------------------------------------------------

#[test]
fn run_command_always_run_cancel_picks_run_command_by_the_rules() {
    let mut d = Drive::new();
    d.show("", 500);
    let keys = d.show(RUN_ALWAYS_CANCEL, 3500);
    assert_eq!(keys, vec![AnswerKey::Text("1".into()), AnswerKey::Enter], "the number, then Enter once the cursor is on that row");
    d.show(IDLE, 4000);
    let set = d.c.into_set();
    assert_eq!(set.answered.len(), 1);
    let a = &set.answered[0];
    assert_eq!((a.label.as_str(), a.via.as_str()), ("Run command", "rules"));
    assert!(a.read.contains(r#"Always run" (broad)"#) && a.read.contains(r#"Run command" (plain yes"#), "{}", a.read);
}

// ---- (a) the parser misses the yes row: the checked AI look ----------------------------------------------

#[test]
fn a_read_menu_without_a_plain_yes_asks_for_the_ai_look_and_the_decision_log_carries_the_menu() {
    let mut d = Drive::new();
    d.show("", 500);
    assert!(d.show(YES_MISSED, 4000).is_empty(), "nothing is pressed by the rules");
    let status = d.c.status_json(d.now);
    assert_eq!(status["needs_you"]["kind"], "permission");
    assert_eq!(status["needs_you"]["unreadable"], true, "the AI look may be asked: {status}");
    let read = status["menu_read"].as_str().expect("the menu as read");
    assert!(read.contains(r#"Always run in this project" (broad, highlighted)"#) && read.contains(r#"No" (no)"#), "{read}");
    assert!(!read.contains("Yes, run command"), "the parser missed the yes row: {read}");
    assert!(d.c.needs_you().unwrap().message.contains("none of the options plainly means yes"));
}

#[test]
fn the_ai_look_names_the_missed_yes_row_rust_checks_it_and_it_is_pressed_one_key_at_a_time_and_verified() {
    let mut d = Drive::new();
    d.show("", 500);
    d.show(YES_MISSED, 4000);
    let fp = d.c.status_json(d.now)["question_fp"].as_str().unwrap().to_string();
    let label = d.c.menu_choice(&fp, "Yes, run command", &["Up".to_string(), "Enter".to_string()]).expect("Rust accepts a plain yes that is on the screen");
    assert_eq!(label, "Yes, run command");
    // Up first; the screen shows the cursor on the yes row; only then Enter
    let first = d.show(YES_MISSED, 750);
    assert_eq!(first, vec![AnswerKey::Up]);
    let second = d.show(YES_MISSED_UP, 750);
    assert_eq!(second, vec![AnswerKey::Enter]);
    d.show(IDLE, 4000);
    let set = d.c.into_set();
    assert_eq!(set.answered.len(), 1, "{:?}", set.answered);
    let a = &set.answered[0];
    assert_eq!((a.label.as_str(), a.via.as_str(), a.keys.as_str()), ("Yes, run command", "ai_look", "Up, Enter"));
    assert!(a.read.contains(r#""Always run in this project" (broad"#) && !a.read.contains("plain yes"), "the menu as the parser read it: {}", a.read);
}

// ---- (c) only broad options: the look is refused, nothing is pressed ---------------------------------------

#[test]
fn a_read_menu_whose_only_non_deny_option_is_broad_is_never_pressed_and_the_ai_look_is_refused() {
    let mut d = Drive::new();
    d.show("", 500);
    assert!(d.show(BROAD_ONLY, 4000).is_empty());
    let fp = d.c.status_json(d.now)["question_fp"].as_str().unwrap().to_string();
    for (option, keys) in [("Always run in this project", vec!["Enter"]), ("Cancel", vec!["Down", "Enter"]), ("Run command", vec!["Enter"])] {
        let keys: Vec<String> = keys.into_iter().map(String::from).collect();
        let err = d.c.menu_choice(&fp, option, &keys).unwrap_err();
        assert!(err.contains("never picks") || err.contains("not on the screen"), "{option}: {err}");
    }
    assert!(d.show(BROAD_ONLY, 6000).is_empty(), "nothing was pressed");
    assert_eq!(d.c.needs_you().unwrap().kind, "permission", "the question stays the person's");
    assert!(d.c.into_set().answered.is_empty());
}

#[test]
fn a_question_that_mentions_sign_in_gets_no_look_even_when_no_option_is_a_yes() {
    let mut d = Drive::new();
    d.show("", 500);
    let screen = "Allow nimbus to sign in to run `ls`?\n\n> Always run\n  Cancel\n";
    d.show(screen, 4000);
    let status = d.c.status_json(d.now);
    assert_ne!(status["needs_you"]["unreadable"], true, "{status}");
}

// ---- the debug file ------------------------------------------------------------------------------------

#[test]
fn a_run_that_stops_leaves_a_redacted_debug_file_with_the_looks_the_menu_and_the_reason() {
    let tree = Tree::new("read-menu-dump");
    let (wizard, host) = tree.visible_wizard();
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "auto_answer": true, "autonomous": true}));
    let id = started["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    let plan = ok(&wizard, "capture", json!({"session": id, "op": "plan"}));
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    ok(&wizard, "capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint}));
    host.set_screen("someone@example.com (Starter Quota)\nAllow nimbus to run `ls`?\n\n> 1. Always run in this project\n  2. Cancel\n");
    let status = wait_for(20, || {
        let s = ok(&wizard, "capture", json!({"session": id, "op": "status"}));
        (s["needs_you"]["kind"] == "permission").then_some(s)
    })
    .expect("the question stopped the wizard");
    assert!(status["menu_read"].as_str().unwrap().contains("(broad, highlighted)"), "{status}");
    assert!(!status.to_string().contains("someone@example.com"), "no raw address reaches the window, not even in the question identity: {status}");

    // what the front end does at a stop
    let written = ok(&wizard, "capture", json!({"session": id, "op": "dump", "reason": "Nimbus asks a question that only you may answer. Mail someone@example.com"}));
    let path = std::path::PathBuf::from(written["path"].as_str().unwrap());
    assert_eq!(path.file_name().unwrap(), "wizard-debug.json");
    let work = wizard.read(&id, |s| s.work_dir.clone()).unwrap();
    assert_eq!(path.parent().unwrap(), work, "in the session's folder");
    let text = std::fs::read_to_string(&path).expect("the file was written");
    assert!(!text.contains("someone@example.com"), "emails are masked: {text}");
    assert!(text.contains("<email>"), "{text}");
    let doc: Value = serde_json::from_str(&text).expect("valid json");
    assert!(doc["reason"].as_str().unwrap().contains("only you may answer"));
    assert_eq!(doc["autonomous"], true);
    let frames = doc["capture"]["frames"].as_array().expect("the last looks");
    let last = frames.last().expect("a look");
    assert!(last["screen"].as_str().unwrap().contains("Always run in this project"));
    assert!(last["highlight"].is_array());
    assert!(last["menu"].as_str().is_some_and(|m| m.contains(r#"Always run in this project" (broad"#)), "the parsed menu: {last}");
    let decisions = doc["capture"]["decisions"].as_array().expect("what the answerer decided");
    assert!(decisions.iter().any(|d| d["decision"].as_str().is_some_and(|t| t.contains("declined: none of the options plainly means yes"))), "{decisions:?}");
    assert!(doc["capture"]["status"]["menu_read"].is_string());
    // a copy that stays when the session's folder is removed
    let copy = written["copy"].as_str().expect("a copy");
    assert!(std::fs::read_to_string(copy).unwrap().contains("<email>"));
    // closing the pane keeps the looks for the file (the front end discards the capture before it asks)
    ok(&wizard, "capture", json!({"session": id, "op": "discard"}));
    let again = ok(&wizard, "capture", json!({"session": id, "op": "dump", "reason": "again"}));
    let text = std::fs::read_to_string(again["path"].as_str().unwrap()).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert!(doc["capture"]["frames"].as_array().is_some_and(|f| !f.is_empty()), "the looks survive the discard");
    ok(&wizard, "cancel", json!({"session": id}));
    let _ = screens::IDLE;
}

#[test]
fn the_questions_identity_never_carries_an_email_address_and_still_binds_the_ai_look() {
    let mut d = Drive::new();
    d.show("", 500);
    let screen = "someone@example.com (Starter Quota)\nAllow nimbus to run `ls`?\n\n> 1. Always run in this project\n  2. Cancel\n";
    d.show(screen, 4000);
    let status = d.c.status_json(d.now);
    let fp = status["question_fp"].as_str().expect("the identity").to_string();
    assert!(!fp.contains("someone@example.com") && fp.contains("<email>"), "{fp}");
    // the same identity comes back with the AI look's answer: Rust still knows which question it is (and refuses the option)
    let err = d.c.menu_choice(&fp, "Always run in this project", &["Enter".to_string()]).unwrap_err();
    assert!(err.contains("never picks"), "{err}");
}
