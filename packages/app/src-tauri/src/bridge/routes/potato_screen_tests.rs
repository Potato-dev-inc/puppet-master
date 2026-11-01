//! The screen viewer routes, in process: a real listener on port 0 (never 17321, 17322 or 9333) that reads requests with the
//! bridge's own reader and answers them with this file's `gate` and `serve`; a fake guest websockify on port 0 that records
//! the handshake it was sent and echoes bytes; the real viewer folder; and, for the phone, the real `pwa_server` in front
//! (it marks every request as remote, like the tunnel) with a really paired device. No app handle, no sandbox, no WSL.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::json;

use super::potato_screen::{gate, mint_for, serve};
use crate::bridge::http::{read_request, split_target, write_json};
use crate::mobile_pairing::{self, Cap, Caller, Caps, PairRequestBody};
use crate::potato::sandbox::screen::{Audience, Mode, ScreenHub, ScreenSource, LINK_TTL_MS};
use crate::potato::sandbox::secrets::ScreenSecrets;
use crate::potato::sandbox::{SbxResult, ScreenTarget};

const T0: i64 = 9_000_000;

fn viewer_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox").join("viewer")
}

// ---- the fake guest ------------------------------------------------------------------------------

struct Guest {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

fn spawn_guest() -> Guest {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind guest");
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let seen = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                seen.lock().push(String::from_utf8_lossy(&head).into_owned());
                let _ = stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: test\r\n\r\n");
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    Guest { port, heads }
}

// ---- the hub --------------------------------------------------------------------------------------

struct Source {
    port: u16,
    generation: AtomicU64,
    secrets: Arc<ScreenSecrets>,
}

impl ScreenSource for Source {
    fn name(&self) -> String {
        "pm-sbx-main".to_string()
    }

    fn target(&self) -> SbxResult<ScreenTarget> {
        Ok(ScreenTarget { view_port: self.port, generation: self.generation.load(Ordering::SeqCst), passwords: Some(self.secrets.clone()) })
    }
}

struct Env {
    hub: Arc<ScreenHub>,
    source: Arc<Source>,
    clock: Arc<AtomicI64>,
    guest: Guest,
    bridge: SocketAddr,
}

fn env() -> Env {
    let clock = Arc::new(AtomicI64::new(T0));
    let tick = clock.clone();
    let hub = Arc::new(ScreenHub::with_clock(Box::new(move || tick.load(Ordering::SeqCst))));
    let guest = spawn_guest();
    let source = Arc::new(Source { port: guest.port, generation: AtomicU64::new(1), secrets: Arc::new(ScreenSecrets::generate()) });
    hub.set_source(Some(source.clone()));
    hub.set_enabled(true);
    let bridge = spawn_bridge(hub.clone());
    Env { hub, source, clock, guest, bridge }
}

/// The bridge's own request reader, then this module's `gate` and `serve`.
fn spawn_bridge(hub: Arc<ScreenHub>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind bridge");
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let hub = hub.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let Ok(Some(req)) = read_request(&mut stream) else { return };
                let (path, _) = split_target(&req.target);
                let raw: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
                let segments = mobile_pairing::normalize_bridge_segments(&raw).to_vec();
                let caller = match gate(&hub, &req.headers_raw, req.peer_loopback, &segments) {
                    None => {
                        let _ = write_json(&mut stream, 404, &json!({ "error": "not a screen path" }));
                        return;
                    }
                    Some(Err((status, value))) => {
                        let _ = write_json(&mut stream, status, &value);
                        return;
                    }
                    Some(Ok(caller)) => caller,
                };
                let viewer = viewer_dir();
                let _ = serve(&mut stream, &hub, Some(&viewer), &req.method, &req.headers_raw, &segments, &caller);
            });
        }
    });
    addr
}

impl Env {
    /// The sealed segment of a link for `audience`.
    fn sealed(&self, mode: Mode, audience: &Audience) -> String {
        if mode == Mode::Control {
            self.hub.take_over().expect("take over");
        }
        let src = self.hub.link("fish", mode, audience, "").expect("link").into_inner();
        let start = src.find("/screen/").unwrap() + "/screen/".len();
        src[start..].split('/').next().unwrap().to_string()
    }

    fn password_texts(&self) -> [String; 2] {
        [self.source.secrets.view().expose().to_string(), self.source.secrets.control().expose().to_string()]
    }
}

// ---- a tiny client -------------------------------------------------------------------------------

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn request(addr: SocketAddr, method: &str, path: &str, extra: &[&str]) -> Reply {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\n");
    if !extra.iter().any(|h| h.to_ascii_lowercase().starts_with("host:")) {
        req.push_str(&format!("Host: 127.0.0.1:{}\r\n", addr.port()));
    }
    for header in extra {
        req.push_str(header);
        req.push_str("\r\n");
    }
    req.push_str("Content-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    Reply { status, head, body: raw[split..].to_vec() }
}

fn screen_path(sealed: &str, rest: &str) -> String {
    format!("/bridge/potato/sandbox/screen/{sealed}/{rest}")
}

/// One GET of `rest` under the sealed link, through this file's in-process bridge (the opt-in live test of the sandbox
/// uses it with the real sandbox behind the hub).
pub(in crate::bridge) fn get_in_process(hub: &Arc<ScreenHub>, sealed: &str, rest: &str) -> (u16, String, String) {
    let addr = spawn_bridge(hub.clone());
    let reply = request(addr, "GET", &screen_path(sealed, rest), &[]);
    (reply.status, reply.head.clone(), reply.text())
}

const FORWARDED: &str = "X-Forwarded-For: 203.0.113.9";

/// A websocket client: sends the upgrade, returns the stream and the handshake answer.
fn open_socket(addr: SocketAddr, sealed: &str, extra: &[&str]) -> (TcpStream, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = format!("GET {} HTTP/1.1\r\n", screen_path(sealed, "websockify"));
    if !extra.iter().any(|h| h.to_ascii_lowercase().starts_with("host:")) {
        req.push_str(&format!("Host: 127.0.0.1:{}\r\n", addr.port()));
    }
    req.push_str("Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: binary\r\n");
    for header in extra {
        req.push_str(header);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        head.push(byte[0]);
    }
    (stream, String::from_utf8_lossy(&head).into_owned())
}

fn echo(stream: &mut TcpStream, text: &str) -> bool {
    if stream.write_all(text.as_bytes()).is_err() {
        return false;
    }
    let mut got = vec![0u8; text.len()];
    stream.read_exact(&mut got).is_ok() && got == text.as_bytes()
}

/// Waits for the relay to close: true when the next read ends (EOF or reset) within `limit`.
fn closes_within(stream: &mut TcpStream, limit: Duration) -> bool {
    let start = Instant::now();
    stream.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut buf = [0u8; 64];
    while start.elapsed() < limit {
        match stream.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return true,
        }
    }
    false
}

// ---- the viewer files ----------------------------------------------------------------------------

#[test]
fn the_page_is_served_with_its_origins_and_without_caching() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[]);
    assert_eq!(r.status, 200, "{}", r.head);
    assert_eq!(r.header("Content-Type").as_deref(), Some("text/html; charset=utf-8"));
    assert_eq!(r.header("Cache-Control").as_deref(), Some("no-store, no-transform"));
    assert_eq!(r.header("X-Content-Type-Options").as_deref(), Some("nosniff"));
    assert_eq!(r.header("Referrer-Policy").as_deref(), Some("no-referrer"));
    assert_eq!(
        r.header("Content-Security-Policy").as_deref(),
        Some("frame-ancestors 'self' http://tauri.localhost https://tauri.localhost")
    );
    let body = r.text();
    assert!(body.contains(r#"name="pm-sbx-parent-origins" content="http://tauri.localhost,https://tauri.localhost""#), "origins filled in");
    // the link's own path also serves the page (no file name)
    assert_eq!(request(e.bridge, "GET", &format!("/bridge/potato/sandbox/screen/{sealed}"), &[]).status, 200);
    // a dev server on this computer that framed it is allowed too; any other site is not
    let dev = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &["Referer: http://localhost:1420/page"]);
    assert!(dev.header("Content-Security-Policy").unwrap().contains("http://localhost:1420"));
    let other = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &["Referer: https://evil.example/page"]);
    assert!(!other.header("Content-Security-Policy").unwrap().contains("evil"));
}

#[test]
fn no_answer_carries_a_password() {
    let e = env();
    let [view, control] = e.password_texts();
    for mode in [Mode::View, Mode::Control] {
        let sealed = e.sealed(mode, &Audience::App);
        for rest in ["embed.html", "novnc/core/rfb.js", "clipboard-bridge.js", "nothing.js", "websockify"] {
            let r = request(e.bridge, "GET", &screen_path(&sealed, rest), &[]);
            let all = format!("{}{}", r.head, r.text());
            assert!(!all.contains(&view) && !all.contains(&control), "{rest}");
        }
    }
}

#[test]
fn the_viewers_scripts_load_as_modules() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    let r = request(e.bridge, "GET", &screen_path(&sealed, "novnc/core/rfb.js"), &[]);
    assert_eq!(r.status, 200);
    assert_eq!(r.header("Content-Type").as_deref(), Some("text/javascript; charset=utf-8"));
    assert_eq!(r.body, std::fs::read(viewer_dir().join("novnc").join("core").join("rfb.js")).unwrap());
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "clipboard-bridge.js"), &[]).status, 200);
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "mobile-keyboard.js"), &[]).status, 200);
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "novnc/vendor/pako/lib/zlib/inflate.js"), &[]).status, 200);
    let head = request(e.bridge, "HEAD", &screen_path(&sealed, "novnc/core/rfb.js"), &[]);
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
}

#[test]
fn nothing_outside_the_viewer_is_served() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    for rest in [
        "README.md",
        "PORTED.md",
        "novnc/LICENSE.txt/../../PORTED.md",
        "novnc/%2e%2e/PORTED.md",
        "novnc/..%2fPORTED.md",
        "novnc/..\\PORTED.md",
        "novnc/../embed.html",
        "novnc/core",
        "novnc/nothing.js",
        "../../../../Cargo.toml",
        "%2e%2e/%2e%2e/Cargo.toml",
    ] {
        let r = request(e.bridge, "GET", &screen_path(&sealed, rest), &[]);
        assert_eq!(r.status, 404, "{rest}: {}", r.head);
    }
    assert_eq!(request(e.bridge, "POST", &screen_path(&sealed, "embed.html"), &[]).status, 405);
    assert_eq!(request(e.bridge, "DELETE", &screen_path(&sealed, "novnc/core/rfb.js"), &[]).status, 405);
    // the websocket path wants an upgrade
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "websockify"), &[]).status, 400);
}

#[test]
fn a_bad_link_gets_no_page() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    let mut chars: Vec<char> = sealed.chars().collect();
    chars[3] = if chars[3] == 'A' { 'B' } else { 'A' };
    let tampered: String = chars.into_iter().collect();
    assert_eq!(request(e.bridge, "GET", &screen_path(&tampered, "embed.html"), &[]).status, 404);
    assert_eq!(request(e.bridge, "GET", &screen_path("garbage", "embed.html"), &[]).status, 404);
    // it runs out after an hour: 410 and a plain code
    e.clock.fetch_add(LINK_TTL_MS, Ordering::SeqCst);
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[]);
    assert_eq!(r.status, 410);
    assert!(r.text().contains("SCREEN_LINK_EXPIRED"));
}

#[test]
fn a_restart_of_the_sandbox_kills_the_page_link() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[]).status, 200);
    e.source.generation.fetch_add(1, Ordering::SeqCst);
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[]);
    assert_eq!(r.status, 410);
    assert!(r.text().contains("SCREEN_LINK_STALE"));
}

#[test]
fn with_the_switch_off_the_route_does_not_exist() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    e.hub.set_enabled(false);
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[]).status, 404);
    let (_, head) = open_socket(e.bridge, &sealed, &[]);
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
}

#[test]
fn other_paths_are_not_this_routes_business() {
    let hub = ScreenHub::new();
    hub.set_enabled(true);
    for path in [vec!["panes"], vec!["potato", "brain", "inbox"], vec!["potato", "sandbox", "screen-link"], vec!["potato", "sandbox", "screen"]] {
        assert!(gate(&hub, "GET / HTTP/1.1\r\n", true, &path).is_none(), "{path:?}");
    }
}

// ---- who may ask ---------------------------------------------------------------------------------

#[test]
fn a_request_that_looks_forwarded_is_not_the_app() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    // the link is for the app: a forwarded request (a phone through the tunnel) cannot use it, and learns nothing
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[FORWARDED]);
    assert_eq!(r.status, 404, "{}", r.head);
    // a foreign Host (DNS rebinding) is the same
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &["Host: rebind.example"]);
    assert_eq!(r.status, 404);
    // and so is a page of another site (a foreign Origin)
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &["Origin: https://evil.example"]);
    assert_eq!(r.status, 404);
    let (_, head) = open_socket(e.bridge, &sealed, &[FORWARDED]);
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    // while the same link is good for the app
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &["Origin: http://tauri.localhost"]).status, 200);
}

// ---- the websocket relay -------------------------------------------------------------------------

#[test]
fn the_websocket_is_relayed_and_only_the_handshake_goes_to_the_guest() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    let secret_header = format!("{}: not-for-the-guest", crate::mobile_pairing::local_secret::HEADER);
    let (mut stream, head) = open_socket(
        e.bridge,
        &sealed,
        &["Cookie: session=abc", "Authorization: Bearer device-token", &secret_header, "Origin: http://127.0.0.1:1", "X-Puppet-Master-Session: s1"],
    );
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(head.contains("Sec-WebSocket-Accept: test"));
    assert!(echo(&mut stream, "hello guest"), "bytes go there and back");
    assert!(echo(&mut stream, &"x".repeat(5000)), "a bigger piece too");

    let heads = e.guest.heads.lock().clone();
    assert_eq!(heads.len(), 1);
    let sent = &heads[0];
    assert!(sent.starts_with("GET /websockify HTTP/1.1\r\n"), "{sent}");
    assert!(sent.contains(&format!("Host: 127.0.0.1:{}", e.guest.port)));
    for kept in ["Upgrade: websocket", "Connection: Upgrade", "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==", "Sec-WebSocket-Version: 13", "Sec-WebSocket-Protocol: binary"] {
        assert!(sent.contains(kept), "{kept} in {sent}");
    }
    let lower = sent.to_ascii_lowercase();
    for gone in ["cookie", "authorization", "origin", "x-puppet-master", &crate::mobile_pairing::local_secret::HEADER.to_ascii_lowercase(), "not-for-the-guest", "sealed", &sealed.to_ascii_lowercase()] {
        assert!(!lower.contains(gone), "{gone} reached the guest: {sent}");
    }
}

#[test]
fn an_unreachable_guest_is_a_plain_502() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    // a guest that is not there: the port of a closed listener
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let source = Arc::new(Source { port: dead, generation: AtomicU64::new(1), secrets: e.source.secrets.clone() });
    e.hub.set_source(Some(source));
    let (_, head) = open_socket(e.bridge, &sealed, &[]);
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
}

#[test]
fn an_open_control_connection_closes_when_the_lease_is_handed_back() {
    let e = env();
    let sealed = e.sealed(Mode::Control, &Audience::App);
    let (mut stream, head) = open_socket(e.bridge, &sealed, &[]);
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(echo(&mut stream, "while you have control"));
    let handed_back = Instant::now();
    e.hub.hand_back().unwrap();
    assert!(closes_within(&mut stream, Duration::from_secs(5)), "the relay must close after the lease is gone");
    assert!(handed_back.elapsed() < Duration::from_secs(4), "within the 1 s recheck, not later");
    // and the link is dead for a new connection too
    let (_, again) = open_socket(e.bridge, &sealed, &[]);
    assert!(again.starts_with("HTTP/1.1 403"), "{again}");
}

#[test]
fn an_open_control_connection_closes_when_the_person_takes_over_again() {
    let e = env();
    let sealed = e.sealed(Mode::Control, &Audience::App);
    let (mut stream, _) = open_socket(e.bridge, &sealed, &[]);
    assert!(echo(&mut stream, "first"));
    e.hub.take_over().unwrap(); // a new fence: the old control link is not the live holder's
    assert!(closes_within(&mut stream, Duration::from_secs(5)));
}

#[test]
fn an_open_view_connection_survives_the_lease_but_not_a_restart() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    let (mut stream, _) = open_socket(e.bridge, &sealed, &[]);
    assert!(echo(&mut stream, "watching"));
    e.hub.take_over().unwrap();
    e.hub.hand_back().unwrap();
    std::thread::sleep(Duration::from_millis(1300));
    assert!(echo(&mut stream, "still watching"), "a view link does not depend on the lease");
    e.source.generation.fetch_add(1, Ordering::SeqCst);
    assert!(closes_within(&mut stream, Duration::from_secs(5)), "a restart closes every viewer");
}

#[test]
fn an_open_connection_closes_when_the_switch_goes_off() {
    let e = env();
    let sealed = e.sealed(Mode::View, &Audience::App);
    let (mut stream, _) = open_socket(e.bridge, &sealed, &[]);
    assert!(echo(&mut stream, "on"));
    e.hub.set_enabled(false);
    assert!(closes_within(&mut stream, Duration::from_secs(5)));
}

// ---- the phone -----------------------------------------------------------------------------------

fn pair(caps: Caps, name: &str) -> String {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("pm-screen-routes-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // The store is process-wide; another test may have made it first. Either is fine: devices are paired here.
        let _ = mobile_pairing::init_pairing_store(dir.join("pairing.json"));
        dir.clone()
    });
    let _ = dir;
    let store = mobile_pairing::pairing_store().expect("pairing store");
    let mut guard = store.lock();
    let session = guard.create_pairing_session_with_caps("https://example.invalid".into(), caps);
    guard
        .pair_device(PairRequestBody { pairing_code: session.pairing_code, device_name: name.into(), device_public_key: session.server_public_key })
        .expect("pair test device")
        .device_id
}

fn watch_only() -> Caps {
    Caps::none().with(Cap::Watch, true)
}

#[test]
fn a_phone_with_a_link_gets_the_page_and_the_socket_through_the_tunnel() {
    let e = env();
    let device = pair(watch_only(), "watch phone");
    let sealed = e.sealed(Mode::View, &Audience::Device(device.clone()));

    // the real pwa_server in front: it marks the request remote, as the tunnel does
    let dir = std::env::temp_dir().join(format!("pm-screen-pwa-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let port_file = dir.join("bridge.port");
    std::fs::write(&port_file, format!("{}\n", e.bridge)).unwrap();
    let pwa = crate::pwa_server::spawn_test_server(dir.clone(), port_file);

    let page = request(pwa, "GET", &screen_path(&sealed, "embed.html"), &["Host: pm.example.test", "X-Forwarded-Proto: https", "Referer: https://pm.example.test/phone"]);
    assert_eq!(page.status, 200, "{}", page.head);
    assert!(page.text().contains("https://pm.example.test"), "the phone page's own origin is allowed");
    assert!(page.header("Content-Security-Policy").unwrap().contains("https://pm.example.test"));
    assert_eq!(request(pwa, "GET", &screen_path(&sealed, "novnc/core/rfb.js"), &["Host: pm.example.test"]).status, 200);

    let (mut stream, head) = open_socket(pwa, &sealed, &["Host: pm.example.test"]);
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(echo(&mut stream, "from the phone"));
    // the link was made for this phone only: the app's webview cannot use it, nor another phone
    assert_eq!(request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[]).status, 403);
    // a phone's link through the tunnel with a tampered tag gets nothing
    let mut bad = sealed.clone();
    bad.push('x');
    assert_eq!(request(pwa, "GET", &screen_path(&bad, "embed.html"), &["Host: pm.example.test"]).status, 404);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_phone_link_for_a_device_that_may_not_watch_is_refused() {
    let e = env();
    let device = pair(Caps::none().with(Cap::Answer, true), "answer phone");
    let sealed = e.sealed(Mode::View, &Audience::Device(device));
    let r = request(e.bridge, "GET", &screen_path(&sealed, "embed.html"), &[FORWARDED]);
    assert_eq!(r.status, 403, "{}", r.head);
    assert!(r.text().contains("DEVICE_NOT_ALLOWED"));
}

#[test]
fn the_bridge_gives_links_to_paired_phones_only() {
    let e = env();
    // the app and local tools do not get a password from the bridge
    for caller in [Caller::Local, Caller::Public, Caller::Brain { bot_id: "b".into() }] {
        let denied = mint_for(&e.hub, &caller, &json!({ "botId": "fish" })).err().unwrap();
        assert_eq!(denied.0, 403);
        assert_eq!(denied.1["code"], "SCREEN_PHONE_ONLY");
    }
    let [view, control] = e.password_texts();

    let watch = Caller::Device { device_id: "d-watch".into(), caps: watch_only() };
    let (status, body) = mint_for(&e.hub, &watch, &json!({ "botId": "fish" })).expect("a view link");
    assert_eq!(status, 200);
    assert_eq!(body["mode"], "view");
    let src = body["src"].as_str().unwrap();
    assert!(src.starts_with("/bridge/potato/sandbox/screen/"), "a relative address: {src}");
    assert!(src.ends_with(&format!("#vnc={view}")));
    assert!(!src.contains('?'));
    assert!(!src.contains(&control));

    // control from a phone: the cap first, then the person's yes at the PC
    let denied = mint_for(&e.hub, &watch, &json!({ "botId": "fish", "mode": "control" })).err().unwrap();
    assert_eq!((denied.0, denied.1["code"].as_str()), (403, Some("DEVICE_NOT_ALLOWED")));
    let typist = Caller::Device { device_id: "d-type".into(), caps: Caps::none().with(Cap::Watch, true).with(Cap::Type, true) };
    e.hub.take_over().unwrap();
    let denied = mint_for(&e.hub, &typist, &json!({ "botId": "fish", "mode": "control" })).err().unwrap();
    assert_eq!((denied.0, denied.1["code"].as_str()), (403, Some("PHONE_NEEDS_DESKTOP_YES")));
    e.hub.allow_phone_control("d-type");
    let (status, body) = mint_for(&e.hub, &typist, &json!({ "botId": "fish", "mode": "control" })).expect("control after the yes");
    assert_eq!(status, 200);
    assert!(body["src"].as_str().unwrap().contains("embed.html?control=1#vnc="));

    // bad input
    let bad_mode = mint_for(&e.hub, &watch, &json!({ "botId": "fish", "mode": "admin" })).err().unwrap();
    assert_eq!(bad_mode.0, 400);
    let bad_bot = mint_for(&e.hub, &watch, &json!({ "botId": "../x" })).err().unwrap();
    assert_eq!((bad_bot.0, bad_bot.1["code"].as_str()), (400, Some("SCREEN_BAD_BOT")));
    // switches off
    e.hub.set_enabled(false);
    assert_eq!(mint_for(&e.hub, &watch, &json!({ "botId": "fish" })).err().unwrap().0, 404);
}
