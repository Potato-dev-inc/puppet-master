//! Server-sent events: the client registry, the `GET /events` stream and the `push_*` helpers
//! other modules use to broadcast pane, settings and operation updates to mobile clients.

use base64::Engine;
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
    // Secrets never go out to phones: every event except the live terminal and structural ones is redacted.
    let payload = crate::redact::fanout::sse_frame(payload);
    // Also kept for `GET /events/poll` (phones on a tunnel that does not pass SSE through).
    super::event_log::record(&payload);
    let clients = get_sse_clients();
    let mut guard = clients.lock();
    guard.retain(|sender| sender.try_send(payload.clone()).is_ok());
}

/// The `event: terminal` SSE payload: the PTY bytes as standard base64 under `b64`
/// (a JSON number array was 3-4x larger on the wire). `offset` is where the chunk starts in the
/// pane's byte stream (`pty::stream_pos`), the same numbering `GET /panes/{id}/screen` and
/// `pane-resize` use, so a phone joins a screen snapshot to live output without losing or
/// repeating a byte. Left out when the source does not number its chunks (older/other producers).
pub(crate) fn terminal_payload(pane_id: &str, data: &[u8], offset: Option<u64>) -> Option<String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let mut payload = json!({ "pane_id": pane_id, "b64": b64 });
    if let Some(offset) = offset {
        payload["offset"] = json!(offset);
    }
    serde_json::to_string(&payload)
        .ok()
        .map(|json| format!("event: terminal\ndata: {json}\n\n"))
}

/// Forward live PTY bytes to mobile xterm.js clients. `offset`: where the chunk starts in the
/// pane's byte stream (`terminal-data`'s `offset`).
pub fn push_terminal_sse(pane_id: &str, data: &[u8], offset: Option<u64>) {
    crate::pane_activity::note_output(pane_id);
    if let Some(payload) = terminal_payload(pane_id, data, offset) {
        push_sse(payload);
    }
    crate::pane_wait_notify::bump_waiters();
}

#[cfg(test)]
mod tests {
    use super::terminal_payload;
    use base64::Engine;

    #[test]
    fn terminal_payload_carries_base64_not_a_number_array() {
        let bytes = [0_u8, 27, 91, 255, 10];
        let payload = terminal_payload("p1", &bytes, None).expect("payload");
        let json = payload
            .strip_prefix("event: terminal\ndata: ")
            .and_then(|rest| rest.strip_suffix("\n\n"))
            .expect("frame shape");
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["pane_id"], "p1");
        assert!(value.get("data").is_none());
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value["b64"].as_str().unwrap())
            .unwrap();
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn terminal_payload_carries_the_chunk_offset_when_the_reader_numbered_it() {
        let with = terminal_payload("p1", b"abc", Some(4096)).unwrap();
        let value: serde_json::Value = serde_json::from_str(
            with.strip_prefix("event: terminal\ndata: ").and_then(|r| r.strip_suffix("\n\n")).unwrap(),
        )
        .unwrap();
        assert_eq!(value["offset"], 4096);
        // A producer that does not number its chunks (older build, native OpenCode panes) sends none.
        let without = terminal_payload("p1", b"abc", None).unwrap();
        assert!(!without.contains("offset"), "{without}");
    }

    #[test]
    fn pane_resize_payload_carries_the_stream_offset_it_falls_after() {
        let with = super::pane_resize_payload("p1", 52, 30, Some(777)).unwrap();
        let value: serde_json::Value = serde_json::from_str(
            with.strip_prefix("event: pane-resize\ndata: ").and_then(|r| r.strip_suffix("\n\n")).unwrap(),
        )
        .unwrap();
        assert_eq!(value, serde_json::json!({"pane_id": "p1", "cols": 52, "rows": 30, "offset": 777}));
        let without = super::pane_resize_payload("p1", 52, 30, None).unwrap();
        assert!(!without.contains("offset"), "{without}");
    }

    #[test]
    fn offsets_survive_the_event_log_that_feeds_long_polling() {
        // The poll fallback replays the very strings pushed to SSE/WS, so the offset rides along.
        let mut log = crate::bridge::event_log::EventLog::default();
        log.push(terminal_payload("pp", b"hello", Some(10)).unwrap());
        log.push(super::pane_resize_payload("pp", 40, 20, Some(15)).unwrap());
        let poll = log.since(0);
        assert!(poll.data.contains("\"offset\":10"), "{}", poll.data);
        assert!(poll.data.contains("\"offset\":15"), "{}", poll.data);
    }

    #[test]
    fn status_hooks_run_in_the_documented_order() {
        use super::StatusHook::*;
        assert_eq!(
            super::STATUS_HOOK_ORDER,
            [PaneActivity, Ambient, Operations, Checkpoints, Push, TurnSummary],
            "the status-hook order is a contract: edit STATUS_HOOK_ORDER and this test together"
        );
        let position = |hook| super::STATUS_HOOK_ORDER.iter().position(|h| *h == hook).unwrap();
        assert!(
            position(Checkpoints) < position(TurnSummary),
            "TurnSummary waits on the ticket Checkpoints takes, so it must run after it"
        );
    }

    #[test]
    fn the_runner_visits_the_real_list_front_to_back() {
        let mut seen = Vec::new();
        super::run_in_order(&super::STATUS_HOOK_ORDER, |hook| seen.push(hook));
        assert_eq!(seen, super::STATUS_HOOK_ORDER);
    }

    #[test]
    fn terminal_payload_is_compact_for_large_chunks() {
        let bytes = vec![b'x'; 4096];
        let payload = terminal_payload("p", &bytes, None).unwrap();
        // base64 is 4/3 of the input; a JSON array of numbers would be >= 2x.
        assert!(payload.len() < 4096 * 2);
    }
}

/// One reaction to a pane status change, named so the order can be pinned by a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusHook {
    PaneActivity,
    Ambient,
    Operations,
    Checkpoints,
    Push,
    TurnSummary,
}

/// Everything that reacts to a pane status change, in the order it runs. The order is a contract:
/// `status_hooks_run_in_the_documented_order` fails when one moves, so a reorder is a deliberate
/// edit of this list AND that test.
///
/// Why this order:
/// 1. `PaneActivity` first: the activity clock is fresh before any hook reads it.
/// 2. `Ambient` (tray icon) is a cheap flag flip with no dependencies.
/// 3. `Operations` records pane errors on the owning run before anything announces the change.
/// 4. `Checkpoints` takes the settle ticket for this rest point (and starts its worker thread).
/// 5. `Push` (phone notifications) is independent of the others.
/// 6. `TurnSummary` MUST come after `Checkpoints`: it waits on the newest ticket, which is the one
///    `Checkpoints` just took (`turn_summary::on_pane_status`).
///
/// These run on the caller's thread, which is the PTY reader thread (the `pty://status` listener
/// in `lib.rs` runs inline in the emitter). Each must stay cheap and move slow work (git, disk,
/// network) to its own worker thread; `Checkpoints` and `TurnSummary` do. A list of names rather
/// than function pointers keeps the order testable without linking the tray and window code the
/// hooks reach.
pub(crate) const STATUS_HOOK_ORDER: [StatusHook; 6] = [
    StatusHook::PaneActivity,
    StatusHook::Ambient,
    StatusHook::Operations,
    StatusHook::Checkpoints,
    StatusHook::Push,
    StatusHook::TurnSummary,
];

/// Visit `order` front to back.
fn run_in_order(order: &[StatusHook], mut run: impl FnMut(StatusHook)) {
    for hook in order {
        run(*hook);
    }
}

fn run_status_hook(hook: StatusHook, pane_id: &str, status: &str) {
    match hook {
        StatusHook::PaneActivity => crate::pane_activity::note_output(pane_id),
        StatusHook::Ambient => crate::ambient::notify_changed(),
        StatusHook::Operations => observe_operation_pane_status(pane_id, status),
        StatusHook::Checkpoints => crate::checkpoints::on_pane_status(pane_id, status),
        StatusHook::Push => crate::push::notify_pane_status(pane_id, status),
        StatusHook::TurnSummary => crate::turn_summary::on_pane_status(pane_id, status),
    }
}

/// Forward pane status changes to mobile clients.
pub fn push_pane_status_sse(pane_id: &str, status: &str) {
    run_in_order(&STATUS_HOOK_ORDER, |hook| run_status_hook(hook, pane_id, status));
    let payload = json!({ "pane_id": pane_id, "status": status });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-status\ndata: {json}\n\n"));
        crate::pane_wait_notify::bump_waiters();
    }
}

/// Forward a pane's plain-language activity phrase (`pane_activity`) to mobile clients.
pub fn push_pane_activity_sse(pane_id: &str, activity: &crate::tui::Activity) {
    let payload = json!({ "pane_id": pane_id, "activity": activity });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-activity
data: {json}

"));
    }
}

/// Forward a pane's dev-server URLs (`pane_preview`) to mobile clients; an empty list clears them.
pub fn push_pane_preview_sse(pane_id: &str, urls: &[crate::pane_preview::PreviewUrl]) {
    let payload = json!({ "pane_id": pane_id, "urls": urls });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: pane-preview
data: {json}

"));
    }
}

/// Tell mobile clients a pane looks stuck (`Some`) or that the warning cleared (`None`).
pub fn push_loop_signal_sse(signal: Option<&crate::loop_guard::LoopSignal>, pane_id: &str) {
    let payload = json!({ "pane_id": pane_id, "signal": signal });
    if let Ok(json) = serde_json::to_string(&payload) {
        push_sse(format!("event: loop-signal
data: {json}

"));
    }
}

/// Forward PTY geometry changes so remote viewers can mirror without resizing the PTY. `offset` is
/// the stream position the resize falls after (`pty::stream_pos::emitted`), so a viewer applies it
/// in order with the `terminal` chunks and knows whether a screen snapshot already shows it.
pub fn push_pane_resize_sse(pane_id: &str, cols: u16, rows: u16, offset: Option<u64>) {
    if let Some(payload) = pane_resize_payload(pane_id, cols, rows, offset) {
        push_sse(payload);
    }
}

/// The `event: pane-resize` SSE payload (see [`push_pane_resize_sse`]).
pub(crate) fn pane_resize_payload(pane_id: &str, cols: u16, rows: u16, offset: Option<u64>) -> Option<String> {
    let mut payload = json!({ "pane_id": pane_id, "cols": cols, "rows": rows });
    if let Some(offset) = offset {
        payload["offset"] = json!(offset);
    }
    serde_json::to_string(&payload)
        .ok()
        .map(|json| format!("event: pane-resize\ndata: {json}\n\n"))
}

pub(crate) fn push_operation_sse(snapshot: &crate::operations::OperationSnapshot) {
    crate::push::notify_operation(snapshot);
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
