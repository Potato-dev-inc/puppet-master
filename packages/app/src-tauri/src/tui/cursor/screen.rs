//! The interactive Cursor Agent TUI: what its screens mean and how to answer them.

use crate::pty::status::{looks_like_prompt, PaneStatus};
use crate::tui::{generic, Reply, StandbyScreen};
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
        r"(?i)workspace\W+trust\W+required",
        r"(?i)trust\W+this\W+workspace",
        r"(?i)do\W+you\W+trust\W+the\W+contents",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid cursor blocked regex"))
    .collect()
});

/// The workspace-trust dialog stays in the scrollback after it was answered; the TUI then prints
/// its banner and input prompt BELOW it. Drops everything up to and including a trust dialog that
/// something below shows has already been dealt with, so it is not read as a live prompt.
/// (Real capture: tests/fixtures workspace_trust_visible_resolved.txt.) Command-approval dialogs
/// keep their footer underneath while live, so only the trust dialog gets this treatment.
fn without_resolved_trust_dialog(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().collect();
    let is_trust = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.contains("trust this")
            || lower.contains("workspace trust")
            || lower.contains("do you trust the")
            || lower.contains("trusting workspace")
    };
    let Some(last_trust) = lines.iter().rposition(|line| is_trust(line)) else {
        return screen.to_string();
    };
    let progressed = lines[last_trust + 1..].iter().any(|line| {
        let lower = line.trim().trim_start_matches(['→', '>', ' ']).to_ascii_lowercase();
        lower.starts_with("plan, search, build anything")
            || lower.starts_with("add follow-up")
            || lower.starts_with("add a follow-up")
            || lower == "cursor agent"
    });
    if progressed || lines[last_trust].to_ascii_lowercase().contains("trusting workspace") {
        lines[last_trust + 1..].join("
")
    } else {
        screen.to_string()
    }
}

/// Status from the last visible lines. Order matters: a dialog beats the working indicator, which
/// beats the idle footer.
pub(super) fn classify(visible_tail: &str) -> PaneStatus {
    let live = without_resolved_trust_dialog(visible_tail);
    let visible_tail = live.as_str();
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

/// Whether the visible rows show Cursor working: its braille spinner (any label: Composing,
/// Running, Thinking), the "ctrl+c to stop" hint that stays on the input row for the whole turn,
/// or the workspace-trust handshake that is still redrawing into the input box.
pub(super) fn shows_work_in_progress(visible_tail: &str) -> bool {
    let lower = visible_tail.to_ascii_lowercase();
    visible_tail.chars().any(|ch| ('\u{2800}'..='\u{28ff}').contains(&ch))
        || lower.contains("ctrl+c to stop")
        || trust_handshake_in_progress(visible_tail)
}

/// "Trusting workspace..." with nothing of the real UI below it yet. Once Cursor has drawn its
/// banner or input row underneath, the old dialog is only scrollback (it stays within the visible
/// rows for the rest of a cold run) and says nothing about the current state.
fn trust_handshake_in_progress(visible_tail: &str) -> bool {
    let lines: Vec<&str> = visible_tail.lines().collect();
    let Some(last) = lines
        .iter()
        .rposition(|line| line.to_ascii_lowercase().contains("trusting workspace"))
    else {
        return false;
    };
    !lines[last + 1..].iter().any(|line| {
        let lower = line.trim().trim_start_matches(['\u{2192}', '>', ' ']).to_ascii_lowercase();
        lower.starts_with("plan, search, build anything")
            || lower.starts_with("add follow-up")
            || lower.starts_with("add a follow-up")
            || lower == "cursor agent"
    })
}

/// Phrases of a login, update or first-run screen: the TUI is waiting for the user, not for a task.
const ATTENTION_PHRASES: [&str; 7] = [
    "log in to cursor",
    "please log in",
    "not logged in",
    "sign in to",
    "update available",
    "new version available",
    "press enter to update",
];

/// Whether a pre-launched pane is at an empty input box a task can be typed into. A dialog (trust,
/// command approval) is reported as such and never answered by the standby pool: whether a folder
/// is trusted is the caller's decision.
pub(super) fn standby_screen(agent_type: &str, screen: &str) -> StandbyScreen {
    if detect_prompt(agent_type, screen, false).is_some() {
        return StandbyScreen::Dialog;
    }
    let live = without_resolved_trust_dialog(screen);
    if BLOCKED_PATTERNS.iter().any(|re| re.is_match(&live)) {
        return StandbyScreen::Dialog;
    }
    let lower = live.to_ascii_lowercase();
    if ATTENTION_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::NeedsAttention;
    }
    if shows_work_in_progress(&live) {
        return StandbyScreen::NotReady;
    }
    let empty_box = live.lines().any(|line| {
        let row = line.trim().trim_start_matches(['\u{2192}', '>', ' ']).to_ascii_lowercase();
        row.starts_with("plan, search, build anything")
            || row == "add a follow-up"
            || row == "add follow-up"
    });
    if empty_box {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
}

/// Non-empty rows from the bottom that may hold the input box: its top border, the arrow row, its
/// bottom border, and the footer (model, usage, folder) under it.
const INPUT_BOX_TAIL_ROWS: usize = 10;

/// Cursor's input box read POSITIVELY (`TuiProfile::ready_recogniser`): an arrow row framed by a row
/// of upper half blocks above and lower half blocks below, near the bottom of the screen. The spinner
/// or the stop hint = `Working`; the placeholder (`Plan, search, build anything` on a fresh session,
/// `Add a follow-up` after a turn) = `Empty`; other text = `HasText` (Ctrl+U clears it). No box, or a
/// dialog drawn under it, = `Unknown`. Real captures: `fixtures/reply_*_screen.txt`,
/// `workspace_trust_visible_resolved.txt`.
pub(super) fn input_box(screen: &str) -> crate::tui::ComposerState {
    use crate::tui::ComposerState;
    let live = without_resolved_trust_dialog(screen);
    let rows: Vec<&str> = live.lines().map(str::trim_end).filter(|row| !row.trim().is_empty()).collect();
    let border = |row: &str, glyph: char| {
        let row = row.trim();
        row.chars().count() >= 8 && row.chars().all(|c| c == glyph)
    };
    let first = rows.len().saturating_sub(INPUT_BOX_TAIL_ROWS).max(1);
    let Some(at) = (first..rows.len().saturating_sub(1)).rev().find(|&at| {
        rows[at].trim_start().starts_with('\u{2192}')
            && border(rows[at - 1], '\u{2584}')
            && border(rows[at + 1], '\u{2580}')
    }) else {
        return ComposerState::Unknown;
    };
    let under = rows[at + 1..].join("\n");
    if BLOCKED_PATTERNS.iter().any(|re| re.is_match(&under)) {
        return ComposerState::Unknown;
    }
    if shows_work_in_progress(&live) {
        return ComposerState::Working;
    }
    let text = rows[at].trim().trim_start_matches('\u{2192}').trim().to_ascii_lowercase();
    if text.is_empty()
        || text.starts_with("plan, search, build anything")
        || text == "add a follow-up"
        || text == "add follow-up"
    {
        ComposerState::Empty
    } else {
        ComposerState::HasText
    }
}

/// Narrow on purpose: only Cursor's trust and command-approval dialogs. There is no broad
/// "allow all" prompt, so `allow_broad` is ignored.
pub(super) fn detect_prompt(agent_type: &str, screen: &str, _allow_broad: bool) -> Option<Value> {
    let live = without_resolved_trust_dialog(screen);
    // A narrow pane wraps the dialog mid-phrase: match on the text with wrapping and box art gone.
    let lower = live
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
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
    let mut prompt = generic::prompt_json(agent_type, kind, screen, choices);
    if kind == "workspace_trust" {
        // One trust dialog per pane at a time; its text re-wraps when the layout changes.
        prompt["identity"] = Value::String("workspace_trust".into());
    }
    if kind == "command_approval" {
        if let Some(found) = super::approval::extract(screen) {
            prompt["command"] = Value::String(found.command);
            if let Some(cwd) = found.cwd {
                prompt["cwd"] = Value::String(cwd);
            }
        }
        prompt["fully_drawn"] = Value::Bool(super::approval::fully_drawn(screen));
    }
    Some(prompt)
}

pub(super) fn prompt_reply(
    prompt: &Value,
    choice: &str,
    allow_broad: bool,
) -> Result<Reply, String> {
    if prompt.get("kind").and_then(Value::as_str) == Some("workspace_trust") {
        // The dialog names its own keys (`[a] Trust this workspace`, `[q] Quit`); older builds
        // answered y/n, so fall back to those when the screen text has no bracketed keys.
        let text = prompt.get("text").and_then(Value::as_str).unwrap_or_default();
        let (trust_key, deny_key) = trust_dialog_keys(text);
        return match choice {
            "trust" => Ok(Reply::key(&trust_key)),
            "deny" => Ok(Reply::key(&deny_key)),
            _ => Err("unsupported workspace trust choice".into()),
        };
    }
    generic::prompt_reply(prompt, choice, allow_broad)
}

/// Keys of the trust dialog's `[x] Trust this workspace` and `[x] Quit` rows.
fn trust_dialog_keys(text: &str) -> (String, String) {
    let key_of = |needle: &str| {
        text.lines().find_map(|line| {
            let lower = line.to_ascii_lowercase();
            let at = lower.find(needle)?;
            let head = &line[..at];
            let open = head.rfind('[')?;
            let key = head[open + 1..].split(']').next()?.trim();
            (key.chars().count() == 1).then(|| key.to_string())
        })
    };
    (
        key_of("trust this").unwrap_or_else(|| "y".into()),
        key_of("quit").unwrap_or_else(|| "n".into()),
    )
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

    // Rows recorded from a live Cursor Agent session (v2026.10.01).
    const WORKING_SCREEN: &str = "DONE.\n  \u{2820}\u{231c} Composing  5 tokens\n  Tip: Use subagents to parallelize work\n  \u{2192} Add a follow-up      ctrl+c to stop\n  Composer 2.5\n";
    const RUNNING_TOOL_SCREEN: &str = "  \u{2820}\u{249b} Running  52 tokens\n  \u{2192} Add a follow-up      ctrl+c to stop\n  1 task\n  Composer 2.5 \u{b7} 7.4%\n";
    const FINISHED_SCREEN: &str = "  PONG\n\n  \u{2192} Add a follow-up\n  Composer 2.5 \u{b7} 7.4%\n  ~\\AppData\\Local\\Temp\n";
    const GHOST_PROMPT_SCREEN: &str = "  PONG\n\n  \u{2192} Reply with only the word PONG. Use no tools.\n  Composer 2.5 \u{b7} 7.4%\n";
    const READY_SCREEN: &str = "  Cursor Agent\n  \u{2192} Plan, search, build anything\n  Composer 2.5\n";

    #[test]
    fn only_a_screen_with_no_sign_of_work_counts_as_quiet() {
        let cursor = crate::tui::profile_for("cursor_agent");
        assert_eq!(cursor.busy_marker(WORKING_SCREEN), Some(true));
        assert_eq!(cursor.busy_marker(RUNNING_TOOL_SCREEN), Some(true), "a tool run is still work");
        assert_eq!(cursor.busy_marker("  Trusting workspace...\n"), Some(true));
        // A cold run keeps the answered dialog in the visible rows above the real UI.
        let stale_dialog = format!("  \u{23f3} Trusting workspace...\n  Cursor Agent\n{FINISHED_SCREEN}");
        assert_eq!(cursor.busy_marker(&stale_dialog), Some(false), "the old dialog is scrollback");
        assert_eq!(cursor.busy_marker(FINISHED_SCREEN), Some(false));
        assert_eq!(cursor.busy_marker(GHOST_PROMPT_SCREEN), Some(false), "the footer is not always 'Add a follow-up'");
        assert_eq!(cursor.busy_marker(READY_SCREEN), Some(false));
    }

    #[test]
    fn the_quiet_window_is_shorter_than_the_busy_one_and_never_below_the_spinner_period() {
        let cursor = crate::tui::profile_for("cursor_agent");
        assert!(cursor.quiet_idle_after() < cursor.idle_after());
        // The busy window stays long: the trust handshake is a static screen for ~2.4s that must not read idle.
        assert!(cursor.idle_after() >= std::time::Duration::from_secs(5));
        // The spinner repaints about every 250ms; the quiet window must ride out several frames.
        assert!(cursor.quiet_idle_after() >= std::time::Duration::from_millis(500));
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
    fn standby_screen_reads_the_empty_input_box_and_never_a_dialog() {
        let cursor = profile_for("cursor_agent");
        assert_eq!(cursor.standby_screen(READY_SCREEN), StandbyScreen::Ready);
        assert_eq!(cursor.standby_screen("  Cursor Agent\n  \u{2192} Add a follow-up\n"), StandbyScreen::Ready);
        assert_eq!(cursor.standby_screen(WORKING_SCREEN), StandbyScreen::NotReady);
        assert_eq!(cursor.standby_screen("  Cursor Agent\n  v2026\n"), StandbyScreen::NotReady, "not drawn yet");
        assert_eq!(cursor.standby_screen(APPROVAL_SCREEN), StandbyScreen::Dialog);
        assert_eq!(cursor.standby_screen("  Update available. Press enter to update\n  \u{2192} Plan, search, build anything\n"), StandbyScreen::NeedsAttention);
        assert_eq!(cursor.standby_screen("Please log in to Cursor\n"), StandbyScreen::NeedsAttention);
    }

    #[test]
    fn standby_screen_flags_a_pending_trust_dialog_as_a_dialog() {
        let cursor = profile_for("cursor_agent");
        assert_eq!(cursor.standby_screen(TRUST_PENDING), StandbyScreen::Dialog, "an untrusted folder is not poolable");
        assert_eq!(cursor.standby_screen(include_str!("fixtures/workspace_trust_screen.txt")), StandbyScreen::Dialog);
    }

    // Real captures from a visible cursor_agent pane in an untrusted folder (2026-10-04).
    const TRUST_PENDING: &str = include_str!("fixtures/workspace_trust_visible_pending.txt");
    const TRUST_RESOLVED: &str = include_str!("fixtures/workspace_trust_visible_resolved.txt");

    #[test]
    fn a_pending_trust_dialog_is_a_live_prompt_with_its_own_keys() {
        assert_eq!(status(TRUST_PENDING), PaneStatus::WaitingInput);
        let cursor = profile_for("cursor_agent");
        let prompt = cursor.detect_prompt(TRUST_PENDING, false).unwrap();
        assert_eq!(prompt["kind"], "workspace_trust");
        assert_eq!(cursor.prompt_reply(&prompt, "trust", false).unwrap(), Reply::key("a"));
        assert_eq!(cursor.prompt_reply(&prompt, "deny", false).unwrap(), Reply::key("q"));
    }

    #[test]
    fn an_answered_trust_dialog_left_in_the_scrollback_is_not_a_live_prompt() {
        // The TUI redraws its banner and input prompt below the old dialog; PM used to keep
        // re-reporting workspace_trust forever.
        assert_ne!(status(TRUST_RESOLVED), PaneStatus::WaitingInput);
        assert!(profile_for("cursor_agent")
            .detect_prompt(TRUST_RESOLVED, false)
            .is_none());
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
