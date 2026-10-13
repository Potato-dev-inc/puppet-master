//! When to notify, and what to say. Pure functions and one small state machine, no I/O.
//!
//! Agent-agnostic: it sees pane ids, the four `PaneStatus` strings, the parsed prompt (whatever
//! the agent's profile produced) and an agent label that the caller looked up from the registry.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// At most one notification per pane in this window.
pub const RATE_LIMIT_MS: u64 = 30_000;
/// The body never carries more than this many characters of anything that came from a terminal.
pub const MAX_BODY_CHARS: usize = 120;
/// How long a freshly entered `waiting_input` settles before we read the prompt, so a pane that
/// flickers through states, or whose dialog is still being drawn, produces one notification.
pub const SETTLE_MS: u64 = 1200;

const MAX_REMEMBERED_OPERATIONS: usize = 256;
const MAX_REMEMBERED_KEYS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    WaitingInput,
    Error,
    RunFinished,
    RunFailed,
    /// An agent asked the human a question (`ask_user`). Answered in the app, never from the lock screen.
    Question,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WaitingInput => "waiting_input",
            Self::Error => "error",
            Self::RunFinished => "run_finished",
            Self::RunFailed => "run_failed",
            Self::Question => "question",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Send,
    /// Same prompt as the last notification for this pane.
    Duplicate,
    /// Another notification for this pane went out less than `RATE_LIMIT_MS` ago.
    RateLimited,
}

/// Per-pane memory behind the triggers.
#[derive(Debug, Default)]
pub struct Gate {
    last_status: HashMap<String, String>,
    last_sent_ms: HashMap<String, u64>,
    last_prompt: HashMap<String, String>,
    seen_operations: HashSet<String>,
}

impl Gate {
    /// Feed a pane status update. Returns the kind of notification worth considering when the pane
    /// just ENTERED `waiting_input` or `error` (a repeat of the same status returns `None`).
    pub fn entered(&mut self, pane_id: &str, status: &str) -> Option<Kind> {
        let previous = self.last_status.insert(pane_id.to_string(), status.to_string());
        if status != "waiting_input" {
            self.last_prompt.remove(pane_id);
        }
        if previous.as_deref() == Some(status) {
            return None;
        }
        match status {
            "waiting_input" => Some(Kind::WaitingInput),
            "error" => Some(Kind::Error),
            _ => None,
        }
    }

    /// Decide whether a candidate goes out now, and remember it if so.
    pub fn admit(&mut self, now_ms: u64, key: &str, kind: Kind, prompt_id: Option<&str>) -> Verdict {
        if kind == Kind::WaitingInput {
            if let Some(id) = prompt_id {
                if self.last_prompt.get(key).map(String::as_str) == Some(id) {
                    return Verdict::Duplicate;
                }
            }
        }
        if let Some(last) = self.last_sent_ms.get(key) {
            if now_ms.saturating_sub(*last) < RATE_LIMIT_MS {
                return Verdict::RateLimited;
            }
        }
        if self.last_sent_ms.len() >= MAX_REMEMBERED_KEYS {
            self.last_sent_ms.retain(|_, last| now_ms.saturating_sub(*last) < RATE_LIMIT_MS);
        }
        self.last_sent_ms.insert(key.to_string(), now_ms);
        if kind == Kind::WaitingInput {
            match prompt_id {
                Some(id) => {
                    self.last_prompt.insert(key.to_string(), id.to_string());
                }
                None => {
                    self.last_prompt.remove(key);
                }
            }
        }
        Verdict::Send
    }

    /// True the first time a terminal operation state is seen (snapshots are republished).
    pub fn first_sight_of_operation(&mut self, operation_id: &str, status: &str) -> bool {
        if self.seen_operations.len() >= MAX_REMEMBERED_OPERATIONS {
            self.seen_operations.clear();
        }
        self.seen_operations.insert(format!("{operation_id}:{status}"))
    }

    /// Forget a pane that no longer exists.
    pub fn forget_pane(&mut self, pane_id: &str) {
        self.last_status.remove(pane_id);
        self.last_sent_ms.remove(pane_id);
        self.last_prompt.remove(pane_id);
    }
}

/// Drop what could make a notification lie about what it shows: control characters and the
/// invisible / bidirectional formatting characters (U+200B-200F, U+202A-202E, U+2060-2064,
/// U+2066-2069, U+061C, U+FEFF, U+00AD, U+180E) that can reorder or hide text.
pub fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !is_hidden_or_control(*c)).collect()
}

fn is_hidden_or_control(c: char) -> bool {
    (c.is_control() && !c.is_whitespace())
        || matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0x061C | 0xFEFF | 0x00AD | 0x180E)
}

/// True when the notification body will show `text` completely and truthfully: nothing hidden
/// was stripped and nothing is cut. Allow / Deny is only offered then.
pub fn shown_in_full(text: &str) -> bool {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    !text.chars().any(is_hidden_or_control) && !flat.is_empty() && flat.chars().count() <= MAX_BODY_CHARS
}

/// Cut `text` to at most `max` characters (not bytes), ending in an ellipsis when cut. Newlines
/// and runs of spaces collapse so the result is one tidy line.
pub fn truncate_chars(text: &str, max: usize) -> String {
    let flat = sanitize(text).split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut = cut.trim_end().to_string();
    cut.push('…');
    cut
}

/// What the notification needs to know about the prompt blocking a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptInfo {
    pub prompt_id: String,
    /// Summary if the profile produced one, else its prompt text. Not yet truncated.
    pub text: String,
    /// A command approval that can be answered with exactly Allow once / Deny.
    pub actionable: bool,
}

impl PromptInfo {
    /// From the JSON `pane_prompt::detect` returns.
    pub fn from_detected(prompt: &Value) -> Option<Self> {
        let get = |key: &str| prompt.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let prompt_id = get("prompt_id")?.to_string();
        let text = get("summary").or_else(|| get("text")).unwrap_or("").to_string();
        let choices: Vec<&str> = prompt
            .get("choices")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let has_command = get("command").is_some();
        let actionable = has_command && choices.contains(&"allow_once") && choices.contains(&"deny");
        Some(Self { prompt_id, text, actionable })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    pub title: String,
    pub body: String,
}

/// The words of a notification. `label` is the agent name from its profile or definition.
pub fn copy_for(kind: Kind, label: &str, prompt: Option<&PromptInfo>) -> Words {
    match kind {
        Kind::WaitingInput => Words {
            title: format!("{} needs you", sanitize(label)),
            body: prompt
                .map(|p| truncate_chars(&p.text, MAX_BODY_CHARS))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "Waiting for your input".into()),
        },
        Kind::Error => Words {
            title: format!("{} hit an error", sanitize(label)),
            body: "Open the pane to see what happened".into(),
        },
        Kind::RunFinished => Words { title: format!("{} finished", sanitize(label)), body: "The run is done".into() },
        Kind::RunFailed => Words { title: format!("{} failed", sanitize(label)), body: "The run did not finish".into() },
        Kind::Question => Words {
            title: format!("{} asks", sanitize(label)),
            body: prompt
                .map(|p| truncate_chars(&p.text, MAX_BODY_CHARS))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "Has a question for you".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_entering_waiting_or_error_triggers() {
        let mut gate = Gate::default();
        assert_eq!(gate.entered("p", "running"), None);
        assert_eq!(gate.entered("p", "waiting_input"), Some(Kind::WaitingInput));
        assert_eq!(gate.entered("p", "waiting_input"), None, "a repeat is not an entry");
        assert_eq!(gate.entered("p", "idle"), None);
        assert_eq!(gate.entered("p", "error"), Some(Kind::Error));
        assert_eq!(gate.entered("p", "waiting_input"), Some(Kind::WaitingInput));
        assert_eq!(gate.entered("q", "idle"), None);
    }

    #[test]
    fn rate_limit_is_one_per_pane_per_thirty_seconds() {
        let mut gate = Gate::default();
        assert_eq!(gate.admit(1_000, "p", Kind::Error, None), Verdict::Send);
        assert_eq!(gate.admit(1_000 + RATE_LIMIT_MS - 1, "p", Kind::WaitingInput, Some("a")), Verdict::RateLimited);
        assert_eq!(gate.admit(1_000, "other", Kind::Error, None), Verdict::Send, "other panes are independent");
        assert_eq!(gate.admit(1_000 + RATE_LIMIT_MS, "p", Kind::Error, None), Verdict::Send);
    }

    #[test]
    fn the_same_prompt_is_not_sent_twice_but_a_new_one_is() {
        let mut gate = Gate::default();
        assert_eq!(gate.admit(0, "p", Kind::WaitingInput, Some("A")), Verdict::Send);
        assert_eq!(gate.admit(100_000, "p", Kind::WaitingInput, Some("A")), Verdict::Duplicate);
        assert_eq!(gate.admit(100_000, "p", Kind::WaitingInput, Some("B")), Verdict::Send);
        // Leaving waiting_input clears the memory, so the same command asked again notifies again.
        gate.entered("p", "waiting_input");
        gate.entered("p", "running");
        assert_eq!(gate.admit(200_000, "p", Kind::WaitingInput, Some("B")), Verdict::Send);
    }

    #[test]
    fn terminal_operation_states_are_seen_once() {
        let mut gate = Gate::default();
        assert!(gate.first_sight_of_operation("op1", "completed"));
        assert!(!gate.first_sight_of_operation("op1", "completed"));
        assert!(gate.first_sight_of_operation("op1", "failed"));
    }

    #[test]
    fn truncation_counts_characters_not_bytes_and_flattens_whitespace() {
        let long = "é".repeat(300);
        let cut = truncate_chars(&long, MAX_BODY_CHARS);
        assert_eq!(cut.chars().count(), MAX_BODY_CHARS);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate_chars("a\n  b\tc", 120), "a b c");
        assert_eq!(truncate_chars("short", 120), "short");
        assert_eq!(truncate_chars(&"x".repeat(120), 120).chars().count(), 120);
    }

    #[test]
    fn copy_uses_the_parsed_prompt_or_a_plain_fallback() {
        let prompt = PromptInfo { prompt_id: "a".into(), text: "Run command: npm test (in /repo)".into(), actionable: true };
        let c = copy_for(Kind::WaitingInput, "Claude Code", Some(&prompt));
        assert_eq!(c.title, "Claude Code needs you");
        assert_eq!(c.body, "Run command: npm test (in /repo)");
        assert_eq!(copy_for(Kind::WaitingInput, "Kilo", None).body, "Waiting for your input");
        let long = PromptInfo { prompt_id: "b".into(), text: "z".repeat(500), actionable: false };
        assert_eq!(copy_for(Kind::WaitingInput, "X", Some(&long)).body.chars().count(), MAX_BODY_CHARS);
        assert_eq!(copy_for(Kind::Error, "Codex", None).title, "Codex hit an error");
        assert_eq!(copy_for(Kind::RunFinished, "Codex", None).title, "Codex finished");
        assert_eq!(copy_for(Kind::RunFailed, "Codex", None).title, "Codex failed");
    }

    #[test]
    fn question_copy_is_the_truncated_sanitised_question() {
        let text = format!("Deploy\u{202E} now?\n{}", "z".repeat(500));
        let q = PromptInfo { prompt_id: String::new(), text, actionable: false };
        let words = copy_for(Kind::Question, "Ki\u{202E}lo", Some(&q));
        assert_eq!(words.title, "Kilo asks");
        assert_eq!(words.body.chars().count(), MAX_BODY_CHARS);
        assert!(words.body.starts_with("Deploy now? zzz") && words.body.ends_with('…'));
        assert!(!words.body.chars().any(is_hidden_or_control));
        assert_eq!(copy_for(Kind::Question, "X", None).body, "Has a question for you");
    }

    #[test]
    fn bidi_and_control_characters_never_reach_the_notification() {
        let nasty = "Run command: ls\u{202E}gnihtemos\u{202C} \u{2066}x\u{2069}\u{200B}y\u{7}z\u{FEFF}";
        let prompt = PromptInfo { prompt_id: "a".into(), text: nasty.into(), actionable: true };
        let words = copy_for(Kind::WaitingInput, "Ki\u{202E}lo", Some(&prompt));
        for text in [&words.title, &words.body] {
            assert!(!text.chars().any(is_hidden_or_control), "left a hidden char in {text:?}");
        }
        assert_eq!(words.title, "Kilo needs you");
        assert_eq!(words.body, "Run command: lsgnihtemos xyz");
        assert!(!shown_in_full(nasty), "text that needed stripping is never shown in full");
    }

    #[test]
    fn shown_in_full_needs_everything_visible_and_short() {
        assert!(shown_in_full("Run command: npm test (in /repo)"));
        assert!(shown_in_full(&"x".repeat(MAX_BODY_CHARS)));
        assert!(!shown_in_full(&"x".repeat(MAX_BODY_CHARS + 1)));
        assert!(!shown_in_full("   "));
    }

    #[test]
    fn only_command_approvals_with_allow_once_and_deny_are_actionable() {
        let command = json!({"prompt_id": "p1", "kind": "command", "command": "ls", "summary": "Run command: ls", "choices": ["allow_once", "allow_all", "deny"]});
        let info = PromptInfo::from_detected(&command).unwrap();
        assert!(info.actionable);
        assert_eq!(info.text, "Run command: ls");
        // Workspace trust has no command: the user must accept it, never from a lock screen.
        let trust = json!({"prompt_id": "p2", "kind": "workspace_trust", "text": "Trust this workspace?", "choices": ["allow_once", "deny"]});
        assert!(!PromptInfo::from_detected(&trust).unwrap().actionable);
        let one_sided = json!({"prompt_id": "p3", "command": "ls", "choices": ["allow_once"]});
        assert!(!PromptInfo::from_detected(&one_sided).unwrap().actionable);
        assert!(PromptInfo::from_detected(&json!({"text": "no id"})).is_none());
    }
}
