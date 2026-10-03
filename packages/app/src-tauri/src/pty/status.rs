//! Status heuristics for a pane — looks at recent scrollback to decide if
//! the agent is waiting on input, idle, etc.
//!
//! Only the TUI-independent part lives here. Screens that one TUI draws differently (Cursor's
//! approval dialog, a shell prompt) are classified by that TUI's `crate::tui` profile.

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

/// Strong natural-language prompts that indicate an action is blocked.
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
        r"(?i)run this command\??",
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

/// Classify a fresh pane observation without treating ordinary agent input
/// affordances (`>`, `:`, “Add follow-up”) as approval requests.
pub fn classify_agent_observation(agent_type: &str, screen: &str, _recent: &str) -> PaneStatus {
    let clean_screen = strip_ansi(screen);
    let visible_tail = clean_screen
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let profile = crate::tui::profile_for(agent_type);
    if is_shell_agent(agent_type) {
        // A shell sitting at its prompt is ready for a command, not blocked on a question.
        return if profile.prompt_visible(&clean_screen) {
            PaneStatus::Idle
        } else {
            PaneStatus::Running
        };
    }

    if let Some(status) = profile.classify_screen(&visible_tail) {
        return status;
    }

    if looks_like_prompt(&visible_tail) {
        PaneStatus::WaitingInput
    } else {
        PaneStatus::Running
    }
}

pub fn is_shell_agent(agent_type: &str) -> bool {
    crate::tui::is_shell(agent_type)
}

pub fn shell_prompt_visible(agent_type: &str, screen: &str) -> bool {
    crate::tui::profile_for(agent_type).prompt_visible(screen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_greater_than_is_not_a_blocked_prompt() {
        assert!(!looks_like_prompt("Add follow-up >"));
    }

    #[test]
    fn stale_prompt_in_scrollback_does_not_override_current_screen() {
        assert_eq!(
            classify_agent_observation("codex", "Working on the patch", "Continue?"),
            PaneStatus::Running
        );
    }

    #[test]
    fn shell_prompts_are_agent_specific() {
        assert_eq!(
            classify_agent_observation("powershell", "PS C:\\repo>", ""),
            PaneStatus::Idle
        );
        assert_eq!(
            classify_agent_observation("cmd", "C:\\repo>", ""),
            PaneStatus::Idle
        );
        assert_eq!(
            classify_agent_observation("bash", "user@host:~/repo$", ""),
            PaneStatus::Idle
        );
        assert_eq!(
            classify_agent_observation("bash", "Composing changes...", ""),
            PaneStatus::Running
        );
    }
}
