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

/// The command and directory behind the approval question currently on `screen`.
pub fn extract(screen: &str) -> Option<CommandApproval> {
    let lines: Vec<&str> = screen.lines().map(strip_frame).collect();
    let anchor = lines
        .iter()
        .rposition(|line| line.to_ascii_lowercase().contains(ANCHOR))?;
    let window_start = anchor.saturating_sub(MAX_COMMAND_LINES);
    let start = lines[window_start..anchor]
        .iter()
        .rposition(|line| line.starts_with("$ ") || *line == "$")
        .map(|offset| window_start + offset)?;
    let mut command_lines: Vec<&str> = vec![lines[start].trim_start_matches('$').trim()];
    let mut cwd = None;
    for line in &lines[start + 1..anchor] {
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
        command_lines.push(line);
    }
    let command = command_lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
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
