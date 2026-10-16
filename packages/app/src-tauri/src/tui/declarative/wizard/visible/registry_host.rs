//! The pane host over the app's own pane registry. `lib.rs` installs it at startup and the end-to-end
//! harness installs it for its windowless app (`host::install`); it is not referenced from anywhere a
//! unit test reaches, so the Tauri window code stays out of those executables.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tauri::AppHandle;

use crate::pty::PaneRegistry;

use super::host::{Delivered, Look, PaneHost, PaneSpec};

pub struct RegistryHost {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
}

impl std::fmt::Debug for RegistryHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RegistryHost")
    }
}

impl RegistryHost {
    pub fn new(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) -> Self {
        Self { registry, app }
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
        Some(Look { screen, watch, status, alive, output_pos, hello_written: false })
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
    }
}
