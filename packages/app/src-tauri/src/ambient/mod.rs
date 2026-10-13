//! Ambient state and auto-titles (docs/features/ambient-and-titles.md).
//!
//! * `glyph` / `raster` / `tray_icon`: a 3x3 dot-matrix tray icon mirroring the aggregate pane state,
//!   the tooltip and the taskbar badge.
//! * `title` / `store`: human titles for panes, derived from the first task or typed line.

pub mod glyph;
pub mod prefs;
pub mod raster;
pub mod store;
pub mod title;
#[cfg(test)]
mod title_tests;
pub mod tray_icon;

pub use tray_icon::notify_changed;

use tauri::{AppHandle, Emitter, Manager};

/// "Codex · auth refactor" when titled, else `None` (callers keep their own fallback).
pub fn titled_label(agent_label: &str, title: Option<&str>) -> Option<String> {
    title.map(|t| format!("{agent_label} \u{b7} {t}"))
}

/// Human label of an agent id (its registry label, else the id).
pub fn agent_label(agent_id: &str) -> String {
    crate::tui::agent_types_listing()
        .get("agents")
        .and_then(|v| v.as_array())
        .and_then(|list| list.iter().find(|a| a.get("id").and_then(|i| i.as_str()) == Some(agent_id)))
        .and_then(|a| a.get("label").and_then(|l| l.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| agent_id.to_string())
}

/// Pop-out window title for a pane: "Codex · auth refactor", or the old "agent · id8".
pub fn window_title(agent_id: &str, pane_id: &str, title: Option<&str>) -> String {
    match title {
        Some(t) => titled_label(&agent_label(agent_id), Some(t)).unwrap_or_default(),
        None => format!("{agent_id} \u{b7} {}", pane_id.chars().take(8).collect::<String>()),
    }
}

/// A title changed: refresh every window's pane list, the pop-out window title and the tray menu.
pub fn announce_title_change(app: &AppHandle, pane_id: &str) {
    if let Some(window) = app.get_webview_window(&crate::pane_windows::window_label(pane_id)) {
        let agent = app
            .try_state::<crate::AppState>()
            .and_then(|s| s.registry.lock().list().into_iter().find(|p| p.id == pane_id).map(|p| p.agent_type));
        if let Some(agent) = agent {
            let _ = window.set_title(&window_title(&agent, pane_id, store::get(pane_id).as_deref()));
        }
    }
    let _ = app.emit("pty://panes-changed", ());
}

/// Call once at startup, after the tray exists.
pub fn init(app: &AppHandle) {
    use tauri::Listener;
    prefs::load();
    app.listen("pty://panes-changed", |_| notify_changed());
    tray_icon::start(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_join_agent_and_title() {
        assert_eq!(titled_label("Codex", Some("auth refactor")).as_deref(), Some("Codex \u{b7} auth refactor"));
        assert_eq!(titled_label("Codex", None), None);
        assert_eq!(window_title("kilo", "abcdef0123", None), "kilo \u{b7} abcdef01");
    }
}
