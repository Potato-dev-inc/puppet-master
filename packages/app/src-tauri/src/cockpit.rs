//! Where a pane is shown, by UI mode (see `ui_mode.rs`), and the mode-switch window choreography.
//!
//! * Orchestration: panes live in the single cockpit window. Nothing creates a pop-out; a hidden
//!   warm window is destroyed when its pane is claimed. Only a person's action selects a pane in
//!   the cockpit (agent-spawned panes never steal focus).
//! * Individual: each pane has its own pop-out, as before.

use crate::ui_mode::UiMode;
use tauri::{AppHandle, Emitter, Manager};

/// Who caused a pane to be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// An agent run / bridge route spawned or claimed it. Never takes focus.
    Agent,
    /// The tray, a second launch or the "detach" action: a person asked from outside the cockpit.
    Tray,
    /// The cockpit's own "new pane" button: the cockpit is already on screen.
    CockpitUi,
}

/// What presenting a pane has to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Open (or show) the pane's pop-out window.
    PopOut { focus: bool },
    /// Show and focus the cockpit and select the pane there.
    ShowCockpit,
    /// Only tell the (already visible) cockpit to select the pane.
    SelectOnly,
    /// Nothing visible: the pane just appears in `pty://panes-changed`.
    Nothing,
}

pub fn placement(mode: UiMode, origin: Origin) -> Placement {
    match (mode, origin) {
        (UiMode::Individual, Origin::Agent) => Placement::PopOut { focus: false },
        (UiMode::Individual, _) => Placement::PopOut { focus: true },
        (UiMode::Orchestration, Origin::Agent) => Placement::Nothing,
        (UiMode::Orchestration, Origin::Tray) => Placement::ShowCockpit,
        (UiMode::Orchestration, Origin::CockpitUi) => Placement::SelectOnly,
    }
}

pub fn emit_select(app: &AppHandle, pane_id: &str) {
    let _ = app.emit("cockpit://select", serde_json::json!({ "pane_id": pane_id }));
}

/// Show a pane the way the current mode says. In orchestration a hidden warm pop-out of this pane
/// (from a standby claim) is destroyed; the pane itself is never touched.
pub fn present_pane(app: &AppHandle, pane_id: &str, agent: &str, cols: u16, rows: u16, origin: Origin) {
    let mode = crate::ui_mode::current();
    if mode == UiMode::Orchestration {
        crate::app_windows::close_warm_pane_window(app, pane_id);
    }
    match placement(mode, origin) {
        Placement::PopOut { focus } => crate::app_windows::open_pane_window(app, pane_id, agent, cols, rows, focus),
        Placement::ShowCockpit => crate::app_windows::open_cockpit_window(app, Some(pane_id), true),
        Placement::SelectOnly => emit_select(app, pane_id),
        Placement::Nothing => {}
    }
}

/// `open_cockpit` command: show, unminimize and focus the cockpit; with a pane, select it too.
pub fn open_cockpit(app: &AppHandle, pane_id: Option<&str>) {
    crate::app_windows::open_cockpit_window(app, pane_id.filter(|id| !id.is_empty()), true);
}

/// Where Settings is shown, by UI mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPlacement {
    /// The separate Settings window (Individual mode: there is no main window to put it in).
    Window,
    /// Inside the cockpit's main area, sidebar kept (Orchestration mode).
    Inline,
}

pub fn settings_placement(mode: UiMode) -> SettingsPlacement {
    match mode {
        UiMode::Orchestration => SettingsPlacement::Inline,
        UiMode::Individual => SettingsPlacement::Window,
    }
}

/// Show Settings the way the current mode says (tray item, `open_settings` command): inline in the cockpit in
/// Orchestration, the Settings window in Individual. `tab` opens that page.
pub fn open_settings(app: &AppHandle, tab: Option<&str>) {
    match settings_placement(crate::ui_mode::current()) {
        SettingsPlacement::Inline => crate::app_windows::open_cockpit_settings(app, tab),
        SettingsPlacement::Window => crate::app_windows::open_settings_window(app, tab, true),
    }
}

/// Left-click on the tray and a plain second launch: the cockpit in orchestration, Settings in
/// individual mode.
pub fn open_primary_window(app: &AppHandle) {
    match crate::ui_mode::current() {
        UiMode::Orchestration => open_cockpit(app, None),
        UiMode::Individual => crate::app_windows::open_settings_window(app, None, true),
    }
}

/// Switch mode: persist it, announce `ui://mode-changed`, then move the windows.
/// Individual: a pop-out for every live pane, then hide the cockpit. Orchestration: destroy every
/// pop-out (without killing panes) and show the cockpit.
pub fn set_mode(app: &AppHandle, mode: UiMode) -> Result<(), String> {
    let previous = crate::ui_mode::current();
    if let Err(e) = crate::ui_mode::persist(app, mode) {
        // The switch still applies for this run; it just will not survive a restart.
        tracing::warn!(error = %e, "ui_mode not persisted");
    }
    // Ownership flips first, so the windows opened below already own their panes.
    crate::ui_mode::set_current(mode);
    let _ = app.emit("ui://mode-changed", serde_json::json!({ "mode": mode.as_str() }));
    if previous == mode {
        return Ok(());
    }
    let app = app.clone();
    std::thread::spawn(move || match mode {
        UiMode::Individual => {
            let panes = app.state::<crate::AppState>().registry.lock().list();
            for pane in panes {
                crate::app_windows::open_pane_window(&app, &pane.id, &pane.agent_type, pane.cols, pane.rows, false);
            }
            crate::app_windows::hide_cockpit_window(&app);
        }
        UiMode::Orchestration => {
            let destroyed = crate::app_windows::destroy_pane_windows(&app);
            tracing::info!(destroyed, "ui mode -> orchestration: pop-outs destroyed, panes kept");
            // A Settings window on screen moves into the cockpit (it names its page back through
            // `open_settings`, which hides it): never a window AND an inline view.
            if crate::app_windows::ask_settings_to_move_inline(&app) {
                tracing::info!("ui mode -> orchestration: Settings window asked to move into the cockpit");
            }
            crate::app_windows::open_cockpit_window(&app, None, true);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_is_inline_in_orchestration_and_a_window_in_individual() {
        assert_eq!(settings_placement(UiMode::Orchestration), SettingsPlacement::Inline);
        assert_eq!(settings_placement(UiMode::Individual), SettingsPlacement::Window);
    }

    #[test]
    fn orchestration_creates_no_pop_out_and_agents_never_steal_focus() {
        for origin in [Origin::Agent, Origin::Tray, Origin::CockpitUi] {
            assert!(!matches!(placement(UiMode::Orchestration, origin), Placement::PopOut { .. }));
        }
        assert_eq!(placement(UiMode::Orchestration, Origin::Agent), Placement::Nothing);
        assert_eq!(placement(UiMode::Orchestration, Origin::Tray), Placement::ShowCockpit);
        assert_eq!(placement(UiMode::Orchestration, Origin::CockpitUi), Placement::SelectOnly);
    }

    #[test]
    fn individual_mode_keeps_todays_pop_out_focus_rules() {
        assert_eq!(placement(UiMode::Individual, Origin::Agent), Placement::PopOut { focus: false });
        assert_eq!(placement(UiMode::Individual, Origin::Tray), Placement::PopOut { focus: true });
        assert_eq!(placement(UiMode::Individual, Origin::CockpitUi), Placement::PopOut { focus: true });
    }
}
