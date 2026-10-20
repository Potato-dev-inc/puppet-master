//! A permission question under the transcript of earlier turns (found by the founder's run on a CLI that prints each tool
//! call as a bullet): the status bullets above the question ("Edited 1 file", "Explored 1 file") were read as the menu, so
//! the numbered block under "Run this command?" was never seen, the checked AI look's right answer was refused ("the
//! highlight is on Edited 1 file") and the wizard stopped.
//!
//! Generic rules under test (nothing here names an agent): a numbered block (long options included) is the menu; two or
//! more marked rows are not a single-selection menu; a block with a cursor marker beats a block of bullets; bullets higher
//! up on the screen are history; the highlight the AI look is held to comes from the block that holds the named option.

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::ai_menu::validate;
use crate::tui::declarative::wizard::visible::answer::{can_look, AnswerCfg, AnswerKey, Answerer, Input, Step};
use crate::tui::declarative::wizard::visible::capture::{Action, Capture, Config};
use crate::tui::declarative::wizard::visible::host::Look;
use crate::tui::declarative::wizard::visible::menu::{describe_menu, parse_menu, safe_pick, MenuKind};
use crate::tui::declarative::wizard::visible::screen::Flavour;

/// The bottom of the real pane at the stop (words masked as the debug file shows them): the transcript of two turns, the
/// prompt echo of the second, a rule, the question and its numbered options with long wording, a hint row and a footer.
fn after_tool_calls() -> String {
    let rule = |n: usize| "\u{2500}".repeat(n);
    [
        "> Create a file named hello.txt in the current folder. It must contain the word <word-3>.".to_string(),
        String::new(),
        "\u{25CB} Thought for 3.4s (Determining the correct file path to create `hello.txt` within the designated directory structu...)".to_string(),
        "\u{25CF} Edited 1 file (hello.txt)".to_string(),
        "\u{25CF} Explored 1 file (hello.txt) (ctrl+o to expand)".to_string(),
        String::new(),
        "  Created hello.txt containing <word-3>.".to_string(),
        String::new(),
        rule(60),
        "> Run this shell command in the current folder and tell me what it printed: echo <word-3>".to_string(),
        String::new(),
        "\u{25CF} Ran (echo <word-3>) (ctrl+o to expand)".to_string(),
        String::new(),
        "Command".to_string(),
        rule(120),
        String::new(),
        "Requesting permission for:".to_string(),
        "   echo <word-3>".to_string(),
        String::new(),
        "Run this command?".to_string(),
        "> 1. Yes, run command".to_string(),
        "  2. Yes, and always allow in this conversation for commands that start with 'echo <word-3>'".to_string(),
        "  3. Yes, and always allow for commands that start with 'echo <word-3>' (Persist to settings.json)".to_string(),
        "  4. No, cancel".to_string(),
        String::new(),
        "  \u{2191}/\u{2193} Navigate \u{00B7} tab Amend \u{00B7} ctrl+g edit/expand command".to_string(),
        format!("esc to cancel{}Gemini 3.8 Flash \u{00B7} hig", " ".repeat(81)),
        String::new(),
    ]
    .join("\n")
}

/// The same screen after the cursor moved to the second option.
fn cursor_on_second() -> String {
    after_tool_calls().replace("> 1. Yes, run command\n  2.", "  1. Yes, run command\n> 2.")
}

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";

// ---- the reader ------------------------------------------------------------------------------------------

#[test]
fn the_numbered_block_under_the_question_is_the_menu_not_the_bullets_above_it() {
    let screen = after_tool_calls();
    let menu = parse_menu(&screen).expect("a menu");
    assert_eq!(menu.kind, MenuKind::Numbered, "{menu:?}");
    assert_eq!(menu.options.len(), 4, "{menu:?}");
    assert_eq!(menu.highlighted(), Some(0));
    assert_eq!(menu.options[0].text, "Yes, run command");
    assert_eq!(menu.options[3].text, "No, cancel");
    assert_eq!(safe_pick(&menu), Ok(0), "only the one-time yes");
    let said = describe_menu(&menu);
    assert!(said.starts_with("numbered rows: \"1. Yes, run command\" (plain yes, highlighted); \"2. Yes, and always allow"), "{said}");
    assert!(said.contains("(broad)") && said.ends_with("\"4. No, cancel\" (no)"), "{said}");
    assert!(!said.contains("Edited 1 file"), "{said}");
}

#[test]
fn the_cursor_is_followed_to_another_row() {
    let menu = parse_menu(&cursor_on_second()).expect("a menu");
    assert_eq!((menu.kind.clone(), menu.highlighted()), (MenuKind::Numbered, Some(1)));
}

#[test]
fn two_marked_rows_are_a_list_of_bullets_not_a_menu() {
    let bullets = "\u{25CF} Edited 1 file (a.txt)\n\u{25CF} Explored 1 file (a.txt)\n\nCreated a.txt.\n";
    assert!(parse_menu(bullets).is_none());
    // a numbered block with two cursors is not one either
    assert!(parse_menu("Run it?\n> 1. Yes\n> 2. No\n").is_none());
}

#[test]
fn bullets_higher_up_the_screen_are_history_even_when_one_row_is_marked() {
    let screen = "\u{25CB} Thought for 2s\n\u{25CF} Ran 1 command (ls)\n\nHere are the files: a.txt and b.txt.\nAnything else?\nJust ask.\nI am ready.\n";
    assert!(parse_menu(screen).is_none(), "{:?}", parse_menu(screen));
}

#[test]
fn a_block_with_a_cursor_marker_beats_bullets_whatever_is_around_it() {
    let screen = "\u{25CB} Thought for 2s\n\u{25CF} Ran 1 command (ls)\n\nAllow it?\n\u{276F} Yes, allow once\n  No\n";
    let menu = parse_menu(screen).expect("a menu");
    assert_eq!(menu.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Yes, allow once", "No"]);
    assert_eq!(menu.highlighted(), Some(0));
}

#[test]
fn the_last_numbered_run_is_the_menu_when_an_earlier_list_was_printed_above_it() {
    let screen = "Steps I took:\n1. Looked at the folder\n2. Wrote the file\n\nRun this command?\n> 1. Yes, run command\n  2. No, cancel\n";
    let menu = parse_menu(screen).expect("a menu");
    assert_eq!(menu.options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Yes, run command", "No, cancel"]);
    assert_eq!(menu.highlighted(), Some(0));
}

#[test]
fn a_menu_drawn_with_a_bullet_at_the_bottom_is_still_read() {
    let menu = parse_menu("Let nimbus read and edit the files in this folder?\n\n\u{25CF} Yes\n  No\n").expect("a menu");
    assert_eq!((menu.kind.clone(), menu.highlighted()), (MenuKind::Arrow, Some(0)));
}

// ---- the checked AI look ---------------------------------------------------------------------------------

#[test]
fn the_ai_looks_one_time_yes_is_valid_on_the_real_screen_and_the_always_options_never_are() {
    let screen = after_tool_calls();
    let plan = validate(&screen, "1. Yes, run command", &["Enter".to_string()]).expect("a one-time yes");
    assert_eq!((plan.target.as_str(), plan.keys.clone()), ("Yes, run command", vec![AnswerKey::Enter]));
    assert!(validate(&screen, "Yes, run command", &["1".to_string()]).is_ok());
    for option in [
        "Yes, and always allow in this conversation for commands that start with 'echo <word-3>'",
        "Yes, and always allow for commands that start with 'echo <word-3>' (Persist to settings.json)",
        "No, cancel",
    ] {
        assert!(validate(&screen, option, &["Enter".to_string()]).is_err(), "{option}");
    }
    assert!(validate(&screen, "Edited 1 file (hello.txt)", &["Enter".to_string()]).is_err());
}

fn permission_input(screen: &str, now: u64) -> Input<'_> {
    Input { screen, styled: None, flavour: Some(Flavour::Permission), stable_ms: 2000, now, hint: None }
}

#[test]
fn the_safe_answerer_reads_the_menu_itself_on_the_real_screen_and_presses_the_yes() {
    let screen = after_tool_calls();
    let mut a = Answerer::new("capture", AnswerCfg::default());
    match a.step(&permission_input(&screen, 1000)) {
        Step::Press { keys, .. } => assert_eq!(keys, vec![AnswerKey::Text("1".into())]),
        other => panic!("{other:?}"),
    }
    assert!(a.menu_read().unwrap().starts_with("numbered rows: \"1. Yes, run command\" (plain yes, highlighted)"), "{:?}", a.menu_read());
    match a.step(&permission_input(&screen, 1700)) {
        Step::Press { keys, .. } => assert_eq!(keys, vec![AnswerKey::Enter], "the cursor is on the yes row"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_ai_look_is_held_to_the_highlight_of_the_block_that_holds_the_option_not_of_another_block() {
    // The only block the reader finds is a pair of status rows at the bottom (one marked) and it holds no plain yes, so the
    // AI look is asked. It names the yes row that sits above them. Its Enter is not refused for "the highlight is on
    // Edited a.txt": that block does not hold the named option, so its highlight says nothing about it.
    let screen = "Run this command?\n  Yes, run command\n\n\u{25CF} Edited a.txt\n  Explored b.txt\n";
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert!(matches!(a.step(&permission_input(screen, 1000)), Step::Declined(_)));
    assert!(a.declined().is_some_and(can_look), "{:?}", a.declined());
    assert!(a.menu_read().unwrap().contains("Edited a.txt"), "{:?}", a.menu_read());
    let fp = a.question_fp().unwrap().to_string();
    assert_eq!(a.offer_ai(screen, &fp, "Yes, run command", &["Enter".to_string()]), Ok("Yes, run command".to_string()));
    match a.step(&permission_input(screen, 1100)) {
        Step::Press { keys, .. } => assert_eq!(keys, vec![AnswerKey::Enter]),
        other => panic!("{other:?}"),
    }
}

// ---- Enter with no read menu to vouch for it: the cursor on the screen ---------------------------------------

/// Offer the AI look's answer (the named option, then Enter) for a question the rules declined, and step once more.
fn ai_enter(screen: &str, option: &str) -> Step {
    let mut a = Answerer::new("capture", AnswerCfg::default());
    let first = a.step(&permission_input(screen, 1000));
    assert!(matches!(first, Step::Declined(_)), "the rules must decline this screen first: {first:?}");
    assert!(a.declined().is_some_and(can_look), "{:?}", a.declined());
    let fp = a.question_fp().unwrap().to_string();
    a.offer_ai(screen, &fp, option, &["Enter".to_string()]).expect("valid on its own");
    a.step(&permission_input(screen, 1100))
}

#[test]
fn enter_on_the_real_screen_is_pressed_when_the_named_row_holds_the_cursor() {
    // the rules would answer this one themselves; the AI path is the same check, so offer it directly
    let screen = after_tool_calls();
    assert_eq!(super::super::visible::menu::cursor_elsewhere(&screen, None, &|t| t == "Yes, run command"), None);
    assert!(validate(&screen, "1. Yes, run command", &["Enter".to_string()]).is_ok());
}

#[test]
fn enter_is_declined_when_the_cursor_is_on_another_row_even_though_the_reader_returns_no_menu() {
    // the yes row and the cursor row are drawn at other indents, so no menu is read at all
    let screen = "Run this command?\n Yes, run command\n> Always run\n";
    assert!(parse_menu(screen).is_none(), "{:?}", parse_menu(screen));
    match ai_enter(screen, "Yes, run command") {
        Step::Declined(why) => assert!(why.contains("the cursor is on another row") && why.contains("Always run"), "{why}"),
        other => panic!("{other:?}"),
    }
    // the same screen with the cursor on the named row is pressed
    let right = "Run this command?\n > Yes, run command\n  Always run\n";
    assert!(matches!(ai_enter(right, "Yes, run command"), Step::Press { .. }));
}

#[test]
fn enter_is_declined_when_the_numbered_always_row_holds_the_cursor() {
    let screen = cursor_on_second();
    assert_eq!(
        super::super::visible::menu::cursor_elsewhere(&screen, None, &|t| t == "Yes, run command").as_deref(),
        Some("Yes, and always allow in this conversation for commands that start with 'echo <word-3>'")
    );
    // and the read menu agrees: the highlight is on the second option
    let mut a = Answerer::new("capture", AnswerCfg::default());
    assert!(matches!(a.step(&permission_input(&screen, 1000)), Step::Press { .. }), "the rules walk to option 1 by its number");
}

#[test]
fn a_cursor_in_another_block_or_on_a_prompt_echo_is_not_a_cursor_on_another_row() {
    // the real screen has "> Run this shell command ..." (a prompt echo) well above the options, separated by blank rows
    let screen = after_tool_calls();
    assert_eq!(super::super::visible::menu::cursor_elsewhere(&screen, None, &|t| t == "No, cancel"), Some("Yes, run command".to_string()));
    assert_eq!(super::super::visible::menu::cursor_elsewhere(&screen, None, &|t| t == "Yes, run command"), None);
    // no cursor anywhere in the block: nothing against Enter
    assert_eq!(super::super::visible::menu::cursor_elsewhere("Run it?\n  Yes\n  No\n", None, &|t| t == "Yes"), None);
}

// ---- through the capture engine --------------------------------------------------------------------------

struct Drive {
    c: Capture,
    now: u64,
}

impl Drive {
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

#[test]
fn the_capture_answers_the_real_screen_by_the_rules_with_the_one_time_yes_only() {
    let screen = after_tool_calls();
    let mut d = Drive { c: Capture::new(words(), (120, 40), Config::default()).with_auto_answer(true), now: 0 };
    d.show("", 500);
    let keys = d.show(&screen, 3500);
    assert_eq!(keys, vec![AnswerKey::Text("1".into()), AnswerKey::Enter], "the number, then Enter once the cursor is on that row");
    d.show(IDLE, 4000);
    let set = d.c.into_set();
    assert_eq!(set.answered.len(), 1, "{:?}", set.answered);
    let a = &set.answered[0];
    assert_eq!((a.label.as_str(), a.via.as_str()), ("Yes, run command", "rules"));
    assert!(a.read.contains("numbered rows") && !a.read.contains("Edited"), "{}", a.read);
}
