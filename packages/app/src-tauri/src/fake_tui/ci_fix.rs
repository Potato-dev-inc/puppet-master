//! The "Red to Green" end-to-end rig (`tests/fake_tui_ci_fix.rs`): the real CI fix engine (`ci_fix::live`)
//! running on this world's pane registry and app handle, with the fake agent as the agent. The test brings
//! its own fake `gh` (first on `PATH`, `PUPPET_MASTER_GH`) and a local bare repository behind
//! `url.<bare>.insteadOf`, so no network and no GitHub is involved.
//!
//! The seams the feature needs: `ci_fix` is a private module, `live::start` is what `lib.rs` calls in
//! `setup` (which the windowless test app does not run), and a pull request is opened the way Changes >
//! Create PR does it (`worktrees::service::pr_create_with`, which tells the engine). Dev only.

use super::world::World;
use crate::ci_fix::live;
use crate::ci_fix::model::Settings;
use serde_json::Value;
use std::path::Path;
use std::sync::Once;

/// The engine of this process (one, like the app's).
pub struct CiFixRig;

impl CiFixRig {
    /// Starts the `ci-fix` thread on this world once; later calls find it running.
    pub fn start() -> CiFixRig {
        static START: Once = Once::new();
        START.call_once(|| {
            let (app, registry) = World::get().handles();
            live::start(registry, app);
        });
        CiFixRig
    }

    /// Saves the Labs settings (what `ci_fix_save_settings` stores, clamped). `settings` is the `Settings` as JSON.
    pub fn save_settings(&self, settings: Value) -> Result<(), String> {
        let settings: Settings = serde_json::from_value(settings).map_err(|error| error.to_string())?;
        live::save_settings(&settings, "desktop").map(|_| ()).map_err(|error| format!("{}: {}", error.code, error.message))
    }

    /// Look at GitHub now instead of in 20 seconds.
    pub fn poll_now(&self) {
        live::poll_now();
    }

    /// Every watched pull request, as the store keeps it.
    pub fn watches(&self) -> Vec<Value> {
        live::store().watches().iter().filter_map(|watch| serde_json::to_value(watch).ok()).collect()
    }

    /// Every fix try, newest first, as the store keeps it.
    pub fn fixes(&self) -> Vec<Value> {
        live::store().fixes().iter().filter_map(|fix| serde_json::to_value(fix).ok()).collect()
    }

    /// The row buttons (`stop_watching`, `watch_again`, `ask_again`, `push`, `dont_push`).
    pub fn act(&self, id: &str, action: &str, who: &str) -> Result<(), String> {
        live::act(id, action, who).map(|_| ()).map_err(|error| format!("{}: {}", error.code, error.message))
    }

    /// Stops every watch the earlier scenarios left, so none of them looks at this scenario's fake GitHub.
    /// Returns how many are still watched (a look at one of them may hold it for a moment: call again).
    pub fn stop_all(&self) -> usize {
        let mut left = 0;
        for watch in live::store().watches() {
            if matches!(watch.state, crate::ci_fix::model::WatchState::Watching | crate::ci_fix::model::WatchState::Paused { .. })
                && live::act(&watch.id, "stop_watching", "desktop").is_err()
            {
                left += 1;
            }
        }
        left
    }

    /// What `Show log` reads for a fix: the stored, cleaned snapshot, as JSON.
    pub fn log(&self, fix_id: &str) -> Result<Value, String> {
        live::log(fix_id)
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
            .map_err(|error| format!("{}: {}", error.code, error.message))
    }

    /// Opens the pull request of the copy `handle` the way Changes > Create PR does (push the branch, `gh pr create
    /// --draft`, remember the pull request on the copy, tell the engine). Returns the stored `PrRef` as JSON.
    pub fn open_pr(&self, project: &Path, handle: &str, base: &str) -> Result<Value, String> {
        let (_app, registry) = World::get().handles();
        let env = crate::worktrees::proc::ToolEnv::default();
        let project = project.to_string_lossy().into_owned();
        let pr = crate::worktrees::service::pr_create_with(
            Some(&project),
            handle,
            "Fix the sum helper",
            "Written by the e2e.",
            base,
            &registry,
            &env,
            &[],
        )
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
        serde_json::to_value(pr).map_err(|error| error.to_string())
    }
}
