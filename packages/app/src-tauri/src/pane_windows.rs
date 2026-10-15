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

/// What closing a pane window (or quitting the app) does while agents may still be working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// Close right away.
    Close,
    /// Stop the close and ask the user first.
    Confirm,
}

/// A pane in these states is doing, or about to do, work that closing would throw away.
pub fn is_busy_status(status: &str) -> bool {
    matches!(status, "running" | "waiting_input")
}

/// Close-decision for one pane. `confirm_setting` is the `confirm_close_running_pane` setting.
pub fn close_decision(status: &str, confirm_setting: bool) -> CloseDecision {
    if confirm_setting && is_busy_status(status) {
        CloseDecision::Confirm
    } else {
        CloseDecision::Close
    }
}

/// Close-decision for quitting the whole app: confirm when any pane is busy.
pub fn quit_decision(statuses: &[String], confirm_setting: bool) -> CloseDecision {
    if statuses.iter().any(|s| close_decision(s, confirm_setting) == CloseDecision::Confirm) {
        CloseDecision::Confirm
    } else {
        CloseDecision::Close
    }
}

/// Event a pane window's React listens to: the X was pressed while its pane is busy.
pub const CONFIRM_CLOSE_EVENT: &str = "pane-window://confirm-close";

#[cfg(not(test))]
fn confirm_setting(app: &AppHandle) -> bool {
    crate::settings_store::read_public_settings(app)
        .get("confirm_close_running_pane")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Tray "Quit Puppet Master": quits at once, or asks first when any agent is still working.
#[cfg(test)]
pub fn request_quit(_app: &AppHandle) {}

#[cfg(not(test))]
pub fn request_quit(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let statuses: Vec<String> = CONTEXT
        .get()
        .map(|(_, registry)| registry.lock().list_all().into_iter().map(|p| p.status).collect())
        .unwrap_or_default();
    if quit_decision(&statuses, confirm_setting(app)) == CloseDecision::Close {
        app.exit(0);
        return;
    }
    let busy = statuses.iter().filter(|s| is_busy_status(s)).count();
    let message = if busy == 1 {
        "1 agent is still working. Quit and stop it?".to_string()
    } else {
        format!("{busy} agents are still working. Quit and stop them?")
    };
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title("Quit Puppet Master?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Stop and quit".to_string(), "Keep open".to_string()))
        .show(move |stop| {
            if stop {
                handle.exit(0);
            }
        });
}

/// The user chose "Stop and close" in a pane window's sheet: kill the pane and close the window.
#[cfg(not(test))]
pub fn close_confirmed(app: &AppHandle, pane_id: &str) {
    use tauri::Manager;
    let Some((_, registry)) = CONTEXT.get() else { return };
    let result = crate::pty::registry::kill_pane_with_reason(registry, pane_id, Some("pane window closed (confirmed)"));
    tracing::info!(pane = %pane_id, ?result, "pane window close confirmed: killed its pane");
    if let Some(window) = app.get_webview_window(&window_label(pane_id)) {
        let _ = window.destroy();
    }
}

#[cfg(test)]
pub fn close_confirmed(_app: &AppHandle, _pane_id: &str) {}

/// Returns true when the close must be prevented (waiting for the user's answer).
#[cfg(test)]
pub fn on_close_requested(_app: &AppHandle, _label: &str) -> bool {
    false
}

/// Runtime hook for `CloseRequested` on any window; returns true when the close must be prevented.
/// A pane window of a busy pane asks first (the window's React shows the sheet); an idle one kills
/// its pane and closes normally; a window showing a dead pane is destroyed directly.
#[cfg(not(test))]
pub fn on_close_requested(app: &AppHandle, label: &str) -> bool {
    use tauri::Manager;
    let Some((_, registry)) = CONTEXT.get() else { return false };
    // Hidden warm panes count as live: their windows exist (hidden) until a claim shows them.
    let panes = registry.lock().list_all();
    let live: Vec<String> = panes.iter().map(|p| p.id.clone()).collect();
    if let Some(pane_id) = pane_for_label(label, &live) {
        let status = panes.iter().find(|p| p.id == pane_id).map(|p| p.status.as_str()).unwrap_or("idle");
        if close_decision(status, confirm_setting(app)) == CloseDecision::Confirm {
            let _ = app.emit_to(label, CONFIRM_CLOSE_EVENT, serde_json::json!({ "pane_id": pane_id }));
            tracing::info!(%label, pane = %pane_id, status, "pane window close held: pane is busy");
            return true;
        }
    }
    if kill_pane_of_closed_window(label, &live, |id| {
        let result = crate::pty::registry::kill_pane_with_reason(registry, id, Some("pane window closed"));
        tracing::info!(%label, pane = %id, ?result, "pane window closed: killed its pane");
    }) {
        return false;
    }
    if close_is_forced(label, &live) {
        if let Some(window) = app.get_webview_window(label) {
            let result = window.destroy();
            tracing::info!(%label, ?result, "closed orphan pane window on close request");
        }
    }
    false
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
    fn busy_panes_ask_before_closing_and_idle_ones_do_not() {
        for busy in ["running", "waiting_input"] {
            assert_eq!(close_decision(busy, true), CloseDecision::Confirm, "{busy}");
            assert_eq!(close_decision(busy, false), CloseDecision::Close, "{busy} with the setting off");
        }
        for calm in ["idle", "error", "exited", ""] {
            assert_eq!(close_decision(calm, true), CloseDecision::Close, "{calm}");
        }
    }

    #[test]
    fn quitting_asks_when_any_pane_is_busy() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(quit_decision(&s(&["idle", "running"]), true), CloseDecision::Confirm);
        assert_eq!(quit_decision(&s(&["idle", "error"]), true), CloseDecision::Close);
        assert_eq!(quit_decision(&s(&["running"]), false), CloseDecision::Close);
        assert_eq!(quit_decision(&[], true), CloseDecision::Close);
    }

    #[test]
    fn the_main_window_is_never_swept() {
        assert!(orphaned_labels(&["main".to_string()], &[]).is_empty());
    }
}
