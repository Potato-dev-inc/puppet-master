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

/// The profile's view of whether the screen shows work in progress (`None` = cannot tell). Built
/// from the same last-12-rows tail the classifier uses.
pub fn busy_marker(agent_type: &str, screen: &str) -> Option<bool> {
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
    crate::tui::profile_for(agent_type).busy_marker(&visible_tail)
}

/// The status to publish for a freshly classified screen. `classify_agent_observation` can only
/// say `running` for a screen that is not a prompt, so every redraw of an idle pane (a resize
/// reprint, a clock tick, typing into the input box) used to flip it to `running` until the idle
/// watcher caught up: the sidebar flashed "Working 0s" on select and the turn tracker counted
/// 0.8s phantom turns. When the profile proves the screen shows no work (`busy == Some(false)`),
/// an idle pane stays idle. Real work shows its marker (spinner, stop hint) and flips it.
pub fn settle_status(previous: PaneStatus, observed: PaneStatus, busy: Option<bool>) -> PaneStatus {
    settle_status_after_resize(previous, observed, busy, false)
}

/// [`settle_status`] plus the generic guard for profiles that cannot read a busy marker: when the
/// pane was resized a moment ago (`pty::resize_clock`), the agent's reprint of its screen is not
/// work either, so an idle pane stays idle unless the profile's own evidence says it is busy
/// (`busy == Some(true)`). Without a recent resize nothing changes.
pub fn settle_status_after_resize(
    previous: PaneStatus,
    observed: PaneStatus,
    busy: Option<bool>,
    recently_resized: bool,
) -> PaneStatus {
    let no_proof_of_work = busy == Some(false) || (recently_resized && busy != Some(true));
    if previous == PaneStatus::Idle && observed == PaneStatus::Running && no_proof_of_work {
        PaneStatus::Idle
    } else {
        observed
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

    // Live bug #9: selecting an idle pane remounted its terminal, the TUI reprinted after the
    // resize, and the redraw read as "Working 0s".
    const IDLE_CLAUDE: &str = include_str!("../tui/claude/fixtures/side_diff_panel_screen.txt");
    const WORKING_CLAUDE: &str = "\u{25cf} Update(sum.js)\n\n\u{273b} Cooking\u{2026} (3s \u{b7} esc to interrupt)\n\n\u{276f} \n  \u{23f8} manual mode on";

    #[test]
    fn a_redraw_of_an_idle_claude_pane_does_not_flip_it_to_running() {
        let claude = "claude";
        // The classifier alone cannot say idle: this is the flip the sidebar showed.
        assert_eq!(classify_agent_observation(claude, IDLE_CLAUDE, ""), PaneStatus::Running);
        let busy = busy_marker(claude, IDLE_CLAUDE);
        assert_eq!(busy, Some(false));
        assert_eq!(settle_status(PaneStatus::Idle, PaneStatus::Running, busy), PaneStatus::Idle);
    }

    #[test]
    fn real_work_still_flips_an_idle_pane_to_running() {
        let busy = busy_marker("claude", WORKING_CLAUDE);
        assert_eq!(busy, Some(true));
        assert_eq!(settle_status(PaneStatus::Idle, PaneStatus::Running, busy), PaneStatus::Running);
        // A profile that cannot tell keeps the old behaviour.
        assert_eq!(settle_status(PaneStatus::Idle, PaneStatus::Running, None), PaneStatus::Running);
        // Prompts and a running pane are never held back.
        assert_eq!(settle_status(PaneStatus::Idle, PaneStatus::WaitingInput, Some(false)), PaneStatus::WaitingInput);
        assert_eq!(settle_status(PaneStatus::Running, PaneStatus::Running, Some(false)), PaneStatus::Running);
    }

    // Live VERIFY #9 (Codex): selecting an idle Codex pane flipped it to running ("Working 0s") for 5 s
    // or more, every time. Captures: Codex 0.159.3 in D:/pm-demo, ready composer / turn running.
    const IDLE_CODEX: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");
    const WORKING_CODEX: &str = include_str!("../tui/codex/fixtures/working_live.txt");

    #[test]
    fn a_redraw_of_an_idle_codex_pane_does_not_flip_it_to_running() {
        let codex = "codex";
        assert_eq!(classify_agent_observation(codex, IDLE_CODEX, ""), PaneStatus::Running, "the classifier alone reads a redraw as work");
        let busy = busy_marker(codex, IDLE_CODEX);
        assert_eq!(busy, Some(false), "Codex shows no working row when idle");
        assert_eq!(settle_status(PaneStatus::Idle, PaneStatus::Running, busy), PaneStatus::Idle);
        // A turn in flight still flips it.
        let working = busy_marker(codex, WORKING_CODEX);
        assert_eq!(working, Some(true));
        assert_eq!(settle_status(PaneStatus::Idle, PaneStatus::Running, working), PaneStatus::Running);
    }

    #[test]
    fn a_resize_redraw_never_flips_an_idle_pane_unless_the_profile_proves_work() {
        use PaneStatus::{Idle, Running, WaitingInput};
        // A profile with no busy marker (None): the old behaviour flipped on any redraw.
        assert_eq!(settle_status_after_resize(Idle, Running, None, false), Running, "no resize: unchanged");
        assert_eq!(settle_status_after_resize(Idle, Running, None, true), Idle, "our own resize redraw");
        // Proof of work wins over the guard, and the guard never holds back other transitions.
        assert_eq!(settle_status_after_resize(Idle, Running, Some(true), true), Running);
        assert_eq!(settle_status_after_resize(Idle, WaitingInput, None, true), WaitingInput);
        assert_eq!(settle_status_after_resize(Running, Running, None, true), Running);
        assert_eq!(settle_status_after_resize(Idle, Running, Some(false), false), Idle);
    }

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
