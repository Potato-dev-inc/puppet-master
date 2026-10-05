//! The interactive Claude Code TUI: what its screens show about whether a turn is running.

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
