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

/// Cursor Agent screens that block on the user: command approval and workspace trust. These
/// take priority over the "Add follow-up" footer, which can stay visible underneath a dialog.
static CURSOR_BLOCKED_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)run this command\??",
        r"(?i)\brun once\b",
        r"(?i)\brun everything\b",
        r"(?i)\bskip\s*\((esc|n)\b",
        r"(?i)workspace trust required",
        r"(?i)trust this workspace",
        r"(?i)do you trust the contents",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid cursor blocked regex"))
    .collect()
});

static POWERSHELL_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^PS\s+.+>\s*$").expect("powershell prompt regex"));
static CMD_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z]:\\.*>\s*$").expect("cmd prompt regex"));
static BASH_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)[^\s]*[$#]\s*$").expect("bash prompt regex"));

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
    if is_shell_agent(agent_type) {
        // A shell sitting at its prompt is ready for a command, not blocked on a question.
        return if shell_prompt_visible(agent_type, &clean_screen) {
            PaneStatus::Idle
        } else {
            PaneStatus::Running
        };
    }

    if matches!(
        agent_type.to_ascii_lowercase().as_str(),
        "cursor" | "cursor_agent"
    ) {
        if CURSOR_BLOCKED_PATTERNS
            .iter()
            .any(|re| re.is_match(&visible_tail))
        {
            return PaneStatus::WaitingInput;
        }
        if visible_tail.contains("Composing") || visible_tail.contains("Working") {
            return PaneStatus::Running;
        }
        if visible_tail.contains("Add follow-up") {
            return PaneStatus::Idle;
        }
        if looks_like_prompt(&visible_tail) {
            return PaneStatus::WaitingInput;
        }
        return PaneStatus::Running;
    }

    if looks_like_prompt(&visible_tail) {
        PaneStatus::WaitingInput
    } else {
        PaneStatus::Running
    }
}

pub fn is_shell_agent(agent_type: &str) -> bool {
    matches!(
        agent_type.to_ascii_lowercase().as_str(),
        "cmd" | "powershell" | "bash"
    )
}

pub fn shell_prompt_visible(agent_type: &str, screen: &str) -> bool {
    let clean = strip_ansi(screen);
    let line = clean
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.is_empty() {
        return false;
    }
    match agent_type.to_ascii_lowercase().as_str() {
        "powershell" => POWERSHELL_PROMPT.is_match(line),
        "cmd" => CMD_PROMPT.is_match(line),
        "bash" => BASH_PROMPT.is_match(line),
        _ => false,
    }
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

    #[test]
    fn generic_greater_than_is_not_a_blocked_prompt() {
        assert!(!looks_like_prompt("Add follow-up >"));
        assert_eq!(
            classify_agent_observation("cursor_agent", "Add follow-up >", ""),
            PaneStatus::Idle
        );
    }

    #[test]
    fn cursor_working_beats_the_follow_up_footer() {
        assert_eq!(
            classify_agent_observation("cursor_agent", "Working...\n\n→ Add follow-up\n", "",),
            PaneStatus::Running
        );
        assert_eq!(
            classify_agent_observation("cursor_agent", "Composing\n\n→ Add follow-up\n", "",),
            PaneStatus::Running
        );
    }

    #[test]
    fn detects_real_cursor_command_approval() {
        assert_eq!(
            classify_agent_observation("cursor_agent", "Run this command?", ""),
            PaneStatus::WaitingInput
        );
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

    // Built from the option line reported from a live Cursor Agent session, keeping the
    // "Add follow-up" footer underneath to cover the idle-while-blocked regression.
    const CURSOR_APPROVAL_SCREEN: &str = "\
 $ cargo test --lib\n\
\n\
 Run this command?\n\
 Run once (y)\n\
 Add to allowlist (tab)\n\
 Run Everything (shift+tab)\n\
 Skip (esc or n)\n\
\n\
 → Add follow-up\n";

    #[test]
    fn cursor_approval_is_blocked_even_with_the_follow_up_footer_visible() {
        assert_eq!(
            classify_agent_observation("cursor_agent", CURSOR_APPROVAL_SCREEN, ""),
            PaneStatus::WaitingInput
        );
    }

    #[test]
    fn cursor_workspace_trust_screen_is_blocked() {
        // Real capture from a headless cursor-agent run in an untrusted folder.
        let screen = include_str!("fixtures/cursor_workspace_trust.txt");
        assert_eq!(
            classify_agent_observation("cursor_agent", screen, ""),
            PaneStatus::WaitingInput
        );
    }

    /// Drop a real capture at `fixtures/cursor_approval_real.txt` to extend golden coverage;
    /// a no-op while the file is absent.
    #[test]
    fn real_cursor_approval_capture_when_present() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/pty/fixtures/cursor_approval_real.txt"
        );
        if let Ok(screen) = std::fs::read_to_string(path) {
            assert_eq!(
                classify_agent_observation("cursor_agent", &screen, ""),
                PaneStatus::WaitingInput
            );
        }
    }
}
