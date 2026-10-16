//! JSON entry points of the wizard, shared by the bridge route (`bridge/routes/wizard.rs`) and the
//! Tauri commands (`commands/agent_wizard.rs`). `handle` is pure over a `Wizard`, so tests drive it
//! with a temp environment; `handle_global` is the same on the process-wide wizard and reloads the
//! definitions registry after a finish.

use serde_json::{json, Value};

use crate::tui::declarative::service::ServiceError;

use super::report::WizardError;
use super::session::Wizard;

/// The actions behind `/agents/wizard/<action>`.
pub const ACTIONS: &[&str] = &["start", "probe", "capture", "verify", "livetest", "finish", "cancel"];

pub fn session_id(body: &Value) -> Result<String, WizardError> {
    body.get("session")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| WizardError::bad_request("session is required"))
}

pub fn handle(wizard: &Wizard, action: &str, body: &Value) -> Result<Value, WizardError> {
    match action {
        "start" => super::locate::start(wizard, body),
        "probe" => super::probe::run(wizard, body),
        "verify" => super::verify::run(wizard, body),
        "livetest" => super::livetest::run(wizard, body),
        "finish" => super::finish::run(wizard, body),
        "cancel" => {
            let id = session_id(body)?;
            let state = wizard.cancel(&id);
            Ok(json!({"ok": true, "state": state}))
        }
        "capture" => Err(WizardError::new(
            501,
            "WIZARD_NOT_YET",
            "The visible-screen path comes in a later version. This version learns agents that have a headless mode.",
        )),
        other => Err(WizardError::new(404, "NOT_FOUND", format!("unknown wizard action {other}"))),
    }
}

/// `handle` on the process-wide wizard. A finish also reloads the registry so the new definition
/// shows up (unapproved) at once, and returns the listing.
pub fn handle_global(action: &str, body: &Value) -> Result<Value, ServiceError> {
    let mut out = handle(Wizard::app(), action, body)?;
    if action == "finish" {
        out["listing"] = crate::tui::declarative::service::reload_global_with_diff();
    }
    Ok(out)
}
