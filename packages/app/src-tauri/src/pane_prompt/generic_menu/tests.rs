use super::*;

const WAITING: Option<&str> = Some("waiting_input");

fn arrow(cursor: usize) -> String {
    let rows = ["Allow once", "Always allow in this project", "Deny"];
    let menu: Vec<String> = rows.iter().enumerate().map(|(i, r)| format!("{} {r}", if i == cursor { "\u{276f}" } else { " " })).collect();
    format!("zeta 1.0\n\nRun this command?\n      rm -rf build\n\n{}\n\n up/down to move, enter to confirm\n", menu.join("\n"))
}

const NUMBERED: &str = "Do you want to write notes.txt?\n\n  1. Yes\n  2. Yes, and don't ask again for this session\n  3. No\n";
const NUMBERED_CURSOR: &str = "Do you want to write notes.txt?\n\n> 1. Yes\n  2. Yes, and don't ask again for this session\n  3. No\n";
const YES_NO: &str = "zeta\n\nAllow zeta to run `python --version`? [y/n]\n";

fn card(screen: &str) -> Value {
    unknown_question("zeta", screen, WAITING).unwrap_or_else(|| panic!("no card for:\n{screen}"))
}

fn id_of(card: &Value) -> String {
    let mut c = card.clone();
    super::super::prompt_id_for(&c).map(|id| c["prompt_id"] = Value::String(id));
    c["prompt_id"].as_str().unwrap_or_default().to_string()
}

#[test]
fn an_arrow_menu_becomes_a_card_with_the_options_as_shown() {
    let card = card(&arrow(0));
    assert_eq!(card["kind"], "unknown_question");
    assert_eq!(card["recognised_by"], "shape");
    assert_eq!(card["choices"], json!(["option_1", "option_2", "option_3"]));
    assert_eq!(card["labels"]["option_2"], "Always allow in this project");
    assert_eq!(card["options"][0]["highlighted"], true);
    assert_eq!(card["options"][1]["broad"], true, "always allow is a broad approval");
    let summary = card["summary"].as_str().unwrap();
    assert!(summary.contains("recognised by shape, not by profile") && summary.contains("Run this command?") || summary.contains("rm -rf build"), "{summary}");
}

#[test]
fn a_working_pane_never_gets_a_card() {
    for status in [None, Some("running"), Some("error")] {
        assert!(unknown_question("zeta", &arrow(0), status).is_none(), "{status:?}");
    }
}

/// The shared heuristics do not know every approval menu (no `[y/n]`, no `1. Yes`): such a pane never reads
/// `waiting_input`, it settles to `idle`. A settled pane gets a card only for what is plainly an approval.
#[test]
fn a_settled_pane_gets_a_card_only_for_a_plain_approval_menu() {
    let file = "zeta 1.0\n\nWrite hello.txt?\n\n \u{276f} Yes, write it\n   Yes, and allow all edits this session\n   No, keep it as it is\n\n up/down to move, enter to confirm\n";
    let card = unknown_question("zeta", file, Some("idle")).expect("an approval menu on a settled pane");
    assert_eq!(card["choices"], json!(["option_1", "option_2", "option_3"]));
    assert_eq!(reply(&card, "deny", false).unwrap().key.as_deref(), Some("\u{1b}[B\u{1b}[B"));
    assert!(unknown_question("zeta", &arrow(0), Some("idle")).is_some());
    // a quoted block in a finished reply has a marker and aligned rows, but asks nothing and offers no yes / no
    assert!(unknown_question("zeta", "zeta 1.0\n\nThe notes say:\n> first thing\n> second thing\n", Some("idle")).is_none());
    // an approval menu that was answered a while ago and is still in the transcript, with the agent's input box under it
    let answered = format!("{file}\nallowed: hello.txt\n\n+------------------------+\n| > Type a task...       |\n+------------------------+\n");
    assert!(unknown_question("zeta", &answered, Some("idle")).is_none());
    // a menu that asks but is not an approval (two projects to pick from)
    assert!(unknown_question("zeta", "zeta 1.0\n\nWhich project?\n\n> alpha\n  beta\n", Some("idle")).is_none());
    // the same menu on a pane that reads waiting is offered to the caller as it is
    assert!(unknown_question("zeta", "zeta 1.0\n\nWhich project?\n\n> alpha\n  beta\n", WAITING).is_some());
}

#[test]
fn the_id_survives_the_cursor_moving_and_a_spinner_under_the_menu_but_not_another_question() {
    let first = id_of(&card(&arrow(0)));
    assert_eq!(first, id_of(&card(&arrow(1))), "the cursor moved");
    let with_spinner = format!("{}\n\u{280b} waiting for you... 12s\n", arrow(0));
    assert_eq!(first, id_of(&card(&with_spinner)), "a spinner redraws under the menu");
    let other = arrow(0).replace("rm -rf build", "rm -rf dist").replace("Run this command?", "Run this other command?");
    assert_ne!(first, id_of(&card(&other)), "another question is another id");
}

#[test]
fn the_id_is_the_same_while_the_menu_is_still_being_drawn() {
    // wait_agents read the pane with two of the three rows drawn; answer_prompt re-reads the finished menu.
    let head = "NIMBUS 0.4.2\n\n> ask me with a menu\n\nRun this command?\n  echo hi\n\n";
    let full = format!("{head}> 1. Allow once\n  2. Always allow in this project\n  3. Deny\n");
    let partial = format!("{head}> 1. Allow once\n  2. Always allow in this project\n");
    let (a, b) = (card(&partial), card(&full));
    assert_eq!(a["options"].as_array().unwrap().len(), 2);
    assert_eq!(b["options"].as_array().unwrap().len(), 3);
    assert_eq!(id_of(&a), id_of(&b), "the same question has one id however much of the menu was drawn");
}

#[test]
fn a_cursor_menu_is_answered_by_moving_the_cursor_and_the_route_adds_enter() {
    let c = card(&arrow(0));
    assert_eq!(reply(&c, "option_1", false).unwrap().key.as_deref(), Some(""));
    assert_eq!(reply(&c, "Deny", false).unwrap().key.as_deref(), Some("\u{1b}[B\u{1b}[B"));
    let on_deny = card(&arrow(2));
    assert_eq!(reply(&on_deny, "allow once", false).unwrap().key.as_deref(), Some("\u{1b}[A\u{1b}[A"));
}

#[test]
fn the_words_every_card_uses_pick_the_one_plain_option() {
    let c = card(&arrow(0));
    assert_eq!(reply(&c, "allow_once", false).unwrap().key.as_deref(), Some(""));
    assert_eq!(reply(&c, "deny", false).unwrap().key.as_deref(), Some("\u{1b}[B\u{1b}[B"));
    assert_eq!(reply(&c, "3", false).unwrap().key.as_deref(), Some("\u{1b}[B\u{1b}[B"));
    let err = reply(&c, "maybe", false).unwrap_err();
    assert!(err.contains("option_1") && err.contains("Allow once"), "{err}");
}

#[test]
fn a_broad_option_needs_the_explicit_opt_in() {
    let c = card(&arrow(0));
    let err = reply(&c, "option_2", false).unwrap_err();
    assert!(err.contains("allow_broad"), "{err}");
    assert_eq!(reply(&c, "option_2", true).unwrap().key.as_deref(), Some("\u{1b}[B"));
}

#[test]
fn a_numbered_menu_without_a_cursor_is_answered_with_the_number() {
    let c = card(NUMBERED);
    assert_eq!(c["menu"], "numbered");
    assert_eq!(reply(&c, "option_3", false).unwrap().key.as_deref(), Some("3"));
    assert_eq!(reply(&c, "no", false).unwrap().key.as_deref(), Some("3"));
    assert!(reply(&c, "option_2", false).is_err(), "'don't ask again for this session' is broad");
    // with a cursor the cursor is moved instead
    let with_cursor = card(NUMBERED_CURSOR);
    assert_eq!(reply(&with_cursor, "option_3", false).unwrap().key.as_deref(), Some("\u{1b}[B\u{1b}[B"));
}

#[test]
fn a_yes_no_line_is_y_or_n() {
    let c = card(YES_NO);
    assert_eq!(c["menu"], "yes_no");
    assert_eq!(reply(&c, "option_1", false).unwrap().key.as_deref(), Some("y"));
    assert_eq!(reply(&c, "deny", false).unwrap().key.as_deref(), Some("n"));
}

#[test]
fn things_that_only_look_like_a_menu_are_not_cards() {
    // a list in the agent's own words: no cursor, nothing asked
    assert!(unknown_question("zeta", "Here is the plan:\n  1. read the code\n  2. write the fix\n  3. run the tests\n", WAITING).is_none());
    // a sign-in question is the person's
    assert!(unknown_question("zeta", "Sign in to continue?\n\n> 1. Yes\n  2. No\n", WAITING).is_none());
    assert!(unknown_question("zeta", "", WAITING).is_none());
}

/// The echo of the user's task starts with the same marker as the menu's cursor row; it is not the menu.
#[test]
fn an_earlier_row_that_starts_with_a_marker_is_not_taken_for_the_menu() {
    let screen = "zeta 1.0\n\n> Check the version and write hello.txt\n\nreply: ran it\n\nWrite hello.txt?\n\n \u{276f} Yes, write it\n   Yes, and allow all edits this session\n   No, keep it as it is\n\n up/down to move, enter to confirm\n";
    let card = unknown_question("zeta", screen, Some("idle")).expect("the menu under the echo");
    let labels: Vec<&str> = card["options"].as_array().unwrap().iter().filter_map(|o| o["label"].as_str()).collect();
    assert_eq!(labels, ["Yes, write it", "Yes, and allow all edits this session", "No, keep it as it is"]);
}
