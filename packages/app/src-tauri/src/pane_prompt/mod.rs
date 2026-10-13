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

/// Refusal to type a task into a pane that is showing a prompt. Dialogs act on hotkeys (Cursor's
/// trust dialog takes `a`/`q`), so typing a prompt over one answers it with the prompt's letters
/// and the task is lost. `None` when the pane is free for input.
pub(crate) fn dispatch_blocker(agent_type: &str, screen: &str) -> Option<OperationError> {
    let prompt = detect_in_screen(agent_type, screen, false)?;
    let mut error = OperationError::new(
        "PANE_BLOCKED_BY_PROMPT",
        "the pane is showing a prompt (workspace trust or command approval); the task was not typed into it. Answer the prompt with answer_prompt, then retry",
        true,
    );
    error.context = json!({"prompt_id": prompt.get("prompt_id"), "kind": prompt.get("kind")});
    Some(error)
}

/// `Run command: <command> (in <cwd>)`, so a caller sees what it is approving without the pane.
pub(crate) fn command_summary(prompt: &Value) -> String {
    let command = prompt
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
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
    // No command to anchor on: hash the semantic identity, never the layout. A profile may name
    // it (`identity`); otherwise it is the prompt text with wrapping, box art and case removed, so
    // a dialog that re-renders at a different width keeps the id that was handed out.
    let identity = prompt
        .get("identity")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            prompt
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect()
        });
    crate::agent_runs::stable_prompt_id(&json!({
        "agent_type": prompt.get("agent_type"),
        "kind": prompt.get("kind"),
        "choices": prompt.get("choices"),
        "identity": identity,
    }))
}

/// The prompt blocking `pane_id` as a run's `required_action` (with an id, kind, options and a
/// summary naming the command), or `None` when the pane shows no supported prompt. The id is the
/// same one `detect` and `answer_prompt` use.
pub fn required_action(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<Value> {
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


/// A pane prompt as the desktop UI shows it (`pane_prompt` command, `pty://prompt` event).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanePrompt {
    pub prompt_id: String,
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub choices: Vec<PromptChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PromptChoice {
    pub id: String,
    pub label: String,
}

fn choice_label(id: &str) -> String {
    let spaced = id.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl PanePrompt {
    /// Shape a detected prompt (`detect` / `detect_in_screen`) for the UI; `None` without an id.
    pub fn from_detected(prompt: &Value) -> Option<Self> {
        let text_of = |key: &str| prompt.get(key).and_then(Value::as_str).map(str::to_string);
        let choices = prompt
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|id| PromptChoice { id: id.to_string(), label: choice_label(id) })
            .collect();
        Some(Self {
            prompt_id: text_of("prompt_id")?,
            kind: text_of("kind").unwrap_or_else(|| "unknown".into()),
            text: text_of("text").unwrap_or_default(),
            summary: text_of("summary"),
            choices,
        })
    }
}

/// The prompt a pane shows right now, shaped for the UI. Never offers broad approvals.
pub fn detect_for_ui(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<PanePrompt> {
    PanePrompt::from_detected(&detect(registry, pane_id, false)?)
}

/// Per-pane memory behind `pty://prompt`: whether the pane was waiting and the last prompt id sent.
#[derive(Debug, Default)]
pub struct PromptTrack {
    waiting: bool,
    last_id: Option<String>,
}

/// What to emit on `pty://prompt` after a screen update: `Some(prompt)` is an event to send
/// (`Some(None)` = prompt cleared), `None` = nothing to say. Emits on entering `waiting_input`
/// (even with no parsed prompt), on leaving it, and when the detected prompt changes. `detect` is
/// only called while waiting.
pub fn next_prompt_event(
    track: &mut PromptTrack,
    waiting: bool,
    detect: impl FnOnce() -> Option<PanePrompt>,
) -> Option<Option<PanePrompt>> {
    if !waiting {
        let was = std::mem::take(&mut track.waiting);
        track.last_id = None;
        return was.then_some(None);
    }
    let prompt = detect();
    let id = prompt.as_ref().map(|p| p.prompt_id.clone());
    let entered = !std::mem::replace(&mut track.waiting, true);
    if entered || id != track.last_id {
        track.last_id = id;
        return Some(prompt);
    }
    None
}

#[cfg(test)]
mod tests;
