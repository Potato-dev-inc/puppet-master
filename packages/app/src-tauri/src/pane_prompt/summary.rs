//! The one-sentence summary of a prompt card: the QUESTION next to the menu, never the top of the screen.
//!
//! A declarative `[[tui.prompt]]` carries the whole screen as its `text`, so a card with no summary of its own showed the
//! start of that screen: an agent's logo banner with the signed-in account's email. Here the summary is built from the
//! rows right above the menu (the question and the command it asks about), capped, and passed through the redaction
//! (`redact::text` for secrets and token shapes, `mask_emails` for addresses).

use crate::tui::declarative::wizard::visible::answer::{parse_menu, MenuKind};

/// Longest summary, in characters.
pub(crate) const MAX_SUMMARY_CHARS: usize = 240;
/// Most rows of the question block.
const MAX_BLOCK_ROWS: usize = 4;

fn is_rule(row: &str) -> bool {
    row.chars().count() >= 3 && row.chars().all(|ch| matches!(ch, '\u{2500}'..='\u{257f}' | '-' | '=' | '_'))
}

fn plain(row: &str) -> String {
    row.trim().trim_start_matches(|ch: char| matches!(ch, '\u{2502}' | '|' | '\u{25cf}' | '\u{2022}')).trim().to_string()
}

/// The rows of the question block of the menu (or `[y/n]` line) in the last rows of `screen`, top to bottom.
pub(crate) fn question_rows(screen: &str) -> Vec<String> {
    // Blank rows are kept: a blank row ends the block (above it is the banner or an earlier reply), except for the one
    // gap an agent leaves between its question and its options.
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    if rows.iter().all(|r| r.trim().is_empty()) {
        return Vec::new();
    }
    // Where the menu starts: the first option's row; a one-line `[y/n]` question is its own row.
    let first_option = parse_menu(screen).and_then(|menu| {
        let first = menu.options.first()?.text.trim().to_string();
        let at = rows.iter().rposition(|row| !row.trim().is_empty() && row.contains(first.as_str()))?;
        Some((at, menu.kind == MenuKind::YesNo))
    });
    let (end, include_end) = match first_option {
        Some((at, yes_no)) => (at, yes_no),
        None => {
            // No menu read: the last row that asks something, else the last rows.
            let asking = rows.iter().rposition(|row| row.contains('?') || row.to_ascii_lowercase().contains("[y/n]"));
            let rows_len = rows.iter().rposition(|r| !r.trim().is_empty()).map_or(0, |at| at + 1);
            match asking {
                Some(at) => (at, true),
                None => (rows_len, false),
            }
        }
    };
    let mut block: Vec<String> = Vec::new();
    if include_end && end < rows.len() && !rows[end].trim().is_empty() {
        block.push(plain(rows[end]));
    }
    let mut gap = block.is_empty();
    for row in rows[..end].iter().rev() {
        if block.len() >= MAX_BLOCK_ROWS || is_rule(row.trim()) {
            break;
        }
        let row = plain(row);
        if row.is_empty() {
            // One blank row right above the menu is allowed; any other blank row ends the block.
            if gap && block.is_empty() {
                gap = false;
                continue;
            }
            break;
        }
        block.push(row);
    }
    block.reverse();
    block
}

/// `text` as a summary may carry it: secrets and token shapes masked, email addresses masked, one line, capped.
pub(crate) fn clean(text: &str) -> String {
    let masked = crate::redact::mask_emails(crate::redact::text(text).as_str());
    let one_line = masked.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX_SUMMARY_CHARS {
        return one_line;
    }
    let cut: String = one_line.chars().take(MAX_SUMMARY_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The summary for the prompt on `screen`: its question block, cleaned. `None` when the screen has nothing to say.
pub(crate) fn for_screen(screen: &str) -> Option<String> {
    let rows = question_rows(screen);
    let text = clean(&rows.join(" "));
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_is_the_rows_above_the_menu_and_stops_at_a_blank_banner_or_rule() {
        let screen = "  Signed in as someone@example.com\n\n> make a file\n\n  Running: echo hi\n  Allow this command to run?\n  > Allow once\n    Deny\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
        let summary = for_screen(screen).unwrap();
        assert!(summary.contains("Allow this command to run?") && summary.contains("echo hi"), "{summary}");
        assert!(!summary.contains("someone@example.com"), "{summary}");
    }

    #[test]
    fn a_one_line_yes_no_question_is_its_own_summary() {
        assert_eq!(for_screen("banner\n\nAllow nimbus to create hello.txt? [y/n]\n").unwrap(), "Allow nimbus to create hello.txt? [y/n]");
    }

    #[test]
    fn emails_and_tokens_are_masked_and_the_length_is_capped() {
        let cleaned = clean("mail me at ana@example.com with sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789");
        assert!(!cleaned.contains("ana@example.com") && !cleaned.contains("abcdefghijklmnopqrstuvwxyz"), "{cleaned}");
        assert!(clean(&"word ".repeat(200)).chars().count() <= MAX_SUMMARY_CHARS);
    }
}
