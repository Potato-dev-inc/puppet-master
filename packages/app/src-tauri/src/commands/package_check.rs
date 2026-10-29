//! Tauri commands of Package check (SPEC-n33 section 3.12). Desktop window only: no bridge route and no MCP
//! tool, so an agent or a phone can never ask for a lookup or change the setting.
//!
//! The lookup takes a pane id and a prompt id, never names: `package_check::live::lookup` reads the names
//! from the pane's live prompt and refuses a prompt id that is no longer on screen (`STALE_PROMPT`). The
//! lookup and the registry calls run on the blocking pool.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::State;

use super::AppState;
use crate::audit;
use crate::package_check::cache::Cache;
use crate::package_check::live::{self, RegistryIo};
use crate::package_check::model::{PackageCheckView, Settings};
use crate::package_check::settings as settings_file;
use crate::pty::PaneRegistry;

/// Runs `work` on the blocking pool and flattens its result.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("PACKAGE_CHECK_INTERNAL: {e}"))?
}

/// The card's lookup for one prompt of one pane (the names come from the pane, not from the caller).
fn lookup_with(
    registry: Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    prompt_id: &str,
    again: bool,
) -> Result<PackageCheckView, String> {
    let io = RegistryIo::new(registry);
    live::lookup(&io, Cache::global(), pane_id, prompt_id, again)
}

/// Save the setting under `root` and give back what was saved.
fn save_in(root: &Path, next: &Settings) -> Result<Settings, String> {
    settings_file::save(root, next)?;
    Ok(next.clone())
}

/// Look up the public facts for the packages on a live approval card (SPEC-n33 3.11 C).
#[tauri::command]
pub async fn package_check_lookup(
    state: State<'_, AppState>,
    pane_id: String,
    prompt_id: String,
    again: bool,
) -> Result<PackageCheckView, String> {
    let registry = state.registry.clone();
    blocking(move || lookup_with(registry, &pane_id, &prompt_id, again)).await
}

/// The one Package check setting ("Check packages before installs", on by default).
#[tauri::command]
pub async fn package_check_settings_get() -> Result<Settings, String> {
    blocking(|| Ok(settings_file::load(&settings_file::live_root()))).await
}

/// Save the setting. The change is written to the audit log like the other Safety switches.
#[tauri::command]
pub async fn package_check_settings_set(settings: Settings) -> Result<Settings, String> {
    blocking(move || {
        let root = settings_file::live_root();
        let before = settings_file::load(&root);
        let saved = save_in(&root, &settings)?;
        audit::record_setting("package_check.lookups", "Package check: check packages before installs", before.lookups, saved.lookups);
        Ok(saved)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pm-pc13-{name}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn setting_saves_and_reads_back_both_ways() {
        let root = temp_root("round-trip");
        assert_eq!(save_in(&root, &Settings { lookups: false }), Ok(Settings { lookups: false }));
        assert!(!settings_file::load(&root).lookups);
        assert_eq!(save_in(&root, &Settings { lookups: true }), Ok(Settings { lookups: true }));
        assert!(settings_file::load(&root).lookups);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn setting_is_on_when_nothing_was_saved() {
        let root = temp_root("default");
        assert_eq!(settings_file::load(&root), Settings::default());
        assert!(settings_file::load(&root).lookups);
    }

    #[test]
    fn unknown_pane_is_a_stale_prompt_and_makes_no_request() {
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        let err = lookup_with(registry, "no-such-pane", "prompt-1", false).unwrap_err();
        assert_eq!(err, "STALE_PROMPT");
    }
}
