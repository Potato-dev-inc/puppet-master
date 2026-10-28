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
/// The first line of an approval body ("High risk · Delete a folder outside this project") is cut to
/// this many characters. Real risk headlines reach 39 characters and "High risk · " adds 12, so the
/// old 40 could not hold them (design decision D5).
pub const MAX_FIRST_LINE_CHARS: usize = crate::attention::copy::MAX_FIRST_LINE_CHARS;
/// A pane title in the notification title is cut to this.
pub const MAX_TITLE_CHARS: usize = 40;
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
    /// A pane stopped at a startup gate only the person may pass (folder trust, a sign-in screen).
    /// Never answered from a lock screen.
    TrustGate,
    /// An agent wants to control a pane it did not start, or answer above low risk in one.
    Consent,
    /// An agent used its budget and is paused.
    Budget,
    /// An agent ran out of usage.
    Limit,
    /// A companion replied (`push/companion_replies.rs`). Built there, never by `copy_for_view`.
    CompanionReply,
    /// The one morning push of a Night Shift run (`push/night.rs`). Counts only. Built there, never by
    /// `copy_for_view`.
    NightShift,
    /// The end of a Focus session, or its one reminder (`push/focus.rs`). Counts only. Built there, never
    /// by `copy_for_view`.
    Focus,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WaitingInput => "waiting_input",
            Self::Error => "error",
            Self::RunFinished => "run_finished",
            Self::RunFailed => "run_failed",
            Self::Question => "question",
            Self::TrustGate => "trust_gate",
            Self::Consent => "consent",
            Self::Budget => "budget",
            Self::Limit => "limit",
            Self::CompanionReply => "companion_reply",
            Self::NightShift => "night_shift",
            Self::Focus => "focus",
        }
    }

    /// Kinds that belong to what a pane is showing right now: they end when the pane leaves
    /// `waiting_input`.
    pub fn is_pane_prompt(self) -> bool {
        matches!(self, Self::WaitingInput | Self::TrustGate)
    }

    /// Kinds that wake a phone at once and may be held back by the rate limit (a finished run is
    /// neither).
    pub fn is_urgent(self) -> bool {
        !matches!(self, Self::RunFinished | Self::RunFailed | Self::CompanionReply | Self::NightShift | Self::Focus)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Send,
    /// Same prompt as the last notification for this pane.
    Duplicate,
    /// Another notification for this pane went out less than `RATE_LIMIT_MS` ago. The hub holds the
    /// newest one and releases it when the window ends (`PushHub::take_due`): it is never dropped.
    RateLimited,
    /// Everything that was held back was dealt with before the window ended: nothing to say.
    Resolved,
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
        if kind.is_pane_prompt() {
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
        if kind.is_pane_prompt() {
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

    /// When the last notification for `key` went out.
    pub fn last_sent(&self, key: &str) -> Option<u64> {
        self.last_sent_ms.get(key).copied()
    }

    /// A notification for `key` goes out now outside `admit` (a coalesced push): start its window.
    pub fn mark_sent(&mut self, now_ms: u64, key: &str) {
        self.last_sent_ms.insert(key.to_string(), now_ms);
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
    /// A command approval that can be answered with exactly Allow once / Deny. True only for LOW
    /// risk: never for "Check first" (medium) or high risk, nor when the risk is unknown (fail
    /// closed). Allow from a lock screen must not be the easy path for `npm install` or `rm -rf ../`.
    pub actionable: bool,
    /// The risk the desktop card shows (`risk.level` / `risk.headline` of the detected prompt).
    /// `None` when the prompt carried none.
    pub risk: Option<PromptRisk>,
}

/// The plain-language risk of a prompt, as put on the detected-prompt JSON by `prompt_risk`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRisk {
    pub level: crate::prompt_risk::RiskLevel,
    pub headline: String,
}

impl PromptRisk {
    fn from_json(prompt: &Value) -> Option<Self> {
        let risk = prompt.get("risk")?;
        let level = match risk.get("level").and_then(Value::as_str)?.trim() {
            "low" => crate::prompt_risk::RiskLevel::Low,
            "medium" => crate::prompt_risk::RiskLevel::Medium,
            "high" => crate::prompt_risk::RiskLevel::High,
            _ => return None,
        };
        let headline = risk.get("headline").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
        Some(Self { level, headline })
    }

    /// "High risk" / "Check first" / "Low risk": the desktop card's word alone.
    fn level_word(&self) -> &'static str {
        crate::attention::model::Level::from(self.level).word()
    }

    /// "High risk · Delete a folder outside this project". The words are the desktop card's
    /// (`attention::model::Level::word`: Low risk / Check first / High risk).
    fn sentence(&self) -> String {
        let line = crate::attention::copy::risk_line(crate::attention::model::Level::from(self.level), Some(&self.headline));
        truncate_chars(&line, MAX_FIRST_LINE_CHARS)
    }
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
        let risk = PromptRisk::from_json(prompt);
        let low_enough = risk.as_ref().is_some_and(|r| r.level == crate::prompt_risk::RiskLevel::Low);
        let actionable = has_command && low_enough && choices.contains(&"allow_once") && choices.contains(&"deny");
        Some(Self { prompt_id, text, actionable, risk })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    pub title: String,
    pub body: String,
}

/// How much a notification says. The same event reads differently on each phone:
/// - `Full`: Android and desktop browsers (Allow / Deny buttons are possible, so a calm command is shown);
/// - `Ios`: Apple's push service shows no action buttons, so the command is left out and a question
///   is just "Question from <agent>";
/// - `Hidden`: the phone's "Hide details on lock screen" switch is on: nothing the agent wrote and
///   nothing the person named appears; the title is the agent, the body is the kind and the risk word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Full,
    Ios,
    Hidden,
}

/// Apple's web push service. Its subscriptions never get lock-screen buttons.
pub fn is_apple_endpoint(endpoint: &str) -> bool {
    super::store::endpoint_host(endpoint).is_ok_and(|host| host == "web.push.apple.com" || host.ends_with(".push.apple.com"))
}

/// The words of a notification. `label` is the agent name from its profile or definition;
/// `heading` is what the sidebar calls the pane: the person's title for it ("auth refactor"), else
/// the agent label numbered when several panes run the same agent ("Claude Code 2").
///
/// Titles are `<heading> · Needs you`; the agent label moves into the body. An approval body is
/// the risk line (first line, at most `MAX_FIRST_LINE_CHARS`), then the command for a calm
/// (actionable, low-risk) command, then the agent label. A prompt that is not low risk never puts
/// its command on a lock screen.
pub fn copy_for(kind: Kind, label: &str, heading: Option<&str>, prompt: Option<&PromptInfo>) -> Words {
    copy_for_view(kind, label, heading, prompt, None, View::Full)
}

/// [`copy_for`] for one phone. `detail` is the one fact a kind adds to its first line: the folder of a
/// trust gate (`None` = a sign-in screen with nothing to choose), the turns of a budget card, the reset
/// time of a usage limit, the whole sentence of a consent request.
pub fn copy_for_view(kind: Kind, label: &str, heading: Option<&str>, prompt: Option<&PromptInfo>, detail: Option<&str>, view: View) -> Words {
    let label = sanitize(label);
    let who = match view {
        // A pane's title is often the first line of the task the person typed.
        View::Hidden => label.clone(),
        _ => heading.map(|t| truncate_chars(t, MAX_TITLE_CHARS)).filter(|t| !t.is_empty()).unwrap_or_else(|| label.clone()),
    };
    // Redact before cutting: a cut can leave half a secret that no rule recognises.
    let cut = |text: &str, max: usize| truncate_chars(crate::redact::text(text).as_str(), max);
    let first_line = |text: &str| cut(text, MAX_FIRST_LINE_CHARS);
    let detail = detail.map(str::trim).filter(|d| !d.is_empty());
    let needs_you = format!("{who} · Needs you");
    match kind {
        Kind::WaitingInput => {
            let waiting = format!("{label} is waiting for you");
            let body = match prompt {
                None => waiting,
                Some(p) => {
                    let text = cut(&p.text, MAX_BODY_CHARS);
                    match &p.risk {
                        Some(risk) => {
                            let first = if view == View::Hidden { risk.level_word().to_string() } else { risk.sentence() };
                            let mut lines = vec![first];
                            // Belt and braces: only a calm command, whatever the flag says, and only where a
                            // button can come with it.
                            if view == View::Full && p.actionable && risk.level == crate::prompt_risk::RiskLevel::Low && !text.is_empty() {
                                lines.push(text);
                            }
                            lines.push(label.clone());
                            lines.join("\n")
                        }
                        None if view != View::Hidden && !text.is_empty() => format!("{text}\n{label}"),
                        None => waiting,
                    }
                }
            };
            Words { title: needs_you, body }
        }
        Kind::TrustGate => {
            let body = match (detail, view) {
                (Some(folder), View::Full | View::Ios) => format!("{}\n{label}", first_line(&format!("Trust this folder? · {folder}"))),
                (Some(_), View::Hidden) => format!("Trust this folder?\n{label}"),
                (None, _) => format!("{label} needs you on your PC"),
            };
            Words { title: needs_you, body }
        }
        Kind::Consent => {
            let body = match (detail, view) {
                (Some(sentence), View::Full | View::Ios) => first_line(sentence),
                _ => format!("{label} wants to control another agent"),
            };
            Words { title: needs_you, body }
        }
        Kind::Budget => {
            let first = match (detail, view) {
                (Some(used), View::Full | View::Ios) => first_line(&format!("Budget used up · {used}")),
                _ => "Budget used up".to_string(),
            };
            Words { title: needs_you, body: format!("{first}\n{label}") }
        }
        Kind::Limit => {
            let first = match (detail, view) {
                (Some(reset), View::Full | View::Ios) => first_line(&format!("Usage limit · resets {reset}")),
                _ => "Usage limit".to_string(),
            };
            Words { title: needs_you, body: format!("{first}\n{label}") }
        }
        Kind::Error => Words { title: needs_you, body: format!("Hit an error · {label}") },
        // Companion replies are worded by `companion_replies::Message`; this arm only keeps the match whole.
        Kind::CompanionReply => Words { title: label.clone(), body: "New reply".to_string() },
        // Night Shift's morning push is worded by `night::Message`; this arm only keeps the match whole.
        Kind::NightShift => Words { title: super::night::TITLE.to_string(), body: crate::night_shift::report::HIDDEN_PUSH.to_string() },
        // Focus's end message and reminder are worded by `focus::Message`; this arm only keeps the match whole.
        Kind::Focus => Words { title: super::focus::SUMMARY_TITLE.to_string(), body: super::focus::HIDDEN_BODY.to_string() },
        Kind::RunFinished => Words { title: format!("{who} · Done"), body: format!("{label} finished") },
        Kind::RunFailed => Words { title: needs_you, body: format!("Run failed · {label}") },
        Kind::Question => {
            let prefix = "Question · ";
            let text = prompt.map(|p| cut(&p.text, MAX_FIRST_LINE_CHARS - prefix.chars().count())).filter(|t| !t.is_empty());
            let body = match (text, view) {
                (Some(text), View::Full) => format!("{prefix}{text}\n{label}"),
                (Some(_), _) => format!("Question from {label}"),
                (None, _) => format!("{label} has a question"),
            };
            Words { title: needs_you, body }
        }
    }
}

/// The words of ONE notification that stands for several things waiting on a pane: a prompt that
/// arrived inside the rate-limit window, and whatever else is open there. It never carries a button.
pub fn coalesced_copy(label: &str, heading: Option<&str>, open: u32, risk: Option<&PromptRisk>) -> Words {
    coalesced_copy_view(label, heading, open, risk, View::Full)
}

pub fn coalesced_copy_view(label: &str, heading: Option<&str>, open: u32, risk: Option<&PromptRisk>, view: View) -> Words {
    let label = sanitize(label);
    let who = match view {
        View::Hidden => label.clone(),
        _ => heading.map(|t| truncate_chars(t, MAX_TITLE_CHARS)).filter(|t| !t.is_empty()).unwrap_or_else(|| label.clone()),
    };
    let body = match (risk, view) {
        (Some(risk), View::Hidden) => format!("{}\n{label}", risk.level_word()),
        (Some(risk), _) => format!("{}\n{label}", risk.sentence()),
        (None, _) => label.clone(),
    };
    Words { title: format!("{who} · {open} things need you"), body }
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
        let prompt = PromptInfo { prompt_id: "a".into(), text: "Run command: npm test (in /repo)".into(), actionable: true, risk: None };
        let c = copy_for(Kind::WaitingInput, "Claude Code", None, Some(&prompt));
        assert_eq!(c.title, "Claude Code · Needs you");
        assert_eq!(c.body, "Run command: npm test (in /repo)\nClaude Code", "no risk read: the text, then who");
        assert_eq!(copy_for(Kind::WaitingInput, "Kilo", None, None).body, "Kilo is waiting for you");
        let long = PromptInfo { prompt_id: "b".into(), text: "z".repeat(500), actionable: false, risk: None };
        let first = copy_for(Kind::WaitingInput, "X", None, Some(&long)).body;
        assert_eq!(first.split('\n').next().unwrap().chars().count(), MAX_BODY_CHARS);
        let error = copy_for(Kind::Error, "Codex", None, None);
        assert_eq!((error.title.as_str(), error.body.as_str()), ("Codex · Needs you", "Hit an error · Codex"));
        let done = copy_for(Kind::RunFinished, "Codex", Some("billing tests"), None);
        assert_eq!((done.title.as_str(), done.body.as_str()), ("billing tests · Done", "Codex finished"));
        let failed = copy_for(Kind::RunFailed, "Codex", None, None);
        assert_eq!((failed.title.as_str(), failed.body.as_str()), ("Codex · Needs you", "Run failed · Codex"));
    }

    #[test]
    fn question_copy_is_the_truncated_sanitised_question() {
        let text = format!("Deploy\u{202E} now?\n{}", "z".repeat(500));
        let q = PromptInfo { prompt_id: String::new(), text, actionable: false, risk: None };
        let words = copy_for(Kind::Question, "Ki\u{202E}lo", None, Some(&q));
        assert_eq!(words.title, "Kilo · Needs you");
        let first = words.body.split('\n').next().unwrap();
        assert_eq!(first.chars().count(), MAX_FIRST_LINE_CHARS, "the question line is cut like any first line");
        assert!(first.starts_with("Question · Deploy now? zzz") && first.ends_with('…'));
        assert!(words.body.ends_with("\nKilo"));
        assert!(!words.body.chars().any(is_hidden_or_control));
        assert_eq!(copy_for(Kind::Question, "X", None, None).body, "X has a question");
    }

    #[test]
    fn bidi_and_control_characters_never_reach_the_notification() {
        let nasty = "Run command: ls\u{202E}gnihtemos\u{202C} \u{2066}x\u{2069}\u{200B}y\u{7}z\u{FEFF}";
        let prompt = PromptInfo { prompt_id: "a".into(), text: nasty.into(), actionable: true, risk: None };
        let words = copy_for(Kind::WaitingInput, "Ki\u{202E}lo", None, Some(&prompt));
        for text in [&words.title, &words.body] {
            assert!(!text.chars().any(is_hidden_or_control), "left a hidden char in {text:?}");
        }
        assert_eq!(words.title, "Kilo · Needs you");
        assert_eq!(words.body, "Run command: lsgnihtemos xyz\nKilo");
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
        let command = json!({"prompt_id": "p1", "kind": "command", "command": "ls", "summary": "Run command: ls", "choices": ["allow_once", "allow_all", "deny"], "risk": {"level": "low", "headline": "Read files", "reasons": []}});
        let info = PromptInfo::from_detected(&command).unwrap();
        assert!(info.actionable);
        assert_eq!(info.text, "Run command: ls");
        // Workspace trust has no command: the user must accept it, never from a lock screen.
        let trust = json!({"prompt_id": "p2", "kind": "workspace_trust", "text": "Trust this workspace?", "choices": ["allow_once", "deny"]});
        assert!(!PromptInfo::from_detected(&trust).unwrap().actionable);
        let one_sided = json!({"prompt_id": "p3", "command": "ls", "choices": ["allow_once"], "risk": {"level": "low", "headline": "x"}});
        assert!(!PromptInfo::from_detected(&one_sided).unwrap().actionable);
        assert!(PromptInfo::from_detected(&json!({"text": "no id"})).is_none());
    }

    fn command_with_risk(risk: Option<Value>) -> Value {
        let mut prompt = json!({"prompt_id": "p1", "kind": "command", "command": "rm -rf ../", "summary": "Run command: rm -rf ../", "choices": ["allow_once", "allow_all", "deny"]});
        if let Some(risk) = risk {
            prompt["risk"] = risk;
        }
        prompt
    }

    #[test]
    fn a_high_risk_command_never_gets_lock_screen_buttons() {
        let high = json!({"level": "high", "headline": "Delete a folder outside this project", "reasons": []});
        let info = PromptInfo::from_detected(&command_with_risk(Some(high))).unwrap();
        assert!(!info.actionable, "High risk must be open-only: no Allow from a lock screen");
        assert_eq!(info.risk.as_ref().unwrap().level, crate::prompt_risk::RiskLevel::High);
    }

    #[test]
    fn a_missing_or_unreadable_risk_fails_closed() {
        assert!(!PromptInfo::from_detected(&command_with_risk(None)).unwrap().actionable, "no risk at all");
        let odd = json!({"level": "catastrophic", "headline": "?"});
        assert!(!PromptInfo::from_detected(&command_with_risk(Some(odd))).unwrap().actionable, "unknown level");
        let no_level = json!({"headline": "Delete things"});
        assert!(!PromptInfo::from_detected(&command_with_risk(Some(no_level))).unwrap().actionable, "no level");
    }

    #[test]
    fn only_low_risk_commands_are_actionable() {
        // Allow from a lock screen is for the calm case only: "Check first" means look first.
        let low = json!({"level": "low", "headline": "Read files"});
        assert!(PromptInfo::from_detected(&command_with_risk(Some(low))).unwrap().actionable, "low");
        for level in ["medium", "high"] {
            let risk = json!({"level": level, "headline": "Install packages"});
            assert!(!PromptInfo::from_detected(&command_with_risk(Some(risk))).unwrap().actionable, "{level}");
        }
    }

    #[test]
    fn the_body_leads_with_the_risk_sentence_then_the_command() {
        let risk = json!({"level": "high", "headline": "Delete a folder outside this project"});
        let info = PromptInfo::from_detected(&command_with_risk(Some(risk))).unwrap();
        let words = copy_for(Kind::WaitingInput, "Claude Code", None, Some(&info));
        // A risky prompt's command never reaches a lock screen: the risk line and who, nothing else.
        assert_eq!(words.body, "High risk · Delete a folder outside this project\nClaude Code");
        let low = json!({"level": "low", "headline": "Read files"});
        let info = PromptInfo::from_detected(&command_with_risk(Some(low))).unwrap();
        assert_eq!(
            copy_for(Kind::WaitingInput, "X", None, Some(&info)).body,
            "Low risk · Read files\nRun command: rm -rf ../\nX",
            "a calm command is shown, because it can be allowed from here"
        );
        // The command part is cut on its own, never by the risk line.
        let mut long = info.clone();
        long.text = "z".repeat(500);
        let body = copy_for(Kind::WaitingInput, "X", None, Some(&long)).body;
        let command_line = body.split('\n').nth(1).unwrap();
        assert_eq!(command_line.chars().count(), MAX_BODY_CHARS);
    }

    #[test]
    fn the_push_says_check_first_where_the_desktop_card_does() {
        // The desktop card's words are `LEVEL_WORD` in lib/approval-risk.ts. A medium prompt was
        // "Check first" on the PC and "Medium risk" on the phone.
        let ts = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/approval-risk.ts")).unwrap();
        let word = |level: &str| {
            let line = ts.lines().find(|l| l.trim_start().starts_with(&format!("{level}:"))).unwrap_or_else(|| panic!("no {level} word"));
            line.split('\'').nth(1).unwrap().to_string()
        };
        for (level, tag) in [("low", "Low"), ("medium", "Medium"), ("high", "High")] {
            let risk = json!({"level": level, "headline": "Install packages"});
            let info = PromptInfo::from_detected(&command_with_risk(Some(risk))).unwrap();
            let body = copy_for(Kind::WaitingInput, "X", None, Some(&info)).body;
            assert!(body.starts_with(&format!("{} · Install packages", word(level))), "{tag}: {body}");
        }
        assert_eq!(word("medium"), "Check first");
    }

    #[test]
    fn the_pane_title_leads_the_notification_title() {
        let words = copy_for(Kind::WaitingInput, "Claude Code", Some("auth refactor"), None);
        assert_eq!(words.title, "auth refactor · Needs you");
        assert_eq!(words.body, "Claude Code is waiting for you", "the agent label moved into the body");
        assert_eq!(copy_for(Kind::Error, "Codex", Some("  "), None).title, "Codex · Needs you", "a blank heading is ignored");
        let long = copy_for(Kind::WaitingInput, "X", Some(&"t".repeat(100)), None);
        assert!(long.title.chars().count() <= MAX_TITLE_CHARS + " · Needs you".chars().count());
        let sneaky = copy_for(Kind::WaitingInput, "X", Some("a\u{202E}b"), None);
        assert!(!sneaky.title.chars().any(is_hidden_or_control));
    }
}
