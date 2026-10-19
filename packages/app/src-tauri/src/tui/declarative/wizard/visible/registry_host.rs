//! The pane host over the app's own pane registry. `lib.rs` installs it at startup and the end-to-end
//! harness installs it for its windowless app (`host::install`); it is not referenced from anywhere a
//! unit test reaches, so the Tauri window code stays out of those executables.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tauri::AppHandle;

use crate::pty::PaneRegistry;

use super::answer::AnswerKey;
use super::host::{Delivered, Look, PaneHost, PaneSpec};

pub struct RegistryHost {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
    /// The panes THIS host started, with the folder each was started in. `answer` presses keys only in
    /// these: a pane anyone else started (the person's own, an agent run's) is never touched.
    owned: Mutex<HashMap<String, PathBuf>>,
}

impl std::fmt::Debug for RegistryHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RegistryHost")
    }
}

impl RegistryHost {
    pub fn new(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) -> Self {
        Self { registry, app, owned: Mutex::new(HashMap::new()) }
    }
}

impl PaneHost for RegistryHost {
    fn spawn(&self, spec: &PaneSpec) -> Result<String, String> {
        let id = spec.profile.id();
        let args = crate::pty::SpawnPaneArgs {
            agent_type: id.to_string(),
            cwd: Some(spec.cwd.to_string_lossy().into_owned()),
            cols: Some(spec.cols),
            rows: Some(spec.rows),
            extra_args: None,
            pane_id: None,
            read_only: false,
            account: None,
        };
        let pane = crate::pty::registry_spawn_pane(&self.registry, &self.app, args)?;
        self.owned.lock().insert(pane.clone(), spec.cwd.clone());
        if spec.window {
            crate::app_windows::open_pane_window(&self.app, &pane, id, spec.cols, spec.rows, true);
        }
        Ok(pane)
    }

    fn look(&self, pane: &str) -> Option<Look> {
        let status = self.registry.lock().list().into_iter().find(|p| p.id == pane)?.status;
        let (screen, output_pos) = crate::pty::registry_read_snapshot_full_at(self.registry.as_ref(), pane).ok()?;
        let watch = crate::pty::registry_read_snapshot(self.registry.as_ref(), pane).unwrap_or_default();
        let alive = crate::agent_runs::delivery::pane_process_state(&self.registry, pane)
            != crate::pty::agent_proc::AgentProcess::Exited;
        let grid = self.registry.lock().list().into_iter().find(|p| p.id == pane).map(|p| (p.cols, p.rows));
        // the same grid with its attributes: a menu may mark its highlighted row only by how it is drawn
        let styled = {
            let emulator = self.registry.lock().panes.get(pane).map(|p| p.screen.clone());
            emulator.map(|screen| super::styled::StyledScreen::from_screen(screen.lock().screen()))
        };
        Some(Look { screen, watch, status, alive, output_pos, hello_written: false, grid, history: String::new(), styled })
    }

    fn deliver(&self, pane: &str, text: &str, trace: &mut Vec<String>) -> Delivered {
        use crate::agent_runs::delivery::{deliver_prompt_traced, Delivery, DeliveryIo, PaneIo};
        let mut io = PaneIo { registry: &self.registry, app: &self.app, pane_id: pane };
        let profile = io.profile();
        match deliver_prompt_traced(&mut io, profile.clear_input_sequence(), text, trace) {
            Delivery::Started | Delivery::SubmittedUnconfirmed => Delivered::Submitted,
            Delivery::NotVisible { .. } => Delivered::Refused("the text did not show up in the input box, so Enter was not pressed".into()),
            Delivery::Blocked(gate) => Delivered::Refused(format!("the agent shows a question ({}), so nothing was typed", gate.summary)),
            Delivery::AgentGone(_) => Delivered::Refused("the agent is not running".into()),
            Delivery::NotReady { why, .. } => Delivered::Refused(format!("the agent was not ready for a task: {why}")),
            Delivery::WriteFailed(e) => Delivered::Refused(format!("typing failed: {e}")),
        }
    }

    fn card(&self, pane: &str) -> Option<Value> {
        crate::pane_prompt::detect(&self.registry, pane, false)
    }

    fn grid(&self, pane: &str) -> Option<(u16, u16)> {
        let info = self.registry.lock().list().into_iter().find(|p| p.id == pane)?;
        Some((info.cols, info.rows))
    }

    fn show(&self, pane: &str) {
        let Some(info) = self.registry.lock().list().into_iter().find(|p| p.id == pane) else { return };
        crate::app_windows::open_pane_window(&self.app, pane, &info.agent_type, info.cols, info.rows, true);
    }

    fn kill(&self, pane: &str) {
        let _ = crate::pty::registry_kill_pane(&self.registry, pane);
        self.owned.lock().remove(pane);
    }

    fn cwd(&self, pane: &str) -> Option<PathBuf> {
        self.owned.lock().get(pane).cloned()
    }

    fn resize(&self, pane: &str, cols: u16, rows: u16) -> Result<(), String> {
        if !self.owned.lock().contains_key(pane) {
            return Err("this is not a pane the wizard started, so the wizard resizes nothing".into());
        }
        // The PTY and the viewer move together: the pane registry resizes the PTY and its emulator in one call, and
        // the same two notices a phone's size lease sends make an open pop-out window follow (`pty://resized` in
        // stream order), so the window's grid is the PTY's grid.
        if crate::pty::registry_resize(&self.registry, pane, cols, rows)? {
            crate::bridge::notify_pane_resized(pane, cols, rows);
            crate::bridge::notify_pty_resized_by_lease(pane, cols, rows);
        }
        Ok(())
    }

    fn history(&self, pane: &str) -> Option<String> {
        crate::pty::registry_read_full_text(self.registry.as_ref(), pane).ok()
    }

    fn viewer_grid(&self, pane: &str) -> Option<(u16, u16)> {
        let screen = self.registry.lock().panes.get(pane)?.screen.clone();
        let (rows, cols) = screen.lock().screen().size();
        Some((cols, rows))
    }

    fn answer(&self, pane: &str, keys: &[AnswerKey]) -> Result<(), String> {
        if !self.owned.lock().contains_key(pane) {
            return Err("this is not a pane the wizard started, so the wizard presses nothing in it".into());
        }
        for key in keys {
            let (text, enter) = match key {
                AnswerKey::Up => ("[A", false),
                AnswerKey::Down => ("[B", false),
                AnswerKey::Right => ("[C", false),
                AnswerKey::Left => ("[D", false),
                AnswerKey::Tab => ("	", false),
                AnswerKey::Enter => ("", true),
                AnswerKey::Text(t) => (t.as_str(), false),
            };
            crate::pty::registry_write_input(&self.registry, &self.app, pane, text, enter, false, None, crate::pty::WriteOrigin::WizardAnswer)?;
        }
        Ok(())
    }
}
