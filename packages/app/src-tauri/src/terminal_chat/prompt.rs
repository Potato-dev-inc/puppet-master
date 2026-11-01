//! The question an agent in a terminal chat is asking, and the answer a person gives from the chat.
//!
//! Two reads and one act, all behind Tauri commands (`commands/terminal_chat_prompt.rs`), none behind the
//! bridge, MCP or the phone:
//!
//! * [`view`]: the prompt the pane shows right now, from the SAME reading `answer_prompt` uses
//!   (`pane_prompt::detect`, which asks the agent's profile), shaped for a card. `None` when the screen shows
//!   nothing recognisable.
//! * [`answer`]: re-reads the screen, refuses a view that is no longer the one on screen
//!   ([`STALE`]), refuses an option the view did not list, and then presses the keys through
//!   `agent_runs::answer_for_pane`, the code behind `POST /agents/answer` (stale-prompt, replay, one-answer-per-
//!   occurrence and audit guards included).
//!
//! Only a person's click answers: [`answer`] needs a `UserGesture` (a fresh, trusted, single-use click made by
//! `lib/user-gesture.ts`), even for a refusal. There is no timer, no default option, no auto-answer, and nothing
//! here lists an option as preferred. Nothing in this file names an agent: what a prompt looks like and which
//! keys an option needs belong to the profile.

use crate::operations::OperationError;
use crate::pane_prompt::answers::{self, UserGesture};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;
use tauri::AppHandle;

/// The view the card holds is not what the pane shows now (the prompt changed, went away, or came back as a
/// new occurrence). The card re-reads and asks again.
pub const STALE: &str = "TERMINAL_CHAT_PROMPT_STALE";
/// The option id is not one the view listed.
pub const UNKNOWN_OPTION: &str = "TERMINAL_CHAT_UNKNOWN_OPTION";
/// No click came with the answer.
pub const NEEDS_A_PERSON: &str = "ANSWER_NEEDS_A_PERSON";

/// One thing the person can pick: the id to send back and the agent's own words for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptOption {
    pub id: String,
    pub name: String,
}

/// What the card shows. No field is a recommended or default option.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptView {
    /// Identifies this question on this screen: changes with the question and with every new occurrence.
    pub prompt_id: String,
    /// The question, redacted, in the agent's words.
    pub text: String,
    pub options: Vec<PromptOption>,
    /// `trust` (a folder or startup gate), `permission` (allow / deny a command or an edit) or `question`
    /// (a menu read by its shape).
    pub kind: String,
    /// The profile's own kind (`workspace_trust`, `command_approval`, ...), for the card's wording.
    pub raw_kind: String,
    /// A plain one-line summary of what saying yes would do, when the screen gave one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// How risky saying yes is (`prompt_risk`), when assessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<crate::prompt_risk::Risk>,
}

/// A refusal the UI can act on: `code` is stable ([`STALE`] means "read again").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptError {
    pub code: String,
    pub message: String,
}

impl PromptError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        PromptError { code: code.to_string(), message: message.into() }
    }

    /// The wire form of a Tauri command's error: `CODE: message`.
    pub fn wire(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

impl From<OperationError> for PromptError {
    fn from(error: OperationError) -> Self {
        PromptError { code: error.code.to_string(), message: error.message }
    }
}

/// A prompt as read from the screen: the profile's own id and the card built from it.
struct Reading {
    inner_id: String,
    view: PromptView,
}

fn kind_word(raw: &str) -> &'static str {
    if crate::pane_prompt::is_startup_gate_kind(raw) {
        "trust"
    } else if raw == "unknown_question" {
        "question"
    } else {
        "permission"
    }
}

/// The id a card holds: the profile's id for the question plus the pane's occurrence count, so the same question
/// asked again (or a menu redrawn) is a different view.
fn view_id(inner_id: &str, generation: u64) -> String {
    crate::agent_runs::stable_prompt_id(&serde_json::json!({ "prompt": inner_id, "generation": generation }))
        .map(|id| id.replacen("prompt-", "tcp-", 1))
        .unwrap_or_else(|| format!("tcp-{generation}"))
}

fn read(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<Reading> {
    // Generation first: if the occurrence moves between the two reads, the id is older than the screen and the
    // answer is refused as stale, never the other way round.
    let generation = answers::occurrence_of(pane_id);
    let detected = crate::pane_prompt::detect(registry, pane_id, false)?;
    reading_from(&detected, generation)
}

/// The card for a detected prompt (`pane_prompt::detect`), at occurrence `generation`.
fn reading_from(detected: &serde_json::Value, generation: u64) -> Option<Reading> {
    let shaped = crate::pane_prompt::PanePrompt::from_detected(detected)?;
    // A prompt with nothing to pick (a startup gate the profile cannot key) is not answerable from a card.
    if shaped.choices.is_empty() {
        return None;
    }
    let view = PromptView {
        prompt_id: view_id(&shaped.prompt_id, generation),
        text: shaped.text,
        options: shaped.choices.into_iter().map(|c| PromptOption { id: c.id, name: c.label }).collect(),
        kind: kind_word(&shaped.kind).to_string(),
        raw_kind: shaped.kind,
        summary: shaped.summary,
        risk: shaped.risk,
    };
    Some(Reading { inner_id: shaped.prompt_id, view })
}

/// The prompt the pane shows now, or `None`.
pub fn view(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<PromptView> {
    read(registry, pane_id).map(|reading| reading.view)
}

/// The checks before any key: pure, so they are tested without a pane. `now` is what the screen shows now.
fn check(now: Option<&PromptView>, prompt_id: &str, option_id: &str) -> Result<(), PromptError> {
    let Some(now) = now else {
        return Err(PromptError::new(STALE, "the question is no longer on the screen; nothing was sent"));
    };
    if now.prompt_id != prompt_id {
        return Err(PromptError::new(STALE, "the question on the screen changed since the card was drawn; nothing was sent"));
    }
    if !now.options.iter().any(|option| option.id == option_id) {
        return Err(PromptError::new(UNKNOWN_OPTION, "that option is not one the question offers; nothing was sent"));
    }
    Ok(())
}

/// The person picked `option_id` on the card for `prompt_id`. `gesture` is their click: without one nothing is
/// sent, not even a refusal.
pub fn answer(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    prompt_id: &str,
    option_id: &str,
    gesture: Option<&UserGesture>,
) -> Result<(), PromptError> {
    let Some(gesture) = gesture else {
        return Err(PromptError::new(NEEDS_A_PERSON, "Nothing was sent: a question in a terminal chat is only answered by a person's click."));
    };
    let reading = read(registry, pane_id);
    check(reading.as_ref().map(|r| &r.view), prompt_id, option_id)?;
    let inner = reading.map(|r| r.inner_id).unwrap_or_default();
    let reply = crate::agent_runs::answer_for_pane(registry, app, pane_id, &inner, option_id, Some(gesture))?;
    // A repeat pressed nothing: it must not rewrite the card's tombstone (same rule as `answer_pane_prompt`).
    if !crate::pane_prompt::reply_was_repeat(&reply) {
        crate::attention::note_desktop_answer(pane_id, option_id);
    }
    Ok(())
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
