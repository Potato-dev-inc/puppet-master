//! Structured view of the approval prompt currently blocking a TUI pane, and the answer path for
//! it. Nothing here answers on its own: a caller must name the prompt id it read, and broad
//! approvals still need an explicit opt-in.

use crate::operations::OperationError;
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
/// Wide enough for a wrapped multi-line command; the prompt id does not depend on these lines
/// when the command itself was extracted.
const CONTEXT_ABOVE: usize = 14;
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
    if matches!(
        crate::tui::profile_for(agent_type).features().kind,
        crate::tui::Kind::NativeProtocol | crate::tui::Kind::Shell
    ) {
        return None;
    }
    let mut prompt = crate::agent_adapters::adapter_for(agent_type)
        .detect_prompt(&prompt_region(screen), allow_broad)?;
    if prompt.get("command").and_then(Value::as_str).is_some() {
        if prompt.get("cwd").is_none() {
            let dir = prompt
                .get("command")
                .and_then(Value::as_str)
                .and_then(crate::agent_runs::approval_policy::leading_cd_dir);
            if let Some(dir) = dir {
                prompt["cwd"] = Value::String(dir);
            }
        }
        prompt["summary"] = Value::String(command_summary(&prompt));
    }
    let id = prompt_id_for(&prompt)?;
    prompt["prompt_id"] = Value::String(id);
    Some(prompt)
}

/// `Run command: <command> (in <cwd>)`, so a caller sees what it is approving without the pane.
pub(crate) fn command_summary(prompt: &Value) -> String {
    let command = prompt.get("command").and_then(Value::as_str).unwrap_or_default();
    match prompt.get("cwd").and_then(Value::as_str) {
        Some(cwd) if !command.contains(cwd) => format!("Run command: {command} (in {cwd})"),
        _ => format!("Run command: {command}"),
    }
}

/// The id names what is being approved. When the command was read off the screen it is the
/// identity (so redraws, a still-arriving option list or scrolling do not change it); otherwise the
/// whole prompt is hashed.
fn prompt_id_for(prompt: &Value) -> Option<String> {
    if prompt.get("command").and_then(Value::as_str).is_some() {
        return crate::agent_runs::stable_prompt_id(&json!({
            "agent_type": prompt.get("agent_type"),
            "kind": prompt.get("kind"),
            "command": prompt.get("command"),
            "cwd": prompt.get("cwd"),
        }));
    }
    crate::agent_runs::stable_prompt_id(prompt)
}

/// The prompt blocking `pane_id` as a run's `required_action` (with an id, kind, options and a
/// summary naming the command), or `None` when the pane shows no supported prompt. The id is the
/// same one `detect` and `answer_prompt` use.
pub fn required_action(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Option<Value> {
    let prompt = detect(registry, pane_id, false)?;
    Some(required_action_from_prompt(prompt))
}

pub(crate) fn required_action_from_prompt(prompt: Value) -> Value {
    let id = prompt.get("prompt_id").cloned().unwrap_or(Value::Null);
    let options: Vec<Value> = prompt
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|choice| json!({"id": choice, "label": choice}))
        .collect();
    let summary = prompt
        .get("summary")
        .or_else(|| prompt.get("text"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut action = json!({
        "id": id,
        "prompt_id": id,
        "kind": prompt.get("kind"),
        "summary": summary,
        "options": options,
        "raw_prompt": prompt,
    });
    for key in ["command", "cwd"] {
        if let Some(value) = action["raw_prompt"].get(key).cloned() {
            action[key] = value;
        }
    }
    action
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
    #[test]
    fn a_command_approval_keeps_its_id_through_redraws_and_scrolling() {
        // Real capture (2026-10-04): the answer route used to hash the whole screen and so never
        // matched the id it had handed out.
        let screen = include_str!("tui/cursor/fixtures/command_approval_visible.txt");
        let first = super::detect_in_screen("cursor_agent", screen, true).expect("prompt");
        assert_eq!(first["kind"], "command_approval");
        assert!(first["command"].as_str().unwrap().contains("python todo.py add buy-milk"));
        let id = first["prompt_id"].as_str().unwrap().to_string();
        // The same dialog with more scrollback above it, a spinner tick, and without allow_broad.
        let scrolled = format!("old output line
another line
{screen}");
        let again = super::detect_in_screen("cursor_agent", &scrolled, false).expect("prompt");
        assert_eq!(again["prompt_id"].as_str(), Some(id.as_str()));
        // A different command is a different prompt.
        let other = screen.replace("add buy-milk", "rm buy-milk");
        let changed = super::detect_in_screen("cursor_agent", &other, true).expect("prompt");
        assert_ne!(changed["prompt_id"].as_str(), Some(id.as_str()));
    }

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

    const CD_SCREEN: &str = "\
 $ cd \"C:\\x\" && python todo.py add \"buy milk\"\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n\n → Add follow-up\n";

    #[test]
    fn prompt_names_the_command_cwd_and_summary() {
        let prompt = detect_in_screen("cursor_agent", CD_SCREEN, false).unwrap();
        assert_eq!(prompt["command"], "cd \"C:\\x\" && python todo.py add \"buy milk\"");
        assert_eq!(prompt["cwd"], "C:\\x");
        assert!(prompt["summary"].as_str().unwrap().starts_with("Run command: cd"));
        assert_eq!(prompt["fully_drawn"], true);
        let action = required_action_from_prompt(prompt.clone());
        assert_eq!(action["prompt_id"], prompt["prompt_id"]);
        assert_eq!(action["command"], prompt["command"]);
        assert!(action["summary"].as_str().unwrap().contains("python todo.py add"));
    }

    #[test]
    fn id_is_stable_while_the_option_list_is_still_drawing() {
        let partial = " $ cargo test --lib\n\n Run this command?\n";
        let full = " $ cargo test --lib\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n";
        let a = detect_in_screen("cursor_agent", partial, false).unwrap();
        let b = detect_in_screen("cursor_agent", full, false).unwrap();
        assert_eq!(a["prompt_id"], b["prompt_id"]);
        assert_eq!(a["fully_drawn"], false);
        assert_eq!(b["fully_drawn"], true);
        assert!(a["prompt_id"].is_string());
    }

    #[test]
    fn multi_line_command_survives_the_context_window() {
        let screen = " $ python -c \"x\" &&\n   python a.py &&\n   python b.py &&\n   python c.py &&\n   python d.py\n\n Run this command?\n Run once (y)\n";
        let prompt = detect_in_screen("cursor_agent", screen, false).unwrap();
        assert!(prompt["command"].as_str().unwrap().starts_with("python -c"));
        assert!(prompt["command"].as_str().unwrap().ends_with("python d.py"));
    }

    #[test]
    fn auto_approval_end_to_end_on_a_real_screen() {
        use crate::agent_runs::approval_policy::decide;
        let allowed = vec!["python todo.py add *".to_string()];
        let prompt = detect_in_screen("cursor_agent", CD_SCREEN, false).unwrap();
        assert!(decide(&prompt, CD_SCREEN, &allowed, Some("C:\\x")).is_some());
        let evil = CD_SCREEN.replace("add \"buy milk\"", "add x && rm -rf /");
        let prompt = detect_in_screen("cursor_agent", &evil, false).unwrap();
        assert!(decide(&prompt, &evil, &allowed, Some("C:\\x")).is_none());
        let partial = " $ python todo.py add x\n\n Run this command?\n";
        let prompt = detect_in_screen("cursor_agent", partial, false).unwrap();
        assert!(
            decide(&prompt, partial, &allowed, None).is_none(),
            "never answer a dialog whose options are not drawn yet"
        );
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
