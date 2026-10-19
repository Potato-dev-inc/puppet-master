//! The safe answerer on questions whose highlighted option is DRAWN (inverse video, a band, bold) rather than marked, and on
//! the other shapes a TUI uses (SPEC 11.20): boxed menus, one row of buttons, `[Y]es / [N]o`, `y/n/a`, numbers without dots.
//! Scripted screens: the attributes come from raw terminal bytes through the same emulator the panes use.

use crate::tui::declarative::wizard::visible::answer::{
    is_unreadable, parse_menu_ex, safe_pick, AnswerCfg, AnswerKey, Answerer, Input, MenuKind, Step,
};
use crate::tui::declarative::wizard::visible::screen::Flavour;
use crate::tui::declarative::wizard::visible::styled::StyledScreen;

const INV: &str = "\x1b[7m";
const OFF: &str = "\x1b[0m";

fn screen(ansi: &str) -> (String, StyledScreen) {
    let styled = StyledScreen::from_ansi(30, 100, ansi);
    (styled.text(), styled)
}

/// The command question with the cursor on `at` (0 = Allow once), drawn in inverse video with no marker.
fn inverse_menu(at: usize) -> (String, StyledScreen) {
    let labels = ["Allow once", "Always allow in this project", "Deny"];
    let mut ansi = String::from("Allow nimbus to run `ls`?\n\n");
    for (i, l) in labels.iter().enumerate() {
        if i == at {
            ansi.push_str(&format!("  {INV} {l:<34} {OFF}\n"));
        } else {
            ansi.push_str(&format!("   {l}\n"));
        }
    }
    screen(&ansi)
}

fn buttons(at: usize) -> (String, StyledScreen) {
    let labels = ["Allow once", "Always allow", "Deny"];
    let row: Vec<String> = labels.iter().enumerate().map(|(i, l)| if i == at { format!("{INV} {l} {OFF}") } else { format!(" {l} ") }).collect();
    screen(&format!("Allow nimbus to run `ls`?\n\n  {}\n", row.join("  ")))
}

fn drive(a: &mut Answerer, scr: &(String, StyledScreen), now: u64) -> Step {
    a.step(&Input { screen: &scr.0, styled: Some(&scr.1), flavour: Some(Flavour::Permission), stable_ms: 2000, now, hint: None })
}

fn gone(a: &mut Answerer) -> Step {
    a.step(&Input { screen: "> ", styled: None, flavour: None, stable_ms: 2000, now: 0, hint: None })
}

fn keys(step: Step) -> Vec<AnswerKey> {
    match step {
        Step::Press { keys, .. } => keys,
        other => panic!("expected keys, got {other:?}"),
    }
}

#[test]
fn an_inverse_video_row_without_a_marker_is_the_highlighted_option() {
    let (text, styled) = inverse_menu(2);
    let menu = parse_menu_ex(&text, Some(&styled)).expect("a menu");
    assert_eq!(menu.kind, MenuKind::Arrow);
    assert_eq!(menu.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Allow once", "Always allow in this project", "Deny"]);
    assert_eq!((menu.highlighted(), safe_pick(&menu)), (Some(2), Ok(0)), "{menu:?}");
    // the text alone shows no cursor at all
    assert!(parse_menu_ex(&text, None).and_then(|m| m.highlighted()).is_none());
}

#[test]
fn the_answerer_walks_an_inverse_menu_and_presses_enter_only_on_the_safe_row() {
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert_eq!(keys(drive(&mut a, &inverse_menu(2), 1000)), vec![AnswerKey::Up]);
    assert!(matches!(drive(&mut a, &inverse_menu(2), 1200), Step::Waiting(_)), "the key gap");
    assert_eq!(keys(drive(&mut a, &inverse_menu(1), 1600)), vec![AnswerKey::Up]);
    assert_eq!(keys(drive(&mut a, &inverse_menu(0), 2200)), vec![AnswerKey::Enter]);
    match gone(&mut a) {
        Step::Answered(done) => assert_eq!((done.label.as_str(), done.keys.as_str(), done.via.as_str()), ("Allow once", "Up, Up, Enter", "rules")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_highlight_that_never_moves_ends_in_a_no_and_enter_is_never_pressed() {
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert_eq!(keys(drive(&mut a, &inverse_menu(2), 1000)), vec![AnswerKey::Up]);
    assert!(matches!(drive(&mut a, &inverse_menu(2), 2000), Step::Waiting(_)));
    match drive(&mut a, &inverse_menu(2), 3000) {
        Step::Declined(why) => assert!(why.contains("did not move"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_menu_in_a_box_loses_its_border_and_keeps_its_marker() {
    let (text, styled) = screen("\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n\u{2502} Run it?      \u{2502}\n\u{2502}   Allow once \u{2502}\n\u{2502} \u{276f} Deny       \u{2502}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}\n");
    let menu = parse_menu_ex(&text, Some(&styled)).expect("a menu");
    assert_eq!(menu.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Allow once", "Deny"], "{menu:?}");
    assert_eq!((menu.kind.clone(), menu.highlighted()), (MenuKind::Arrow, Some(1)));
}

#[test]
fn bold_alone_marks_the_highlighted_row_when_the_others_are_plain() {
    let (text, styled) = screen("Allow it?\n\n  Yes, allow once\n  \x1b[1;36mNo\x1b[0m\n");
    let menu = parse_menu_ex(&text, Some(&styled)).expect("a menu");
    assert_eq!(menu.highlighted(), Some(1));
    // a bold TITLE glued to the options is not a highlighted option
    let (text, styled) = screen("\x1b[1mRun this command?\x1b[0m\n  Allow once\n  Deny\n");
    assert_eq!(parse_menu_ex(&text, Some(&styled)).and_then(|m| m.highlighted()), None);
}

#[test]
fn a_row_of_buttons_is_one_menu_and_its_inverse_button_is_the_highlighted_one() {
    let (text, styled) = buttons(2);
    let menu = parse_menu_ex(&text, Some(&styled)).expect("a row");
    assert_eq!(menu.kind, MenuKind::Row);
    assert_eq!(menu.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Allow once", "Always allow", "Deny"]);
    assert_eq!((menu.highlighted(), safe_pick(&menu)), (Some(2), Ok(0)));
}

#[test]
fn a_row_of_buttons_is_walked_with_the_arrows_and_then_with_tab_when_they_do_nothing() {
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert_eq!(keys(drive(&mut a, &buttons(2), 1000)), vec![AnswerKey::Left]);
    assert!(matches!(drive(&mut a, &buttons(2), 2000), Step::Waiting(_)), "waiting for the highlight to move");
    // nothing moved after 1.5 s: Tab is the one other key tried
    assert_eq!(keys(drive(&mut a, &buttons(2), 2700)), vec![AnswerKey::Tab]);
    assert_eq!(keys(drive(&mut a, &buttons(0), 3300)), vec![AnswerKey::Enter]);
    match gone(&mut a) {
        Step::Answered(done) => assert_eq!(done.keys, "Left, Tab, Enter"),
        other => panic!("{other:?}"),
    }
    // a row where neither works ends in a no
    let mut b = Answerer::new("capture", AnswerCfg::default());
    keys(drive(&mut b, &buttons(2), 1000));
    keys(drive(&mut b, &buttons(2), 2700));
    assert!(matches!(drive(&mut b, &buttons(2), 4400), Step::Declined(_)));
}

#[test]
fn bracketed_letters_are_hotkeys_and_the_letter_is_pressed() {
    let (text, styled) = screen("Allow nimbus to run `ls`? [Y]es / [N]o\n");
    let menu = parse_menu_ex(&text, Some(&styled)).expect("a row");
    assert_eq!(menu.kind, MenuKind::Row);
    assert_eq!(menu.options.iter().map(|o| (o.text.as_str(), o.hotkey)).collect::<Vec<_>>(), [("Yes", Some('y')), ("No", Some('n'))]);
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert_eq!(keys(drive(&mut a, &(text.clone(), styled.clone()), 1000)), vec![AnswerKey::Text("y".into())]);
    // the letter did not close it and the highlight cannot be read: Enter is NOT pressed on a guess
    assert!(matches!(drive(&mut a, &(text, styled), 1700), Step::Waiting(_)));
}

#[test]
fn yes_no_letters_in_any_bracket_style_are_read() {
    for q in ["Allow it? [y/n/a]", "Allow it? (y/n)", "Allow it? [Y/n]", "Allow it? y/n", "Allow it? (yes/no)"] {
        let menu = parse_menu_ex(q, None).unwrap_or_else(|| panic!("{q}"));
        assert_eq!(menu.kind, MenuKind::YesNo, "{q}");
    }
}

#[test]
fn numbered_lists_without_dots_are_numbered_menus() {
    for text in ["Run it?\n 1 Allow once\n 2 Always allow\n 3 Deny\n", "Run it?\n [1] Allow once\n [2] Always allow\n [3] Deny\n", "Run it?\n 1) Allow once\n 2) Always allow\n 3) Deny\n", "Run it?\n (1) Allow once\n (2) Always allow\n (3) Deny\n", "Run it?\n 1: Allow once\n 2: Always allow\n 3: Deny\n"] {
        let menu = parse_menu_ex(text, None).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(menu.kind, MenuKind::Numbered, "{text}");
        assert_eq!(menu.options.iter().map(|o| o.number).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
        assert_eq!(safe_pick(&menu), Ok(0));
    }
}

#[test]
fn radio_rows_are_read_as_a_menu_with_the_chosen_row_highlighted() {
    let menu = parse_menu_ex("Run it?\n ( ) Allow once\n (\u{2022}) Deny\n", None).expect("a menu");
    assert_eq!((menu.kind.clone(), menu.highlighted()), (MenuKind::Arrow, Some(1)), "{menu:?}");
}

#[test]
fn the_only_yes_being_always_allow_is_never_pressed_and_is_not_offered_to_an_ai_look() {
    let (text, styled) = screen(&format!("Allow nimbus to run `ls`?\n\n  {INV} Always allow in this project {OFF}\n   Deny\n"));
    let mut a = Answerer::new("capture", AnswerCfg::default());
    match drive(&mut a, &(text.clone(), styled), 1000) {
        Step::Declined(why) => {
            assert!(why.contains("none of the options plainly means yes"), "{why}");
            assert!(!is_unreadable(&why), "a menu that was read and has no plain yes is nobody's to help with");
        }
        other => panic!("{other:?}"),
    }
    assert!(a.offer_ai(&text, "", "Always allow in this project", &["Enter".to_string()]).is_err());
}

// ---- the AI look (SPEC 11.20) ------------------------------------------------------------------------

const BLIND: &str = "Allow nimbus to run `ls`?\n\n   Allow once\n   Always allow in this project\n   Deny\n";

fn blind_step(a: &mut Answerer, now: u64) -> Step {
    a.step(&Input { screen: BLIND, styled: None, flavour: Some(Flavour::Permission), stable_ms: 2000, now, hint: None })
}

fn keys_of(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_menu_nobody_can_read_is_declined_as_unreadable_then_answered_with_the_checked_ai_choice() {
    let mut a = Answerer::new("capture", AnswerCfg::default());
    match blind_step(&mut a, 1000) {
        Step::Declined(why) => assert!(is_unreadable(&why), "{why}"),
        other => panic!("{other:?}"),
    }
    let fp = a.question_fp().unwrap().to_string();
    assert_eq!(a.offer_ai(BLIND, &fp, "Allow once", &keys_of(&["Enter"])), Ok("Allow once".to_string()));
    assert_eq!(keys(blind_step(&mut a, 1100)), vec![AnswerKey::Enter]);
    match gone(&mut a) {
        Step::Answered(done) => assert_eq!((done.label.as_str(), done.via.as_str(), done.keys.as_str()), ("Allow once", "ai_look", "Enter")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_ai_choice_is_refused_when_it_names_a_broad_option_a_missing_one_or_a_key_off_the_list() {
    for (option, list, why) in [
        ("Always allow in this project", vec!["Enter"], "never picks"),
        ("Deny", vec!["Enter"], "never picks"),
        ("Allow once forever", vec!["Enter"], "not on the screen"),
        ("Allow once", vec!["Escape"], "not one the wizard may press"),
        ("Allow once", vec!["Enter", "Enter"], "last key"),
        ("Allow once", vec!["Down"; 7], "at most"),
        ("Allow once", vec!["3"], "not the number"),
    ] {
        let mut a = Answerer::new("capture", AnswerCfg::default());
        blind_step(&mut a, 1000);
        let err = a.offer_ai(BLIND, "", option, &keys_of(&list)).unwrap_err();
        assert!(err.contains(why), "{option} {list:?}: {err}");
        // nothing was installed: the question is still declined and nothing is pressed
        assert!(matches!(blind_step(&mut a, 1200), Step::Declined(_)));
    }
}

#[test]
fn the_ai_choice_is_only_taken_for_a_question_the_answerer_could_not_read_and_only_before_any_key() {
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert!(a.offer_ai(BLIND, "", "Allow once", &keys_of(&["Enter"])).is_err(), "no question yet");
    // a readable menu is the rules' own
    let (text, styled) = inverse_menu(0);
    drive(&mut a, &(text.clone(), styled), 1000);
    assert!(a.offer_ai(&text, "", "Allow once", &keys_of(&["Enter"])).unwrap_err().contains("did not ask"));
    // a different question than the one looked at
    let mut b = Answerer::new("capture", AnswerCfg::default());
    blind_step(&mut b, 1000);
    assert!(b.offer_ai("Allow nimbus to create x?\n   Allow once\n   Deny\n", "", "Allow once", &keys_of(&["Enter"])).unwrap_err().contains("changed"));
}

#[test]
fn an_ai_answer_that_navigates_stops_when_a_key_changes_nothing_and_never_presses_enter() {
    let text = "Allow nimbus to run `ls`?\n\n   Allow once\n   Always allow in this project\n   Deny\n";
    let mut a = Answerer::new("capture", AnswerCfg::default());
    blind_step(&mut a, 1000);
    a.offer_ai(text, "", "Allow once", &keys_of(&["Down", "Enter"])).expect("valid keys");
    assert_eq!(keys(blind_step(&mut a, 1100)), vec![AnswerKey::Down]);
    assert!(matches!(blind_step(&mut a, 1300), Step::Waiting(_)));
    match blind_step(&mut a, 2800) {
        Step::Declined(why) => assert!(why.contains("changed nothing"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_ai_answer_that_ends_on_a_readable_highlight_elsewhere_is_stopped_before_enter() {
    // the question is unreadable at first (no menu), then the screen shows a highlight on Deny after the key
    let first = "Allow nimbus to run `ls`?\n   Allow once\n   Deny\n";
    let mut a = Answerer::new("capture", AnswerCfg::default());
    a.step(&Input { screen: first, styled: None, flavour: Some(Flavour::Permission), stable_ms: 2000, now: 1000, hint: None });
    // `first` reads as a menu without a highlight: unreadable
    a.offer_ai(first, "", "Allow once", &keys_of(&["Down", "Enter"])).expect("valid");
    assert_eq!(keys(a.step(&Input { screen: first, styled: None, flavour: Some(Flavour::Permission), stable_ms: 2000, now: 1100, hint: None })), vec![AnswerKey::Down]);
    let (text, styled) = screen("Allow nimbus to run `ls`?\n   Allow once\n  \x1b[7m Deny \x1b[0m\n");
    let moved = a.step(&Input { screen: &text, styled: Some(&styled), flavour: Some(Flavour::Permission), stable_ms: 2000, now: 1700, hint: None });
    match moved {
        Step::Declined(why) => assert!(why.contains("not on the named option"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_shape_is_a_permission_dialog_to_the_classifier() {
    use crate::tui::declarative::wizard::visible::screen::{classify, ScreenKind};
    for q in [
        "~ Thinking... (1s)\n\nAllow nimbus to run `ls`? [Y]es / [N]o",
        "~ Thinking... (1s)\n\nAllow nimbus to run `ls`?\n\n  Allow once   Always allow   Deny\n",
        "Allow nimbus to run `ls`?\n\n   Allow once\n   Always allow in this project\n   Deny\n\n  up/down to move, enter to confirm\n",
    ] {
        assert_eq!(classify(q), ScreenKind::Dialog(Flavour::Permission), "{q}");
    }
}
