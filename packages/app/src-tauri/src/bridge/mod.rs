//! Embedded HTTP bridge for external MCP clients.
//!
//! This bridge intentionally shares the same Rust `PaneRegistry` used by the
//! Tauri commands. External MCP calls therefore operate on the real visible
//! terminal panes instead of a stub sidecar process.
//!
//! Layout: `http`/`router`/`routes` parse and dispatch requests, `sse` broadcasts events,
//! `delegation`/`dispatch`/`locks`/`handoff` implement operations, and the rest are helpers.

mod approvals;
pub mod port_file;
mod delegation;
mod dispatch;
mod errors;
mod event_log;
mod handoff;
mod http;
#[cfg(test)]
pub(crate) use http::spawn_gate_listener;
mod locks;
mod observe;
mod pane_util;
mod projects;
mod router;
mod routes;
mod sse;
pub(crate) mod startup_gate;
mod state;
mod store;
#[cfg(test)]
mod tests;
mod tool_names;
mod ws;

pub use delegation::{delegate_operation, publish_operation, stop_operation_worker_with_kill};
pub use dispatch::dispatch_existing_operation;
#[cfg(test)]
pub(crate) use locks::acquire_operation_locks;
pub(crate) use locks::{
    fail_operation_on_lock_conflict, recover_interrupted_operations_on_startup,
    release_operation_locks,
};
pub use sse::{
    push_loop_signal_sse, push_pane_activity_sse, push_pane_preview_sse, push_pane_resize_sse, push_pane_status_sse, push_panes_sse, push_settings_sse, push_sse,
    push_terminal_sse, start_definitions_watcher,
};

use parking_lot::Mutex;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tauri::AppHandle;

use crate::mobile_pairing;
use crate::pty::PaneRegistry;
pub(crate) use observe::{checkpoint_pane_facts, checkpoint_public_settings, live_app, live_registry};
pub(crate) use sse::push_operation_sse;
use state::{BRIDGE_APP, BRIDGE_REGISTRY};

/// Tell the desktop windows to re-read pane info (`pty://panes-changed`) and the bridge viewers.
pub fn notify_panes_changed() {
    if let Some(app) = BRIDGE_APP.get() {
        use tauri::Emitter;
        let _ = app.emit("pty://panes-changed", serde_json::json!({ "changed": true }));
    }
}

/// A pane's PTY size changed outside its desktop window (phone-sized panes): tell bridge viewers
/// (`pane-resize` SSE) and the desktop windows, which re-read pane info on `pty://panes-changed`.
pub fn notify_pane_resized(pane_id: &str, cols: u16, rows: u16) {
    push_pane_resize_sse(pane_id, cols, rows, crate::pty::stream_pos::emitted(pane_id));
    if let Some(app) = BRIDGE_APP.get() {
        use tauri::Emitter;
        let _ = app.emit("pty://panes-changed", serde_json::json!({ "changed": true }));
    }
}

/// A pane's PTY size was changed by a phone lease (`pty/viewer_size.rs::apply`). Emits the ordered
/// `pty://resized` event that owner (pop-out) windows follow, so their xterm grid tracks the PTY grid
/// in stream order. Deliberately separate from `notify_pane_resized`, which the desktop's own
/// `resize_pane` command also calls: a desktop window must not get echoes of its own resizes.
pub fn notify_pty_resized_by_lease(pane_id: &str, cols: u16, rows: u16) {
    if let Some(app) = BRIDGE_APP.get() {
        use tauri::Emitter;
        let _ = app.emit(
            "pty://resized",
            serde_json::json!({
                "pane_id": pane_id,
                "cols": cols,
                "rows": rows,
                "offset": crate::pty::stream_pos::emitted(pane_id),
            }),
        );
    }
}

const HOST: &str = "127.0.0.1";
const PORT_MIN: u16 = 17321;
const PORT_MAX: u16 = 17399;

pub struct BridgeHandle {
    pub url: String,
}

impl BridgeHandle {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

fn bind_listener() -> Result<(TcpListener, u16), String> {
    for port in PORT_MIN..=PORT_MAX {
        match TcpListener::bind((HOST, port)) {
            Ok(listener) => return Ok((listener, port)),
            Err(_) => continue,
        }
    }
    Err(format!("no free bridge port in {PORT_MIN}-{PORT_MAX}"))
}

pub fn start_embedded_bridge(
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
    port_file: PathBuf,
    pairing_file: PathBuf,
) -> Result<BridgeHandle, String> {
    let _ = BRIDGE_APP.set(app.clone());
    let _ = BRIDGE_REGISTRY.set(registry.clone());
    if let Ok(recovered) = recover_interrupted_operations_on_startup() {
        for snapshot in recovered {
            push_operation_sse(&snapshot);
        }
    }
    // Tidy leftovers of worktree-isolated runs from a previous process (never removes work).
    let _ = thread::Builder::new().name("worktree-sweep".into()).spawn(|| {
        for project in crate::operations::indexed_project_paths() {
            let report = crate::worktrees::sweep(&project);
            if report != crate::worktrees::SweepReport::default() {
                tracing::info!(%project, ?report, "swept leftover worktrees");
            }
        }
    });
    mobile_pairing::init_pairing_store(pairing_file)?;
    let _ = crate::app_paths::ensure_app_data_dir();
    let (listener, port) = bind_listener()?;
    let url = format!("http://{HOST}:{port}");
    fs::write(&port_file, format!("{HOST}:{port}\n"))
        .map_err(|err| format!("write bridge port file {}: {err}", port_file.display()))?;

    thread::Builder::new()
        .name("puppet-master-http-bridge".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let registry = registry.clone();
                        let app = app.clone();
                        thread::spawn(move || {
                            if let Err(err) = http::handle_connection(stream, registry, app) {
                                tracing::debug!(%err, "bridge request failed");
                            }
                        });
                    }
                    Err(err) => tracing::debug!(%err, "bridge accept failed"),
                }
            }
        })
        .map_err(|err| format!("spawn bridge thread: {err}"))?;

    Ok(BridgeHandle::new(url))
}
