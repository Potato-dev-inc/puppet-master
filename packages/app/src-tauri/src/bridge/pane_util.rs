//! Pane helpers shared by the routes and the dispatcher: spawning with a timeout, idle waits,
//! dispatch-readiness checks and the "panes changed" notifications.

use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use super::sse::push_panes_sse;
use crate::pty::{registry_read_snapshot, registry_spawn_pane, PaneRegistry, SpawnPaneArgs};

pub(super) fn spawn_pane_with_timeout(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    args: SpawnPaneArgs,
    timeout: Duration,
) -> Result<String, crate::operations::OperationError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let registry = Arc::clone(registry);
    let app = app.clone();
    thread::Builder::new()
        .name("operation-pane-spawn".into())
        .spawn(move || {
            let _ = tx.send(registry_spawn_pane(&registry, &app, args));
        })
        .map_err(|err| {
            crate::operations::OperationError::new("PANE_SPAWN_FAILED", err.to_string(), true)
        })?;
    match rx.recv_timeout(timeout) {
        Ok(Ok(pane_id)) => Ok(pane_id),
        Ok(Err(err)) => Err(crate::operations::OperationError::new(
            "PANE_SPAWN_FAILED",
            err,
            true,
        )),
        Err(_) => Err(crate::operations::OperationError::new(
            "PANE_SPAWN_FAILED",
            format!(
                "timed out after {}s waiting for worker pane spawn",
                timeout.as_secs()
            ),
            true,
        )),
    }
}

/// Screen must hold still this long after the pane reads idle before a prompt is typed into it.
/// Idle already means the screen held still for the profile's `idle_after`, so this only has to
/// confirm two consecutive equal reads.
const PANE_SETTLE: Duration = Duration::from_millis(250);

/// Wait until the pane reads idle AND its screen has stopped changing. Status goes idle before a
/// TUI has finished redrawing its input box, and a prompt typed into that window is dropped.
pub(super) fn wait_for_pane_idle(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    let mut settled: Option<(String, Instant)> = None;
    loop {
        let status = registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
            .map(|pane| pane.status);
        match status.as_deref() {
            Some("error") | None => return,
            Some("idle") => {
                let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
                match &settled {
                    Some((previous, since)) if *previous == screen => {
                        if since.elapsed() >= PANE_SETTLE {
                            return;
                        }
                    }
                    _ => settled = Some((screen, Instant::now())),
                }
            }
            _ => settled = None,
        }
        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Whether an existing pane's read-only state fits the run (`read_only_spawned_pane` backends
/// only; every other backend is unaffected).
pub(super) fn read_only_pane_matches(agent_type: &str, run_read_only: bool, pane_id: &str) -> bool {
    !crate::tui::profile_for(agent_type)
        .features()
        .read_only_spawned_pane
        || crate::tui::opencode::read_only::is_enforced(pane_id) == run_read_only
}

pub(super) fn pane_ready_for_dispatch(
    pane: &crate::pty::PaneInfo,
    agent_type: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> bool {
    if pane.agent_type != agent_type || pane.status == "error" {
        return false;
    }
    if agent_type == "opencode_native" {
        return crate::opencode::status::pane_native_accepts_prompt(registry, &pane.id)
            .unwrap_or(pane.status == "idle");
    }
    pane.status == "idle"
}

pub(super) fn emit_panes_changed(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) {
    push_panes_sse(registry);
    let _ = app.emit("pty://panes-changed", json!({ "changed": true }));
    crate::pane_windows::schedule_sweep(registry, app);
}

/// Open a visible run's pane in its own pop-out window, created from Rust (no main webview is
/// involved). Never takes focus: agents open these. `_placement` is accepted for older callers;
/// every pane is a window now (panes-first).
pub(super) fn present_spawned_pane(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    _placement: Option<&str>,
) {
    if pane_id.starts_with("puppet-master-orchestrator-") {
        emit_panes_changed(registry, app);
        return;
    }
    let pane = registry.lock().list().into_iter().find(|pane| pane.id == pane_id);
    if let Some(pane) = pane {
        // Mode-aware: a pop-out in individual mode; in orchestration the pane just appears in the
        // cockpit (a hidden warm window is destroyed) and nothing takes focus.
        crate::cockpit::present_pane(app, &pane.id, &pane.agent_type, pane.cols, pane.rows, crate::cockpit::Origin::Agent);
    }
    emit_panes_changed(registry, app);
}

pub(super) fn resolve_default_opencode_pane_id(
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Option<String> {
    let panes: Vec<_> = registry
        .lock()
        .list()
        .into_iter()
        .filter(|pane| pane.agent_type == "opencode_native" && pane.status != "error")
        .collect();
    if panes.len() == 1 {
        Some(panes[0].id.clone())
    } else {
        None
    }
}

pub(super) fn pane_buffer_view(
    requested: Option<&str>,
    agent_type: &str,
) -> Result<&'static str, &'static str> {
    let default = if crate::pty::status::is_shell_agent(agent_type) {
        "scrollback"
    } else {
        "screen"
    };
    match requested {
        None => Ok(default),
        Some("screen") => Ok("screen"),
        Some("scrollback") => Ok("scrollback"),
        Some(_) => Err("view must be screen or scrollback"),
    }
}

pub(super) fn register_spawned_pane(
    session: Option<&str>,
    agent_type: &str,
    pane_id: &str,
) -> Result<(), crate::operations::OperationError> {
    let Some(session) = session else {
        return Ok(());
    };
    if crate::pty::status::is_shell_agent(agent_type) {
        crate::mcp_sessions::register_shell_pane(session, pane_id)
    } else {
        crate::mcp_sessions::register_owned_pane(session, pane_id)
    }
}

pub(super) fn registry_pane_ids(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<String> {
    registry.lock().panes.keys().cloned().collect()
}
