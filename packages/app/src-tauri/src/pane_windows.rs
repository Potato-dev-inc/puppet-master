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

/// Extra wait before the second sweep pass.
#[cfg_attr(test, allow(dead_code))]
const SWEEP_RECHECK: Duration = Duration::from_millis(2500);

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

/// True when a close request on `label` must be honoured by the backend itself: a detached pane
/// window whose pane no longer exists. The webview normally answers the request (it destroys the
/// window), but a window showing a dead pane can be wedged, so the X must never depend on it.
pub fn close_is_forced(label: &str, live_pane_ids: &[String]) -> bool {
    !orphaned_labels(&[label.to_string()], live_pane_ids).is_empty()
}

/// The live pane a pop-out window belongs to (labels are a lossy encoding of the pane id, so the
/// match goes through `window_label`).
pub fn pane_for_label(label: &str, live_pane_ids: &[String]) -> Option<String> {
    live_pane_ids.iter().find(|id| window_label(id) == label).cloned()
}

/// Panes-first: the X of a pane window closes (kills) its pane; there is no grid to reattach to.
/// Calls `kill` with the pane id when `label` is the window of a live pane and returns whether it did.
pub fn kill_pane_of_closed_window(label: &str, live_pane_ids: &[String], kill: impl FnOnce(&str)) -> bool {
    match pane_for_label(label, live_pane_ids) {
        Some(id) => {
            kill(&id);
            true
        }
        None => false,
    }
}

#[cfg(test)]
pub fn on_close_requested(_app: &AppHandle, _label: &str) {}

/// Runtime hook for `CloseRequested` on any window. A pane window of a live pane kills that pane
/// (the window then closes normally); a window showing a dead pane is destroyed directly.
#[cfg(not(test))]
pub fn on_close_requested(app: &AppHandle, label: &str) {
    use tauri::Manager;
    let Some((_, registry)) = CONTEXT.get() else { return };
    // Hidden warm panes count as live: their windows exist (hidden) until a claim shows them.
    let live: Vec<String> = registry.lock().list_all().into_iter().map(|p| p.id).collect();
    if kill_pane_of_closed_window(label, &live, |id| {
        let result = crate::pty::registry::kill_pane_with_reason(registry, id, Some("pane window closed"));
        tracing::info!(%label, pane = %id, ?result, "pane window closed: killed its pane");
    }) {
        return;
    }
    if close_is_forced(label, &live) {
        if let Some(window) = app.get_webview_window(label) {
            let result = window.destroy();
            tracing::info!(%label, ?result, "closed orphan pane window on close request");
        }
    }
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
            // Second pass: a window can be created just after its pane was killed (spawn then
            // quick close), after the first pass already looked.
            for delay in [SWEEP_GRACE, SWEEP_RECHECK] {
                thread::sleep(delay);
                let live: Vec<String> = registry
                    .lock()
                    .list_all()
                    .into_iter()
                    .map(|pane| pane.id)
                    .collect();
                let labels: Vec<String> = app.webview_windows().keys().cloned().collect();
                let orphans = orphaned_labels(&labels, &live);
                if !orphans.is_empty() {
                    tracing::info!(?labels, ?live, ?orphans, "orphan pane window sweep");
                }
                for label in orphans {
                    match app.get_webview_window(&label) {
                        Some(window) => {
                            let result = window.destroy();
                            tracing::info!(%label, ?result, "destroyed orphan pane window");
                            // Destroy only queues the request; if the window is still registered
                            // shortly after, escalate so nothing empty stays on screen.
                            thread::sleep(Duration::from_millis(500));
                            if let Some(window) = app.get_webview_window(&label) {
                                let closed = window.close();
                                let hidden = window.hide();
                                tracing::warn!(%label, ?closed, ?hidden, "orphan pane window survived destroy; closed and hid it");
                            }
                        }
                        None => tracing::warn!(%label, "orphan pane window vanished before destroy"),
                    }
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
    fn x_on_an_orphan_pane_window_is_forced_closed_but_live_and_main_are_not() {
        let live = vec!["aaaa1111-live".to_string()];
        assert!(close_is_forced(&window_label("bbbb2222-gone"), &live));
        assert!(!close_is_forced(&window_label("aaaa1111-live"), &live));
        assert!(!close_is_forced("main", &live));
    }

    #[test]
    fn closing_a_pane_window_kills_exactly_its_pane() {
        let live = vec!["aaaa1111-live".to_string(), "odd id/1".to_string()];
        let killed = std::cell::RefCell::new(Vec::new());
        assert!(kill_pane_of_closed_window(&window_label("odd id/1"), &live, |id| killed.borrow_mut().push(id.to_string())));
        assert_eq!(*killed.borrow(), vec!["odd id/1".to_string()]);
        // Windows that are not the window of a live pane kill nothing.
        assert!(!kill_pane_of_closed_window(&window_label("gone"), &live, |_| panic!("must not kill")));
        assert!(!kill_pane_of_closed_window("settings", &live, |_| panic!("must not kill")));
        assert!(!kill_pane_of_closed_window("main", &live, |_| panic!("must not kill")));
    }

    #[test]
    fn the_main_window_is_never_swept() {
        assert!(orphaned_labels(&["main".to_string()], &[]).is_empty());
    }
}
