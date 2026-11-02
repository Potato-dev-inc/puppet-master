//! Tauri commands of the chat session over ACP (A2): start one (agent, folder), stop its turn, read its state.
//! Answering a permission question needs no command of its own: it is a question card (`question_answer`).
//! The same start/stop/state are on the bridge (`bridge/routes/acp_chat.rs`) for the phone.
//!
//! Errors are `"CODE: sentence"` like every command. A code that starts with `ACP_` and a message that says the
//! terminal can run the agent instead mean: run it in the terminal (`StartError::fallback`).

use super::*;
use crate::acp::chat::{self, ChatHost, StartDeps, StartError, StartRequest};
use crate::pty::status::PaneStatus;
use serde_json::json;

/// The chat host over the live app: Tauri events for the windows, the event log for the status.
pub struct LiveChatHost {
    app: AppHandle,
}

impl LiveChatHost {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ChatHost for LiveChatHost {
    fn status_changed(&self, pane_id: &str, status: PaneStatus) {
        crate::event_log::append_system_event(crate::events::SystemEvent::PaneStatusChanged {
            pane_id: crate::events::PaneId(pane_id.to_string()),
            status: status.as_str().to_string(),
        });
        let _ = self.app.emit("pty://status", json!({ "pane_id": pane_id, "status": status.as_str() }));
    }

    fn state_changed(&self, pane_id: &str, state: &Value) {
        let _ = self.app.emit("acp-chat-state", json!({ "paneId": pane_id, "state": state }));
    }

    fn questions_changed(&self, _raised: Option<&crate::questions::Question>) {
        crate::questions::routes::publish_changed(&self.app);
    }

    fn panes_changed(&self) {
        let _ = self.app.emit("pty://panes-changed", ());
    }
}

/// What a start needs from the live app.
pub fn live_deps(app: &AppHandle, registry: &Arc<Mutex<PaneRegistry>>) -> StartDeps {
    StartDeps { registry: registry.clone(), host: Arc::new(LiveChatHost::new(app.clone())), env: Arc::new(crate::acp::config::RealHost), conversations: Some(crate::acp::conversations::global().clone()) }
}

pub fn start_error_text(error: &StartError) -> String {
    let tail = if error.fallback { " The terminal can run this agent instead." } else { "" };
    format!("{}: {}{}", error.code, error.message, tail)
}

/// Starts a chat session: `{pane_id, agent, lane: "acp"}`. Needs the Labs switch, and the agent's choice to be Chat
/// unless `explicit` is true. The adapter is started before the pane exists, so a failure leaves nothing behind.
#[tauri::command]
pub async fn acp_chat_start(app: AppHandle, state: State<'_, AppState>, agent_id: String, folder: String, explicit: Option<bool>) -> Result<Value, String> {
    let deps = live_deps(&app, &state.registry);
    let request = StartRequest { agent_id, folder: PathBuf::from(folder), explicit: explicit.unwrap_or(false) };
    let started = tauri::async_runtime::spawn_blocking(move || chat::start_gated(&deps, request)).await.map_err(|e| format!("ACP_START_FAILED: {e}"))?;
    match started {
        Ok(session) => Ok(json!({ "pane_id": session.pane_id(), "agent": session.agent_id(), "lane": chat::LANE })),
        Err(error) => Err(start_error_text(&error)),
    }
}

/// Stops the turn of a chat session (`session/cancel`). True when a turn was running. The state says "stopping"
/// until the agent ends the turn.
#[tauri::command]
pub async fn acp_chat_cancel(pane_id: String) -> Result<bool, String> {
    let session = chat::get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id} is not a chat session"))?;
    session.cancel().map_err(|e| e.to_string())
}

/// "Keep waiting": the agent was quiet, the person chose to wait; the quiet window starts again. True when a turn was running.
#[tauri::command]
pub async fn acp_chat_keep_waiting(pane_id: String) -> Result<bool, String> {
    let session = chat::get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id} is not a chat session"))?;
    Ok(session.keep_waiting())
}

/// "Restart chat": brings the adapter of a chat that stopped back. Resolves with the new state; fails with the reason
/// the adapter could not start (the chat then still says it stopped).
#[tauri::command]
pub async fn acp_chat_restart(pane_id: String) -> Result<Value, String> {
    let chat = chat::get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id} is not a chat session"))?;
    let worker = chat.clone();
    tauri::async_runtime::spawn_blocking(move || worker.restart())
        .await
        .map_err(|e| format!("ACP_START_FAILED: {e}"))?
        .map_err(|e| format!("ACP_START_FAILED: The chat could not restart: {e}"))?;
    Ok(chat.state())
}

/// The session behind a chat pane: phase, stopping, what waits on the person, settings. `null` for a pane that is not
/// a chat session.
#[tauri::command]
pub async fn acp_chat_state(pane_id: String) -> Result<Value, String> {
    Ok(match chat::get(&pane_id) {
        Some(session) => {
            session.reconcile_cards();
            session.state()
        }
        None => Value::Null,
    })
}

/// Changes a setting the agent offers (model, mode, effort) in a chat session. Refused while a turn runs. Returns the
/// new state.
#[tauri::command]
pub async fn acp_chat_set_config(pane_id: String, option_id: String, value: String) -> Result<Value, String> {
    let session = chat::get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id} is not a chat session"))?;
    let worker = session.clone();
    tauri::async_runtime::spawn_blocking(move || worker.set_config(&option_id, &value))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
        .map_err(|e| match e {
            crate::acp::error::AcpError::Busy => "BUSY: The agent is working. Change this when the turn ends.".to_string(),
            crate::acp::error::AcpError::BadOption(what) => format!("INVALID_ARGUMENT: the agent does not offer {what}"),
            other => format!("ACP_CONFIG_FAILED: {other}"),
        })?;
    Ok(session.state())
}

/// The person's switch: may agents answer this chat's permission questions (default: no, only you)? Only this
/// window can turn it on; the bridge and the MCP tools have no route to it.
#[tauri::command]
pub async fn acp_chat_set_agent_answering(pane_id: String, allowed: bool) -> Result<Value, String> {
    let session = chat::get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id} is not a chat session"))?;
    session.set_agent_answering(allowed);
    Ok(session.state())
}

/// "I'm signed in — try again": sends the message the agent refused for want of a sign-in. Fails with the agent's own
/// reason (and the sign-in card stays) when it still wants one.
#[tauri::command]
pub async fn acp_chat_retry_sign_in(pane_id: String) -> Result<(), String> {
    let chat = chat::get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id} is not a chat session"))?;
    let result = tauri::async_runtime::spawn_blocking(move || chat.retry_after_sign_in()).await.map_err(|e| format!("ACP_START_FAILED: {e}"))?;
    match result {
        Ok(_) => Ok(()),
        Err(chat::SendError::Refused(why)) => Err(format!("ACP_SIGN_IN_NEEDED: {why}")),
        Err(chat::SendError::Busy) => Err("ACP_BUSY: The agent is working. Try again when the turn ends.".to_string()),
        Err(chat::SendError::Gone) => Err("PANE_NOT_FOUND: The chat was closed.".to_string()),
    }
}
