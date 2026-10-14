//! Live bug #6: the Transcript tab showed Claude Code's diff panel interleaved with the chat.
//! The screen is the layout of `03b-claude-done.png` (164x50, panel divider at column 90).

use super::completion::{clean_extracted_result, tui_turn_evidence};

const AGENT: &str = "claude";
const TASK: &str = "Fix the failing test in sum.js, then run npm test.";
const FIXTURE: &str = include_str!("../tui/claude/fixtures/side_diff_panel_screen.txt");

/// What `read_snapshot` gave before the fix: `vt100`'s `contents()`, which glues full-width rows.
fn raw_contents() -> String {
    let mut parser = vt100::Parser::new(50, 164, 0);
    for row in FIXTURE.lines() {
        parser.process(row.as_bytes());
        if row.chars().count() < 164 {
            parser.process(b"\r\n");
        }
    }
    parser.screen().contents()
}

fn reply_from(screen: &str) -> String {
    tui_turn_evidence(screen, "", AGENT)
        .and_then(|raw| clean_extracted_result(&raw, TASK, AGENT))
        .unwrap_or_default()
}

#[test]
fn the_uncropped_screen_carries_the_panel_in_its_evidence() {
    // Documents the bug at its source: contents() glues rows and the panel text rides along.
    let evidence = tui_turn_evidence(&raw_contents(), "", AGENT).unwrap_or_default();
    // (The logo-banner row the panel header was glued to, `source: Current`, is chrome now and is
    // dropped; the panel's own rows still ride along.)
    assert!(evidence.contains("[ ask ]"), "{evidence}");
}

#[test]
fn the_cropped_screen_gives_the_chat_reply_only() {
    let rows: Vec<&str> = FIXTURE.lines().collect();
    let cropped = crate::tui::side_panel::crop_rows(&rows).expect("panel").join("\n");
    let reply = reply_from(&cropped);
    for panel_text in ["source: Current", "1 file changed", "[ ask ]", "\u{2502}"] {
        assert!(!reply.contains(panel_text), "panel text {panel_text:?} in reply:\n{reply}");
    }
    assert!(reply.contains("npm test now prints all tests passed"), "{reply}");
    assert!(reply.contains("I changed the loop to start at index 0"), "{reply}");
}

#[test]
fn reply_text_that_still_carries_the_panel_is_tidied_by_the_profile() {
    // Rows as the scrollback reader returns them: chat text, a gap, the divider, panel text.
    let text = FIXTURE
        .lines()
        .skip(1)
        .take(30)
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    let tidied = crate::tui::tidy_reply(&text, AGENT);
    assert!(!tidied.contains("source: Current") && !tidied.contains("[ ask ]"), "{tidied}");
    assert!(tidied.contains("Update(sum.js)"), "{tidied}");
}
