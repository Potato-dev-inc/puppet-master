//! The "Assign an issue" end-to-end rig (`tests/fake_tui_issue_intake.rs`): the real intake engine
//! (`issue_intake::live`) running on this world's pane registry and app handle, with the fake agent
//! as the agent. The test brings its own fake `gh` (first on `PATH`, `PUPPET_MASTER_GH`) and a local
//! bare repository behind `url.<bare>.insteadOf`, so no network and no GitHub is involved.
//!
//! This is the only seam the feature needs: `issue_intake` is a private module, and `live::start` is
//! what `lib.rs` calls in `setup` (which the windowless test app does not run). Dev only.

use super::world::World;
use crate::issue_intake::live;
use serde_json::Value;
use std::sync::Once;

/// The engine of this process (one, like the app's).
pub struct IssueRig;

impl IssueRig {
    /// Starts the `issue-intake` thread on this world once; later calls find it running.
    pub fn start() -> IssueRig {
        static START: Once = Once::new();
        START.call_once(|| {
            let (app, registry) = World::get().handles();
            live::start(registry, app);
        });
        IssueRig
    }

    /// Replaces the saved settings (what `issue_intake_save` stores after its checks). `config` is the
    /// `Config` as JSON.
    pub fn save_config(&self, config: Value) -> Result<(), String> {
        let config = serde_json::from_value(config).map_err(|error| error.to_string())?;
        live::store().save_config(&config).map(|_| ())
    }

    /// Look at GitHub now instead of in a minute.
    pub fn poll_now(&self) {
        live::poll_now();
    }

    /// Every saved run, newest first, as the store keeps it (the issue snapshot is in each).
    pub fn runs(&self) -> Vec<Value> {
        live::store().runs().iter().filter_map(|run| serde_json::to_value(run).ok()).collect()
    }

    /// The Settings buttons (`start`, `ask_again`, `push`, `throw_away`, `unignore`).
    pub fn act(&self, run_id: &str, action: &str, who: &str) -> Result<(), String> {
        live::act(run_id, action, who).map(|_| ()).map_err(|error| format!("{}: {}", error.code, error.message))
    }
}
