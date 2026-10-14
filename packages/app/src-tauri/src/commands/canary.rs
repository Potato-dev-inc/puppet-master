//! Tauri commands for version checks (Settings > Agents, the pane header chip, the Inbox card).

use crate::canary::{self, model::{AgentView, Status}};

/// Every agent's version standing, the last check, Inbox cards and the saved-screen count.
#[tauri::command(async)]
pub fn canary_status() -> Status {
    canary::status()
}

/// "Check this agent": starts it in a scratch folder, reaches its input box and quits without
/// sending anything. Takes up to a minute; spends no tokens.
#[tauri::command(async)]
pub fn canary_check(agent: String) -> Result<AgentView, String> {
    canary::check(&agent)
}

#[tauri::command]
pub fn canary_set_auto(on: bool) -> Status {
    canary::set_auto_check(on);
    canary::status()
}

#[tauri::command]
pub fn canary_dismiss_notice(id: String) {
    canary::dismiss_notice(&id);
}

/// Opens the folder of screens Puppet Master did not recognise; returns its path.
#[tauri::command]
pub fn canary_open_captures() -> Result<String, String> {
    canary::open_captures()
}
