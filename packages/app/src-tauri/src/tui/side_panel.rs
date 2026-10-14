//! Side panels: some TUIs draw a second column beside the chat (Claude Code's diff panel is one).
//! The panel is separated from the chat by a vertical box-drawing divider that sits in the same
//! column on most rows. Read as plain text the panel's rows are interleaved with the chat's
//! (and a row that fills the full width is glued to the next one by `vt100`'s `contents()`), so
//! the transcript came out as two columns of text mixed together.
//!
//! This is detected by structure only (never by agent name): a column where most rows carry a
//! vertical bar and where real text sits to the right of it. A profile opts in with
//! `TuiProfile::side_panel()`; the pane readers then crop each row at the divider.

/// Vertical box-drawing characters a divider is made of.
const BARS: [char; 6] = ['\u{2502}', '\u{2503}', '\u{2551}', '\u{258f}', '\u{2595}', '\u{250a}'];
/// A divider closer to the left edge than this is a quote bar or a dialog border, not a panel.
const MIN_LEFT: usize = 20;
/// A panel narrower than this is a scrollbar or a box edge.
const MIN_RIGHT: usize = 12;
const MIN_ROWS: usize = 6;

fn is_bar(c: char) -> bool {
    BARS.contains(&c)
}

/// The char column of the panel divider in `rows` (one string per screen row), if there is one.
pub fn divider_col<S: AsRef<str>>(rows: &[S]) -> Option<usize> {
    use std::collections::HashMap;
    let mut with_bar: HashMap<usize, usize> = HashMap::new();
    let mut with_text_right: HashMap<usize, usize> = HashMap::new();
    let mut non_blank = 0usize;
    for row in rows {
        let row = row.as_ref();
        if row.trim().is_empty() {
            continue;
        }
        non_blank += 1;
        let chars: Vec<char> = row.chars().collect();
        for (col, c) in chars.iter().enumerate() {
            if col >= MIN_LEFT && is_bar(*c) {
                *with_bar.entry(col).or_default() += 1;
                if chars[col + 1..].iter().any(|c| !c.is_whitespace() && !is_bar(*c)) {
                    *with_text_right.entry(col).or_default() += 1;
                }
            }
        }
    }
    let widest = rows.iter().map(|r| r.as_ref().trim_end().chars().count()).max().unwrap_or(0);
    with_bar
        .into_iter()
        // Most rows have the bar, a few of them have text to its right, and the panel is wide
        // enough to matter (a dialog's right border has nothing beyond it).
        .filter(|(col, n)| {
            *n >= MIN_ROWS
                && *n * 2 >= non_blank
                && with_text_right.get(col).copied().unwrap_or(0) >= 3
                && widest >= col + MIN_RIGHT
        })
        .map(|(col, _)| col)
        .min()
}

/// `row` without the panel: everything left of the divider, trailing blanks trimmed. A row that
/// has no divider at `col` is returned as it is (it is from a time the panel was not open).
fn crop_row(row: &str, col: usize) -> String {
    let chars: Vec<char> = row.chars().collect();
    if chars.get(col).is_some_and(|c| is_bar(*c)) {
        chars[..col].iter().collect::<String>().trim_end().to_string()
    } else {
        row.trim_end().to_string()
    }
}

/// Screen or scrollback rows with the panel cropped away. `None` when there is no panel, so the
/// caller keeps its usual reading of the screen.
pub fn crop_rows<S: AsRef<str>>(rows: &[S]) -> Option<Vec<String>> {
    let col = divider_col(rows)?;
    Some(rows.iter().map(|r| crop_row(r.as_ref(), col)).collect())
}

/// How far left a row's divider can sit after its leading blanks were trimmed (indent of a
/// bullet's continuation lines, tool output, diff numbers).
const TRIM_SLACK: usize = 8;

/// The same cleanup for reply text whose lines are already extracted. Their leading blanks were
/// trimmed, so the divider sits up to `TRIM_SLACK` columns left of where it was drawn, and a
/// panel-only row starts with its bar. A divider that at least three chat rows share (within the
/// slack) is cut from each of them, and the panel-only rows are dropped.
pub fn strip_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let first_bar = |l: &str| l.chars().enumerate().find(|(c, ch)| *c >= MIN_LEFT && is_bar(*ch)).map(|(c, _)| c);
    let mut votes: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for l in lines.iter().filter(|l| !l.starts_with(is_bar)) {
        if let Some(c) = first_bar(l) {
            *votes.entry(c).or_default() += 1;
        }
    }
    // The divider as drawn is the rightmost column that enough rows share with the others
    // within the slack.
    let Some(drawn) = votes
        .keys()
        .copied()
        .filter(|d| votes.iter().filter(|(c, _)| **c <= *d && **c + TRIM_SLACK >= *d).map(|(_, n)| n).sum::<usize>() >= 3)
        .max()
    else {
        return text.to_string();
    };
    let kept: Vec<String> = lines
        .iter()
        .filter(|l| !l.starts_with(is_bar))
        .map(|l| match first_bar(l) {
            Some(c) if c <= drawn && c + TRIM_SLACK >= drawn => l.chars().take(c).collect::<String>().trim_end().to_string(),
            _ => l.to_string(),
        })
        .collect();
    let mut out = kept.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: &str = include_str!("claude/fixtures/side_diff_panel_screen.txt");

    fn rows() -> Vec<&'static str> {
        SCREEN.lines().collect()
    }

    #[test]
    fn finds_the_divider_of_the_real_layout() {
        assert_eq!(divider_col(&rows()), Some(90));
    }

    #[test]
    fn cropping_removes_every_panel_row_and_keeps_the_chat() {
        let cropped = crop_rows(&rows()).expect("panel").join("\n");
        for panel_text in ["1 file changed", "source: Current", "[ ask ]", "+1 -1", "\u{2715}"] {
            assert!(!cropped.contains(panel_text), "panel text {panel_text:?} leaked:\n{cropped}");
        }
        assert!(cropped.contains("I'll change\n  it to start at 0."), "{cropped}");
        assert!(cropped.contains("npm test now prints all tests passed"), "{cropped}");
        assert!(cropped.contains("      4 +  for (let i = 0; i < numbers.length; i++) {\n"), "{cropped}");
        // The chat's own diff keeps its line numbers; the panel's copy (no indent) is gone.
        assert_eq!(cropped.matches("for (let i = 0; i < numbers.length; i++) {").count(), 2, "{cropped}");
    }

    #[test]
    fn a_screen_without_a_panel_is_left_alone() {
        let plain = ["hello", "", "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}", "\u{2502} Do you want to proceed? \u{2502}"];
        assert_eq!(divider_col(&plain), None);
        assert!(crop_rows(&plain).is_none());
    }

    #[test]
    fn a_boxed_dialog_is_not_a_panel() {
        let mut dialog: Vec<String> = vec!["\u{256d}".to_string() + &"\u{2500}".repeat(38) + "\u{256e}"];
        for n in 0..10 {
            dialog.push(format!("\u{2502} option {n}{:width$}\u{2502}", "", width = 28));
        }
        assert_eq!(divider_col(&dialog), None);
    }

    #[test]
    fn strip_text_cuts_trimmed_rows() {
        // What the reply text looked like: leading blanks trimmed, panel-only rows start at the bar.
        let chat = [
            "\u{25cf} Fixed. sum.js had its loop starting at index 1, so it was never added.",
            "sum([1, 2, 3]) returned 5 instead of 6.",
            "I changed the loop to start at index 0.",
        ];
        let mut lines: Vec<String> = Vec::new();
        for (i, c) in chat.iter().cycle().take(9).enumerate() {
            let row = format!("{c:<80}\u{2502}panel row {i} with text");
            lines.push(row);
            lines.push(format!("\u{2502}only the panel {i}"));
        }
        let out = strip_text(&lines.join("\n"));
        assert!(!out.contains("panel"), "{out}");
        assert!(out.contains("I changed the loop to start at index 0."));
        assert_eq!(out.lines().count(), 9);
    }

    #[test]
    fn strip_text_leaves_ordinary_text_alone() {
        let text = "a \u{2502} b\nplain reply\n";
        assert_eq!(strip_text(text), text);
    }
}
