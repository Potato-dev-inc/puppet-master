//! A pane that shows a menu is a pane that is asking, whatever its profile's tables say (night 2026-10-28).
//!
//! Live, in the founder's app: Claude Code's AskUserQuestion menu and a wizard-added agent's workspace-trust screen
//! were both `idle` (Inbox: "Nothing needs you"), while the run supervisor already said `needs_input`. These tests
//! drive the real classifier (`pty::status::classify_agent_observation`) and the card reader with real-shaped screens,
//! and the negatives that must NOT count: an idle input box, a finished reply with a numbered list, a busy spinner.

use super::*;
use crate::pty::status::{classify_agent_observation, PaneStatus};
use crate::tui::declarative::{parse_definition, DeclarativeProfile};

const CLAUDE_QUESTION: &str = include_str!("../tui/claude/fixtures/ask_user_question_menu_2_1_311_live.txt");
const CLAUDE_READY_FRESH: &str = include_str!("../tui/claude/fixtures/ready_fresh_2_1_299_live.txt");
const CLAUDE_AFTER_TURN: &str = include_str!("../tui/claude/fixtures/ready_after_turn_2_1_299_live.txt");
const CLAUDE_WORKING: &str = include_str!("../tui/claude/fixtures/working_2_1_299_live.txt");
const CLAUDE_DRAFT: &str = include_str!("../tui/claude/fixtures/draft_2_1_299_live.txt");

/// A definition the Agent Wizard wrote for an Antigravity-like CLI: a `[[tui.prompt]]` for the trust screen and one for a
/// command, and NO `[tui.classify]` table (wizard-made definitions often lack it).
const AGY_LIKE: &str = r##"
schema_version = 1

[agent]
id = "agyprobe"
label = "Agy Probe"
experimental = true

[binary]
names = ["agyprobe-not-installed"]
install_hint = "see https://example.invalid/agyprobe"

[tui]
argv = []

[[tui.prompt]]
kind = "workspace_trust"
detect_regex = 'do you trust the contents of this project'
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny", key = "escape" },
]

[[tui.prompt]]
kind = "command_approval"
detect_regex = 'allow this command to run'
not_when_regex = 'command approved'
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny", key = "escape" },
]
"##;

const AGY_TRUST: &str = "
 Antigravity CLI v1.4.0

Accessing workspace:
C:\\Users\\you\\project

Do you trust the contents of this project?
Antigravity CLI requires permission to read, edit, and run files here.

> Yes, I trust this folder
  No, exit

\u{2191}/\u{2193} Navigate \u{b7} enter Confirm
";

/// A command approval of the same CLI, under a logo banner that carries the signed-in account.
const AGY_COMMAND: &str = "
      \u{2584}\u{2580}\u{2580}\u{2584}        Antigravity CLI v1.4.0
     \u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}       Signed in as someone@example.com
   \u{2584}\u{2580}\u{2580}    \u{2580}\u{2580}\u{2584}     C:\\Users\\you\\project

> create hello.txt with the word hi

  Running: echo hi > hello.txt
  Allow this command to run?
  > Allow once
    Always allow in this project
    Deny

\u{2191}/\u{2193} Navigate \u{b7} enter Confirm
";

/// The agy-like agent, registered once for this test run (the tests of this file run side by side).
fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let profile = std::sync::Arc::new(DeclarativeProfile::new(parse_definition(AGY_LIKE).expect("parses")).expect("builds"));
        crate::tui::registry::registry().add_transient(profile).expect("registered");
    });
}

// ---- the pane status ------------------------------------------------------------------------------------------------

#[test]
fn claudes_ask_user_question_menu_is_a_pane_waiting_for_the_person() {
    assert_eq!(classify_agent_observation("claude", CLAUDE_QUESTION, ""), PaneStatus::WaitingInput);
}

#[test]
fn the_menu_stays_waiting_while_it_is_on_screen_and_stops_when_it_is_gone() {
    // Redrawn with spaces trailing every row, as the emulator pads it: still the same question.
    let padded: String = CLAUDE_QUESTION.lines().map(|l| format!("{l:<150}\n")).collect();
    assert_eq!(classify_agent_observation("claude", &padded, ""), PaneStatus::WaitingInput);
    // Answered: Claude draws its own screen over the menu.
    assert_ne!(classify_agent_observation("claude", CLAUDE_AFTER_TURN, ""), PaneStatus::WaitingInput);
}

#[test]
fn an_idle_input_box_a_draft_and_a_working_spinner_are_not_questions() {
    for (name, screen) in [("fresh", CLAUDE_READY_FRESH), ("after a turn", CLAUDE_AFTER_TURN), ("draft", CLAUDE_DRAFT), ("working", CLAUDE_WORKING)] {
        assert_ne!(classify_agent_observation("claude", screen, ""), PaneStatus::WaitingInput, "{name}");
    }
}

#[test]
fn a_finished_reply_with_a_numbered_list_is_not_a_question() {
    let reply = "\u{25cf} Here is the plan:\n\n  1. Open the file\n  2. Change the colour\n  3. Save it\n\n\u{273b} Churned for 3s \u{b7} done 15:27\n\n";
    let screen = format!("{reply}{}", CLAUDE_AFTER_TURN.lines().skip(8).collect::<Vec<_>>().join("\n"));
    assert_ne!(classify_agent_observation("claude", &screen, ""), PaneStatus::WaitingInput);
    // The same list while the spinner turns.
    let busy = format!("{reply}{}", CLAUDE_WORKING.lines().skip(8).collect::<Vec<_>>().join("\n"));
    assert_ne!(classify_agent_observation("claude", &busy, ""), PaneStatus::WaitingInput);
    // A list under an arrow marker with no hint footer under it is only a list.
    let listed = "Pick one of these:\n> 1. Red\n  2. Blue\n";
    assert_ne!(classify_agent_observation("claude", listed, ""), PaneStatus::WaitingInput);
}

#[test]
fn a_wizard_added_agents_trust_screen_is_a_pane_waiting_for_the_person() {
    register();
    // Its own `[[tui.prompt]]` matches, but there is no `[tui.classify] waiting_regex`: before, `running`, then `idle`.
    assert_eq!(classify_agent_observation("agyprobe", AGY_TRUST, ""), PaneStatus::WaitingInput);
    assert_eq!(classify_agent_observation("agyprobe", AGY_COMMAND, ""), PaneStatus::WaitingInput);
}

#[test]
fn the_same_agents_idle_input_box_is_not() {
    register();
    let idle = "\n Antigravity CLI v1.4.0\n\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n> \n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n? for shortcuts\n";
    assert_ne!(classify_agent_observation("agyprobe", idle, ""), PaneStatus::WaitingInput);
    // A command approval that was answered ("command approved" is in the scrollback) is not asking any more.
    let answered = format!("{AGY_COMMAND}\n  command approved\n  Created hello.txt\n  Done.\n\n> \n? for shortcuts\n");
    assert_ne!(classify_agent_observation("agyprobe", &answered, ""), PaneStatus::WaitingInput);
}

#[test]
fn a_menu_by_shape_is_waiting_for_an_agent_with_no_tables_at_all() {
    // No profile knows this agent: the cursor row among listed options and the hint footer are the whole evidence.
    let menu = "Which database should I use?\n\n\u{276f} 1. SQLite\n  2. Postgres\n\nEnter to select \u{b7} \u{2191}/\u{2193} to navigate \u{b7} Esc to cancel\n";
    assert_eq!(classify_agent_observation("some-unregistered-agent", menu, ""), PaneStatus::WaitingInput);
    let no_footer = "Which database should I use?\n\n\u{276f} 1. SQLite\n  2. Postgres\n";
    assert_ne!(classify_agent_observation("some-unregistered-agent", no_footer, ""), PaneStatus::WaitingInput);
}

// ---- the card ---------------------------------------------------------------------------------------------------------

#[test]
fn claudes_question_menu_becomes_an_answerable_question_card() {
    let card = detect_in_screen_with_status("claude", CLAUDE_QUESTION, false, Some("waiting_input")).expect("a card");
    assert_eq!(card["kind"], "question");
    assert_eq!(card["choices"], json!(["1", "2"]), "the options with their number keys; the two rows that leave the menu are not offered");
    assert_eq!(card["labels"]["1"], "Red");
    assert_eq!(card["labels"]["2"], "Blue");
    assert!(card["summary"].as_str().unwrap().contains("Pick a colour"), "{card}");
    let text = card.to_string();
    assert!(!text.contains("Type something") || card["terminal_only"].is_array(), "{card}");
    // The card id is the question's: a redraw keeps it, another question is another card.
    let again = detect_in_screen_with_status("claude", &format!("older output\n{CLAUDE_QUESTION}"), false, Some("waiting_input")).unwrap();
    assert_eq!(card["prompt_id"], again["prompt_id"]);
    let other = detect_in_screen_with_status("claude", &CLAUDE_QUESTION.replace("Pick a colour", "Pick a size"), false, Some("waiting_input")).unwrap();
    assert_ne!(card["prompt_id"], other["prompt_id"]);
}

#[test]
fn answering_a_question_card_moves_the_cursor_and_never_types_a_number() {
    let card = detect_in_screen_with_status("claude", CLAUDE_QUESTION, false, Some("waiting_input")).expect("a card");
    // The cursor is on row 1: "Blue" is one Down; the route's own Enter then confirms it. A number key would select and
    // submit on its own, and the Enter after it could answer the NEXT question.
    assert_eq!(reply_key("claude", &card, "2", false).unwrap().0, "\u{1b}[B");
    assert_eq!(reply_key("claude", &card, "1", false).unwrap().0, "");
    assert!(reply_key("claude", &card, "3", false).is_err(), "the row that opens free text is the person's");
    assert!(reply_key("claude", &card, "4", false).is_err(), "so is the row that leaves the menu");
}

// ---- the summary --------------------------------------------------------------------------------------------------------

#[test]
fn a_cards_summary_is_the_question_next_to_the_menu_and_never_an_email_address() {
    register();
    let card = detect_in_screen("agyprobe", AGY_COMMAND, false).expect("a card");
    let summary = card["summary"].as_str().expect("a summary");
    assert!(summary.contains("Allow this command to run?"), "{summary}");
    assert!(summary.contains("echo hi > hello.txt"), "the command being approved is part of the question: {summary}");
    assert!(!summary.contains("someone@example.com") && !summary.contains('@'), "{summary}");
    assert!(!summary.contains("Antigravity CLI v1.4.0"), "the logo banner is not the question: {summary}");
    assert!(summary.chars().count() <= 240, "{summary}");
}

#[test]
fn an_email_in_the_question_itself_is_masked() {
    register();
    let screen = AGY_COMMAND.replace("Allow this command to run?", "Allow this command to run? (as ana@example.com)");
    let card = detect_in_screen("agyprobe", &screen, false).expect("a card");
    let summary = card["summary"].as_str().unwrap();
    assert!(summary.contains("Allow this command to run?") && !summary.contains("ana@example.com"), "{summary}");
}

#[test]
fn a_long_question_block_is_capped() {
    register();
    let long = format!("  Allow this command to run? {}\n", "x".repeat(600));
    let screen = AGY_COMMAND.replace("  Allow this command to run?\n", &long);
    let card = detect_in_screen("agyprobe", &screen, false).expect("a card");
    assert!(card["summary"].as_str().unwrap().chars().count() <= 240);
}

// ---- the startup gate -----------------------------------------------------------------------------------------------------

#[test]
fn a_declared_trust_prompt_is_a_startup_gate_only_the_person_passes() {
    register();
    let gate = crate::tui::profile_for("agyprobe").startup_gate(AGY_TRUST).expect("the trust screen is a startup gate");
    assert_eq!(gate.id, crate::tui::StartupGate::WORKSPACE_TRUST);
    assert_eq!(gate.dismiss, None, "nothing closes a trust gate for the person");
    // A command approval is a question, not a startup gate; so is the idle box.
    assert!(crate::tui::profile_for("agyprobe").startup_gate(AGY_COMMAND).is_none());
    assert!(crate::tui::profile_for("agyprobe").startup_gate("> \n? for shortcuts\n").is_none());
}

// ---- no field of a card carries an email ---------------------------------------------------------------------------------

#[test]
fn no_field_of_a_card_carries_a_banner_email() {
    register();
    // A declarative prompt carries the whole screen as `text`: the banner's account must not ride along.
    let card = detect_in_screen("agyprobe", AGY_COMMAND, false).expect("a card");
    assert!(card["text"].as_str().unwrap().contains("Allow this command to run?"), "the text is still the screen: {card}");
    assert!(!card.to_string().contains("someone@example.com"), "{card}");
    let action = required_action_from_prompt(card);
    assert!(!action.to_string().contains("someone@example.com"), "{action}");
    // Claude's question card: an email in the banner above the menu, in the question and in an option.
    let screen = CLAUDE_QUESTION
        .replace("Claude Max", "someone@example.com")
        .replace("Pick a colour", "Pick a colour for someone@example.com")
        .replace("2. Blue", "2. Blue (someone@example.com)");
    let card = detect_in_screen_with_status("claude", &screen, false, Some("waiting_input")).expect("a card");
    assert_eq!(card["kind"], "question");
    assert!(!card.to_string().contains("someone@example.com"), "{card}");
    // The menu shape card of an agent no profile knows.
    let menu = "Signed in as someone@example.com\n\nWhich database?\n\n\u{276f} 1. SQLite\n  2. Postgres\n\nEnter to select \u{b7} Esc to cancel\n";
    let card = detect_in_screen_with_status("some-unregistered-agent", menu, false, Some("waiting_input")).expect("a card");
    assert!(!card.to_string().contains("someone@example.com"), "{card}");
}
