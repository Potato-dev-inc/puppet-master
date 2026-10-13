//! Panic button: one action interrupts every running agent pane and reports what it stopped.
//!
//! Entry points: the palette ("Stop all agents", Tauri command `stop_all_agents`), the tray menu
//! item and the optional global shortcut (both go through `stop_all_and_show`, which also opens the
//! cockpit and emits `panic://stopped` so the sheet appears), and `POST /panes/stop-all` for the
//! phone. Interrupt = the same Ctrl+C `interrupt_pane` sends. Only panes whose status is
//! `running` are touched; plain shell panes are left alone (a dev server in a shell is not an
//! agent burning credits), and so are idle, waiting and errored panes.
//!
//! TODO(checkpoints): when `feat/checkpoints` lands, take a checkpoint of each stopped pane's
//! project before interrupting (hook point: `stop_running`, `before_interrupt`).

use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;
use tauri::AppHandle;

/// What the panic button needs to know about one pane.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub pane_id: String,
    pub agent_type: String,
    pub cwd: String,
    pub status: String,
    pub is_shell: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedPane {
    pub pane_id: String,
    pub agent_type: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct StopReport {
    pub stopped: Vec<StoppedPane>,
    /// Panes that were running but could not be interrupted: `(pane_id, reason)`.
    pub failed: Vec<(String, String)>,
}

/// Interrupts every running agent pane through `interrupt` (pure; tests inject the interrupt).
pub fn stop_running(candidates: &[Candidate], interrupt: &dyn Fn(&str) -> Result<(), String>) -> StopReport {
    let mut report = StopReport::default();
    for c in candidates.iter().filter(|c| c.status == "running" && !c.is_shell) {
        match interrupt(&c.pane_id) {
            Ok(()) => report.stopped.push(StoppedPane {
                pane_id: c.pane_id.clone(),
                agent_type: c.agent_type.clone(),
                cwd: c.cwd.clone(),
            }),
            Err(reason) => report.failed.push((c.pane_id.clone(), reason)),
        }
    }
    report
}

fn candidates(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<Candidate> {
    let panes = registry.lock().list();
    panes
        .into_iter()
        .map(|p| Candidate {
            is_shell: crate::tui::profile_for(&p.agent_type).features().kind == crate::tui::Kind::Shell,
            pane_id: p.id,
            agent_type: p.agent_type,
            cwd: p.cwd,
            status: p.status,
        })
        .collect()
}

/// Interrupt every running agent pane of the registry.
pub fn stop_all(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) -> StopReport {
    let list = candidates(registry);
    stop_running(&list, &|id| crate::pty::registry_write_input(registry, app, id, "\u{3}", false, false, None))
}

/// Shortcut and tray entry: stop everything, bring the cockpit forward and tell it to show the sheet.
pub fn stop_all_and_show(app: &AppHandle) -> StopReport {
    use tauri::{Emitter, Manager};
    let report = match app.try_state::<crate::AppState>() {
        Some(state) => stop_all(&state.registry, app),
        None => StopReport::default(),
    };
    crate::cockpit::open_cockpit(app, None);
    let _ = app.emit("panic://stopped", &report);
    report
}

pub const SHORTCUT: &str = "Ctrl+Alt+Shift+P";

/// Register (or unregister) the global shortcut according to the saved setting. Off by default.
pub fn init_shortcut(app: &AppHandle) {
    apply_shortcut(app, crate::loop_guard::settings().panic_shortcut);
}

pub fn apply_shortcut(app: &AppHandle, enabled: bool) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister(SHORTCUT);
    if !enabled {
        return;
    }
    let handle = app.clone();
    let result = shortcuts.on_shortcut(SHORTCUT, move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let handle = handle.clone();
            std::thread::spawn(move || {
                stop_all_and_show(&handle);
            });
        }
    });
    if let Err(e) = result {
        tracing::warn!(error = %e, "panic shortcut could not be registered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn c(id: &str, status: &str, shell: bool) -> Candidate {
        Candidate { pane_id: id.into(), agent_type: if shell { "bash" } else { "claude" }.into(), cwd: "/p".into(), status: status.into(), is_shell: shell }
    }

    #[test]
    fn interrupts_only_running_agent_panes() {
        let calls = RefCell::new(Vec::<String>::new());
        let panes = [
            c("run-1", "running", false),
            c("idle-1", "idle", false),
            c("wait-1", "waiting_input", false),
            c("err-1", "error", false),
            c("run-2", "running", false),
            c("shell-1", "running", true),
        ];
        let report = stop_running(&panes, &|id| {
            calls.borrow_mut().push(id.to_string());
            Ok(())
        });
        assert_eq!(*calls.borrow(), vec!["run-1", "run-2"]);
        assert_eq!(report.stopped.iter().map(|s| s.pane_id.as_str()).collect::<Vec<_>>(), vec!["run-1", "run-2"]);
        assert!(report.failed.is_empty());
    }

    #[test]
    fn a_failing_interrupt_is_reported_and_does_not_stop_the_rest() {
        let panes = [c("a", "running", false), c("b", "running", false)];
        let report = stop_running(&panes, &|id| if id == "a" { Err("pane has exited".into()) } else { Ok(()) });
        assert_eq!(report.stopped.len(), 1);
        assert_eq!(report.stopped[0].pane_id, "b");
        assert_eq!(report.failed, vec![("a".to_string(), "pane has exited".to_string())]);
    }

    #[test]
    fn nothing_running_stops_nothing() {
        let report = stop_running(&[c("a", "idle", false)], &|_| panic!("must not interrupt"));
        assert_eq!(report, StopReport::default());
    }
}
