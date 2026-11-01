//! Tauri commands for "Add chat (ACP)" on an agent that has no `[acp]` table (`acp::overlay`). Desktop app only:
//! no bridge route, no MCP tool, so a phone or an agent cannot add a command the app will start. The person's
//! click is the approval: `acp_overlay_set` takes the exact command line and refuses unless `confirmed` is true.
//! All three need the Labs switch (like `acp_conversation_create`).

use serde_json::{json, Value};

use crate::acp::overlay::{self, OverlayEntry, OverlayStore};
use crate::acp::{self, config};

fn labs_gate() -> Result<(), String> {
    if acp::chat_enabled() {
        Ok(())
    } else {
        Err("FEATURE_OFF: chat (ACP) is switched off in Labs".into())
    }
}

/// An agent that is registered and can take an overlay: a runnable agent (no shell) with no table of its own.
fn check_target(agent: &str) -> Result<String, String> {
    let id = agent.trim().to_ascii_lowercase();
    let Some(profile) = crate::tui::registry::registry().find(&id) else {
        return Err(format!("INVALID_ARGUMENT: unknown agent {agent}"));
    };
    if profile.features().kind == crate::tui::Kind::Shell {
        return Err(format!("INVALID_ARGUMENT: {agent} is a shell, not an agent"));
    }
    if profile.acp_table().is_some() {
        return Err(format!("INVALID_ARGUMENT: {agent} already has its own chat (ACP) definition; the overlay is only for agents without one"));
    }
    Ok(profile.id().to_string())
}

/// The fixed help probes of the agent's program, then the text analysis (`acp_detect`). Starts a temporary
/// wizard session, runs nothing but the fixed probes, and always cancels the session.
fn detect_in(wizard: &crate::tui::declarative::wizard::Wizard, program: &str) -> Result<Value, String> {
    use crate::tui::declarative::wizard::api;
    let fail = |e: crate::tui::declarative::wizard::WizardError| format!("{}: {}", e.code, e.message);
    let started = api::handle(wizard, "start", &json!({"command": program})).map_err(fail)?;
    let session = started["session"].as_str().unwrap_or_default().to_string();
    let result = api::handle(wizard, "probe", &json!({"session": session}))
        .and_then(|_| api::handle(wizard, "acp_detect", &json!({"session": session})))
        .map_err(fail);
    let _ = api::handle(wizard, "cancel", &json!({"session": session}));
    result
}

/// How the agent's program offers the Agent Client Protocol: `{ok, command, candidates: [{command, args, evidence,
/// confidence}], stdio_hint, mentions_protocol}`, the same as the wizard's `acp_detect`. Only the wizard's fixed
/// help probes run (`--version`, `--help` and the like); nothing else starts.
#[tauri::command]
pub async fn acp_overlay_detect(agent: String) -> Result<Value, String> {
    labs_gate()?;
    let id = check_target(&agent)?;
    let program = overlay::program_of(&id).ok_or_else(|| format!("INVALID_ARGUMENT: unknown agent {agent}"))?;
    tauri::async_runtime::spawn_blocking(move || detect_in(crate::tui::declarative::wizard::Wizard::app(), &program))
        .await
        .map_err(|e| e.to_string())?
}

/// Validates and writes the overlay. `acp` = `{command, args, env_allow, resume_command?}` is the exact command
/// line the person read; nothing is written unless `confirmed`.
pub(crate) fn set_in(store: &OverlayStore, agent: &str, acp: &Value, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("NOT_CONFIRMED: the person must confirm the exact command line (confirmed: true)".into());
    }
    let id = check_target(agent)?;
    let entry = OverlayEntry::from_json(acp)?;
    let label = crate::tui::descriptor_for(&id).map(|d| d.label).unwrap_or_else(|| id.clone());
    overlay::build_config(&id, &label, &entry)?;
    store.put(&id, &entry)
}

/// Saves the chat setup of an agent that has none. Returns the lane view (the agent now has an `overlay: true` row).
#[tauri::command]
pub fn acp_overlay_set(agent: String, acp: Value, confirmed: bool) -> Result<Value, String> {
    labs_gate()?;
    set_in(&OverlayStore::in_app_data(), &agent, &acp, confirmed)?;
    Ok(super::acp_lane::view())
}

/// Removes the chat setup. Existing conversations stay as records (they say the agent is not available). Returns the view.
#[tauri::command]
pub fn acp_overlay_remove(agent: String) -> Result<Value, String> {
    labs_gate()?;
    if agent.trim().is_empty() {
        return Err("INVALID_ARGUMENT: agent is required".into());
    }
    OverlayStore::in_app_data().remove(&agent)?;
    Ok(super::acp_lane::view())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (OverlayStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("pm-acp-ovcmd-{}", uuid::Uuid::new_v4()));
        (OverlayStore::at(dir.join("acp_overlay.json")), dir)
    }

    fn target() -> String {
        overlay::addable_agents(&OverlayStore::at(std::env::temp_dir().join("pm-nonexistent-overlay.json"))).remove(0).id
    }

    #[test]
    fn set_needs_the_confirmation_and_writes_only_then() {
        let (store, dir) = temp();
        let id = target();
        let acp = json!({"command": "adapter", "args": ["acp"], "env_allow": ["MY_KEY"]});
        assert!(set_in(&store, &id, &acp, false).unwrap_err().starts_with("NOT_CONFIRMED"));
        assert!(store.get(&id).is_none());
        set_in(&store, &id, &acp, true).unwrap();
        assert_eq!(store.get(&id).unwrap().args, vec!["acp".to_string()]);
        set_in(&store, &id.to_uppercase(), &json!({"command": "adapter2"}), true).unwrap();
        assert_eq!(store.get(&id).unwrap().command, "adapter2", "setting again replaces");
        assert!(store.remove(&id).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn set_refuses_unknown_shell_own_table_and_invalid() {
        let (store, dir) = temp();
        let acp = json!({"command": "adapter"});
        assert!(set_in(&store, "no_such_agent_zz", &acp, true).unwrap_err().contains("unknown agent"));
        let shell = crate::tui::registry::registry().ordered().into_iter().find(|p| p.features().kind == crate::tui::Kind::Shell);
        if let Some(shell) = shell {
            assert!(set_in(&store, shell.id(), &acp, true).unwrap_err().contains("shell"));
        }
        let own = config::builtin_agents().into_iter().next().expect("an agent with its own table");
        assert!(set_in(&store, &own.id, &acp, true).unwrap_err().contains("already has its own"));
        let id = target();
        assert!(set_in(&store, &id, &json!({"command": "bash", "args": ["-c", "x"]}), true).unwrap_err().contains("AGENT_DEF_SHELL_INTERPRETER"));
        assert!(set_in(&store, &id, &json!({"command": "adapter", "env_allow": ["PATH"]}), true).unwrap_err().contains("AGENT_DEF_DENIED_ENV"));
        assert!(set_in(&store, &id, &json!({"command": "adapter", "extra": 1}), true).is_err());
        assert!(set_in(&store, &id, &json!("adapter"), true).is_err());
        assert!(store.get(&id).is_none(), "nothing was written");
        assert!(!dir.join("acp_overlay.json").exists());
    }

    #[test]
    fn the_commands_need_the_labs_switch() {
        if !acp::chat_enabled() {
            assert!(acp_overlay_set("x".into(), json!({}), true).unwrap_err().starts_with("FEATURE_OFF"));
            assert!(acp_overlay_remove("x".into()).unwrap_err().starts_with("FEATURE_OFF"));
        }
    }

    #[test]
    fn only_registered_agents_have_a_program_to_probe() {
        assert!(check_target("no_such_agent_zz").is_err());
        assert!(overlay::program_of("no_such_agent_zz").is_none());
        assert!(!overlay::program_of(&target()).unwrap().is_empty());
    }
}
