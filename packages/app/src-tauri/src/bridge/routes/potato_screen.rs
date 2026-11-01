//! `/potato/sandbox/screen/<sealed>/...`: the sandbox screen viewer (spec "screen.rs"), and
//! `POST /potato/sandbox/screen-link`: how a paired phone gets a link.
//!
//! - `GET .../embed.html` (and the viewer's own scripts and `novnc/`): the files of `packages/potato-sandbox/viewer/`.
//!   The page gets its parent-origin allowlist filled in (`screen::parent_origins`) and is sent with
//!   `Cache-Control: no-store, no-transform`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and a
//!   `frame-ancestors` policy for those same origins.
//! - `GET .../websockify` with `Upgrade: websocket`: relayed byte for byte to the guest's websockify on
//!   `127.0.0.1:<view port>`. Only the handshake headers the websocket needs are forwarded (no cookie, no bearer, no
//!   local secret, no `Origin`). While the relay is open it is checked every second (`ScreenHub::revalidate`, and for a
//!   phone that its pairing still allows watching) and closed the moment the link stops being good: the lease was lost,
//!   the generation moved on, the link ran out, a Labs switch went off.
//! - `POST /potato/sandbox/screen-link` `{ "botId", "mode": "view"|"control" }`: a paired phone's way to a link. The
//!   desktop gets its links through the Tauri command, never through the bridge: a local program (an agent in a pane
//!   calls the bridge too) must not be able to ask for a screen password.
//!
//! Who may call: `gate` runs from `bridge/http.rs::gate_with`. A request that does not need a pairing token (a loopback
//! peer with no forwarder header, no foreign `Host`, no foreign `Origin`) is the app's own webview; anything else must be
//! a link made for a paired device that may still watch. Then the sealed link decides what it reaches
//! (`ScreenHub::open`). The VNC password is never in a path or a query and never on this side of the viewer: it is in the
//! URL fragment of the iframe address, which no request carries. Nothing here logs.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::bridge::errors::RouteResult;
use crate::mobile_pairing::{self, Cap, Caller};
use crate::potato::sandbox::screen::{self, Audience, Mode, ScreenError, ScreenHub};

use super::Ctx;

const HEAD_LIMIT: usize = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

fn not_found() -> (u16, Value) {
    (404, json!({ "error": "not found" }))
}

/// Whether these segments are under `/potato/sandbox/screen/<sealed>/`.
pub(in crate::bridge) fn is_screen_path(segments: &[&str]) -> bool {
    screen::route_parts(segments).is_some()
}

/// The caller kind for a screen request, or `None` when the request is not about the screen.
pub(in crate::bridge) fn gate(hub: &ScreenHub, headers: &str, peer_loopback: bool, segments: &[&str]) -> Option<Result<Caller, (u16, Value)>> {
    let (sealed, _) = screen::route_parts(segments)?;
    if !hub.enabled() {
        return Some(Err(not_found()));
    }
    if !mobile_pairing::requires_pairing_token(headers, peer_loopback) {
        return Some(Ok(Caller::Local));
    }
    // From outside: only a link made for a paired phone that may still watch.
    let Some(Audience::Device(device_id)) = hub.peek_audience(sealed) else { return Some(Err(not_found())) };
    Some(match mobile_pairing::stream_caps(&device_id) {
        Some(caps) if caps.has(Cap::Watch) => Ok(Caller::Device { device_id, caps }),
        _ => Err((403, json!({ "code": "DEVICE_NOT_ALLOWED", "error": "This phone may not watch right now." }))),
    })
}

fn audience_of(caller: &Caller) -> Option<Audience> {
    match caller {
        Caller::Local => Some(Audience::App),
        Caller::Device { device_id, .. } => Some(Audience::Device(device_id.clone())),
        Caller::Public | Caller::Brain { .. } => None,
    }
}

/// Whether the phone behind `caller` may still do what the link is for (watch; for control also type).
fn caller_still_allowed(caller: &Caller, mode: Mode) -> bool {
    match caller {
        Caller::Local => true,
        Caller::Device { device_id, .. } => mobile_pairing::stream_caps(device_id).is_some_and(|caps| {
            caps.has(Cap::Watch) && (mode == Mode::View || caps.has(Cap::Type))
        }),
        Caller::Public | Caller::Brain { .. } => false,
    }
}

// ---- answering -----------------------------------------------------------------------------------

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        502 => "Bad Gateway",
        _ => "Service Unavailable",
    }
}

/// The headers of every answer from this file: nothing is kept, nothing is rewritten on the way, nothing is sniffed.
const COMMON: &str = "Cache-Control: no-store, no-transform\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n";

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, extra: &str, body: &[u8], head_only: bool) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{COMMON}{extra}\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes()).map_err(|e| format!("write response: {e}"))?;
    if !head_only {
        stream.write_all(body).map_err(|e| format!("write response: {e}"))?;
    }
    Ok(())
}

fn respond_json(stream: &mut TcpStream, status: u16, value: &Value) -> Result<(), String> {
    respond(stream, status, "application/json; charset=utf-8", "", value.to_string().as_bytes(), false)
}

fn respond_error(stream: &mut TcpStream, error: &ScreenError) -> Result<(), String> {
    respond_json(stream, error.status(), &error.json())
}

// ---- serving -------------------------------------------------------------------------------------

/// One request under `/potato/sandbox/screen/<sealed>/`. `caller` is what `gate` decided. `viewer` is the folder with
/// `embed.html` and `novnc/`.
pub(in crate::bridge) fn serve(
    stream: &mut TcpStream,
    hub: &Arc<ScreenHub>,
    viewer: Option<&Path>,
    method: &str,
    headers: &str,
    segments: &[&str],
    caller: &Caller,
) -> Result<(), String> {
    let Some((sealed, rest)) = screen::route_parts(segments) else { return respond_json(stream, 404, &not_found().1) };
    let Some(audience) = audience_of(caller) else { return respond_json(stream, 403, &json!({ "error": "not allowed" })) };
    let grant = match hub.open(sealed, &audience) {
        Ok(grant) => grant,
        Err(error) => return respond_error(stream, &error),
    };
    if rest == [screen::WEBSOCKET_FILE] {
        if method != "GET" || !is_websocket_upgrade(headers) {
            return respond_json(stream, 400, &json!({ "error": "a websocket upgrade is expected here" }));
        }
        let (hub, caller_for_check, audience, check_grant) = (hub.clone(), caller.clone(), audience.clone(), grant.clone());
        let check = move || hub.revalidate(&check_grant, &audience).is_ok() && caller_still_allowed(&caller_for_check, check_grant.claims.m);
        return relay_websocket(stream, grant.port, headers, check, screen::RECHECK_EVERY);
    }
    if method != "GET" && method != "HEAD" {
        return respond_json(stream, 405, &json!({ "error": "method not allowed" }));
    }
    let Some(rel) = screen::viewer_file(rest) else { return respond_json(stream, 404, &not_found().1) };
    let Some(root) = viewer else {
        return respond_json(stream, 503, &json!({ "code": "SCREEN_VIEWER_MISSING", "error": "The screen viewer files are missing from this install." }));
    };
    let Some(bytes) = read_viewer_file(root, &rel) else { return respond_json(stream, 404, &not_found().1) };
    let remote = matches!(caller, Caller::Device { .. });
    if rel == "embed.html" {
        let origins = screen::parent_origins(headers, remote);
        let page = screen::fill_viewer_page(&String::from_utf8_lossy(&bytes), &origins);
        let extra = format!("Content-Security-Policy: {}\r\n", screen::frame_policy(&origins));
        return respond(stream, 200, screen::content_type(&rel), &extra, page.as_bytes(), method == "HEAD");
    }
    respond(stream, 200, screen::content_type(&rel), "", &bytes, method == "HEAD")
}

fn is_websocket_upgrade(headers: &str) -> bool {
    mobile_pairing::header_value(headers, "Upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

/// Reads `rel` (already checked by `screen::viewer_file`) from under `root`; a link out of the folder is refused.
fn read_viewer_file(root: &Path, rel: &str) -> Option<Vec<u8>> {
    let root = root.canonicalize().ok()?;
    let path: PathBuf = root.join(rel);
    let path = path.canonicalize().ok()?;
    if !path.starts_with(&root) || !path.is_file() {
        return None;
    }
    std::fs::read(path).ok()
}

// ---- the websocket relay -------------------------------------------------------------------------

/// The handshake headers that go to the guest. Everything else (cookie, bearer, local secret, `Origin`, proxy
/// headers) stays on this side.
const FORWARDED: [&str; 5] = ["Upgrade", "Connection", "Sec-WebSocket-Key", "Sec-WebSocket-Version", "Sec-WebSocket-Protocol"];

fn handshake(port: u16, headers: &str) -> String {
    let mut out = format!("GET /{} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n", screen::WEBSOCKET_FILE);
    for name in FORWARDED {
        if let Some(value) = mobile_pairing::header_value(headers, name) {
            // A header value never holds a line break (the request was split on them), but make sure.
            if !value.contains(['\r', '\n']) {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
        }
    }
    out.push_str("\r\n");
    out
}

/// Connects to the guest, forwards the handshake, then copies bytes both ways until either side closes or `check`
/// (asked every `every`) says the link is no longer good.
pub(in crate::bridge) fn relay_websocket(
    client: &mut TcpStream,
    port: u16,
    headers: &str,
    check: impl Fn() -> bool + Send + 'static,
    every: Duration,
) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut upstream = match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(_) => return respond_json(client, 502, &json!({ "code": "SCREEN_NOT_REACHABLE", "error": "The sandbox screen did not answer." })),
    };
    let _ = upstream.set_nodelay(true);
    let _ = client.set_nodelay(true);
    upstream.write_all(handshake(port, headers).as_bytes()).map_err(|e| format!("write handshake: {e}"))?;

    // The guest's answer head goes back as it is (101, the accepted key, the chosen protocol).
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = upstream.read(&mut chunk).map_err(|e| format!("read handshake: {e}"))?;
        if n == 0 {
            return respond_json(client, 502, &json!({ "code": "SCREEN_NOT_REACHABLE", "error": "The sandbox screen did not answer." }));
        }
        head.extend_from_slice(&chunk[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if head.len() > HEAD_LIMIT {
            return respond_json(client, 502, &json!({ "code": "SCREEN_NOT_REACHABLE", "error": "The sandbox screen did not answer." }));
        }
    }
    client.write_all(&head).map_err(|e| format!("write handshake answer: {e}"))?;
    if !head.starts_with(b"HTTP/1.1 101") {
        return Ok(());
    }

    let mut client_reader = client.try_clone().map_err(|e| format!("clone client socket: {e}"))?;
    let mut upstream_writer = upstream.try_clone().map_err(|e| format!("clone guest socket: {e}"))?;
    let (client_kill, upstream_kill) = (client.try_clone().map_err(|e| e.to_string())?, upstream.try_clone().map_err(|e| e.to_string())?);
    let (done_tx, done_rx) = mpsc::channel::<()>();

    // The watcher: ends the relay when the link stops being good; ends itself when the relay ends.
    let (watch_client, watch_upstream) = (client_kill.try_clone().map_err(|e| e.to_string())?, upstream_kill.try_clone().map_err(|e| e.to_string())?);
    let watcher = std::thread::spawn(move || loop {
        match done_rx.recv_timeout(every) {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !check() {
                    let _ = watch_client.shutdown(Shutdown::Both);
                    let _ = watch_upstream.shutdown(Shutdown::Both);
                    break;
                }
            }
            _ => break,
        }
    });

    let to_guest = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
        let _ = upstream_writer.shutdown(Shutdown::Both);
    });
    let _ = std::io::copy(&mut upstream, client);
    let _ = client_kill.shutdown(Shutdown::Both);
    let _ = upstream_kill.shutdown(Shutdown::Both);
    let _ = done_tx.send(());
    let _ = to_guest.join();
    let _ = watcher.join();
    Ok(())
}

// ---- a phone asks for a link ---------------------------------------------------------------------

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.method == "POST" && cx.segments == ["potato", "sandbox", "screen-link"] {
        let caller = match mobile_pairing::authorize_caller(cx.headers, cx.peer_loopback, cx.segments, cx.method) {
            Ok(caller) => caller,
            Err(denied) => return Some(Err(denied)),
        };
        return Some(mint_for(screen::hub(), &caller, &cx.request_body));
    }
    None
}

/// The link for a paired phone: `{ "src": "/bridge/potato/sandbox/screen/<sealed>/embed.html#vnc=..." , "mode" }`. The
/// address is relative: the phone page and the bridge share one origin. Only a paired device gets one.
pub(in crate::bridge) fn mint_for(hub: &ScreenHub, caller: &Caller, body: &Value) -> RouteResult {
    let Caller::Device { device_id, caps } = caller else {
        return Err((403, json!({ "code": "SCREEN_PHONE_ONLY", "error": "The bridge gives screen links to paired phones only. The app asks for its own." })));
    };
    let bot = body.get("botId").and_then(Value::as_str).unwrap_or("");
    let mode = match body.get("mode").and_then(Value::as_str) {
        None => Mode::View,
        Some(text) => Mode::parse(text).ok_or((400, json!({ "code": "SCREEN_BAD_MODE", "error": "mode is view or control" })))?,
    };
    if !caps.has(Cap::Watch) || (mode == Mode::Control && !caps.has(Cap::Type)) {
        return Err((403, json!({ "code": "DEVICE_NOT_ALLOWED", "error": "This phone is not allowed to do that." })));
    }
    match hub.link(bot, mode, &Audience::Device(device_id.clone()), "") {
        Ok(src) => Ok((200, json!({ "src": src.into_inner(), "mode": mode }))),
        Err(error) => Err((error.status(), error.json())),
    }
}
