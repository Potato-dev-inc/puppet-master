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
