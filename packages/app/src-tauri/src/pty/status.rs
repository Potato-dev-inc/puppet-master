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
pub fn classify_agent_observation(agent_type: &str, screen: &str, recent: &str) -> PaneStatus {
    classify_agent_observation_in(agent_type, screen, recent, None)
}

/// [`classify_agent_observation`] for a pane whose working `folder` is known. An agent's banner or status row names
/// that folder, so a project called `deny` or `approve` read as a question on the very first screen (the run then
/// failed `OPERATION_NOT_DISPATCHING`). The folder's path and its name as a path part are not read.
pub fn classify_agent_observation_in(agent_type: &str, screen: &str, _recent: &str, folder: Option<&str>) -> PaneStatus {
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
    let visible_tail = mask_folder(&visible_tail, folder);
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

    // One rule for "the pane is asking" (`tui::menu_shape`): the profile's own prompt tables match, or a menu with a
    // cursor row and a hint footer is the last thing on screen. Without it a question nobody's `classify_screen` knew
    // (a wizard-added agent's trust screen, an agent's option menu) read `running`, then `idle`, and nothing needed you.
    if profile.declares_prompt(&visible_tail) || crate::tui::menu_shape::asks(profile.as_ref(), &visible_tail) {
        return PaneStatus::WaitingInput;
    }

    if looks_like_prompt(&visible_tail) {
        PaneStatus::WaitingInput
    } else {
        PaneStatus::Running
    }
}

/// `text` with the working folder blanked out: its whole path (either slash), and its name wherever it stands as a path
/// part (`~/work/deny`, `...\\deny`). Blanked with `_`, a word character, so no word-boundary pattern matches in it.
pub(crate) fn mask_folder(text: &str, folder: Option<&str>) -> String {
    let Some(folder) = folder.map(|f| f.trim().trim_end_matches(['/', '\\'])).filter(|f| !f.is_empty()) else { return text.to_string() };
    let mut out = text.to_string();
    let blank = |out: &mut String, needle: &str, part_of_path: bool| {
        if needle.is_empty() {
            return;
        }
        let lower = out.to_ascii_lowercase();
        let needle = needle.to_ascii_lowercase();
        let mut result = String::with_capacity(out.len());
        let mut from = 0;
        while let Some(found) = lower[from..].find(&needle) {
            let (start, end) = (from + found, from + found + needle.len());
            let before = lower[..start].chars().next_back();
            let after = lower[end..].chars().next();
            let whole_part = !part_of_path
                || (before.is_some_and(|ch| matches!(ch, '/' | '\\' | '~'))
                    && after.is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.')));
            if whole_part && out.is_char_boundary(start) && out.is_char_boundary(end) {
                result.push_str(&out[from..start]);
                result.push_str(&"_".repeat(end - start));
            } else {
                result.push_str(&out[from..end]);
            }
            from = end;
        }
        result.push_str(&out[from..]);
        *out = result;
    };
    blank(&mut out, folder, false);
    blank(&mut out, &folder.replace('\\', "/"), false);
    blank(&mut out, &folder.replace('/', "\\"), false);
    if let Some(name) = folder.rsplit(['/', '\\']).next().filter(|name| !name.is_empty() && *name != folder) {
        blank(&mut out, name, true);
    }
    out
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

/// Whether the screen shows a startup gate (`TuiProfile::startup_gate`): a notice, trust question or sign-in the
/// agent draws before it can take any task. Such a pane is not idle: the sidebar must not say "Idle" over a menu
/// that swallows whatever is typed next.
pub fn startup_gate_visible(agent_type: &str, screen: &str) -> bool {
    if is_shell_agent(agent_type) {
        return false;
    }
    crate::tui::profile_for(agent_type).startup_gate(&strip_ansi(screen)).is_some()
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

    /// A banner names the project folder; a folder called like a choice must not make the first screen a question.
    #[test]
    fn a_project_folder_named_like_a_choice_is_not_a_question() {
        for name in ["deny", "approve", "allow", "yes", "no", "1", "Deny", "yes-no"] {
            let folder = format!("C:\\Users\\me\\projects\\{name}");
            let banner = format!("NIMBUS 0.4.2 | local | {folder}\n\n\n| > type a task\n+------+\n");
            let status = classify_agent_observation_in("some-unregistered-agent", &banner, "", Some(&folder));
            assert_ne!(status, PaneStatus::WaitingInput, "{name}: {status:?}");
            // a short banner that shows the folder by its name only
            let short = format!("nimbus ~/projects/{name}\n> ");
            assert_ne!(classify_agent_observation_in("some-unregistered-agent", &short, "", Some(&folder)), PaneStatus::WaitingInput, "{name} (short)");
        }
        // the unmasked classifier is what read the folder as a question
        let folder = "C:\\Users\\me\\projects\\deny";
        let banner = format!("NIMBUS 0.4.2 | local | {folder}\n\n\n| > type a task\n");
        assert_eq!(classify_agent_observation("some-unregistered-agent", &banner, ""), PaneStatus::WaitingInput);
    }

    #[test]
    fn a_real_question_is_still_read_when_the_folder_is_known() {
        let folder = "C:\\work\\deny";
        let screen = format!("NIMBUS | {folder}\n\nRun this command?\n> 1. Allow once\n  2. Deny\n");
        assert_eq!(classify_agent_observation_in("some-unregistered-agent", &screen, "", Some(folder)), PaneStatus::WaitingInput);
        // the word on its own row is not part of the path
        let own = format!("NIMBUS | {folder}\n\nDeny\n");
        assert_eq!(classify_agent_observation_in("some-unregistered-agent", &own, "", Some(folder)), PaneStatus::WaitingInput);
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
