//! The generic "a menu is the last thing on screen" rule on real-shaped screens, and the screens it must not take.

use super::*;
use crate::tui::profile_for;

const CLAUDE_QUESTION: &str = include_str!("claude/fixtures/ask_user_question_menu_2_1_311_live.txt");

fn tail(screen: &str) -> String {
    let rows: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    rows[rows.len().saturating_sub(12)..].join("\n")
}

fn asks_on(agent: &str, screen: &str) -> bool {
    asks(profile_for(agent).as_ref(), &tail(screen))
}

#[test]
fn an_option_menu_with_a_cursor_row_and_a_hint_footer_is_asking() {
    assert!(asks_on("claude", CLAUDE_QUESTION));
    let arrow = "Do you trust the contents of this project?\n\n> Yes, I trust this folder\n  No, exit\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
    assert!(asks_on("unregistered-agent", arrow));
    let numbered = "Which database?\n\n\u{276f} 1. SQLite\n  2. Postgres\n\nEnter to select \u{b7} Esc to cancel\n";
    assert!(asks_on("unregistered-agent", numbered));
}

#[test]
fn a_list_without_a_cursor_or_without_a_footer_is_not_asking() {
    let no_cursor = "Which database?\n\n  1. SQLite\n  2. Postgres\n\nEnter to select \u{b7} Esc to cancel\n";
    assert!(!asks_on("unregistered-agent", no_cursor));
    let no_footer = "Which database?\n\n\u{276f} 1. SQLite\n  2. Postgres\n";
    assert!(!asks_on("unregistered-agent", no_footer));
}

#[test]
fn a_menu_with_an_input_box_under_it_was_answered_long_ago() {
    let answered = format!("{CLAUDE_QUESTION}\nThanks, red it is.\n\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\n? for shortcuts\n");
    assert!(!asks_on("unregistered-agent", &answered));
}

#[test]
fn an_idle_input_box_and_a_reply_with_a_list_are_not_asking() {
    let idle = "Reply done.\n\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n> \n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n? for shortcuts\n";
    assert!(!asks_on("unregistered-agent", idle));
    let reply = "Steps:\n  1. Open the file\n  2. Edit it\n  3. Save\n\n> \n\u{2191}/\u{2193} history\n";
    assert!(!asks_on("unregistered-agent", reply));
}

#[test]
fn no_recorded_idle_or_working_screen_of_any_profile_is_asking() {
    for dir in ["claude/fixtures", "codex/fixtures", "cursor/fixtures", "opencode/fixtures"] {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui").join(dir);
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let quiet = ["ready", "working", "idle", "draft", "reply", "after_turn"].iter().any(|w| name.contains(w));
            if !name.ends_with(".txt") || !quiet {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
            let agent = dir.split('/').next().unwrap();
            let agent = if agent == "cursor" { "cursor_agent" } else { agent };
            assert!(!asks_on(agent, &text), "{dir}/{name} is not a question");
        }
    }
}

// A menu footer is any short row that names a key, not one of a list of exact phrases.
const SHORT_KEY_FOOTER: &str = "  Update available \u{b7} 1.0 \u{2192} 2.0\n  Release notes: https://example.com/releases/latest\n\n\u{203a} 1. Update now (runs `a long installer command that wraps\n    onto a second row`)\n  2. Skip\n  3. Skip until next version\n\n  enter continue \u{b7} esc skip\n";

#[test]
fn a_menu_with_a_short_key_footer_is_asking() {
    assert!(asks_on("unregistered-agent", SHORT_KEY_FOOTER));
    // Whatever the words around the key names.
    for footer in ["press enter to go on", "Esc: skip", "ctrl+c quit", "space toggles \u{b7} tab next", "\u{2191}\u{2193} move"] {
        let screen = format!("Pick one\n\n\u{203a} 1. Alpha\n  2. Beta\n\n  {footer}\n");
        assert!(asks_on("unregistered-agent", &screen), "{footer}");
    }
}

#[test]
fn a_key_name_inside_a_longer_word_is_not_a_footer() {
    // "enterprise", "tabs", "pressed" and "returns" contain key names but are not them.
    let screen = "Pick one\n\n\u{203a} 1. Alpha\n  2. Beta\n\n  enterprise tabs returns\n";
    assert!(!asks_on("unregistered-agent", screen));
}

#[test]
fn a_finished_reply_with_a_list_and_an_input_box_is_not_asking_even_with_a_key_footer_word() {
    let reply = "Steps:\n  1. Open the file\n  2. Edit it\n  3. Save\n\nDone.\n\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\n? for shortcuts\n";
    assert!(!asks_on("unregistered-agent", reply));
}

#[test]
fn a_numbered_list_with_no_cursor_and_a_key_footer_is_not_asking() {
    let screen = "Pick one\n\n  1. Alpha\n  2. Beta\n  3. Gamma\n\n  enter continue \u{b7} esc skip\n";
    assert!(!asks_on("unregistered-agent", screen));
}

#[test]
fn a_long_prose_row_that_mentions_enter_under_a_list_without_a_cursor_is_not_asking() {
    let prose = "To use it, run the command, wait until the build has finished and then press enter to continue with the next step of the setup guide.";
    let screen = format!("Steps:\n  1. Open the file\n  2. Edit it\n  3. Save\n\n{prose}\n");
    assert!(!asks_on("unregistered-agent", &screen));
    // Even under a list that has a cursor row, a row that long is prose and not a footer.
    let with_cursor = format!("Steps:\n\u{203a} 1. Open the file\n  2. Edit it\n  3. Save\n\n{prose}\n");
    assert!(!asks_on("unregistered-agent", &with_cursor));
}
