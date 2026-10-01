//! Structured view of the approval prompt currently blocking a TUI pane, and the answer path for
//! it. Nothing here answers on its own: a caller must name the prompt id it read, and broad
//! approvals still need an explicit opt-in.

use crate::operations::OperationError;
use crate::pty::status::is_shell_agent;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

/// Phrases that open an approval dialog. The prompt text is taken from the last such line, so
/// output scrolling above the dialog cannot change its id while the same question is showing.
const PROMPT_ANCHORS: &[&str] = &[
    "run this command",
    "allow once",
    "approve this command",
    "trust this workspace",
    "trust the authors",
    "allow all",
    "always allow",
];
/// Lines kept above the anchor: the command or file being approved is usually shown there.
const CONTEXT_ABOVE: usize = 3;
const FALLBACK_TAIL_LINES: usize = 25;

fn prompt_region(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    let anchor = lines.iter().rposition(|line| {
        let lower = line.to_ascii_lowercase();
        PROMPT_ANCHORS.iter().any(|phrase| lower.contains(phrase))
    });
    let start = match anchor {
        Some(index) => index.saturating_sub(CONTEXT_ABOVE),
        None => lines.len().saturating_sub(FALLBACK_TAIL_LINES),
    };
    lines[start..].join(
        "
",
    )
}

/// Prompt currently shown by `pane_id`, with a stable `prompt_id`, or `None` when the pane has
/// no supported prompt (shell panes and native OpenCode panes have their own mechanisms).
pub fn detect(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    allow_broad: bool,
) -> Option<Value> {
    let (agent_type, screen) = {
        let reg = registry.lock();
        let pane = reg.panes.get(pane_id)?;
        let contents = pane.screen.lock().screen().contents();
        (pane.info.agent_type.clone(), contents)
    };
    detect_in_screen(&agent_type, &screen, allow_broad)
}

pub(crate) fn detect_in_screen(agent_type: &str, screen: &str, allow_broad: bool) -> Option<Value> {
    if agent_type == "opencode_native" || is_shell_agent(agent_type) {
        return None;
    }
    let mut prompt = crate::agent_adapters::adapter_for(agent_type)
        .detect_prompt(&prompt_region(screen), allow_broad)?;
    let id = crate::agent_runs::stable_prompt_id(&prompt)?;
    prompt["prompt_id"] = Value::String(id);
    Some(prompt)
}

pub fn answer(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    prompt_id: &str,
    choice: &str,
    allow_broad: bool,
) -> Result<Value, OperationError> {
    let agent_type = registry
        .lock()
        .panes
        .get(pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| {
            OperationError::new(
                "PANE_NOT_FOUND",
                crate::pty::registry::unknown_pane_message(pane_id),
                false,
            )
        })?;
    let prompt = detect(registry, pane_id, true).ok_or_else(|| {
        OperationError::new(
            "STALE_PROMPT",
            "this pane is not showing a supported prompt; it may already be answered (re-check with wait_for_panes)",
            false,
        )
    })?;
    if prompt.get("prompt_id").and_then(Value::as_str) != Some(prompt_id) {
        return Err(OperationError::new(
            "STALE_PROMPT",
            "the prompt on screen changed since you read it; read the new prompt before answering",
            false,
        ));
    }
    let (key, reply_choice) = reply_key(&agent_type, &prompt, choice, allow_broad)?;
    crate::pty::registry::write_input(registry, app, pane_id, &key, true, false, None)
        .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
    Ok(
        json!({"pane_id": pane_id, "prompt_id": prompt_id, "kind": prompt["kind"], "choice": reply_choice, "sent_key": key}),
    )
}

/// Map a caller's choice to the key the agent expects. Broad approvals need `allow_broad`.
fn reply_key(
    agent_type: &str,
    prompt: &Value,
    choice: &str,
    allow_broad: bool,
) -> Result<(String, String), OperationError> {
    if matches!(choice, "allow_all" | "allow_broad" | "always") && !allow_broad {
        return Err(OperationError::new(
            "BROAD_APPROVAL_REQUIRES_OPT_IN",
            "allow_all requires allow_broad=true",
            false,
        ));
    }
    let choice = match choice {
        "once" => "allow_once",
        "reject" => "deny",
        "always" | "allow_broad" => "allow_all",
        other => other,
    };
    let reply = crate::agent_adapters::adapter_for(agent_type)
        .prompt_reply(prompt, choice, allow_broad)
        .map_err(|error| OperationError::new("INVALID_PROMPT_CHOICE", error, false))?;
    let key = reply.key.or(reply.text).ok_or_else(|| {
        OperationError::new(
            "INVALID_PROMPT_CHOICE",
            "adapter did not produce a reply",
            false,
        )
    })?;
    Ok((key, choice.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPROVAL: &str = "\
 $ cargo test --lib\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n\n → Add follow-up\n";

    #[test]
    fn cursor_approval_becomes_a_structured_prompt_with_a_stable_id() {
        let prompt = detect_in_screen("cursor_agent", APPROVAL, false).unwrap();
        assert_eq!(prompt["kind"], "command_approval");
        assert_eq!(prompt["choices"], json!(["allow_once", "deny"]));
        let again = detect_in_screen("cursor_agent", APPROVAL, false).unwrap();
        assert_eq!(prompt["prompt_id"], again["prompt_id"]);
        let other = APPROVAL.replace("cargo test --lib", "rm -rf build");
        let changed = detect_in_screen("cursor_agent", &other, false).unwrap();
        assert_ne!(
            prompt["prompt_id"], changed["prompt_id"],
            "a different command is a different prompt"
        );
    }

    #[test]
    fn output_far_above_the_prompt_does_not_change_its_id() {
        let dialog = "step one
step two
 $ cargo test --lib

 Run this command?
 Run once (y)
 Skip (esc or n)
";
        let short = format!(
            "earlier output
{dialog}"
        );
        let long = format!(
            "{}{dialog}",
            "earlier output
"
            .repeat(200)
        );
        let a = detect_in_screen("cursor_agent", &short, false).unwrap();
        let b = detect_in_screen("cursor_agent", &long, false).unwrap();
        assert_eq!(a["prompt_id"], b["prompt_id"]);
    }

    #[test]
    fn shell_and_idle_screens_have_no_prompt() {
        assert!(detect_in_screen("powershell", "PS C:\repo>", true).is_none());
        assert!(detect_in_screen("cursor_agent", " → Add follow-up\n", true).is_none());
    }

    #[test]
    fn choices_map_to_keys_and_broad_needs_opt_in() {
        let prompt = detect_in_screen("cursor_agent", APPROVAL, false).unwrap();
        assert_eq!(
            reply_key("cursor_agent", &prompt, "allow_once", false)
                .unwrap()
                .0,
            "y"
        );
        assert_eq!(
            reply_key("cursor_agent", &prompt, "deny", false).unwrap().0,
            "n"
        );
        assert_eq!(
            reply_key("cursor_agent", &prompt, "allow_all", false)
                .unwrap_err()
                .code,
            "BROAD_APPROVAL_REQUIRES_OPT_IN"
        );
        assert_eq!(
            reply_key("cursor_agent", &prompt, "sudo", false)
                .unwrap_err()
                .code,
            "INVALID_PROMPT_CHOICE"
        );
    }
}
