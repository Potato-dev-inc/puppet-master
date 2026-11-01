//! The bridge's view of chat sessions (`POST /acp/sessions`, `GET /acp/sessions/{pane}`,
//! `POST /acp/sessions/{pane}/cancel`), free of the HTTP and Tauri types so the tests drive it directly.

use serde_json::{json, Value};

use super::{get, start_gated, StartDeps, StartRequest, LANE};

pub type ApiResult = Result<(u16, Value), (u16, Value)>;

fn not_a_chat(pane_id: &str) -> (u16, Value) {
    (404, json!({ "code": "PANE_NOT_FOUND", "error": format!("{pane_id} is not a chat session") }))
}

/// `POST /acp/sessions` `{agent, folder, lane?}`: `lane: "acp"` asks for a chat session whatever the agent's saved
/// choice is (the Labs switch always applies). A refusal says whether the terminal can run the agent instead.
pub fn start(body: &Value, deps: &StartDeps) -> ApiResult {
    let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let agent = text("agent").ok_or_else(|| (400, json!({ "code": "INVALID_ARGUMENT", "error": "agent is required" })))?;
    let folder = text("folder").ok_or_else(|| (400, json!({ "code": "INVALID_ARGUMENT", "error": "folder is required" })))?;
    let explicit = text("lane").as_deref() == Some(LANE);
    match start_gated(deps, StartRequest { agent_id: agent, folder: folder.into(), explicit }) {
        Ok(session) => Ok((200, json!({ "pane_id": session.pane_id(), "agent": session.agent_id(), "lane": LANE }))),
        Err(error) => {
            let status = match error.code {
                "INVALID_ARGUMENT" => 400,
                "ACP_START_FAILED" | "ACP_SIGN_IN_NEEDED" => 502,
                _ => 409,
            };
            Err((status, json!({ "code": error.code, "error": error.message, "fallback": error.fallback.then_some("terminal"), "recoverable": error.fallback })))
        }
    }
}

/// `GET /acp/sessions/{pane}`.
pub fn state(pane_id: &str) -> ApiResult {
    let session = get(pane_id).ok_or_else(|| not_a_chat(pane_id))?;
    session.reconcile_cards();
    Ok((200, session.state()))
}

/// `POST /acp/sessions/{pane}/cancel`.
pub fn cancel(pane_id: &str) -> ApiResult {
    let session = get(pane_id).ok_or_else(|| not_a_chat(pane_id))?;
    match session.cancel() {
        Ok(stopped) => Ok((200, json!({ "pane_id": pane_id, "stopped": stopped }))),
        Err(error) => Err((502, json!({ "code": "ACP_CANCEL_FAILED", "error": error.to_string() }))),
    }
}
