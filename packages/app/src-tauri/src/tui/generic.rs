//! Behaviour shared by every TUI that has no special handling, plus the profile for unregistered
//! agent types.

use super::{Features, Kind, Reply, StandbyScreen, TuiProfile};
use serde_json::{json, Value};

/// Words that mean a trust or approval question is on screen (never answered by a standby).
const GENERIC_DIALOG_WORDS: [&str; 6] = ["trust", "do you want to", "allow", "approve", "(y/n)", "[y/n]"];

/// Words that mean a login, update or press-to-continue screen the user has to deal with.
const GENERIC_ATTENTION_WORDS: [&str; 6] = [
    "log in",
    "login",
    "sign in",
    "authenticate",
    "api key",
    "update available",
];
const GENERIC_ATTENTION_PHRASES: [&str; 1] = ["press enter to continue"];

/// Readiness for an agent with no agent-specific ready screen (experimental standby). A dialog or
/// login word anywhere on screen wins; otherwise any non-empty screen counts as ready. The caller
/// must also see the screen hold still before it trusts this (`standby::stillness`).
pub fn experimental_standby_screen(screen: &str) -> StandbyScreen {
    let lower = screen.to_ascii_lowercase();
    if GENERIC_DIALOG_WORDS.iter().any(|word| lower.contains(word)) {
        return StandbyScreen::Dialog;
    }
    if GENERIC_ATTENTION_WORDS.iter().any(|word| lower.contains(word))
        || GENERIC_ATTENTION_PHRASES.iter().any(|phrase| lower.contains(phrase))
    {
        return StandbyScreen::NeedsAttention;
    }
    if screen.trim().is_empty() {
        StandbyScreen::NotReady
    } else {
        StandbyScreen::Ready
    }
}

/// Profile for an `agent_type` that is not registered: bare pane, generic heuristics.
pub struct Unknown;

impl TuiProfile for Unknown {
    fn id(&self) -> &'static str {
        "unknown"
    }

    fn features(&self) -> Features {
        Features {
            kind: Kind::Unknown,
            dispatchable: false,
            ..Features::CLI
        }
    }
}

/// Approval screens common to the supported CLIs. `allow_broad` opts into "allow all" prompts.
pub fn detect_prompt(agent_type: &str, screen: &str, allow_broad: bool) -> Option<Value> {
    let lower = screen.to_ascii_lowercase();
    let (kind, choices) = if lower.contains("allow once") || lower.contains("approve this command")
    {
        (
            "command_approval",
            vec!["allow_once".to_string(), "deny".to_string()],
        )
    } else if allow_broad && (lower.contains("allow all") || lower.contains("always allow")) {
        (
            "broad_approval",
            vec![
                "allow_once".to_string(),
                "deny".to_string(),
                "allow_all".to_string(),
            ],
        )
    } else {
        return None;
    };
    Some(prompt_json(agent_type, kind, screen, choices))
}

/// The prompt object every `detect_prompt` returns.
pub fn prompt_json(agent_type: &str, kind: &str, screen: &str, choices: Vec<String>) -> Value {
    json!({"agent_type": agent_type, "kind": kind, "text": screen.trim(), "choices": choices})
}

pub fn prompt_reply(prompt: &Value, choice: &str, allow_broad: bool) -> Result<Reply, String> {
    let kind = prompt
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match choice {
        "allow_once" => Ok(Reply::key("y")),
        "deny" => Ok(Reply::key("n")),
        "allow_all" if allow_broad && kind == "broad_approval" => Ok(Reply::key("a")),
        "allow_all" => Err("broad approval requires explicit allow_broad".into()),
        _ => Err("unsupported approval choice".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experimental_screen_is_ready_only_when_something_is_drawn_and_no_dialog_shows() {
        assert_eq!(experimental_standby_screen(""), StandbyScreen::NotReady);
        assert_eq!(experimental_standby_screen("   
  
"), StandbyScreen::NotReady);
        assert_eq!(experimental_standby_screen("my-agent v1.2
> "), StandbyScreen::Ready);
        assert_eq!(experimental_standby_screen("Do you trust the files in this folder?"), StandbyScreen::Dialog);
        assert_eq!(experimental_standby_screen("Run rm -rf? (y/n)"), StandbyScreen::Dialog);
        assert_eq!(experimental_standby_screen("Allow once  [y/n]"), StandbyScreen::Dialog);
        assert_eq!(experimental_standby_screen("Please LOG IN to continue"), StandbyScreen::NeedsAttention);
        assert_eq!(experimental_standby_screen("Set your API KEY"), StandbyScreen::NeedsAttention);
        assert_eq!(experimental_standby_screen("Update available: 2.0
Press enter to continue"), StandbyScreen::NeedsAttention);
    }

    #[test]
    fn broad_approval_requires_opt_in() {
        let broad = detect_prompt("claude", "Always allow this command?", true).unwrap();
        assert!(prompt_reply(&broad, "allow_all", false).is_err());
        assert_eq!(
            prompt_reply(&broad, "allow_all", true).unwrap(),
            Reply::key("a")
        );
        assert!(detect_prompt("claude", "Always allow this command?", false).is_none());
    }
}
