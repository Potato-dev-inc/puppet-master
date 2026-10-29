//! Dev-only harness for the `pm-fake-tui` end-to-end tests (`tests/fake_tui_e2e.rs`). Compiled only
//! with the `dev-fake-tui` cargo feature, so nothing here exists in the app users run.
//!
//! What it builds: a real (windowless, event-loop-only) Tauri app handle, a real `PaneRegistry`, the
//! real pane observers (`pty://status` -> `bridge::push_pane_status_sse`), and two extra agent types,
//! `fake_claude` and `fake_codex`, whose profiles are the real Claude / Codex profiles launching
//! `tools/pm-fake-tui` on a real ConPTY under a PowerShell host shell (see `profile.rs`). A test then
//! drives the real `/agents/*` routes (`agent_runs::handle_request`), the same code the MCP shim reaches
//! over HTTP, so `run_agent` -> pane -> delivery -> readiness -> status -> turn summary -> receipt is
//! the production path with only the agent replaced.
//!
//! No bridge port is bound, no port file is written, and the process runs as the app profile
//! `fake-tui-e2e` (its own app-data folder), so a running Puppet Master is never touched.

mod canary_rig;
mod chat;
mod focus_rig;
mod issue_intake;
mod linux;
mod night;
mod package_rig;
mod profile;
mod store_rig;
mod team;
mod wizard_rig;
mod world;

pub use canary_rig::CanaryRig;
pub use chat::ChatProbe;
pub use focus_rig::FocusRig;
pub use issue_intake::IssueRig;
pub use night::NightRig;
pub use package_rig::{PackageRig, Seen};
pub use store_rig::StoreRig;
pub(crate) use store_rig::catalog_files as store_catalog_files;
pub use wizard_rig::{Answers, WizardRig};
pub use team::{leak_check, Person, Remote};
pub use world::{Session, World};

use parking_lot::Mutex;
use std::time::Duration;

/// Output the pane readers hold back: a chunk containing the marker waits this long before the pane
/// sees it ([`Session::hold_output`]).
static HOLD_OUTPUT: Mutex<Option<(String, Duration)>> = Mutex::new(None);

pub(crate) fn set_hold_output(hold: Option<(String, Duration)>) {
    *HOLD_OUTPUT.lock() = hold;
}

/// Called by every pane reader (`pty::registry::spawn`) with each chunk it read, before the pane
/// sees it.
pub(crate) fn hold_output_chunk(chunk: &[u8]) {
    let hold = HOLD_OUTPUT.lock().clone();
    if let Some((marker, wait)) = hold {
        if String::from_utf8_lossy(chunk).contains(&marker) {
            std::thread::sleep(wait);
        }
    }
}
