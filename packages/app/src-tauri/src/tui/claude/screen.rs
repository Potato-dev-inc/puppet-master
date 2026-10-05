//! The interactive Claude Code TUI: what its screens show about whether a turn is running.

use crate::tui::StandbyScreen;

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
    let empty_prompt = screen.lines().any(|line| {
        line.trim()
            .strip_prefix('\u{276f}')
            .is_some_and(|rest| rest.trim().is_empty() || rest.trim().starts_with("Try \""))
    });
    if empty_prompt && screen.contains("Claude Code") {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
}

/// Status rows Claude draws under a reply: the finished-turn summary (`<glyph> Churned for 16s \u{b7}
/// done 23:03`), the agents hint and the permission-mode footer. `lower` is the lowercased row.
pub(super) fn is_chrome_line(lower: &str) -> bool {
    let row = lower.trim();
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
        assert_eq!(standby_screen(WORKING), StandbyScreen::NotReady);
        assert_eq!(standby_screen("\u{276f} half typed text\n Claude Code"), StandbyScreen::NotReady);
        assert_eq!(standby_screen(""), StandbyScreen::NotReady);
        assert_eq!(standby_screen("Select login method:\n\u{276f} 1. Claude account"), StandbyScreen::NeedsAttention);
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
