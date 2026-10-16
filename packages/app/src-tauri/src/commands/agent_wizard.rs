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

/// Check a draft; the answer lists every problem in words an AI can act on.
#[tauri::command]
pub async fn agent_wizard_verify(session: String, toml: String) -> Result<Value, String> {
    run("verify", serde_json::json!({"session": session, "toml": toml})).await
}

/// The live test: runs the verified draft three times against the real program (uses the plan).
#[tauri::command]
pub async fn agent_wizard_livetest(session: String, confirm: bool) -> Result<Value, String> {
    run("livetest", serde_json::json!({"session": session, "confirm": confirm})).await
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

/// The local drafter: one read-only question to an installed agent, in the session's work folder.
/// Returns `{"answer": "..."}`.
#[tauri::command]
pub async fn agent_wizard_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    session: String,
    agent: String,
    prompt: String,
) -> Result<Value, String> {
    let request = ask::prepare(Wizard::app(), &session, &agent, &prompt).map_err(|e| format!("{}: {}", e.code, e.message))?;
    let registry = state.registry.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::agent_runs::quick::quick_ask(request, registry, app, None)
    })
    .await
    .map_err(|e| e.to_string())?;
    match result {
        Ok(value) => match value.get("status").and_then(Value::as_str) {
            Some("running") => Err("DRAFTER_TIMEOUT: the agent is taking too long to answer; try again or pick another agent".into()),
            Some("needs_input") => {
                Err("DRAFTER_NEEDS_INPUT: the agent stopped to ask for permission; pick another agent".into())
            }
            _ => match value.get("answer").and_then(Value::as_str).map(str::trim).filter(|a| !a.is_empty()) {
                Some(answer) => Ok(serde_json::json!({"answer": answer})),
                None => Err("DRAFTER_EMPTY: the agent gave an empty answer".into()),
            },
        },
        Err(error) => {
            let (_, plain) = crate::agent_health::classify_failure(&error.code, &error.message);
            Err(format!("{}: {}", error.code, plain))
        }
    }
}
