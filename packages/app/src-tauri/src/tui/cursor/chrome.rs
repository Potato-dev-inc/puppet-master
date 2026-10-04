//! Screen lines that are Cursor TUI furniture, not part of a reply. Inputs are trimmed and
//! lowercased by the caller.

/// Text that only Cursor's modal dialogs (workspace trust, command approval) print.
const DIALOG_MARKERS: &[&str] = &[
    "workspace trust required",
    "do you trust the contents",
    "trust this workspace",
    "trusting workspace",
    "run this command",
    "add to allowlist",
    "run everything",
    "skip (esc or n)",
];

/// Drop every boxed dialog (a `╭…╰` block containing dialog text, or a loose line of dialog text)
/// from screen text. A dialog is never part of a reply, and after it was answered its box stays in
/// the scrollback with the TUI redrawn below. Other boxed content (a table in a reply) is kept.
pub(super) fn strip_dialog_boxes(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = vec![true; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if trimmed.starts_with('╭') {
            if let Some(rel) = lines[index..].iter().position(|l| l.trim_start().starts_with('╰')) {
                let end = index + rel;
                let is_dialog = lines[index..=end].iter().any(|l| {
                    let lower = l.to_ascii_lowercase();
                    DIALOG_MARKERS.iter().any(|m| lower.contains(m))
                });
                if is_dialog {
                    keep[index..=end].iter_mut().for_each(|k| *k = false);
                }
                index = end + 1;
                continue;
            }
        }
        // The same dialog with its top/bottom border rows already removed (screen text that went
        // through box-art compaction): a run of `│` rows that contains dialog text.
        if trimmed.starts_with('│') {
            let end = lines[index..]
                .iter()
                .position(|l| !l.trim_start().starts_with('│'))
                .map_or(lines.len(), |rel| index + rel);
            let is_dialog = lines[index..end].iter().any(|l| {
                let lower = l.to_ascii_lowercase();
                DIALOG_MARKERS.iter().any(|m| lower.contains(m))
            });
            if is_dialog {
                keep[index..end].iter_mut().for_each(|k| *k = false);
            }
            index = end;
            continue;
        }
        index += 1;
    }
    lines
        .iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(line, _)| *line)
        .collect::<Vec<_>>()
        .join("
")
}

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
    // Every `$ <command>` row is Cursor's header for a shell call (its duration may be on the
    // same row or on the next one), and a bare `6.0s` is that duration.
    if lower.starts_with("$ ") || is_duration(lower) {
        return true;
    }
    lower.strip_prefix("read ").is_some_and(|target| {
        !target.contains(' ')
            && target.chars().any(|ch| matches!(ch, '.' | '/' | '\\'))
            && !target.ends_with('.')
    })
}

/// `6.0s`, `14s`, `1m 3s`: how long a tool call took, printed under its header.
fn is_duration(lower: &str) -> bool {
    let text = lower.trim();
    !text.is_empty()
        && text.len() <= 9
        && text.ends_with('s')
        && text.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | 's' | 'm' | ' '))
        && text.chars().any(|ch| ch.is_ascii_digit())
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

/// Footer such as `Composer 2.5 · 7.2%` or `Composer 2.5 · 12% · 1 file edited` (model name, then
/// context usage and a change summary). A reply that only mentions the model, or uses ` · ` in a
/// sentence, does not match: every part after the model name must itself be a status figure.
fn is_status_line(lower: &str) -> bool {
    if lower.len() <= 80 && lower.contains(" · ") && lower.ends_with('%') {
        return true;
    }
    if lower.len() > 120 {
        return false;
    }
    let mut parts = lower.split(" · ").map(str::trim);
    let (Some(model), rest) = (parts.next(), parts.collect::<Vec<_>>()) else {
        return false;
    };
    let model_like = !model.is_empty()
        && model.len() <= 30
        && model.chars().next().is_some_and(char::is_alphabetic)
        && !model.contains([',', ';', ':', '!', '?'])
        && !model.ends_with('.');
    model_like && !rest.is_empty() && rest.iter().all(|part| is_status_figure(part))
}

/// `7.2%`, `1 file edited`, `3 files changed`, `+14 -3`: one footer figure.
fn is_status_figure(part: &str) -> bool {
    if part.ends_with('%') {
        return part[..part.len() - 1]
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.')
            && part.len() > 1;
    }
    let words: Vec<&str> = part.split_whitespace().collect();
    let is_count = |word: &str| word.chars().all(|ch| ch.is_ascii_digit()) && !word.is_empty();
    let is_delta = |word: &str| {
        word.strip_prefix(['+', '-'])
            .is_some_and(|n| is_count(n))
    };
    if !words.is_empty() && words.iter().all(|word| is_delta(word)) {
        return true;
    }
    matches!(
        words.as_slice(),
        [n, "file" | "files", "edited" | "changed" | "created" | "modified" | "deleted"] if is_count(n)
    )
}

/// Drop later copies of a long line. Cursor redraws its commentary (the "I will now ..." lines
/// above a tool call) as it streams, so the same sentence can sit on screen more than once;
/// short lines are data (`ok`, `[]`) and may repeat on purpose.
pub(super) fn tidy_reply(text: &str) -> String {
    const LONG_LINE: usize = 40;
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let key = line.trim();
        if key.chars().count() >= LONG_LINE {
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
        }
        out.push(line);
    }
    out.join("\n")
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

    const TRUST_RESOLVED: &str = include_str!("fixtures/workspace_trust_visible_resolved.txt");

    #[test]
    fn a_boxed_trust_dialog_is_stripped_but_other_boxes_survive() {
        let stripped = strip_dialog_boxes(TRUST_RESOLVED);
        assert!(!stripped.contains("Workspace Trust Required"), "{stripped}");
        assert!(!stripped.contains("Trust this workspace"), "{stripped}");
        // The dialog's own directory row goes; the `~\...` footer outside it is ordinary chrome.
        assert!(!stripped.contains(r"C:\Users\Ren-pc\AppData\Local\Temp\cur-_b1a_ip3"));
        assert!(stripped.contains("Plan, search, build anything"), "TUI below the dialog stays");
        let table = "\u{256d}\u{2500}\u{2500}\u{256e}\n\u{2502} a \u{2502} b \u{2502}\n\u{2570}\u{2500}\u{2500}\u{256f}";
        assert_eq!(strip_dialog_boxes(table), table);
    }

    #[test]
    fn a_trust_dialog_without_its_border_rows_is_stripped_too() {
        let borderless: String = TRUST_RESOLVED
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with('╭') && !t.starts_with('╰')
            })
            .collect::<Vec<_>>()
            .join("
");
        let stripped = strip_dialog_boxes(&borderless);
        assert!(!stripped.contains("Workspace Trust Required"), "{stripped}");
        assert!(!stripped.contains("Cursor Agent can execute"), "{stripped}");
        assert!(stripped.contains("Plan, search, build anything"));
    }

    #[test]
    fn shell_call_headers_and_durations_are_tool_rows() {
        assert!(is_tool_line("$ cd \"c:\\proj\" && python todo.py add buy-milk && python todo.py list"));
        assert!(is_tool_line("6.0s"));
        assert!(is_tool_line("1m 3s"));
        assert!(!is_tool_line("2 seconds later it finished"));
        assert!(!is_tool_line("done"));
    }

    #[test]
    fn banner_rows_are_chrome() {
        assert!(is_chrome_line("cursor agent"));
        assert!(is_chrome_line("v2026.10.01-e373342"));
        assert!(is_chrome_line("tip: press ? for help"));
        assert!(is_chrome_line("~\\proj"));
    }
}
