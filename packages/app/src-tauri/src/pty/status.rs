//! Status heuristics for a pane — looks at recent scrollback to decide if
//! the agent is waiting on input, idle, etc.

use super::ansi::strip_ansi;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneStatus {
    Running,
    WaitingInput,
    Idle,
    Error,
}

impl PaneStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaneStatus::Running => "running",
            PaneStatus::WaitingInput => "waiting_input",
            PaneStatus::Idle => "idle",
            PaneStatus::Error => "error",
        }
    }
}

/// Patterns that look like an interactive prompt asking the user something.
static PROMPT_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)\b(y/n)\b",
        r"(?i)\b(yes/no)\b",
        r"\(Y/n\)",
        r"\(y/N\)",
        r"(?i)press enter",
        r"(?i)continue\?",
        r"(?i)are you sure",
        r"(?i)\bproceed\?\s*$",
        r"(?i)what would you like",
        r"(?i)how can i help",
        r"(?i)allow once",
        r"(?i)allow always",
        r"(?i)don't allow",
        r"(?i)do not allow",
        r"(?i)\bdeny\b",
        r"(?i)\bapprove\b",
        r"(?i)enter submit",
        r"(?i)esc dismiss",
        r"(?i)↑↓ select",
        r"(?i)\d+\.\s*yes\b",
        r"(?i)\d+\.\s*no\b",
        r"(?i)type your own answer",
        r"[>?›»]\s*$",
        r":\s*$",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid prompt regex"))
    .collect()
});

/// Returns true if the recent scrollback (typically last 1–3 lines) looks
/// like the agent is waiting on user input.
pub fn looks_like_prompt(text: &str) -> bool {
    let clean = strip_ansi(text);
    // Look at the last ~5 non-empty lines.
    let tail: String = clean
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    PROMPT_PATTERNS.iter().any(|re| re.is_match(&tail))
}

/// OpenCode TUI menus (numbered yes/no, submit hints) — used by wait `settled`.
static OPENCODE_TUI_MENU_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)enter submit",
        r"(?i)esc dismiss",
        r"(?i)↑↓ select",
        r"(?i)\d+\.\s*yes\b",
        r"(?i)\d+\.\s*no\b",
        r"(?i)type your own answer",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid opencode tui regex"))
    .collect()
});

/// OpenCode TUI menus (numbered yes/no, submit hints) — used by wait `settled`.
pub fn looks_like_opencode_tui_menu(text: &str) -> bool {
    let clean = strip_ansi(text);
    let tail: String = clean
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    OPENCODE_TUI_MENU_PATTERNS
        .iter()
        .any(|re| re.is_match(&tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_opencode_numbered_yes_no_menu() {
        let text = "1. Yes\n2. No\n3. Type your own answer\n↑↓ select  enter submit  esc dismiss";
        assert!(looks_like_opencode_tui_menu(text));
    }
}
