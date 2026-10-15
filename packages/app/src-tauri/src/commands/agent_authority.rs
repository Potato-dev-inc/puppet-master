//! Tauri commands for the Settings window: the two agent-authority switches and the read-only
//! list of recent approvals and control changes. Only the app's own webview can call these; the
//! bridge has no route for them, so a phone or an agent cannot flip a switch.

use crate::agent_authority::config::{self, Config};
use crate::audit::{self, Entry, VerifyReport};

const LABEL_ASK: &str = "Ask me before an agent controls a pane it didn't start";
const LABEL_MERGE: &str = "Let agents merge separate copies";

#[tauri::command]
pub fn agent_authority_get() -> Config {
    config::current()
}

/// Change one or both switches. Each change is written to the audit log.
#[tauri::command]
pub fn agent_authority_set(
    ask_before_agent_control: Option<bool>,
    agents_may_merge: Option<bool>,
) -> Result<Config, String> {
    let before = config::current();
    let next = Config {
        ask_before_agent_control: ask_before_agent_control.unwrap_or(before.ask_before_agent_control),
        agents_may_merge: agents_may_merge.unwrap_or(before.agents_may_merge),
    };
    config::set(next)?;
    audit::record_setting("ask_before_agent_control", LABEL_ASK, before.ask_before_agent_control, next.ask_before_agent_control);
    audit::record_setting("agents_may_merge", LABEL_MERGE, before.agents_may_merge, next.agents_may_merge);
    Ok(next)
}

/// The newest lines of the audit log, newest first.
#[tauri::command]
pub fn audit_recent(limit: Option<usize>) -> Vec<Entry> {
    audit::global().recent(limit.unwrap_or(50).clamp(1, 500))
}

/// Check the whole chain. `ok: false` names the first line that does not fit.
#[tauri::command]
pub fn audit_verify() -> VerifyReport {
    audit::global().verify()
}
