//! The interactive Cursor Agent TUI: what its screens mean and how to answer them.

use crate::pty::status::{looks_like_prompt, PaneStatus};
use crate::tui::{generic, Reply};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

/// Screens that block on the user: command approval and workspace trust. These take priority
/// over the "Add follow-up" footer, which can stay visible underneath a dialog.
static BLOCKED_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
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

/// Status from the last visible lines. Order matters: a dialog beats the working indicator, which
/// beats the idle footer.
pub(super) fn classify(visible_tail: &str) -> PaneStatus {
    if BLOCKED_PATTERNS.iter().any(|re| re.is_match(visible_tail)) {
        return PaneStatus::WaitingInput;
    }
    if visible_tail.contains("Composing") || visible_tail.contains("Working") {
        return PaneStatus::Running;
    }
    if visible_tail.contains("Add follow-up") {
        return PaneStatus::Idle;
    }
    if looks_like_prompt(visible_tail) {
        return PaneStatus::WaitingInput;
    }
    PaneStatus::Running
}

/// Narrow on purpose: only Cursor's trust and command-approval dialogs. There is no broad
/// "allow all" prompt, so `allow_broad` is ignored.
pub(super) fn detect_prompt(agent_type: &str, screen: &str, _allow_broad: bool) -> Option<Value> {
    let lower = screen.to_ascii_lowercase();
    let (kind, choices) =
        if lower.contains("trust this workspace") || lower.contains("trust the authors") {
            ("workspace_trust", vec!["trust".to_string(), "deny".to_string()])
        } else if lower.contains("allow once") || lower.contains("run this command") {
            (
                "command_approval",
                vec!["allow_once".to_string(), "deny".to_string()],
            )
        } else {
            return None;
        };
    Some(generic::prompt_json(agent_type, kind, screen, choices))
}

pub(super) fn prompt_reply(
    prompt: &Value,
    choice: &str,
    allow_broad: bool,
) -> Result<Reply, String> {
    if prompt.get("kind").and_then(Value::as_str) == Some("workspace_trust") {
        return match choice {
            "trust" => Ok(Reply::key("y")),
            "deny" => Ok(Reply::key("n")),
            _ => Err("unsupported workspace trust choice".into()),
        };
    }
    generic::prompt_reply(prompt, choice, allow_broad)
}

/// Cursor's TUI footer reads `Composer 2.5 · 7.2%` (model, then context usage); the last such row
/// on screen names the live model.
pub(super) fn footer_model(buffer: &str) -> Option<String> {
    buffer.lines().rev().find_map(|line| {
        let (model, usage) = line.trim().rsplit_once(" · ")?;
        let digits = usage.strip_suffix('%')?;
        let model = model.trim();
        let plausible = !digits.is_empty()
            && digits.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            && !model.is_empty()
            && model.len() <= 40
            && model.chars().next().is_some_and(char::is_alphabetic)
            && model
                .chars()
                .all(|ch| ch.is_alphanumeric() || matches!(ch, ' ' | '.' | '-' | '_' | '/' | ':'));
        plausible.then(|| model.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::status::classify_agent_observation;
    use crate::tui::profile_for;

    // Built from the option line reported from a live Cursor Agent session, keeping the
    // "Add follow-up" footer underneath to cover the idle-while-blocked regression.
    const APPROVAL_SCREEN: &str = "\
 $ cargo test --lib\n\
\n\
 Run this command?\n\
 Run once (y)\n\
 Add to allowlist (tab)\n\
 Run Everything (shift+tab)\n\
 Skip (esc or n)\n\
\n\
 → Add follow-up\n";

    fn status(screen: &str) -> PaneStatus {
        classify_agent_observation("cursor_agent", screen, "")
    }

    #[test]
    fn follow_up_footer_is_idle_not_a_prompt() {
        assert_eq!(status("Add follow-up >"), PaneStatus::Idle);
    }

    #[test]
    fn working_beats_the_follow_up_footer() {
        assert_eq!(
            status("Working...\n\n→ Add follow-up\n"),
            PaneStatus::Running
        );
        assert_eq!(
            status("Composing\n\n→ Add follow-up\n"),
            PaneStatus::Running
        );
    }

    #[test]
    fn command_approval_is_waiting_input() {
        assert_eq!(status("Run this command?"), PaneStatus::WaitingInput);
    }

    #[test]
    fn approval_is_blocked_even_with_the_follow_up_footer_visible() {
        assert_eq!(status(APPROVAL_SCREEN), PaneStatus::WaitingInput);
    }

    #[test]
    fn workspace_trust_screen_is_blocked() {
        // Real capture from a headless cursor-agent run in an untrusted folder.
        let screen = include_str!("fixtures/workspace_trust_screen.txt");
        assert_eq!(status(screen), PaneStatus::WaitingInput);
    }

    /// Drop a real capture at `fixtures/approval_real.txt` to extend golden coverage; a no-op
    /// while the file is absent.
    #[test]
    fn real_approval_capture_when_present() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/tui/cursor/fixtures/approval_real.txt"
        );
        if let Ok(screen) = std::fs::read_to_string(path) {
            assert_eq!(status(&screen), PaneStatus::WaitingInput);
        }
    }

    #[test]
    fn the_ide_launcher_shares_the_screen_heuristics() {
        assert_eq!(
            classify_agent_observation("cursor", APPROVAL_SCREEN, ""),
            PaneStatus::WaitingInput
        );
    }

    #[test]
    fn prompts_are_narrow_and_trust_has_its_own_choices() {
        let cursor = profile_for("cursor_agent");
        let prompt = cursor.detect_prompt("Trust this workspace?", false).unwrap();
        assert_eq!(prompt["kind"], "workspace_trust");
        assert!(cursor.detect_prompt("Are you sure?", true).is_none());
        assert_eq!(cursor.prompt_reply(&prompt, "trust", false).unwrap(), Reply::key("y"));
        assert_eq!(cursor.prompt_reply(&prompt, "deny", false).unwrap(), Reply::key("n"));
        assert!(cursor.prompt_reply(&prompt, "allow_once", false).is_err());
    }

    #[test]
    fn broad_approval_is_never_offered() {
        let cursor = profile_for("cursor_agent");
        assert!(cursor.detect_prompt("Always allow this command?", true).is_none());
    }

    #[test]
    fn footer_model_is_read_from_the_last_usage_row() {
        assert_eq!(
            footer_model("reply text\nComposer 2.5 · 7.2%\n").as_deref(),
            Some("Composer 2.5")
        );
        assert!(footer_model("done · fast\nabout 50%").is_none());
    }
}
