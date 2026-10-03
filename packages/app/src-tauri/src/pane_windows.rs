//! Detached pane windows follow their panes: when a pane leaves the registry (an agent called
//! `close_agent`, a run finished with `keep_pane=false`, a kill from the MCP) its window is
//! destroyed instead of being left behind as a blank "No terminal pane selected." window.
//!
//! Panes are killed from many places that have no `AppHandle`, so the handle and registry are
//! stored once at startup and every kill path calls `notify_pane_removed`.

use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// A pane that is only being replaced (killed and respawned under the same id) keeps its window:
/// the sweep re-reads the registry after this grace period.
#[cfg_attr(test, allow(dead_code))]
const SWEEP_GRACE: Duration = Duration::from_millis(1500);

static CONTEXT: OnceLock<(AppHandle, Arc<Mutex<PaneRegistry>>)> = OnceLock::new();

/// Call once at startup.
pub fn init(app: &AppHandle, registry: Arc<Mutex<PaneRegistry>>) {
    let _ = CONTEXT.set((app.clone(), registry));
}

/// A pane was removed from the registry. Tells every window to refresh its pane list and sweeps
/// the detached windows of panes that no longer exist. No-op before `init` (unit tests).
pub fn notify_pane_removed() {
    if let Some((app, registry)) = CONTEXT.get() {
        let _ = app.emit("pty://panes-changed", ());
        schedule_sweep(registry, app);
    }
}

/// Label the frontend gives a detached pane window (mirrors `detachedPaneLabel` in
/// `src/lib/detached-pane-window.ts`).
pub fn window_label(pane_id: &str) -> String {
    let safe: String = pane_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '/' | ':' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("detached-pane-{safe}")
}

/// Detached pane windows whose pane is gone. Pure so it can be tested without a window system.
pub fn orphaned_labels(window_labels: &[String], live_pane_ids: &[String]) -> Vec<String> {
    let live: HashSet<String> = live_pane_ids.iter().map(|id| window_label(id)).collect();
    window_labels
        .iter()
        .filter(|label| label.starts_with("detached-pane-") && !live.contains(*label))
        .cloned()
        .collect()
}

/// Destroy the detached windows of panes that are gone, after the grace period. Only
/// `detached-pane-*` windows are touched, so a worker host's own main window is safe; destroying
/// (unlike closing) skips the worker-host close handler that would kill the pane and quit the
/// app. Compiled out of unit-test builds: the window APIs need
/// the Windows application manifest, which the test executable does not carry.
#[cfg(test)]
pub fn schedule_sweep(_registry: &Arc<Mutex<PaneRegistry>>, _app: &AppHandle) {}

#[cfg(not(test))]
pub fn schedule_sweep(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) {
    use std::thread;
    use tauri::Manager;
    let (registry, app) = (registry.clone(), app.clone());
    let _ = thread::Builder::new()
        .name("puppet-orphan-window-sweep".into())
        .spawn(move || {
            thread::sleep(SWEEP_GRACE);
            let live: Vec<String> = registry
                .lock()
                .list()
                .into_iter()
                .map(|pane| pane.id)
                .collect();
            let labels: Vec<String> = app.webview_windows().keys().cloned().collect();
            let orphans = orphaned_labels(&labels, &live);
            tracing::info!(?labels, ?live, ?orphans, "orphan pane window sweep");
            for label in orphans {
                match app.get_webview_window(&label) {
                    Some(window) => {
                        let result = window.destroy();
                        tracing::info!(%label, ?result, "destroyed orphan pane window");
                    }
                    None => tracing::warn!(%label, "orphan pane window vanished before destroy"),
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_of_closed_panes_are_swept_and_live_ones_kept() {
        let live = vec!["11f5b783-aaaa".to_string(), "odd id/1".to_string()];
        let windows = vec![
            "main".to_string(),
            window_label("11f5b783-aaaa"),
            window_label("odd id/1"),
            window_label("1b078cd4-gone"),
        ];
        assert_eq!(
            orphaned_labels(&windows, &live),
            vec!["detached-pane-1b078cd4-gone".to_string()]
        );
        assert_eq!(window_label("odd id/1"), "detached-pane-odd_id/1");
    }

    #[test]
    fn the_main_window_is_never_swept() {
        assert!(orphaned_labels(&["main".to_string()], &[]).is_empty());
    }
}
