//! End-to-end authorization tests for the remote path: client -> `pwa_server` (the loopback server
//! cloudflared points at) -> bridge listener. The bridge side is the real request reader and the
//! real `gate` (see `bridge::spawn_gate_listener`); only the `AppHandle`-bound route dispatch is
//! replaced, by a reply that says whether the caller was authorized and whether it would have been
//! treated as the desktop app. The pairing store is the real one.
//!
//! Regression for the 2026-10-14 tunnel auth bypass (docs/security/2026-10-14-tunnel-auth-bypass.md):
//! `pwa_server` forwarded requests without `X-PM-Proxied`, so the bridge saw a loopback peer and
//! trusted every tunnel caller as the local app.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use crate::mobile_pairing::{init_pairing_store, pairing_store, PairRequestBody};

struct Rig {
    /// The bridge listener, reached directly (the desktop app's own tooling).
    bridge: SocketAddr,
    /// `pwa_server`, reached the way a tunnel caller reaches it.
    pwa: SocketAddr,
    /// A valid paired-device bearer token.
    token: String,
}

fn rig() -> &'static Rig {
    static RIG: OnceLock<Rig> = OnceLock::new();
    RIG.get_or_init(|| {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("pm-tunnel-auth-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = init_pairing_store(dir.join("pairing.json"));
        let store = pairing_store().expect("pairing store");
        let token = {
            let mut guard = store.lock();
            let session = guard.create_pairing_session("https://example.invalid".into());
            guard
                .pair_device(PairRequestBody {
                    pairing_code: session.pairing_code,
                    device_name: "test phone".into(),
                    device_public_key: session.server_public_key,
                })
                .expect("pair test device")
                .device_token
        };
        let bridge = crate::bridge::spawn_gate_listener();
        let port_file = dir.join("bridge.port");
        std::fs::write(&port_file, format!("{bridge}\n")).unwrap();
        let pwa = crate::pwa_server::spawn_test_server(dir.clone(), port_file);
        Rig { bridge, pwa, token }
    })
}

/// Send raw request text, read until the server closes. Returns (status, body).
fn send(addr: SocketAddr, request: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("").to_string();
    (status, body)
}

fn get(path: &str, extra_headers: &[&str]) -> String {
    let mut request = format!("GET {path} HTTP/1.1\r\nHost: tunnel.example\r\n");
    for header in extra_headers {
        request.push_str(header);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    request
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|err| panic!("not json ({err}): {body:?}"))
}

#[test]
fn tunnel_request_without_bearer_is_rejected() {
    let r = rig();
    let (status, body) = send(r.pwa, &get("/bridge/panes", &[]));
    assert_eq!(status, 401, "an anonymous tunnel caller must not reach a pane route: {body}");
    assert_eq!(json(&body)["error"], "pairing_required");
}

#[test]
fn tunnel_caller_cannot_forge_or_strip_the_proxied_marker() {
    let r = rig();
    for header in ["X-PM-Proxied: 0", "x-pm-proxied: 0", "X-PM-Proxied:", " X-PM-Proxied: 0"] {
        let (status, body) = send(r.pwa, &get("/bridge/panes", &[header]));
        assert_eq!(status, 401, "forged header {header:?} must not open the bridge: {body}");
    }
    // a client that sends no marker at all is the same case as "header stripped"
    let (status, _) = send(
        r.pwa,
        "POST /bridge/panes/pane-1/input HTTP/1.1\r\nHost: t\r\nContent-Length: 2\r\n\r\n{}",
    );
    assert_eq!(status, 401, "pane input without a bearer");
}

#[test]
fn tunnel_request_with_a_wrong_bearer_is_rejected() {
    let r = rig();
    let (status, body) = send(
        r.pwa,
        &get("/bridge/panes", &["Authorization: Bearer AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]),
    );
    assert_eq!(status, 401);
    assert_eq!(json(&body)["error"], "invalid_token");
}

#[test]
fn tunnel_request_with_a_paired_bearer_is_authorized_but_is_not_the_local_app() {
    let r = rig();
    let auth = format!("Authorization: Bearer {}", r.token);
    let (status, body) = send(r.pwa, &get("/bridge/panes", &[&auth]));
    assert_eq!(status, 200, "{body}");
    let reply = json(&body);
    assert_eq!(reply["authorized"], true);
    assert_eq!(
        reply["local_app"], false,
        "a paired phone is never the desktop app"
    );
}

#[test]
fn public_paths_stay_reachable_through_the_tunnel() {
    let r = rig();
    let (status, _) = send(r.pwa, &get("/bridge/health", &[]));
    assert_eq!(status, 200);
}

#[test]
fn direct_loopback_request_is_still_the_local_app() {
    let r = rig();
    let request = "GET /panes HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    let (status, body) = send(r.bridge, request);
    assert_eq!(status, 200, "the desktop app and the MCP shim need no token: {body}");
    assert_eq!(json(&body)["local_app"], true);
}

#[test]
fn direct_loopback_request_with_forwarding_headers_needs_a_bearer() {
    let r = rig();
    for header in [
        "Cf-Connecting-Ip: 203.0.113.9",
        "cf-ray: 8a1b2c3d-AMS",
        "X-Forwarded-For: 203.0.113.9",
        "X-Forwarded-Host: tunnel.example",
        "X-Real-Ip: 203.0.113.9",
        "Forwarded: for=203.0.113.9",
        "X-PM-Proxied: 1",
    ] {
        let request = format!("GET /panes HTTP/1.1\r\nHost: 127.0.0.1\r\n{header}\r\n\r\n");
        let (status, body) = send(r.bridge, &request);
        assert_eq!(status, 401, "{header} marks a proxied remote caller: {body}");
    }
    let request = format!(
        "GET /panes HTTP/1.1\r\nHost: 127.0.0.1\r\nCf-Connecting-Ip: 203.0.113.9\r\nAuthorization: Bearer {}\r\n\r\n",
        r.token
    );
    let (status, body) = send(r.bridge, &request);
    assert_eq!(status, 200, "{body}");
    assert_eq!(json(&body)["local_app"], false);
}

#[test]
fn direct_request_naming_a_foreign_host_needs_a_bearer() {
    // A reverse proxy pointed straight at the bridge, or a DNS-rebinding page, arrives from
    // loopback but names its own host.
    let r = rig();
    let (status, _) = send(r.bridge, "GET /panes HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert_eq!(status, 401);
    for host in ["127.0.0.1:17321", "localhost:17321", "[::1]:17321", "localhost"] {
        let (status, body) = send(r.bridge, &format!("GET /panes HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert_eq!(status, 200, "local Host {host} must keep working: {body}");
    }
}

#[test]
fn a_web_page_in_the_users_browser_cannot_use_the_loopback_bridge_as_the_app() {
    // Any site can fetch http://127.0.0.1:<port>; the browser names the page in `Origin`.
    let r = rig();
    for origin in ["https://evil.example", "null"] {
        let request =
            format!("POST /panes/p1/input HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nOrigin: {origin}\r\nContent-Length: 2\r\n\r\n{{}}");
        let (status, body) = send(r.bridge, &request);
        assert_eq!(status, 401, "Origin {origin}: {body}");
    }
    // the desktop webview and the dev server are loopback origins and stay trusted
    for origin in ["tauri://localhost", "http://tauri.localhost", "http://localhost:1420"] {
        let request = format!("GET /panes HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nOrigin: {origin}\r\n\r\n");
        let (status, body) = send(r.bridge, &request);
        assert_eq!(status, 200, "Origin {origin}: {body}");
        assert_eq!(json(&body)["local_app"], true);
    }
}

#[test]
fn sse_stream_through_the_tunnel_needs_a_bearer() {
    let r = rig();
    for path in ["/bridge/events"] {
        let (status, _) = send(r.pwa, &get(path, &[]));
        assert_eq!(status, 401, "{path}");
        let auth = format!("Authorization: Bearer {}", r.token);
        let (status, _) = send(r.pwa, &get(path, &[&auth]));
        assert_eq!(status, 200, "{path} with a bearer");
    }
}

#[test]
fn proxy_sends_exactly_one_marker_of_its_own_and_keeps_the_query_string() {
    let r = rig();
    let auth = format!("Authorization: Bearer {}", r.token);
    // The client lies about the marker (in two spellings); the bridge must see only the proxy's.
    let (status, body) = send(
        r.pwa,
        &get(
            "/bridge/panes/p1/buffer?view=screen&lines=5",
            &[&auth, "X-PM-Proxied: 0", "x-pm-proxied: nope"],
        ),
    );
    assert_eq!(status, 200, "{body}");
    let reply = json(&body);
    assert_eq!(reply["proxied_markers"], serde_json::json!(["1"]));
    assert_eq!(reply["target"], "/panes/p1/buffer?view=screen&lines=5");
}
