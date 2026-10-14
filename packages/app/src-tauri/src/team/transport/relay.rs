//! `RelayTransport`: the team's mailbox on a relay server (`relay/server`, the protocol in
//! `relay/protocol`), self-hosted or PM-hosted.
//!
//! - **Outbound only.** This app opens connections to the relay; the relay never calls this app,
//!   and nothing here touches the bridge or device pairing.
//! - **Opaque to the relay.** Only sealed envelopes and signed slots travel; the relay sees the
//!   team id, the sending device, sizes and times (`pm_relay_protocol` docs).
//! - **Every request is signed** by this device's key (`x-pm-auth`); the relay admits only devices
//!   on the team's access list, which only the root key can change.
//! - **Receiving:** a WebSocket (`/ws?after=`); when it can't connect, long-polling
//!   (`/envelopes?after=&wait=`) for a while, then the WebSocket is tried again.

use super::{now_ms, Inbound, TeamTransport, TransportError};
use crate::team::crypto::SigningKey;
use parking_lot::Mutex;
use pm_relay_protocol::{auth_header, Batch, Delivered, Envelope, JoinBox, Resync, SignedAcl, SignedBlob, TeamRegistration, AUTH_HEADER};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Long-poll wait asked of the relay (seconds).
const POLL_WAIT_S: u64 = 20;
/// After the WebSocket fails, poll for this long before trying it again.
const POLL_BEFORE_WS_RETRY: Duration = Duration::from_secs(60);
/// Pause after a failed request.
const BACKOFF: Duration = Duration::from_secs(3);

const MODE_OFFLINE: u8 = 0;
const MODE_LIVE: u8 = 1;
const MODE_POLLING: u8 = 2;

pub struct RelayTransport {
    /// `https://relay.example.com` (or `http://127.0.0.1:8787`), no trailing slash.
    base: String,
    /// The path part of `base` (`""` or `/pm`), signed into every request.
    prefix: String,
    /// Scheme and host of `base`.
    origin: String,
    team: String,
    device: SigningKey,
    agent: ureq::Agent,
    stops: Mutex<Vec<Arc<AtomicBool>>>,
    mode: Arc<AtomicU8>,
    /// Test switch: never try the WebSocket (exercise the poll fallback).
    poll_only: bool,
}

/// `base` with a scheme the relay speaks, or why not.
pub fn normalize_relay_url(raw: &str) -> Result<String, String> {
    let url = raw.trim().trim_end_matches('/').to_string();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("The relay address starts with https:// (or http:// on this computer).")?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err("That relay address has no host.".into());
    }
    // Plain http only on this machine: a relay elsewhere must use TLS.
    let host_only = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if url.starts_with("http://") && !matches!(host_only, "127.0.0.1" | "localhost" | "[::1]") {
        return Err("Use https:// for a relay on another computer.".into());
    }
    Ok(url)
}

impl RelayTransport {
    pub fn new(base: &str, team: &str, device: SigningKey) -> Result<Self, String> {
        let base = normalize_relay_url(base)?;
        let after_scheme = base.split_once("://").map(|(_, r)| r).unwrap_or("");
        let prefix = after_scheme.find('/').map(|at| after_scheme[at..].to_string()).unwrap_or_default();
        let origin = base[..base.len() - prefix.len()].to_string();
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout(Duration::from_secs(POLL_WAIT_S + 15))
            .build();
        Ok(Self {
            base,
            prefix,
            origin,
            team: team.to_string(),
            device,
            agent,
            stops: Mutex::new(Vec::new()),
            mode: Arc::new(AtomicU8::new(MODE_OFFLINE)),
            poll_only: false,
        })
    }

    /// Tests: skip the WebSocket and long-poll only.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn poll_only(mut self) -> Self {
        self.poll_only = true;
        self
    }

    fn team_path(&self, rest: &str) -> String {
        format!("{}/v1/teams/{}{}", self.prefix, self.team, rest)
    }

    fn call(&self, method: &str, path: &str, body: &[u8], signed: bool) -> Result<(u16, Vec<u8>), TransportError> {
        let url = format!("{}{}", self.origin, path);
        let mut req = self.agent.request(method, &url).set("x-pm-relay", pm_relay_protocol::PROTOCOL);
        if signed {
            req = req.set(AUTH_HEADER, &auth_header(&self.device, method, path, now_ms(), body));
        }
        if !body.is_empty() {
            req = req.set("content-type", "application/json");
        }
        let result = if body.is_empty() && method == "GET" { req.call() } else { req.send_bytes(body) };
        let response = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                return Err(TransportError(relay_error(code, &text)));
            }
            Err(e) => return Err(TransportError(format!("Can't reach the relay: {e}"))),
        };
        let status = response.status();
        let mut bytes = Vec::new();
        use std::io::Read;
        response
            .into_reader()
            .take(pm_relay_protocol::MAX_STATE_BYTES as u64 * 4)
            .read_to_end(&mut bytes)
            .map_err(|e| TransportError(e.to_string()))?;
        Ok((status, bytes))
    }

    fn fetch_batch(&self, after: u64, wait_s: u64) -> Result<Batch, TransportError> {
        let path = self.team_path(&format!("/envelopes?after={after}&wait={wait_s}"));
        let (_, bytes) = self.call("GET", &path, b"", true)?;
        serde_json::from_slice(&bytes).map_err(|_| TransportError::new("The relay sent something unreadable."))
    }

    fn spawn_reader(&self, after: u64, tx: Sender<Inbound>, stop: Arc<AtomicBool>) -> Result<(), TransportError> {
        let me = ReaderCtx {
            base: self.base.clone(),
            ws_path: self.team_path("/ws"),
            device: self.device.clone(),
            mode: self.mode.clone(),
            poll_only: self.poll_only,
            cursor: Arc::new(AtomicU64::new(after)),
            pending_reset: AtomicBool::new(false),
        };
        let poller = RelayTransport {
            base: self.base.clone(),
            prefix: self.prefix.clone(),
            origin: self.origin.clone(),
            team: self.team.clone(),
            device: self.device.clone(),
            agent: self.agent.clone(),
            stops: Mutex::new(Vec::new()),
            mode: self.mode.clone(),
            poll_only: self.poll_only,
        };
        std::thread::Builder::new()
            .name("team-relay".into())
            .spawn(move || me.run(&poller, &tx, &stop))
            .map(|_| ())
            .map_err(|e| TransportError(format!("could not start the relay reader: {e}")))
    }
}

fn relay_error(code: u16, body: &str) -> String {
    let said = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
    match code {
        403 => format!("{} yet{}.", super::DENIED, said.map(|s| format!(" ({s})")).unwrap_or_default()),
        401 => format!("The relay refused this computer's signature{}.", said.map(|s| format!(" ({s})")).unwrap_or_default()),
        404 => "The relay doesn't know this team.".into(),
        409 => "The relay already has a newer copy.".into(),
        413 => "That's too large for the relay.".into(),
        507 => "The team's mailbox on the relay is full. Try again later.".into(),
        429 => "The relay asks to slow down. Try again in a minute.".into(),
        _ => format!("The relay said {code}{}.", said.map(|s| format!(": {s}")).unwrap_or_default()),
    }
}

struct ReaderCtx {
    base: String,
    ws_path: String,
    device: SigningKey,
    mode: Arc<AtomicU8>,
    poll_only: bool,
    cursor: Arc<AtomicU64>,
    /// The relay said our cursor was past its counter; the next delivered envelope tells the
    /// service to take its (lower) position as the saved cursor.
    pending_reset: AtomicBool,
}

impl ReaderCtx {
    fn deliver(&self, item: Delivered, tx: &Sender<Inbound>) -> bool {
        let reset = self.pending_reset.swap(false, Ordering::SeqCst);
        if reset || item.seq > self.cursor.load(Ordering::SeqCst) {
            self.cursor.store(item.seq, Ordering::SeqCst);
        }
        tx.send(Inbound { envelope: item.envelope, received_at_ms: now_ms(), cursor: item.seq, reset }).is_ok()
    }

    /// The relay's counter is behind our cursor (a restored or replaced relay database): start
    /// again from what it has. Envelopes already seen are dropped by the session's replay guard.
    fn resync(&self, head: u64) {
        tracing::info!(head, cursor = self.cursor.load(Ordering::SeqCst), "team: the relay's mailbox counter went back; reading it again");
        self.cursor.store(0, Ordering::SeqCst);
        self.pending_reset.store(true, Ordering::SeqCst);
    }

    fn run(&self, poller: &RelayTransport, tx: &Sender<Inbound>, stop: &AtomicBool) {
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if !self.poll_only {
                match self.websocket(tx, stop) {
                    WsEnd::Stopped => break,
                    WsEnd::ReceiverGone => break,
                    WsEnd::Failed(why) => tracing::debug!(%why, "team: relay WebSocket unavailable, polling"),
                }
            }
            // Long-poll until it is time to try the WebSocket again.
            let until = std::time::Instant::now() + POLL_BEFORE_WS_RETRY;
            while self.poll_only || std::time::Instant::now() < until {
                if stop.load(Ordering::SeqCst) {
                    self.mode.store(MODE_OFFLINE, Ordering::SeqCst);
                    return;
                }
                match poller.fetch_batch(self.cursor.load(Ordering::SeqCst), POLL_WAIT_S) {
                    Ok(batch) => {
                        self.mode.store(MODE_POLLING, Ordering::SeqCst);
                        if batch.reset {
                            self.resync(batch.head.unwrap_or(0));
                        }
                        for item in batch.items {
                            if stop.load(Ordering::SeqCst) || !self.deliver(item, tx) {
                                return;
                            }
                        }
                        if batch.reset || batch.next > self.cursor.load(Ordering::SeqCst) {
                            self.cursor.store(batch.next, Ordering::SeqCst);
                        }
                    }
                    Err(error) => {
                        self.mode.store(MODE_OFFLINE, Ordering::SeqCst);
                        tracing::debug!(%error, "team: relay poll failed");
                        std::thread::sleep(BACKOFF);
                    }
                }
            }
        }
        self.mode.store(MODE_OFFLINE, Ordering::SeqCst);
    }

    fn websocket(&self, tx: &Sender<Inbound>, stop: &AtomicBool) -> WsEnd {
        use tungstenite::client::IntoClientRequest;
        use tungstenite::http::HeaderValue;
        let path = format!("{}?after={}", self.ws_path, self.cursor.load(Ordering::SeqCst));
        let host_part = self.base.split_once("://").map(|(_, r)| r.split('/').next().unwrap_or("")).unwrap_or("");
        let scheme = if self.base.starts_with("https://") { "wss" } else { "ws" };
        let url = format!("{scheme}://{host_part}{path}");
        let mut request = match url.into_client_request() {
            Ok(r) => r,
            Err(e) => return WsEnd::Failed(e.to_string()),
        };
        let header = auth_header(&self.device, "GET", &path, now_ms(), b"");
        match HeaderValue::from_str(&header) {
            Ok(v) => {
                request.headers_mut().insert(AUTH_HEADER, v);
            }
            Err(e) => return WsEnd::Failed(e.to_string()),
        }
        let (mut socket, _) = match tungstenite::connect(request) {
            Ok(pair) => pair,
            Err(e) => return WsEnd::Failed(e.to_string()),
        };
        set_read_timeout(socket.get_ref(), Duration::from_millis(800));
        self.mode.store(MODE_LIVE, Ordering::SeqCst);
        loop {
            if stop.load(Ordering::SeqCst) {
                let _ = socket.close(None);
                return WsEnd::Stopped;
            }
            match socket.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    if let Ok(item) = serde_json::from_str::<Delivered>(&text) {
                        if !self.deliver(item, tx) {
                            return WsEnd::ReceiverGone;
                        }
                    } else if let Ok(Resync { reset: true, head }) = serde_json::from_str::<Resync>(&text) {
                        self.resync(head);
                    }
                }
                Ok(tungstenite::Message::Close(_)) => return WsEnd::Failed("closed by the relay".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return WsEnd::Failed(e.to_string()),
            }
        }
    }
}

enum WsEnd {
    Stopped,
    ReceiverGone,
    Failed(String),
}

fn set_read_timeout(stream: &tungstenite::stream::MaybeTlsStream<std::net::TcpStream>, timeout: Duration) {
    use tungstenite::stream::MaybeTlsStream;
    match stream {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(timeout));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(timeout));
        }
        _ => {}
    }
}

impl TeamTransport for RelayTransport {
    fn kind(&self) -> &'static str {
        "relay"
    }

    fn send(&self, envelope: &Envelope) -> Result<(), TransportError> {
        let body = serde_json::to_vec(envelope).map_err(|e| TransportError(e.to_string()))?;
        self.call("POST", &self.team_path("/envelopes"), &body, true).map(|_| ())
    }

    fn subscribe(&self, after: u64) -> Result<Receiver<Inbound>, TransportError> {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        self.stops.lock().push(stop.clone());
        self.spawn_reader(after, tx, stop)?;
        Ok(rx)
    }

    fn put_state(&self, blob: &SignedBlob) -> Result<(), TransportError> {
        let body = serde_json::to_vec(blob).map_err(|e| TransportError(e.to_string()))?;
        self.call("PUT", &self.team_path(&format!("/state/{}", blob.slot)), &body, true).map(|_| ())
    }

    fn get_state(&self, slot: &str) -> Result<Option<SignedBlob>, TransportError> {
        match self.call("GET", &self.team_path(&format!("/state/{slot}")), b"", true) {
            Ok((204, _)) => Ok(None),
            Ok((_, bytes)) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| TransportError::new("The relay sent an unreadable team list.")),
            Err(e) => Err(e),
        }
    }

    fn post_join(&self, join: &JoinBox) -> Result<(), TransportError> {
        let body = serde_json::to_vec(join).map_err(|e| TransportError(e.to_string()))?;
        self.call("POST", &self.team_path("/joins"), &body, false).map(|_| ())
    }

    fn joins(&self) -> Result<Vec<JoinBox>, TransportError> {
        let (_, bytes) = self.call("GET", &self.team_path("/joins"), b"", true)?;
        serde_json::from_slice(&bytes).map_err(|_| TransportError::new("The relay sent unreadable join requests."))
    }

    fn register(&self, registration: &TeamRegistration) -> Result<(), TransportError> {
        let body = serde_json::to_vec(registration).map_err(|e| TransportError(e.to_string()))?;
        match self.call("POST", &format!("{}/v1/teams", self.prefix), &body, false) {
            Ok(_) => Ok(()),
            // Already registered by me earlier: fine (the ACL update below is what counts).
            Err(e) if e.0.contains("newer copy") => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn set_acl(&self, acl: &SignedAcl) -> Result<(), TransportError> {
        let body = serde_json::to_vec(acl).map_err(|e| TransportError(e.to_string()))?;
        self.call("PUT", &self.team_path("/acl"), &body, false).map(|_| ())
    }

    fn status(&self) -> &'static str {
        match self.mode.load(Ordering::SeqCst) {
            MODE_LIVE => "live",
            MODE_POLLING => "polling",
            _ => "offline",
        }
    }

    fn stop(&self) {
        for stop in self.stops.lock().drain(..) {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for RelayTransport {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_addresses_need_tls_off_this_computer() {
        assert_eq!(normalize_relay_url("https://relay.example.com/").unwrap(), "https://relay.example.com");
        assert!(normalize_relay_url("http://127.0.0.1:8787").is_ok());
        assert!(normalize_relay_url("http://localhost:8787").is_ok());
        assert!(normalize_relay_url("http://relay.example.com").is_err());
        assert!(normalize_relay_url("ftp://x").is_err());
        assert!(normalize_relay_url("https://").is_err());
    }
}
