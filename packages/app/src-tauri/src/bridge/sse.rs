//! Server-sent events: the client registry, the `GET /events` stream and the `push_*` helpers
//! other modules use to broadcast pane, settings and operation updates to mobile clients.

use parking_lot::Mutex;
use serde_json::json;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use tauri::AppHandle;

use super::observe::observe_operation_pane_status;
use crate::pty::PaneRegistry;
use crate::settings_store;

/// SSE client — each connected GET /events response gets a sender.
pub(super) type SseSender = std::sync::mpsc::SyncSender<String>;
pub type SseClients = Arc<Mutex<Vec<SseSender>>>;

/// Global SSE client registry, shared between bridge thread and Tauri commands.
pub(super) static SSE_CLIENTS: once_cell::sync::OnceCell<SseClients> =
    once_cell::sync::OnceCell::new();

pub fn get_sse_clients() -> SseClients {
    SSE_CLIENTS
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

/// Push a raw SSE payload (e.g. "event: chat\ndata: {...}\n\n") to all connected clients.
pub fn push_sse(payload: String) {
    // Also kept for `GET /events/poll` (phones on a tunnel that does not pass SSE through).
    super::event_log::record(&payload);
    let clients = get_sse_clients();
    let mut guard = clients.lock();
    guard.retain(|sender| sender.try_send(payload.clone()).is_ok());
}

/// Forward live PTY bytes to mobile xterm.js clients.
pub fn push_terminal_sse(pane_id: &str, data: &[u8]) {
    let payload = json!({ "pane_id": pane_id, "data": data });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: terminal\ndata: {json}\n\n"));
    }
    crate::pane_wait_notify::bump_waiters();
}

/// Forward pane status changes to mobile clients.
pub fn push_pane_status_sse(pane_id: &str, status: &str) {
    observe_operation_pane_status(pane_id, status);
    let payload = json!({ "pane_id": pane_id, "status": status });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-status\ndata: {json}\n\n"));
        crate::pane_wait_notify::bump_waiters();
    }
}

/// Forward PTY geometry changes so remote viewers can mirror without resizing the PTY.
pub fn push_pane_resize_sse(pane_id: &str, cols: u16, rows: u16) {
    let payload = json!({ "pane_id": pane_id, "cols": cols, "rows": rows });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-resize\ndata: {json}\n\n"));
    }
}

pub(super) fn push_operation_sse(snapshot: &crate::operations::OperationSnapshot) {
    if let Ok(payload) = serde_json::to_string(snapshot) {
        push_sse(format!("event: operation\ndata: {payload}\n\n"));
    }
}

pub(super) fn handle_sse(
    stream: &mut TcpStream,
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) {
    let headers = "HTTP/1.1 200 OK\r\n\
        Content-Type: text/event-stream\r\n\
        Cache-Control: no-cache\r\n\
        Connection: keep-alive\r\n\
        Access-Control-Allow-Origin: *\r\n\
        \r\n\
        : connected\n\n";
    if stream.write_all(headers.as_bytes()).is_err() {
        return;
    }

    let panes = registry.lock().list();
    if let Ok(json) = serde_json::to_string(&panes) {
        let snapshot = format!("event: panes\ndata: {json}\n\n");
        if stream.write_all(snapshot.as_bytes()).is_err() {
            return;
        }
    }

    // Push the current public settings snapshot so the mobile PWA is in sync
    // from the moment it connects (and doesn't have to race GET /settings).
    let settings = settings_store::read_public_settings(app);
    if let Ok(json) = serde_json::to_string(&settings) {
        let snapshot = format!("event: settings\ndata: {json}\n\n");
        if stream.write_all(snapshot.as_bytes()).is_err() {
            return;
        }
    }

    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(64);
    get_sse_clients().lock().push(tx);

    for payload in rx {
        if stream.write_all(payload.as_bytes()).is_err() {
            break;
        }
    }
}

pub fn push_settings_sse(settings: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(settings) {
        push_sse(format!("event: settings\ndata: {json}\n\n"));
    }
}

pub fn push_panes_sse(registry: &Arc<Mutex<PaneRegistry>>) {
    let panes = registry.lock().list();
    if let Ok(json) = serde_json::to_string(&panes) {
        push_sse(format!("event: panes\ndata: {json}\n\n"));
    }
}

/// Broadcast `agent_definitions_changed` with the diff `{added, removed, changed}` (SSE clients).
pub fn push_agent_definitions_changed_sse(diff: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(diff) {
        push_sse(format!("event: agent_definitions_changed\ndata: {json}\n\n"));
    }
}

/// Start the definitions watcher (T33): every applied change is sent to SSE clients and to the
/// Tauri UI as `agent_definitions_changed`, and drops the warm standby workers of every agent
/// whose definition changed or went away.
pub fn start_definitions_watcher(app: AppHandle) {
    use tauri::Emitter;
    let result = crate::tui::declarative::watch::start(move |diff| {
        for id in diff.invalidated() {
            let dropped = crate::standby::discard_agent(id);
            if dropped > 0 {
                tracing::info!(agent = %id, dropped, "standby workers dropped: definition changed");
            }
        }
        let body = diff.to_json();
        push_agent_definitions_changed_sse(&body);
        let _ = app.emit("agent_definitions_changed", body);
    });
    if let Err(error) = result {
        tracing::warn!(%error, "agent definitions watcher not started");
    }
}
