//! The interactive Claude Code TUI: what its screens show about whether a turn is running.

use crate::tui::{ComposerState, StandbyScreen};

/// Spinner glyphs Claude cycles through in front of its activity verb.
const SPINNER_GLYPHS: [char; 8] = ['\u{2722}', '*', '\u{2736}', '\u{273b}', '\u{273d}', '\u{2733}', '\u{b7}', '\u{2731}'];

/// Whether the visible rows show Claude working: the activity row `<spinner> <Verb>\u{2026}` that stays
/// up for the whole turn (tool runs included), or the interrupt hint. A finished turn replaces it
/// with `<glyph> <Verb> for 1s`, which has no ellipsis.
pub(super) fn shows_work_in_progress(visible_tail: &str) -> bool {
    if visible_tail.to_ascii_lowercase().contains("esc to interrupt") {
        return true;
    }
    visible_tail.lines().any(|line| {
        let mut chars = line.trim_start().chars();
        let first = chars.next();
        let rest: String = chars.collect();
        first.is_some_and(|glyph| SPINNER_GLYPHS.contains(&glyph))
            && rest.starts_with(' ')
            && rest.contains('\u{2026}')
    })
}

/// Dialogs that wait for the user and take Enter/arrow keys: the folder trust check, tool
/// permission requests. A standby never answers one.
const DIALOG_PHRASES: [&str; 6] = [
    "quick safety check",
    "yes, i trust this folder",
    "do you trust the files",
    "do you want to proceed",
    "do you want to allow",
    "enter to confirm",
];

/// Login, update and first-run screens.
const ATTENTION_PHRASES: [&str; 8] = [
    "select login method",
    "please run /login",
    "invalid api key",
    "choose the text style",
    "welcome to claude code",
    "update available",
    "auto-update failed",
    "claude update",
];

/// Whether a pre-launched Claude pane is at an empty prompt: the `\u{276f}` row between the rules holds
/// nothing or only the rotating `Try "..."` placeholder. The folder-trust dialog also draws a
/// `\u{276f}` (on its selected option), so dialogs are ruled out first.
pub(super) fn standby_screen(screen: &str) -> StandbyScreen {
    let lower = screen.to_ascii_lowercase();
    if DIALOG_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::Dialog;
    }
    if ATTENTION_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::NeedsAttention;
    }
    if shows_work_in_progress(screen) {
        return StandbyScreen::NotReady;
    }
    // vt100 joins a soft-wrapped row to the next one: when the box's top rule fills the pane width
    // the prompt arrives glued to it ("----> Try ..."), so leading rule characters are skipped.
    let empty_prompt = screen.lines().any(|line| {
        line.trim()
            .trim_start_matches('\u{2500}')
            .strip_prefix('\u{276f}')
            .is_some_and(|rest| rest.trim().is_empty() || rest.trim().starts_with("Try \""))
    });
    if empty_prompt && screen.contains("Claude Code") {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
}

/// Non-empty rows from the bottom that may hold the input box: the box (rule, prompt row, rule),
/// a few continuation rows of a typed draft, and the footer under it.
const INPUT_BOX_TAIL_ROWS: usize = 10;

/// Claude's input box read POSITIVELY (`TuiProfile::ready_recogniser`). The box is the `\u{276f}` row
/// that starts a row (dialog options are indented), with a rule above it (its own row, or glued in
/// front of it when the rule filled the row and vt100 joined the two) and a rule below it within a
/// few rows, near the bottom of the screen. Then: the activity row = `Working`; nothing or the
/// rotating `Try "..."` placeholder after the glyph = `Empty`; anything else = `HasText`. No such box
/// (a dialog, a menu, the login page, a splash, a blank pane) = `Unknown`. Works on follow-ups: it
/// needs no banner (the startup logo scrolls away after the first turn).
/// Live captures: `fixtures/ready_*_live.txt`, the delivery traces, the side-panel screen.
pub(super) fn input_box(screen: &str) -> ComposerState {
    let rows: Vec<&str> = screen.lines().map(str::trim_end).filter(|row| !row.trim().is_empty()).collect();
    let is_rule = |row: &str| row.trim_start().starts_with('\u{2500}');
    let first = rows.len().saturating_sub(INPUT_BOX_TAIL_ROWS);
    let Some((at, rest)) = (first..rows.len()).rev().find_map(|at| {
        let row = rows[at];
        let glued = row.starts_with('\u{2500}');
        if !glued && !row.starts_with('\u{276f}') {
            return None;
        }
        let rest = row.trim_start_matches('\u{2500}').strip_prefix('\u{276f}')?;
        let rule_above = glued || (at > 0 && is_rule(rows[at - 1]));
        rule_above.then_some((at, rest))
    }) else {
        return ComposerState::Unknown;
    };
    // The rule under the box, after the prompt row and any continuation rows of a draft.
    let Some(below) = rows[at + 1..].iter().position(|row| is_rule(row)) else {
        return ComposerState::Unknown;
    };
    // A dialog drawn under the box is not an input box to type into.
    let under = rows[at + 1..].join("\n").to_ascii_lowercase();
    if DIALOG_PHRASES.iter().chain(ATTENTION_PHRASES.iter()).any(|phrase| under.contains(phrase)) {
        return ComposerState::Unknown;
    }
    if shows_work_in_progress(screen) {
        return ComposerState::Working;
    }
    let draft = rest.trim();
    if below == 0 && (draft.is_empty() || draft.starts_with("Try \"")) {
        ComposerState::Empty
    } else {
        ComposerState::HasText
    }
}

/// Whether `glyph` is one of the block elements Claude's startup logo is drawn with.
fn is_logo_glyph(glyph: char) -> bool {
    ('\u{2580}'..='\u{259f}').contains(&glyph)
}

/// Status rows Claude draws under a reply: the finished-turn summary (`<glyph> Churned for 16s \u{b7}
/// done 23:03`), the agents hint, the permission-mode footer (`\u{23f8} manual mode on \u{b7} ? for
/// shortcuts`, `\u{23f5}\u{23f5} auto mode on (shift+tab to cycle)`), the effort indicator (`\u{25d0} medium \u{b7}
/// /effort`, drawn at the right of the same row) and the startup logo banner. `lower` is the
/// lowercased row.
pub(super) fn is_chrome_line(lower: &str) -> bool {
    let row = lower.trim();
    let first = row.chars().next();
    // The footer: a pause or play glyph for the permission mode, the shortcuts hint, the effort
    // indicator. A reply never starts with those glyphs.
    let footer = first.is_some_and(|glyph| glyph == '\u{23f8}' || glyph == '\u{23f5}')
        || row.starts_with("? for shortcuts")
        || row.ends_with("\u{b7} /effort")
        || row == "/effort";
    // The logo banner: block-element glyphs, then two or more spaces, then text (`\u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}
    // Claude Code v2.1.299`). A row of nothing but glyphs is a rule or fill (handled generically).
    let banner = first.is_some_and(is_logo_glyph) && row.contains("  ") && row.chars().any(char::is_alphanumeric);
    if footer || banner {
        return true;
    }
    let summary = row
        .chars()
        .next()
        // Not '*' or the middle dot: those are also an ordinary bullet and a separator.
        .is_some_and(|glyph| glyph != '*' && glyph != '\u{b7}' && SPINNER_GLYPHS.contains(&glyph))
        && row.contains(" for ")
        && row.contains("\u{b7} done");
    // The agents hint is sometimes drawn after a middle dot ("\u{b7} \u{2190} for agents").
    let hint = row.trim_start_matches(['\u{b7}', ' ']);
    summary || hint.starts_with("\u{2190} for agents") || row.contains("shift+tab to cycle")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rows recorded from a live Claude Code session (v2.1.287).
    const WORKING: &str = "\u{276f} Reply with only the word PONG. Use no tools.\n\u{2722} Osmosing\u{2026}\n  \u{23bf}  Tip: Want a side agent watching your back\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\n";
    const WORKING_STAR: &str = "* Osmosing\u{2026}\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n";
    const FINISHED: &str = "\u{276f} Reply with only the word PONG. Use no tools.\n\u{25cf} PONG\n\u{273b} Brewed for 1s \u{b7} done 23:01\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\n";
    const READY: &str = " \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}   Claude Code v2.1.287\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Try \"how does <filepath> work?\"\n\u{2500}\u{2500}\u{2500}\u{2500}\n";

    // Live captures (v2.1.287, 2026-10-06): the folder-trust dialog and a freshly started session.
    const TRUST_DIALOG: &str = "\u{2500}\u{2500}\u{2500}\n Accessing workspace:\n\n sandbox\n\n Quick safety check: Is this a project you created or one you trust?\n\n \u{276f} No, exit\n   Yes, I trust this folder\n\n Enter to confirm \u{b7} Esc to cancel";
    const STARTED: &str = "\n \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}   Claude Code v2.1.287\n\u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{259b}\u{2580}  Sonnet 5.5 \u{b7} Claude Pro\n\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Try \"edit <filepath> to...\"\n\u{2500}\u{2500}\u{2500}\u{2500}\n  \u{23f5}\u{23f5} auto mode on (shift+tab to cycle)";

    #[test]
    fn standby_screen_is_ready_only_at_an_empty_prompt() {
        assert_eq!(standby_screen(STARTED), StandbyScreen::Ready);
        assert_eq!(standby_screen(READY), StandbyScreen::Ready);
        assert_eq!(standby_screen(TRUST_DIALOG), StandbyScreen::Dialog, "the dialog's own selector is not a prompt");
        assert_eq!(
            standby_screen("Claude Code v2\n\u{2500}\u{2500}\u{2500}\u{2500}\u{276f}\u{a0}Try \"fix typecheck errors\"\n  auto mode on"),
            StandbyScreen::Ready,
            "a rule glued to the prompt by a soft wrap is still the empty box"
        );
        assert_eq!(standby_screen(WORKING), StandbyScreen::NotReady);
        assert_eq!(standby_screen("\u{276f} half typed text\n Claude Code"), StandbyScreen::NotReady);
        assert_eq!(standby_screen(""), StandbyScreen::NotReady);
        assert_eq!(standby_screen("Select login method:\n\u{276f} 1. Claude account"), StandbyScreen::NeedsAttention);
    }

    // Real run results (Claude Code 2.1.299, live run 2026-10-14): the screen's footer rows came along.
    const RESULT_MANUAL: &str = include_str!("fixtures/result_footer_manual_mode_live.txt");

    #[test]
    fn the_footer_and_logo_banner_rows_of_live_screens_are_chrome() {
        // NEW #5: `op.result` ended with these rows.
        let footer = RESULT_MANUAL.lines().last().expect("footer row").to_lowercase();
        assert!(footer.contains("manual mode on") && footer.contains("/effort"), "{footer}");
        assert!(is_chrome_line(&footer));
        assert!(is_chrome_line("⏸ manual mode on · ? for shortcuts"));
        assert!(is_chrome_line("⏵⏵ accept edits on (shift+tab to cycle)"));
        assert!(is_chrome_line("◐ medium · /effort"));
        assert!(is_chrome_line("? for shortcuts"));
        // The banner of a fresh session (live capture, startup_gate_tests::READY).
        assert!(is_chrome_line("▐▛███▜▌   claude code v2.1.299"));
        assert!(is_chrome_line("▝▜██████▛▀  haiku 5.5 · claude pro"));
        assert!(is_chrome_line("▝▝   ▝▝   d:\\pm-demo-untrusted-2"));
        // Replies are not.
        assert!(!is_chrome_line("● npm test passed: the output was \"all tests passed.\""));
        assert!(!is_chrome_line("the effort level is shown after the middle dot"));
        assert!(!is_chrome_line("press ? for shortcuts, they said"), "a sentence that merely mentions it is chrome-adjacent but kept");
    }

    #[test]
    fn status_rows_under_a_reply_are_chrome_but_the_reply_is_not() {
        assert!(is_chrome_line("\u{273b} churned for 16s \u{b7} done 23:03"));
        assert!(is_chrome_line("\u{2190} for agents"));
        assert!(is_chrome_line("\u{b7} \u{2190} for agents"));
        assert!(is_chrome_line("  \u{23f5}\u{23f5} auto mode on (shift+tab to cycle) \u{b7} \u{2190} for agents"));
        assert!(!is_chrome_line("42. the answer to life, the universe and everything"));
        assert!(!is_chrome_line("* for loops are run once \u{b7} done"), "a bullet that merely mentions 'for'");
        assert!(!is_chrome_line("pong"));
    }

    #[test]
    fn the_activity_row_is_work_and_the_summary_row_is_not() {
        assert!(shows_work_in_progress(WORKING));
        assert!(shows_work_in_progress(WORKING_STAR));
        assert!(shows_work_in_progress("  esc to interrupt\n"));
        assert!(!shows_work_in_progress(FINISHED));
        assert!(!shows_work_in_progress(READY));
    }
}
