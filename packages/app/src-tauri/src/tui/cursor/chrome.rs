//! Screen lines that are Cursor TUI furniture, not part of a reply. Inputs are trimmed and
//! lowercased by the caller.

/// Banner, footer, input placeholder or tool-activity row.
pub(super) fn is_chrome_line(lower: &str) -> bool {
    is_status_line(lower)
        || is_header_line(lower)
        || is_tool_line(lower)
        || is_idle_chrome(lower)
        || is_diff_line(lower)
}

/// A row of Cursor's inline diff view. The gutter is a left-block glyph (`▎`) followed by a
/// `+`/`-` marker (or a context row), e.g. `▎+ import os`. Diff text is file content the agent
/// edited, never the reply, so it must not leak into a captured result.
fn is_diff_line(lower: &str) -> bool {
    lower.starts_with(['\u{258d}', '\u{258e}', '\u{258f}'])
}

/// `… truncated (14 more lines)`: Cursor's cut-off notice under a long tool output or diff.
fn is_truncation_notice(lower: &str) -> bool {
    let text = lower.trim_start_matches(['…', '.', ' ']);
    text.starts_with("truncated")
        && (text.contains("more line") || text.contains("lines)") || text.contains("ctrl+o"))
}

/// `Edited todo.py +3 -1` / `Created notes.md +12`: the file-change header above a diff.
fn is_edit_header(lower: &str) -> bool {
    let mut words = lower.split_whitespace();
    if !matches!(
        words.next(),
        Some("edited" | "created" | "deleted" | "wrote" | "updated")
    ) {
        return false;
    }
    let rest: Vec<&str> = words.collect();
    let is_count = |word: &&str| {
        word.strip_prefix(['+', '-'])
            .is_some_and(|n| !n.is_empty() && n.chars().all(|ch| ch.is_ascii_digit()))
    };
    (2..=3).contains(&rest.len())
        && rest[0].chars().any(|ch| matches!(ch, '.' | '/' | '\\'))
        && rest[1..].iter().all(is_count)
}

/// What an idle, untouched Cursor pane shows: the input placeholder (`→ Plan, search, build
/// anything`) and a bare model footer (`Composer 2.5`, before any context usage is printed).
fn is_idle_chrome(lower: &str) -> bool {
    let text = lower.trim_start_matches(['→', '>', ' ']);
    if text.starts_with("plan, search, build anything") {
        return true;
    }
    text.strip_prefix("composer ").is_some_and(|version| {
        !version.is_empty()
            && version.len() <= 8
            && version.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    })
}

/// Cursor's tool-activity rows shown above a reply: the collapsed-output notice
/// (`… 2 output lines hidden · ctrl+o to expand`), a shell header (`$ cd … 11s`) and a one-file
/// read (`Read todo.py`). None of them is part of the reply text.
pub(super) fn is_tool_line(lower: &str) -> bool {
    if lower.contains("ctrl+o to expand")
        || (lower.contains("output line") && lower.contains("hidden"))
        || is_truncation_notice(lower)
        || is_edit_header(lower)
    {
        return true;
    }
    if let Some(command) = lower.strip_prefix("$ ") {
        let last = command.split_whitespace().last().unwrap_or("");
        let is_duration = last.strip_suffix('s').is_some_and(|digits| {
            !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        });
        if is_duration {
            return true;
        }
    }
    lower.strip_prefix("read ").is_some_and(|target| {
        !target.contains(' ')
            && target.chars().any(|ch| matches!(ch, '.' | '/' | '\\'))
            && !target.ends_with('.')
    })
}

/// Cursor's startup banner and footer rows: `Cursor Agent`, `v2026.10.01-e373342`, `Tip: ...`,
/// and the `~\folder` working-directory line. None of them is ever a reply.
fn is_header_line(lower: &str) -> bool {
    let is_version = lower.strip_prefix('v').is_some_and(|rest| {
        rest.len() >= 8
            && rest.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && rest.as_bytes()[4] == b'.'
    });
    lower == "cursor agent"
        || lower.starts_with("cursor agent v")
        || is_version
        || lower.starts_with("tip:")
        || ((lower.starts_with("~\\") || lower.starts_with("~/")) && !lower.contains(' '))
}

/// Footer such as `Composer 2.5 · 7.2%` (model name and context usage).
fn is_status_line(lower: &str) -> bool {
    lower.len() <= 80 && lower.contains(" · ") && lower.ends_with('%')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_placeholder_and_footer_are_chrome() {
        assert!(is_chrome_line("→ plan, search, build anything"));
        assert!(is_chrome_line("composer 2.5"));
        assert!(is_chrome_line("composer 2.5 · 7.2%"));
    }

    #[test]
    fn tool_rows_are_chrome_but_prose_is_not() {
        assert!(is_tool_line("$ cd proj && python -m pytest 11s"));
        assert!(is_tool_line("… 2 output lines hidden · ctrl+o to expand"));
        assert!(is_tool_line("read todo.py"));
        assert!(!is_chrome_line("read the docs first."));
    }

    #[test]
    fn diff_rows_and_truncation_notices_are_chrome() {
        assert!(is_chrome_line("\u{258e}+ import os"));
        assert!(is_chrome_line("\u{258e}- old = 1"));
        assert!(is_chrome_line("\u{258e}  unchanged context"));
        assert!(is_tool_line("\u{2026} truncated (14 more lines)"));
        assert!(is_chrome_line("... truncated (3 more lines)"));
        assert!(is_tool_line("edited todo.py +3 -1"));
        assert!(is_tool_line("created src/notes.md +12"));
        assert!(!is_chrome_line("edited the file to add a feature."));
        assert!(!is_chrome_line("the output was truncated because it was long."));
    }

    #[test]
    fn banner_rows_are_chrome() {
        assert!(is_chrome_line("cursor agent"));
        assert!(is_chrome_line("v2026.10.01-e373342"));
        assert!(is_chrome_line("tip: press ? for help"));
        assert!(is_chrome_line("~\\proj"));
    }
}
