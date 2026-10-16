//! Tauri commands of the Agent Wizard (docs/specs/agent-wizard/SPEC.md 4.8). The webview is the app
//! session, so these are the same operations as `/agents/wizard/*` (the bridge route refuses MCP
//! sessions and remote devices; there is no MCP tool for any of this). Each runs on a blocking
//! thread: probes and the live test start programs and can take a while.

use super::*;
use crate::tui::declarative::wizard::{api, ask, Wizard};

async fn run(action: &'static str, body: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        api::handle_global(action, &body).map_err(|e| format!("{}: {}", e.code, e.message))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Find the program for `command`, try `--version`, open a session.
#[tauri::command]
pub async fn agent_wizard_start(command: String) -> Result<Value, String> {
    run("start", serde_json::json!({"command": command})).await
}

/// Run the fixed help and version probes.
#[tauri::command]
pub async fn agent_wizard_probe(session: String) -> Result<Value, String> {
    run("probe", serde_json::json!({"session": session})).await
}

/// Check a draft; the answer lists every problem in words an AI can act on. `expect` holds the drafter's
/// optional expectation blocks for the screen captures, by capture name.
#[tauri::command]
pub async fn agent_wizard_verify(session: String, toml: String, expect: Option<Value>) -> Result<Value, String> {
    run("verify", serde_json::json!({"session": session, "toml": toml, "expect": expect})).await
}

/// The visible test: starts the draft in a new pane and checks it (uses the plan: two small messages).
/// `plan` is the fingerprint of the planned start (`planned_visible`) the person was shown.
#[tauri::command]
pub async fn agent_wizard_visibletest(session: String, confirm: bool, plan: String) -> Result<Value, String> {
    run("visibletest", serde_json::json!({"session": session, "confirm": confirm, "plan": plan})).await
}

/// The guided capture (plan, start, status, act, show, finish, captures, test_status, discard). `body` is
/// the request without the action: `{session, op, ...}`.
#[tauri::command]
pub async fn agent_wizard_capture(body: Value) -> Result<Value, String> {
    run("capture", body).await
}

/// The live test: runs the verified draft three times against the real program (uses the plan).
/// `plan` is the fingerprint of the planned commands the person was shown (`verify` returns it).
#[tauri::command]
pub async fn agent_wizard_livetest(session: String, confirm: bool, plan: String) -> Result<Value, String> {
    run("livetest", serde_json::json!({"session": session, "confirm": confirm, "plan": plan})).await
}

/// Write the tested draft as an unapproved definition with its fixtures; reloads the registry.
#[tauri::command]
pub async fn agent_wizard_finish(session: String, provenance: Value) -> Result<Value, String> {
    run("finish", serde_json::json!({"session": session, "provenance": provenance})).await
}

/// Stop whatever runs and remove the work folder. Safe to call twice.
#[tauri::command]
pub async fn agent_wizard_cancel(session: String) -> Result<Value, String> {
    run("cancel", serde_json::json!({"session": session})).await
}

/// The local drafter: one read-only turn of an installed agent, in the session's work folder, with the
/// prompt on standard input. Returns `{"answer": "..."}`.
#[tauri::command]
pub async fn agent_wizard_ask(session: String, agent: String, prompt: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || ask::run_for_agent(Wizard::app(), &session, &agent, &prompt))
        .await
        .map_err(|e| e.to_string())?
        .map(|answer| serde_json::json!({"answer": answer}))
        .map_err(|e| format!("{}: {}", e.code, e.message))
}
