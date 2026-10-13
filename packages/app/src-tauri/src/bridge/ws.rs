//! `GET /events/ws`: the live event feed over a WebSocket (Cloudflare quick tunnels pass
//! WebSockets, so a phone there gets pushed events without the long-poll round trip).
//!
//! Protocol (text frames only):
//! - server -> client: each frame is one or more concatenated SSE payloads
//!   (`event: X\ndata: {...}\n\n`), the same strings `GET /events` streams. The first frames are
//!   the `panes` and `settings` snapshots.
//! - client -> server `{"type":"watch","pane_ids":["a","b"]}`: from then on `event: terminal`
//!   payloads are forwarded only for those panes. Before any watch message everything is forwarded.
//! - client -> server `{"type":"input","pane_id":"p","text":"...","append_newline":false}`: types
//!   into the pane (same as `POST /panes/{id}/input`). Failures come back as
//!   `{"type":"input-error","pane_id":"p","error":"..."}`.
//! - auth: browsers cannot set `Authorization` on a WebSocket, so the token travels as the
//!   subprotocol `pm-bearer.<token>`; the server echoes only `pm-events`.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{ErrorKind, Write};
use std::net::TcpStream;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::{CloseFrame, Role};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Error as WsError, Message, WebSocket};

use super::errors::{enrich_handle_error, operation_http_error};
use super::sse::get_sse_clients;
use crate::pty::PaneRegistry;
use crate::settings_store;

/// Per-connection channel depth. A client that falls this far behind is dropped (resync).
const CHANNEL_DEPTH: usize = 2048;
/// Max payloads folded into one outgoing text frame.
const MAX_BATCH: usize = 256;
const PING_EVERY: Duration = Duration::from_secs(20);
const SUBPROTOCOL: &str = "pm-events";
const BEARER_PREFIX: &str = "pm-bearer.";

/// The bearer token from a `Sec-WebSocket-Protocol` offer (`pm-events, pm-bearer.<token>`).
pub(crate) fn bearer_from_protocols(offer: &str) -> Option<String> {
    offer
        .split(',')
        .map(str::trim)
        .find_map(|item| item.strip_prefix(BEARER_PREFIX))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

/// Whether the client offered the `pm-events` subprotocol (only that one is ever echoed).
pub(crate) fn offers_pm_events(offer: &str) -> bool {
    offer.split(',').map(str::trim).any(|item| item == SUBPROTOCOL)
}

/// Pure filter: `event: terminal` payloads only for watched panes; everything else always.
pub(crate) fn should_forward(payload: &str, watch: &Option<HashSet<String>>) -> bool {
    let Some(watch) = watch else { return true };
    let Some(rest) = payload.strip_prefix("event: terminal\ndata: ") else {
        return true;
    };
    let body = rest.strip_suffix("\n\n").unwrap_or(rest);
    match serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("pane_id").and_then(Value::as_str).map(str::to_string))
    {
        Some(pane_id) => watch.contains(&pane_id),
        None => true,
    }
}

/// Serve an already-authorized upgrade request. `auth_headers` is the request head (including
/// any bearer token taken from the subprotocol); it is used for the input authorization.
pub(super) fn serve(
    mut stream: TcpStream,
    key: &str,
    offered_pm_events: bool,
    auth_headers: &str,
    peer_loopback: bool,
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) {
    let accept = derive_accept_key(key.as_bytes());
    let protocol = if offered_pm_events {
        format!("Sec-WebSocket-Protocol: {SUBPROTOCOL}\r\n")
    } else {
        String::new()
    };
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\
         {protocol}\r\n"
    );
    if stream.write_all(response.as_bytes()).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);

    let mut snapshot = String::new();
    if let Ok(json) = serde_json::to_string(&registry.lock().list()) {
        snapshot.push_str(&format!("event: panes\ndata: {json}\n\n"));
    }
    let settings = settings_store::read_public_settings(app);
    if let Ok(json) = serde_json::to_string(&settings) {
        snapshot.push_str(&format!("event: settings\ndata: {json}\n\n"));
    }

    let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);
    if ws.send(Message::Text(snapshot)).is_err() {
        return;
    }
    if ws.get_ref().set_nonblocking(true).is_err() {
        return;
    }

    let (tx, rx) = mpsc::sync_channel::<String>(CHANNEL_DEPTH);
    get_sse_clients().lock().push(tx);

    let session = super::http::request_session(auth_headers).map(str::to_string);
    let ctx = InputCtx {
        headers: auth_headers,
        peer_loopback,
        session: session.as_deref(),
        registry: &registry,
        app,
    };
    let mut watch: Option<HashSet<String>> = None;
    let mut last_ping = Instant::now();

    loop {
        // Outbound: wait briefly for the next payload, then fold in whatever else is queued.
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(first) => {
                let mut batch = String::new();
                if should_forward(&first, &watch) {
                    batch.push_str(&first);
                }
                let mut disconnected = false;
                for _ in 0..MAX_BATCH {
                    match rx.try_recv() {
                        Ok(next) => {
                            if should_forward(&next, &watch) {
                                batch.push_str(&next);
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                if !batch.is_empty() && queue(&mut ws, Message::Text(batch)).is_err() {
                    return;
                }
                if disconnected {
                    return close_for_resync(&mut ws);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // push_sse dropped us (overflow or shutdown): the client must resync.
            Err(mpsc::RecvTimeoutError::Disconnected) => return close_for_resync(&mut ws),
        }

        // Inbound: drain every complete client message without blocking.
        loop {
            match ws.read() {
                Ok(Message::Text(text)) => {
                    if let Some(reply) = handle_client_text(&text, &mut watch, &ctx) {
                        if queue(&mut ws, Message::Text(reply)).is_err() {
                            return;
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    let _ = ws.close(None);
                    let _ = flush_briefly(&mut ws);
                    return;
                }
                Ok(_) => {}
                Err(WsError::Io(err)) if err.kind() == ErrorKind::WouldBlock => break,
                Err(_) => return,
            }
        }

        if last_ping.elapsed() >= PING_EVERY {
            last_ping = Instant::now();
            if queue(&mut ws, Message::Ping(Vec::new())).is_err() {
                return;
            }
        }
        // Keep pending pongs and partially written frames moving.
        if !matches!(ws.flush(), Ok(()) | Err(WsError::Io(_))) {
            return;
        }
    }
}

struct InputCtx<'a> {
    headers: &'a str,
    peer_loopback: bool,
    session: Option<&'a str>,
    registry: &'a Arc<Mutex<PaneRegistry>>,
    app: &'a AppHandle,
}

/// Handle one client text message; returns a reply text frame when there is one to send.
fn handle_client_text(
    text: &str,
    watch: &mut Option<HashSet<String>>,
    ctx: &InputCtx<'_>,
) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    match value.get("type").and_then(Value::as_str)? {
        "watch" => {
            let ids = value.get("pane_ids")?.as_array()?;
            *watch = Some(
                ids.iter()
                    .filter_map(|id| id.as_str().map(str::to_string))
                    .collect(),
            );
            None
        }
        "input" => {
            let pane_id = value.get("pane_id").and_then(Value::as_str)?.to_string();
            let body_text = value.get("text").and_then(Value::as_str)?.to_string();
            let append_newline = value
                .get("append_newline")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match write_input(ctx, &pane_id, &body_text, append_newline, &value) {
                Ok(()) => None,
                Err(error) => Some(
                    json!({"type": "input-error", "pane_id": pane_id, "error": error}).to_string(),
                ),
            }
        }
        _ => None,
    }
}

/// Same path as `POST /panes/{id}/input`, including its authorization: a paired phone (pairing
/// token required, no MCP session) may type into any pane; anything else goes through the
/// MCP session check.
fn write_input(
    ctx: &InputCtx<'_>,
    pane_id: &str,
    text: &str,
    append_newline: bool,
    message: &Value,
) -> Result<(), Value> {
    let paired_phone_typing = ctx.session.is_none()
        && crate::mobile_pairing::requires_pairing_token(ctx.headers, ctx.peer_loopback);
    if !paired_phone_typing {
        let route_path = format!("/panes/{pane_id}/input");
        crate::mcp_sessions::authorize(ctx.session, "POST", &route_path, message)
            .map_err(|error| enrich_handle_error(error, ctx.registry))
            .map_err(|error| {
                let (_, value) = operation_http_error(error);
                value.get("error").cloned().unwrap_or(value)
            })?;
    }
    crate::pty::registry_write_input(ctx.registry, ctx.app, pane_id, text, append_newline, false, None)
        .map_err(|err| Value::String(err))
}

/// Queue a message and push what can be written now. WouldBlock is fine: the rest goes out on
/// later flushes. Any other error ends the connection.
fn queue(ws: &mut WebSocket<TcpStream>, message: Message) -> Result<(), ()> {
    match ws.write(message) {
        Ok(()) | Err(WsError::Io(_)) => {}
        Err(_) => return Err(()),
    }
    match ws.flush() {
        Ok(()) | Err(WsError::Io(_)) => Ok(()),
        Err(_) => Err(()),
    }
}

/// Send a 1011 close so the client knows to reconnect and resync, then give it a moment to drain.
fn close_for_resync(ws: &mut WebSocket<TcpStream>) {
    let _ = ws.close(Some(CloseFrame {
        code: CloseCode::Error,
        reason: "resync".into(),
    }));
    let _ = flush_briefly(ws);
}

fn flush_briefly(ws: &mut WebSocket<TcpStream>) -> Result<(), ()> {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match ws.flush() {
            Ok(()) => return Ok(()),
            Err(WsError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(());
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_is_taken_from_the_offer() {
        assert_eq!(
            bearer_from_protocols("pm-events, pm-bearer.abc123"),
            Some("abc123".into())
        );
        assert_eq!(bearer_from_protocols("pm-bearer.tok"), Some("tok".into()));
        assert_eq!(bearer_from_protocols("pm-events"), None);
        assert_eq!(bearer_from_protocols("pm-bearer."), None);
    }

    #[test]
    fn pm_events_is_detected_in_the_offer() {
        assert!(offers_pm_events("pm-events, pm-bearer.x"));
        assert!(offers_pm_events("pm-bearer.x,pm-events"));
        assert!(!offers_pm_events("pm-bearer.x"));
        assert!(!offers_pm_events("pm-eventsx"));
    }

    #[test]
    fn unfiltered_forwards_everything() {
        assert!(should_forward("event: terminal\ndata: {\"pane_id\":\"a\",\"b64\":\"\"}\n\n", &None));
    }

    #[test]
    fn watch_filters_terminal_payloads_only() {
        let watch = Some(HashSet::from(["a".to_string()]));
        let term_a = "event: terminal\ndata: {\"pane_id\":\"a\",\"b64\":\"eA==\"}\n\n";
        let term_b = "event: terminal\ndata: {\"pane_id\":\"b\",\"b64\":\"eA==\"}\n\n";
        assert!(should_forward(term_a, &watch));
        assert!(!should_forward(term_b, &watch));
        assert!(should_forward("event: panes\ndata: []\n\n", &watch));
        assert!(should_forward("event: pane-status\ndata: {\"pane_id\":\"b\"}\n\n", &watch));
        assert!(should_forward("event: terminal\ndata: not json\n\n", &watch));
    }

    #[test]
    fn accept_key_matches_rfc_example() {
        assert_eq!(
            derive_accept_key(b"dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    /// A real handshake and one frame through tungstenite's client, on loopback.
    #[test]
    fn handshake_and_frame_round_trip() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Read the request head only; the handshake response is what this test checks.
            let mut buf = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = std::io::Read::read(&mut stream, &mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf).to_string();
            let key = crate::mobile_pairing::header_value(&head, "Sec-WebSocket-Key").unwrap();
            let accept = derive_accept_key(key.as_bytes());
            let response = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\nSec-WebSocket-Protocol: pm-events\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
            let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);
            ws.send(Message::Text("event: panes\ndata: []\n\n".into())).unwrap();
            // Wait for the client's close so the test does not race the socket teardown.
            let _ = ws.read();
        });

        let url = format!("ws://{addr}/events/ws");
        let mut request = tungstenite::client::IntoClientRequest::into_client_request(url).unwrap();
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            "pm-events, pm-bearer.test".parse().unwrap(),
        );
        let (mut client, response) = tungstenite::connect(request).unwrap();
        assert_eq!(response.status(), 101);
        match client.read().unwrap() {
            Message::Text(text) => assert_eq!(text, "event: panes\ndata: []\n\n"),
            other => panic!("unexpected frame {other:?}"),
        }
        let _ = client.close(None);
        let _ = server.join();
    }
}
