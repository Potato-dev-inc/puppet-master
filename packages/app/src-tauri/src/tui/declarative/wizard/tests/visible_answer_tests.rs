//! The wizard answering its own panes' setup questions (SPEC 11.16), on scripted screens: which options it
//! may choose, which it must never choose, how it walks an arrow menu (re-reading the screen after every
//! key), that it answers each question once, and where it may answer at all.

use std::path::Path;

use crate::tui::declarative::wizard::visible::answer::{
    classify_option, off_limits, parse_menu, safe_pick, scope_ok, under, AnswerCfg, AnswerKey, Answerer, Hint, Input, MenuKind, OptionClass, Step,
};
use crate::tui::declarative::wizard::visible::screen::Flavour;

/// The first-run question of Antigravity CLI (agy 1.4.0), as the founder saw it.
const AGY: &str = "Accessing workspace:\nC:\\Users\\me\\AppData\\Local\\Temp\\pm-wizard-wz_ab12\n\nDo you trust the contents of this project?\nAntigravity CLI requires permission to read, edit, and run files here.\n\n> Yes, I trust this folder\n  No, exit\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const SECOND_ROW: &str = "Do you trust the files in this folder?\n\n> No, exit\n  Yes, I trust this folder\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const SECOND_ROW_MOVED: &str = "Do you trust the files in this folder?\n\n  No, exit\n> Yes, I trust this folder\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const NUMBERED: &str = "Do you want to trust this folder?\n\n  1. Yes, trust this folder\n  2. No, exit\n";
const NUMBERED_CURSOR: &str = "Do you want to trust this folder?\n\n> 1. Yes, trust this folder\n  2. No, exit\n";
const YES_NO: &str = "Allow nimbus to create hello.txt? [y/n]\n";
const ALWAYS_ONLY: &str = "Allow this command to run?\n\n> Yes, always allow\n  No\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const SESSION_AND_ONCE: &str = "Allow this command to run?\n\n  Yes\n> Yes, and don't ask again for this session\n  No\n";

fn answerer() -> Answerer {
    Answerer::new("capture", AnswerCfg::default()).with_folder(Some(std::path::PathBuf::from(r"C:\Users\me\AppData\Local\Temp\pm-wizard-wz_ab12")))
}

fn look(a: &mut Answerer, screen: &str, flavour: Option<Flavour>, stable_ms: u64, now: u64) -> Step {
    a.step(&Input { screen, styled: None, flavour, stable_ms, now, hint: None })
}

fn press(step: Step) -> Vec<AnswerKey> {
    match step {
        Step::Press { keys, .. } => keys,
        other => panic!("expected keys, got {other:?}"),
    }
}

#[test]
fn the_agy_question_is_read_as_an_arrow_menu_with_the_cursor_on_the_safe_row() {
    let menu = parse_menu(AGY).expect("a menu");
    assert_eq!(menu.kind, MenuKind::Arrow);
    assert_eq!(menu.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Yes, I trust this folder", "No, exit"]);
    assert_eq!((menu.highlighted(), safe_pick(&menu)), (Some(0), Ok(0)));
    // the path row and the key hint are not options
    assert_eq!(menu.options.len(), 2);
}

#[test]
fn options_are_classified_by_what_they_say() {
    for yes in ["Yes", "Yes, I trust this folder", "Allow once", "Allow", "Approve", "Trust this folder", "y"] {
        assert_eq!(classify_option(yes), OptionClass::SafeYes, "{yes}");
    }
    for risky in [
        "Yes, always allow",
        "Yes, and don't ask again",
        "Yes, for this session",
        "Allow all tools",
        "Trust all folders",
        "Yes (yolo)",
        "Bypass permissions",
        "Yes, trust the parent folder",
        "Always allow",
        "Allow and remember",
    ] {
        assert_eq!(classify_option(risky), OptionClass::Risky, "{risky}");
    }
    for other in ["No", "No, exit", "Deny", "Quit", "Cancel", "Skip"] {
        assert_eq!(classify_option(other), OptionClass::Other, "{other}");
    }
}

#[test]
fn nothing_is_pressed_before_the_question_has_held_still_for_a_second() {
    let mut a = answerer();
    assert!(matches!(look(&mut a, AGY, Some(Flavour::Trust), 400, 400), Step::Waiting(_)));
    assert!(matches!(look(&mut a, AGY, Some(Flavour::Trust), 999, 999), Step::Waiting(_)));
    assert_eq!(press(look(&mut a, AGY, Some(Flavour::Trust), 1000, 1000)), vec![AnswerKey::Enter]);
}

#[test]
fn a_highlighted_safe_option_is_answered_with_one_enter_and_the_answer_is_recorded_once_the_question_is_gone() {
    let mut a = answerer();
    assert_eq!(press(look(&mut a, AGY, Some(Flavour::Trust), 1200, 1200)), vec![AnswerKey::Enter]);
    // the question is still drawn a moment: no second key, one answer per occurrence
    assert!(matches!(look(&mut a, AGY, Some(Flavour::Trust), 1700, 1700), Step::Waiting(_)));
    assert!(matches!(look(&mut a, AGY, Some(Flavour::Trust), 2700, 2700), Step::Waiting(_)));
    // it goes away
    let Step::Answered(done) = look(&mut a, "agy 1.4.0\n\n| > Ask anything", None, 0, 3000) else { panic!("answered") };
    assert_eq!((done.label.as_str(), done.keys.as_str(), done.flavour.as_str()), ("Yes, I trust this folder", "Enter", "trust"));
    assert!(done.screen.contains("Do you trust the contents of this project?"));
    // and nothing more happens
    assert_eq!(look(&mut a, "agy 1.4.0\n\n| > Ask anything", None, 0, 3500), Step::Nothing);
}

#[test]
fn a_safe_option_on_the_second_row_is_reached_with_down_and_the_highlight_is_checked_before_enter() {
    let mut a = answerer();
    assert_eq!(press(look(&mut a, SECOND_ROW, Some(Flavour::Trust), 1500, 1500)), vec![AnswerKey::Down]);
    // too soon for another key
    assert!(matches!(look(&mut a, SECOND_ROW, Some(Flavour::Trust), 1600, 1600), Step::Waiting(_)));
    // the highlight moved to the intended row: now Enter
    assert_eq!(press(look(&mut a, SECOND_ROW_MOVED, Some(Flavour::Trust), 1200, 2200)), vec![AnswerKey::Enter]);
    let Step::Answered(done) = look(&mut a, "ready", None, 0, 3000) else { panic!("answered") };
    assert_eq!(done.keys, "Down, Enter");
}

#[test]
fn a_highlight_that_does_not_move_ends_in_a_no_not_in_enter() {
    let mut a = answerer();
    assert_eq!(press(look(&mut a, SECOND_ROW, Some(Flavour::Trust), 1500, 1500)), vec![AnswerKey::Down]);
    let mut got = Vec::new();
    for t in (2000..=4000).step_by(250) {
        got.push(look(&mut a, SECOND_ROW, Some(Flavour::Trust), 2500, t));
    }
    assert!(got.iter().all(|s| !matches!(s, Step::Press { .. })), "no key without confirming the highlight: {got:?}");
    assert!(got.iter().any(|s| matches!(s, Step::Declined(r) if r.contains("did not move"))), "{got:?}");
}

#[test]
fn a_numbered_menu_gets_the_number_then_enter_only_if_the_cursor_is_on_that_row() {
    let mut a = answerer();
    assert_eq!(press(look(&mut a, NUMBERED, Some(Flavour::Trust), 1500, 1500)), vec![AnswerKey::Text("1".into())]);
    // the number only moved the cursor: Enter confirms
    assert_eq!(press(look(&mut a, NUMBERED_CURSOR, Some(Flavour::Trust), 1200, 2200)), vec![AnswerKey::Enter]);
    // the other kind of agent acts on the number alone and the question goes
    let mut b = answerer();
    assert_eq!(press(look(&mut b, NUMBERED, Some(Flavour::Trust), 1500, 1500)), vec![AnswerKey::Text("1".into())]);
    let Step::Answered(done) = look(&mut b, "ready", None, 0, 2300) else { panic!("answered") };
    assert_eq!(done.keys, "1");
    // neither: it waits, then gives the question back to the person
    let mut c = answerer();
    press(look(&mut c, NUMBERED, Some(Flavour::Trust), 1500, 1500));
    assert!(matches!(look(&mut c, NUMBERED, Some(Flavour::Trust), 4000, 3000), Step::Waiting(_)));
    assert!(matches!(look(&mut c, NUMBERED, Some(Flavour::Trust), 6000, 6600), Step::Declined(_)));
}

#[test]
fn a_yes_no_question_gets_y_and_enter_only_if_it_is_still_there() {
    let mut a = answerer();
    assert_eq!(press(look(&mut a, YES_NO, Some(Flavour::Permission), 1500, 1500)), vec![AnswerKey::Text("y".into())]);
    assert_eq!(press(look(&mut a, YES_NO, Some(Flavour::Permission), 2600, 2100)), vec![AnswerKey::Enter]);
    assert!(matches!(look(&mut a, YES_NO, Some(Flavour::Permission), 3000, 2700), Step::Waiting(_)), "never a third key");
    // a prompt that took the letter alone
    let mut b = answerer();
    press(look(&mut b, YES_NO, Some(Flavour::Permission), 1500, 1500));
    let Step::Answered(done) = look(&mut b, "done", None, 0, 1800) else { panic!("answered") };
    assert_eq!((done.keys.as_str(), done.flavour.as_str(), done.label.as_str()), ("y", "permission", "Yes"));
}

#[test]
fn a_question_still_there_five_seconds_after_the_answer_goes_back_to_the_person() {
    let mut a = answerer();
    press(look(&mut a, AGY, Some(Flavour::Trust), 1500, 1500));
    let mut last = Step::Nothing;
    for t in (2000..=7000).step_by(500) {
        last = look(&mut a, AGY, Some(Flavour::Trust), 5000, t);
    }
    assert!(matches!(&last, Step::Declined(r) if r.contains("5 seconds")), "{last:?}");
    // and it stays declined: no key, ever, for this question
    assert!(matches!(look(&mut a, AGY, Some(Flavour::Trust), 9000, 9000), Step::Declined(_)));
}

#[test]
fn an_always_allow_option_is_never_chosen_and_the_person_is_asked() {
    for screen in [ALWAYS_ONLY, SESSION_AND_ONCE] {
        let mut a = answerer();
        let step = look(&mut a, screen, Some(Flavour::Permission), 3000, 3000);
        if screen == SESSION_AND_ONCE {
            // a plain "Yes" is there, so that row (and only that row) is the safe one
            assert_eq!(press(step), vec![AnswerKey::Up], "{screen}");
        } else {
            assert!(matches!(step, Step::Declined(_)), "{step:?}");
        }
    }
    let menu = parse_menu(ALWAYS_ONLY).unwrap();
    assert!(safe_pick(&menu).is_err());
}

#[test]
fn two_candidates_or_no_highlight_or_no_menu_is_a_no() {
    let two = "Allow?\n\n> Yes\n  Allow once\n  No\n";
    assert!(matches!(look(&mut answerer(), two, Some(Flavour::Permission), 3000, 3000), Step::Declined(r) if r.contains("more than one")));
    let no_cursor = "Allow?\n\n  Yes\n  No\n";
    assert!(matches!(look(&mut answerer(), no_cursor, Some(Flavour::Permission), 3000, 3000), Step::Declined(_)));
    let none = "Allow nimbus to do something? Think it over.\n";
    assert!(matches!(look(&mut answerer(), none, Some(Flavour::Permission), 3000, 3000), Step::Declined(_)));
}

#[test]
fn sign_in_payment_model_and_telemetry_questions_are_never_answered() {
    for text in [
        "Sign in to continue\n\n> Yes, open the browser\n  No\n",
        "Pick the model to use?\n\n> Yes, use the default model\n  No\n",
        "Share anonymous usage data to improve the product?\n\n> Yes\n  No\n",
        "Upgrade to Pro for $20 billing?\n\n> Yes\n  No\n",
        "Paste your API key to trust this folder?\n\n> Yes\n  No\n",
    ] {
        assert!(off_limits(text).is_some(), "{text}");
        let step = look(&mut answerer(), text, Some(Flavour::Trust), 3000, 3000);
        assert!(matches!(step, Step::Declined(_)), "{text}: {step:?}");
    }
    // and by flavour, whatever the words
    for flavour in [Flavour::Login, Flavour::Question] {
        assert!(matches!(look(&mut answerer(), AGY, Some(flavour), 3000, 3000), Step::Declined(_)), "{flavour:?}");
    }
}

#[test]
fn a_bulleted_summary_row_and_the_echo_of_a_task_above_the_question_are_not_an_arrow_menu() {
    // "▸ Thought for 3s" and "> List the files" start at the same column and both begin with a marker, one blank row apart
    let screen = "> Reply with exactly the word: x\nreply: x\n\u{25b8} Thought for 2s, 298 tokens\n\n> List the files in this folder, then reply with exactly: y\n\nreply: y\n\u{25b8} Thought for 3s, 298 tokens\n\nAllow nimbus to create hello.txt? [y/n]\n";
    let menu = parse_menu(screen).expect("the yes/no question");
    assert_eq!(menu.kind, MenuKind::YesNo, "{menu:?}");
    assert_eq!(safe_pick(&menu), Ok(0));
    // rows that are really adjacent still make a menu, whatever stands above them
    let real = format!("{screen}\nDo you trust the files in this folder?\n\n> Yes, I trust this folder\n  No, exit\n");
    let menu = parse_menu(&real).expect("the arrow menu");
    assert_eq!(menu.kind, MenuKind::Arrow);
    assert_eq!(menu.options.len(), 2, "{menu:?}");
}

#[test]
fn a_count_of_tokens_in_a_summary_row_above_the_question_is_not_a_question_about_a_token() {
    // the row an agent prints after a turn stays in the rows above the next question
    let after_a_turn = format!("reply: the sky is blue.\n\u{25b8} Thought for 3s, 298 tokens\n\n{YES_NO}");
    assert_eq!(off_limits(&after_a_turn), None, "{after_a_turn}");
    for row in ["1.2k tokens used", "12,345 input tokens", "used 3 output tokens so far", "5 tokens"] {
        assert_eq!(off_limits(&format!("{row}\n{YES_NO}")), None, "{row}");
    }
    let step = look(&mut answerer(), &after_a_turn, Some(Flavour::Permission), 3000, 3000);
    assert!(matches!(step, Step::Press { .. }), "{step:?}");
    // a question about a token is still the person's
    for text in ["Paste your token to continue?\n\n> Yes\n  No\n", "Use the saved API token? [y/n]\n", "Your tokens expired. Sign in again?\n> Yes\n  No\n"] {
        assert!(off_limits(text).is_some(), "{text}");
    }
}

#[test]
fn the_drafts_own_allow_once_keys_are_used_when_the_menu_agrees_and_never_for_an_always_menu() {
    let hint = Hint { keys: vec![AnswerKey::Enter], label: "Allow once".into() };
    let mut a = answerer();
    let step = a.step(&Input { screen: AGY, styled: None, flavour: Some(Flavour::Trust), stable_ms: 1500, now: 1500, hint: Some(&hint) });
    assert_eq!(press(step), vec![AnswerKey::Enter]);
    let mut b = answerer();
    let step = b.step(&Input { screen: ALWAYS_ONLY, styled: None, flavour: Some(Flavour::Permission), stable_ms: 1500, now: 1500, hint: Some(&hint) });
    assert!(matches!(step, Step::Declined(_)), "{step:?}");
    // a one-line question with no menu: the hint alone decides
    let mut c = answerer();
    let hint_y = Hint { keys: vec![AnswerKey::Text("y".into())], label: "Allow once".into() };
    let step = c.step(&Input { screen: YES_NO, styled: None, flavour: Some(Flavour::Permission), stable_ms: 1500, now: 1500, hint: Some(&hint_y) });
    assert_eq!(press(step), vec![AnswerKey::Text("y".into())]);
    assert_eq!(AnswerKey::from_name("enter"), Some(AnswerKey::Enter));
    assert_eq!(AnswerKey::from_name("y"), Some(AnswerKey::Text("y".into())));
    assert_eq!(AnswerKey::from_name("escape"), None);
}

#[test]
fn the_cursor_moving_is_still_the_same_question() {
    let mut a = answerer();
    press(look(&mut a, SECOND_ROW, Some(Flavour::Trust), 1500, 1500));
    // moving the highlight must not be taken for "a different question replaced it"
    assert!(matches!(look(&mut a, SECOND_ROW_MOVED, Some(Flavour::Trust), 200, 1700), Step::Waiting(_)));
}

#[test]
fn the_wizard_answers_only_inside_its_own_isolated_folder() {
    let temp = Path::new(r"C:\Users\me\AppData\Local\Temp");
    let session = Path::new(r"C:\Users\me\AppData\Local\Temp\pm-wizard-wz_ab12");
    let project = Path::new(r"C:\Users\me\AppData\Local\Temp\pm-wizard-wz_ab12\test-project");
    assert!(scope_ok(Some(project), session, temp).is_ok());
    assert!(scope_ok(Some(Path::new(r"c:\users\ME\appdata\local\temp\PM-WIZARD-WZ_AB12\capture-project")), session, temp).is_ok(), "case does not matter on Windows");
    assert!(scope_ok(Some(Path::new(r"\\?\C:\Users\me\AppData\Local\Temp\pm-wizard-wz_ab12\x")), session, temp).is_ok());
    // the person's project, another session, no folder known, a session outside the wizard's root
    assert!(scope_ok(Some(Path::new(r"C:\Users\me\work\project")), session, temp).is_err());
    assert!(scope_ok(Some(Path::new(r"C:\Users\me\AppData\Local\Temp\pm-wizard-wz_other")), session, temp).is_err());
    assert!(scope_ok(Some(Path::new(r"C:\Users\me\AppData\Local\Temp")), session, temp).is_err());
    assert!(scope_ok(None, session, temp).is_err());
    assert!(scope_ok(Some(project), Path::new(r"C:\Users\me\work"), temp).is_err());
    assert!(under(project, session) && !under(session, project));
}

#[test]
fn a_folder_that_climbs_out_with_dot_dot_is_never_inside() {
    let temp = Path::new(r"C:\Users\me\AppData\Local\Temp");
    let session = Path::new(r"C:\Users\me\AppData\Local\Temp\pm-wizard-wz_ab12");
    let escape = Path::new(r"C:\Users\me\AppData\Local\Temp\pm-wizard-wz_ab12\..\..\..\..\work\project");
    assert!(!under(escape, session), "`..` must not be dropped: this is the person's project");
    assert!(scope_ok(Some(escape), session, temp).is_err());
    let climbing_session = Path::new(r"C:\Users\me\AppData\Local\Temp\..\..\work");
    assert!(scope_ok(Some(Path::new(r"C:\Users\me\work\p")), climbing_session, temp).is_err());
}
