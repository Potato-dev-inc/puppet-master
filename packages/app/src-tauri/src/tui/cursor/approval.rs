//! Reading Cursor's command-approval screen: which command is being approved, and where.
//!
//! The dialog draws the command above the question:
//!
//! ```text
//!  $ cd "C:\proj" && python todo.py add "buy milk"
//!
//!  Run this command?
//!  Run once (y)
//!  Skip (esc or n)
//! ```
//!
//! Long commands wrap onto continuation lines directly under the `$` line. The extracted text is
//! shown to callers and judged by the run's `allowed_commands`, so anything ambiguous yields
//! `None` rather than a guess.

const ANCHOR: &str = "run this command";
/// How far above the question the command line may start.
const MAX_COMMAND_LINES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandApproval {
    /// The command line exactly as drawn (continuation lines joined with newlines).
    pub command: String,
    /// Working directory when the screen shows one.
    pub cwd: Option<String>,
}

fn strip_frame(line: &str) -> &str {
    line.trim()
        .trim_matches(|ch| matches!(ch, '│' | '┃' | '|' | '║'))
        .trim()
}

fn cwd_from_label(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    for label in ["working directory:", "directory:", "cwd:", "run in ", "in directory "] {
        if lower.starts_with(label) {
            let value = line[label.len()..].trim().trim_matches(|c| c == '"' || c == '`');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// A row wrapped by the terminal fills the pane's width (the widest row on screen, usually the
/// input box border); prose that Cursor word-wraps stops a few columns short.
const WRAP_SLACK: usize = 2;
/// Rows shorter than this were not cut by the terminal.
const WRAP_MIN_ROW: usize = 60;

fn row_width(line: &str) -> usize {
    line.trim_end()
        .trim_end_matches(|ch| matches!(ch, '│' | '┃' | '|' | '║'))
        .trim_end()
        .chars()
        .count()
}

/// The command and directory behind the approval question currently on `screen`.
///
/// A command longer than the pane is hard-wrapped by the terminal, continuing on the next row
/// (indented past the `$`). Such a break can fall inside a quoted path, so rows cut at the full pane
/// width are glued back together without a separator; any other line break stays a newline, which
/// the approval policy refuses.
pub fn extract(screen: &str) -> Option<CommandApproval> {
    let widths: Vec<usize> = screen.lines().map(row_width).collect();
    let pane_width = widths.iter().copied().max().unwrap_or(0);
    let cut_by_terminal =
        |row: usize| widths[row] >= WRAP_MIN_ROW && widths[row] + WRAP_SLACK >= pane_width;
    let lines: Vec<&str> = screen.lines().map(strip_frame).collect();
    let anchor = lines
        .iter()
        .rposition(|line| line.to_ascii_lowercase().contains(ANCHOR))?;
    let window_start = anchor.saturating_sub(MAX_COMMAND_LINES);
    let start = lines[window_start..anchor]
        .iter()
        .rposition(|line| line.starts_with("$ ") || *line == "$")
        .map(|offset| window_start + offset)?;
    let mut command_lines: Vec<(usize, &str)> =
        vec![(start, lines[start].trim_start_matches('$').trim())];
    let mut cwd = None;
    for (offset, line) in lines[start + 1..anchor].iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        if let Some(found) = cwd_from_label(line) {
            cwd = Some(found);
            continue;
        }
        if line.to_ascii_lowercase().starts_with("not in allowlist") {
            continue;
        }
        command_lines.push((start + 1 + offset, line));
    }
    let mut command = String::new();
    let mut previous_row: Option<usize> = None;
    for (row, text) in command_lines.into_iter().filter(|(_, text)| !text.is_empty()) {
        if let Some(previous) = previous_row {
            if !(row == previous + 1 && cut_by_terminal(previous)) {
                command.push('\n');
            }
        }
        command.push_str(text);
        previous_row = Some(row);
    }
    if command.is_empty() {
        return None;
    }
    Some(CommandApproval { command, cwd })
}

/// The option lines are on screen, so a keypress will land on the dialog rather than on a screen
/// that is still being drawn.
pub fn fully_drawn(screen: &str) -> bool {
    let lower = screen.to_ascii_lowercase();
    lower.contains("run once") || lower.contains("skip (esc")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(command_block: &str) -> String {
        format!(
            "earlier reply text\n\n{command_block}\n\n Run this command?\n Run once (y)\n Add to allowlist (tab)\n Skip (esc or n)\n\n → Add follow-up\n"
        )
    }

    #[test]
    fn simple_command() {
        let found = extract(&screen(" $ cargo test --lib")).unwrap();
        assert_eq!(found.command, "cargo test --lib");
        assert_eq!(found.cwd, None);
    }

    #[test]
    fn cd_prefix_gives_the_cwd_and_keeps_the_exact_command() {
        let found = extract(&screen(r#" $ cd "C:\Users\me\proj" && python todo.py add "buy milk""#)).unwrap();
        assert_eq!(found.command, r#"cd "C:\Users\me\proj" && python todo.py add "buy milk""#);
        assert_eq!(found.cwd, None, "a cd prefix is interpreted by the caller");
    }

    #[test]
    fn multi_line_command_is_joined() {
        let found = extract(&screen(
            " $ python -c \"print(1)\" &&\n   python todo.py list",
        ))
        .unwrap();
        assert_eq!(found.command, "python -c \"print(1)\" &&\npython todo.py list");
    }

    /// The dialog as the terminal draws a command wider than the pane: the first row is cut at the
    /// pane width (119 of 120 columns, the border's width), the rest continues indented past the `$`.
    fn wrapped_dialog(command: &str) -> String {
        const PANE: usize = 120;
        let first_prefix = "  $ ";
        let room = PANE - 1 - first_prefix.chars().count();
        let chars: Vec<char> = command.chars().collect();
        let cut = room.min(chars.len());
        let mut rows = vec![format!("{first_prefix}{}", chars[..cut].iter().collect::<String>())];
        let mut rest = &chars[cut..];
        while !rest.is_empty() {
            let take = (PANE - 1 - 4).min(rest.len());
            rows.push(format!("    {}", rest[..take].iter().collect::<String>()));
            rest = &rest[take..];
        }
        format!(
            "  Cursor Agent\n\n{}\n{}\n  Run this command?\n  Not in allowlist: cd, python\n  \u{2192} Run (once) (y)\n    Skip (esc or n)\n",
            "\u{2584}".repeat(PANE),
            rows.join("\n")
        )
    }

    const PROJECT: &str = r"C:\Users\you-pc\AppData\Local\Temp\claude\C--Users-you-pc-Desktop-work2-puppet-master\7f91bd8b-8ce7-4122-9d68-3ea20af2faed\scratchpad\verify5-1";

    #[test]
    fn a_command_wrapped_at_the_pane_width_is_joined_without_a_separator() {
        let command = format!(r#"cd "{PROJECT}" && python todo.py add buy-milk && python todo.py list"#);
        let screen = wrapped_dialog(&command);
        assert!(screen.lines().nth(3).unwrap().chars().count() == 119, "fixture is wrapped at the pane width");
        assert!(screen.lines().nth(4).unwrap().starts_with("    "), "continuation row");
        assert_eq!(extract(&screen).unwrap().command, command);
    }

    #[test]
    fn a_wrapped_command_is_judged_by_the_allow_list_not_refused_for_its_newline() {
        use crate::agent_runs::approval_policy::approved_command;
        let allow = vec!["python todo.py add *".to_string(), "python todo.py list".to_string()];
        let judged = |command: &str, allow: &[String]| {
            let found = extract(&wrapped_dialog(command)).unwrap();
            approved_command(&found.command, allow, Some(PROJECT))
        };
        let wanted = format!(r#"cd "{PROJECT}" && python todo.py add buy-milk && python todo.py list"#);
        assert!(judged(&wanted, &allow).is_some());
        // Same wrapping, but a command nobody allowed, or a directory outside the project.
        let other_program = format!(r#"cd "{PROJECT}" && python todo.py add buy-milk && python evil.py"#);
        assert!(judged(&other_program, &allow).is_none());
        let outside = r#"cd "C:\Windows\System32\some\very\long\directory\name\that\wraps\the\pane\width\for\sure\yes\it\does" && python todo.py add x"#;
        assert!(judged(outside, &allow).is_none());
        let smuggled = format!(r#"cd "{PROJECT}" && python todo.py add buy-milk && python todo.py list; rm x"#);
        assert!(judged(&smuggled, &allow).is_none());
        assert!(judged(&wanted, &[]).is_none());
    }

    #[test]
    fn a_line_break_short_of_the_pane_width_is_still_a_newline() {
        let border = "\u{2584}".repeat(120);
        let short = format!("  Cursor Agent\n\n{border}\n  $ python todo.py add a &&\n    python evil.py\n  Run this command?\n  Run once (y)\n");
        assert_eq!(extract(&short).unwrap().command, "python todo.py add a &&\npython evil.py");
        // A row cut early at a word (prose-style wrapping, ~100 columns) must not be glued either.
        let early = format!(
            "{border}\n  $ python -c \"{}\" &&\n    python todo.py list\n  Run this command?\n  Run once (y)\n",
            "x ".repeat(48)
        );
        assert!(extract(&early).unwrap().command.contains('\n'));
    }

    #[test]
    fn boxed_dialog_and_labelled_cwd() {
        let boxed = "│ $ npm test │\n│ Working directory: C:\\proj │\n\n │ Run this command? │\n│ Run once (y) │\n";
        let found = extract(boxed).unwrap();
        assert_eq!(found.command, "npm test");
        assert_eq!(found.cwd.as_deref(), Some("C:\\proj"));
    }

    #[test]
    fn last_dialog_wins_over_earlier_scrollback() {
        let both = format!(
            " $ rm old\n Run this command?\n Run once (y)\n\n{}",
            screen(" $ cargo build")
        );
        assert_eq!(extract(&both).unwrap().command, "cargo build");
    }

    #[test]
    fn no_command_line_means_none() {
        assert!(extract("Run this command?\n Run once (y)\n").is_none());
        assert!(extract(" $ ls\n").is_none());
    }

    #[test]
    fn drawn_state() {
        assert!(fully_drawn(&screen(" $ ls")));
        assert!(!fully_drawn(" $ ls\n\n Run this command?\n"));
    }
}
