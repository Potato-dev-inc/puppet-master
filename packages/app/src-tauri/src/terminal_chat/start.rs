//! Starting a terminal chat: the ordinary new-pane path, then the pane is tagged `lane = "terminal_chat"` so the UI
//! opens it on its Chat tab. Desktop app only: no bridge route and no MCP tool, so a phone or an agent cannot start
//! one. No conversation record is made (the pane, the session file and the resume plan are enough).

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use super::lane::{available_for, LANE};
use crate::pty::registry::PaneRegistry;

/// Tags `pane_id` as a terminal chat. False when the pane is gone or already has another lane.
pub(crate) fn tag_pane(registry: &Mutex<PaneRegistry>, pane_id: &str) -> bool {
    registry.lock().set_lane(pane_id, LANE)
}

/// Opens `agent` in a normal pane (folder `cwd`, or the project the cockpit shows) that opens on its Chat tab, and
/// returns the pane id. Refuses an agent that cannot run as a terminal chat (`NOT_AVAILABLE`).
#[tauri::command]
pub async fn terminal_chat_start(app: AppHandle, agent: String, cwd: Option<String>) -> Result<String, String> {
    let id = agent.trim().to_ascii_lowercase();
    if id.is_empty() {
        return Err("INVALID_ARGUMENT: agent is required".into());
    }
    available_for(&id).map_err(|reason| format!("NOT_AVAILABLE: {reason}"))?;
    // The same call the + menu makes, so isolation, folder memory, warm workers and window rules are unchanged.
    let pane = crate::commands::new_pane(app.clone(), id.clone(), None, None, cwd).await?;
    let state = app.state::<crate::commands::AppState>();
    if !tag_pane(&state.registry, &pane) {
        tracing::warn!(agent = %id, pane = %pane, "terminal chat: pane vanished before it was tagged");
        return Err("PANE_GONE: the pane closed before the chat could open".into());
    }
    // The pane list was announced before the tag existed; announce again so the view picks the Chat tab.
    let _ = app.emit("pty://panes-changed", ());
    Ok(pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagging_marks_a_live_pane_and_shows_in_its_json() {
        let registry = Mutex::new(PaneRegistry::new());
        registry.lock().panes.insert("p1".into(), PaneRegistry::test_pane_stub("p1"));
        assert!(tag_pane(&registry, "p1"));
        let listed = registry.lock().list();
        assert_eq!(listed[0].lane.as_deref(), Some("terminal_chat"));
        assert_eq!(serde_json::to_value(&listed[0]).unwrap()["lane"], "terminal_chat");
        assert!(tag_pane(&registry, "p1"), "tagging twice is fine");
    }

    #[test]
    fn a_missing_pane_or_a_chat_session_is_not_tagged() {
        let registry = Mutex::new(PaneRegistry::new());
        assert!(!tag_pane(&registry, "nope"));
        let mut acp = PaneRegistry::test_pane_stub("acp1");
        acp.info.lane = Some("acp".into());
        registry.lock().panes.insert("acp1".into(), acp);
        assert!(!tag_pane(&registry, "acp1"), "an ACP chat keeps its lane");
        assert_eq!(registry.lock().list()[0].lane.as_deref(), Some("acp"));
    }

    #[test]
    fn a_shell_cannot_start() {
        // `available_for` is what the command checks first; a shell is not an agent.
        let shell = crate::tui::registry::snapshot().iter().find(|p| p.features().kind == crate::tui::Kind::Shell).map(|p| p.id());
        if let Some(id) = shell {
            assert!(available_for(id).is_err());
        }
    }
}
