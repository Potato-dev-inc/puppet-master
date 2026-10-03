//! Behaviour shared by every TUI that has no special handling, plus the profile for unregistered
//! agent types.

use super::{Features, Kind, Reply, TuiProfile};
use serde_json::{json, Value};

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
