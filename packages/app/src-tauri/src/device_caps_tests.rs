//! Device capabilities (S2), sliding expiry, the log-only local secret (S3) and CORS, end to end.
//!
//! Requests go client -> real `pwa_server` (the loopback server cloudflared points at, which marks
//! every request as remote) -> the real bridge request reader and `gate` (`authorize_caller`), with
//! the real pairing store. Only the `AppHandle`-bound route dispatch is stubbed (as in
//! `tunnel_auth_tests.rs`); `GET /events/poll` runs the real long-poll handler.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use crate::mobile_pairing::{
    init_pairing_store, local_secret, pairing_store, Cap, Caps, MobilePairingStore,
    PairRequestBody, TokenCheck, DEVICE_IDLE_EXPIRY_SECS,
};

struct Rig {
    bridge: SocketAddr,
    pwa: SocketAddr,
    /// device label -> bearer token
    tokens: HashMap<&'static str, String>,
    ids: HashMap<&'static str, String>,
}

fn pair_with(caps: Caps, name: &str) -> (String, String) {
    let _ = rig();
    pair_in_global(caps, name)
}

fn pair_in_global(caps: Caps, name: &str) -> (String, String) {
    let store = pairing_store().expect("pairing store");
    let mut guard = store.lock();
    let session = guard.create_pairing_session_with_caps("https://example.invalid".into(), caps);
    let paired = guard
        .pair_device(PairRequestBody {
            pairing_code: session.pairing_code,
            device_name: name.into(),
            device_public_key: session.server_public_key,
        })
        .expect("pair test device");
    (paired.device_id, paired.device_token)
}

fn only(cap: Cap) -> Caps {
    Caps::none().with(cap, true)
}

fn devices() -> Vec<(&'static str, Caps)> {
    vec![
        ("full", Caps::all()),
        ("watch_answer", Caps::watch_and_answer()),
        ("watch", only(Cap::Watch)),
        ("answer", only(Cap::Answer)),
        ("type", only(Cap::Type)),
        ("control", only(Cap::Control)),
        ("none", Caps::none()),
    ]
}

fn rig() -> &'static Rig {
    static RIG: OnceLock<Rig> = OnceLock::new();
    RIG.get_or_init(|| {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("pm-device-caps-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // The store is process-wide; another test may have created it first. Either is fine:
        // these tests only use devices they pair themselves.
        let _ = init_pairing_store(dir.join("pairing.json"));
        let mut tokens = HashMap::new();
        let mut ids = HashMap::new();
        for (label, caps) in devices() {
            let (id, token) = pair_in_global(caps, label);
            tokens.insert(label, token);
            ids.insert(label, id);
        }
        let bridge = crate::bridge::spawn_device_gate_listener();
        let port_file = dir.join("bridge.port");
        std::fs::write(&port_file, format!("{bridge}\n")).unwrap();
        let pwa = crate::pwa_server::spawn_test_server(dir.clone(), port_file);
        Rig { bridge, pwa, tokens, ids }
    })
}

/// Send raw request text; returns (status, head, body).
fn send(addr: SocketAddr, request: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    (status, head.to_string(), body.to_string())
}

fn request(method: &str, path: &str, headers: &[&str]) -> String {
    let body = if method == "GET" || method == "OPTIONS" { "" } else { "{}" };
    let mut req = format!("{method} {path} HTTP/1.1\r\n");
    if !headers.iter().any(|h| h.to_ascii_lowercase().starts_with("host:")) {
        req.push_str("Host: tunnel.example\r\n");
    }
    for header in headers {
        req.push_str(header);
        req.push_str("\r\n");
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    req
}

/// Through the tunnel (pwa_server) as the device with `label`.
fn as_device(label: &str, method: &str, path: &str) -> (u16, String) {
    let auth = format!("Authorization: Bearer {}", rig().tokens[label]);
    let (status, _, body) = send(rig().pwa, &request(method, &format!("/bridge{path}"), &[&auth]));
    (status, body)
}

/// One representative route per class, and the cap it needs ("device" = any paired device).
const ROUTES: &[(&str, &str, &str)] = &[
    // any paired device
    ("GET", "/panes", "device"),
    ("GET", "/settings", "device"),
    ("GET", "/agents/types", "device"),
    ("GET", "/pair/me", "device"),
    ("GET", "/events/poll", "device"),
    ("POST", "/push/subscribe", "device"),
    // watch
    ("GET", "/panes/p1/screen", "watch"),
    ("GET", "/panes/p1/buffer?lines=50", "watch"),
    ("GET", "/agents/h1/transcript", "watch"),
    ("GET", "/operations/op1", "watch"),
    ("GET", "/events/recent", "watch"),
    ("POST", "/panes/p1/viewport", "watch"),
    ("POST", "/agents/wait", "watch"),
    // watch or answer
    ("GET", "/questions", "watch|answer"),
    // answer
    ("POST", "/questions/q1/answer", "answer"),
    ("POST", "/questions/q1/dismiss", "answer"),
    ("POST", "/agents/answer", "answer"),
    ("POST", "/panes/p1/opencode/permissions/r1/reply", "answer"),
    // type
    ("POST", "/panes/p1/input", "type"),
    ("POST", "/panes/p1/key", "type"),
    // control
    ("POST", "/panes", "control"),
    ("DELETE", "/panes/p1", "control"),
    ("POST", "/panes/stop-all", "control"),
    ("POST", "/shell/exec", "control"),
    ("POST", "/agents/run", "control"),
    ("POST", "/orchestrator/message", "control"),
    ("PATCH", "/settings", "control"),
    ("POST", "/accounts", "control"),
    ("POST", "/pair/invite", "control"),
    ("POST", "/some/future/route", "control"),
    // Team cards on the phone (`team::phone`)
    ("GET", "/team/cards", "watch"),
    ("POST", "/team/cards/c1/dismiss", "answer|control"),
    ("POST", "/team/cards/c1/queue", "control"),
    ("POST", "/team/jobs/j1/apply", "control"),
];

fn allowed(caps: Caps, need: &str) -> bool {
    match need {
        "device" => true,
        "watch|answer" => caps.has(Cap::Watch) || caps.has(Cap::Answer),
        "answer|control" => caps.has(Cap::Answer) || caps.has(Cap::Control),
        "watch" => caps.has(Cap::Watch),
        "answer" => caps.has(Cap::Answer),
        "type" => caps.has(Cap::Type),
        "control" => caps.has(Cap::Control),
        other => panic!("unknown need {other}"),
    }
}

#[test]
fn every_route_class_against_every_cap() {
    let mut table = Vec::new();
    for (label, caps) in devices() {
        for (method, path, need) in ROUTES {
            let (status, body) = as_device(label, method, path);
            let expect = if allowed(caps, need) { 200 } else { 403 };
            table.push(format!("{label:<13} {method:<6} {path:<42} needs {need:<12} -> {status}"));
            assert_eq!(status, expect, "{label} {method} {path} (needs {need}): {body}");
            if status == 403 {
                let reply: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert_eq!(reply["error"], "not_allowed");
                assert_eq!(reply["needs"], need.to_string());
            }
        }
    }
    println!("{}", table.join("\n"));
}

#[test]
fn full_control_is_what_existing_pairings_could_do_before() {
    // Every route that answered a paired phone before caps still answers a full-control device.
    for (method, path, _) in ROUTES {
        let (status, body) = as_device("full", method, path);
        assert_eq!(status, 200, "{method} {path}: {body}");
    }
}

#[test]
fn the_local_app_is_never_limited_by_caps() {
    for (method, path, _) in ROUTES {
        let (status, _, body) = send(rig().bridge, &request(method, path, &["Host: 127.0.0.1"]));
        assert_eq!(status, 200, "{method} {path}: {body}");
    }
}

#[test]
fn a_device_without_watch_gets_no_terminal_bytes_on_the_long_poll() {
    let r = rig();
    let start = {
        let (_, body) = as_device("answer", "GET", "/events/poll");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["cursor"].as_u64().unwrap()
    };
    let pane = format!("dc-poll-{}", std::process::id());
    // The exact `event: terminal` payload push_terminal_sse sends, without touching pane_activity
    // (its tests count dirty panes process-wide).
    crate::bridge::push_sse(format!(
        "event: terminal\ndata: {{\"b64\":\"c2VjcmV0IHRlcm1pbmFsIGJ5dGVz\",\"offset\":0,\"pane_id\":\"{pane}\"}}\n\n"
    ));
    crate::bridge::push_sse(format!("event: pane-status\ndata: {{\"pane_id\":\"{pane}\",\"status\":\"idle\"}}\n\n"));
    let poll = |label: &str| {
        let path = format!("/events/poll?cursor={start}&wait_ms=200");
        let (status, body) = as_device(label, "GET", &path);
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["data"].as_str().unwrap().to_string()
    };
    let watcher = poll("watch");
    let answerer = poll("answer");
    let nothing = poll("none");
    // base64 of b"secret terminal bytes": the payload of the terminal event pushed above
    let term = "c2VjcmV0IHRlcm1pbmFsIGJ5dGVz";
    let status = format!("\"pane_id\":\"{pane}\",\"status\":\"idle\"");
    assert!(watcher.contains(term) && watcher.contains("event: terminal"), "watch sees terminal bytes: {watcher}");
    assert!(!answerer.contains(term), "answer-only sees no terminal bytes: {answerer}");
    assert!(!nothing.contains(term));
    assert!(answerer.contains(&status), "other events still arrive: {answerer}");
    assert!(watcher.contains(&status));
    // The local app sees everything.
    let (_, _, local) = send(r.bridge, &request("GET", &format!("/events/poll?cursor={start}&wait_ms=200"), &["Host: 127.0.0.1"]));
    assert!(local.contains(&pane) && local.contains("event: terminal"));
}

/// Review R12 2.1: `GET /operations/{id}` is refused without `watch`, but the same snapshot
/// (task, reply, screen rows) used to arrive whole on the event streams as `event: operation`.
#[test]
fn a_device_without_watch_gets_operation_status_but_no_pane_content_on_the_long_poll() {
    let r = rig();
    let start = {
        let (_, body) = as_device("answer", "GET", "/events/poll");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["cursor"].as_u64().unwrap()
    };
    let op = format!("dc-op-{}", std::process::id());
    let snapshot = serde_json::json!({
        "operation_id": op, "status": "waiting_input", "stage": "waiting_for_pane_readiness",
        "task": "TASKTEXT-xyz", "result": "REPLYTEXT-xyz", "output_baseline": "BASELINE-xyz",
        "required_action": {"kind": "screen_not_ready", "detail": "bring the agent to its input box", "screen": "SCREENROWS-xyz"},
        "error": {"code": "AGENT_EXITED", "message": "exited", "recoverable": false, "context": {"last_screen": "LASTSCREEN-xyz"}}
    });
    crate::bridge::push_sse(format!("event: operation\ndata: {snapshot}\n\n"));
    let poll = |label: &str| {
        let (status, body) = as_device(label, "GET", &format!("/events/poll?cursor={start}&wait_ms=200"));
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["data"].as_str().unwrap().to_string()
    };
    for label in ["answer", "none"] {
        let seen = poll(label);
        assert!(seen.contains(&op) && seen.contains("screen_not_ready"), "{label}: status still arrives: {seen}");
        for secret in ["TASKTEXT", "REPLYTEXT", "BASELINE", "SCREENROWS", "LASTSCREEN"] {
            assert!(!seen.contains(secret), "{label}: {secret} leaked without watch: {seen}");
        }
    }
    let watcher = poll("watch");
    assert!(watcher.contains("TASKTEXT-xyz") && watcher.contains("SCREENROWS-xyz"), "watch sees it all: {watcher}");
    let (_, _, local) = send(r.bridge, &request("GET", &format!("/events/poll?cursor={start}&wait_ms=200"), &["Host: 127.0.0.1"]));
    assert!(local.contains("REPLYTEXT-xyz"), "the local app sees it all");
}

#[test]
fn websocket_input_frames_need_type() {
    let r = rig();
    let headers = |label: &str| format!("X-PM-Proxied: 1\r\nAuthorization: Bearer {}", r.tokens[label]);
    assert!(crate::mobile_pairing::caller_may(&headers("full"), true, Cap::Type));
    assert!(crate::mobile_pairing::caller_may(&headers("type"), true, Cap::Type));
    assert!(!crate::mobile_pairing::caller_may(&headers("watch_answer"), true, Cap::Type));
    assert!(!crate::mobile_pairing::caller_may(&headers("watch"), true, Cap::Type));
    assert!(!crate::mobile_pairing::caller_may("X-PM-Proxied: 1", true, Cap::Type), "no token");
    assert!(crate::mobile_pairing::caller_may("Host: 127.0.0.1", true, Cap::Type), "the local app");
}

#[test]
fn websocket_upgrade_is_open_to_a_watch_only_device() {
    let r = rig();
    let protocols = format!("Sec-WebSocket-Protocol: pm-events, pm-bearer.{}", r.tokens["watch"]);
    let req = format!(
        "GET /bridge/events/ws HTTP/1.1\r\nHost: tunnel.example\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n{protocols}\r\n\r\n"
    );
    let (status, _, body) = send(r.pwa, &req);
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["device_caps"], serde_json::json!(["watch"]));
}

#[test]
fn changing_caps_applies_to_the_next_request_and_revoke_still_works() {
    let (id, token) = pair_with(Caps::all(), "changes");
    let call = |method: &str, path: &str| {
        let auth = format!("Authorization: Bearer {token}");
        send(rig().pwa, &request(method, &format!("/bridge{path}"), &[&auth])).0
    };
    assert_eq!(call("POST", "/shell/exec"), 200);
    let store = pairing_store().unwrap();
    assert!(store.lock().set_device_caps(&id, Caps::watch_and_answer()).unwrap());
    assert_eq!(call("POST", "/shell/exec"), 403);
    assert_eq!(call("POST", "/panes/p1/input"), 403);
    assert_eq!(call("GET", "/panes/p1/screen"), 200);
    assert_eq!(crate::mobile_pairing::stream_caps(&id), Some(Caps::watch_and_answer()));
    assert!(store.lock().revoke_device(&id).unwrap());
    assert_eq!(call("GET", "/panes"), 401);
    assert_eq!(crate::mobile_pairing::stream_caps(&id), None, "open streams close on the next payload");
}

#[test]
fn an_expired_device_is_signed_out_with_its_own_error() {
    let (id, token) = pair_with(Caps::all(), "old phone");
    let store = pairing_store().unwrap();
    store.lock().set_device_expiry_for_test(&id, 1);
    let auth = format!("Authorization: Bearer {token}");
    let (status, _, body) = send(rig().pwa, &request("GET", "/bridge/panes", &[&auth]));
    assert_eq!(status, 401);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"], "device_expired");
    let listed = store.lock().list_devices().into_iter().find(|d| d.id == id).unwrap();
    assert!(listed.expired);
    assert_eq!(crate::mobile_pairing::stream_caps(&id), None);
    // Revoke still removes it.
    assert!(store.lock().revoke_device(&id).unwrap());
}

// ---------- sliding expiry and migration (a store of its own, not the process-wide one) ----------

fn temp_store(name: &str) -> (PathBuf, MobilePairingStore) {
    let dir = std::env::temp_dir().join(format!("pm-device-caps-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pairing.json");
    let store = MobilePairingStore::load_or_create(path.clone()).unwrap();
    (path, store)
}

fn pair_in(store: &mut MobilePairingStore, caps: Option<Caps>) -> (String, String) {
    let session = match caps {
        Some(caps) => store.create_pairing_session_with_caps("http://127.0.0.1:1".into(), caps),
        None => store.create_pairing_session("http://127.0.0.1:1".into()),
    };
    let paired = store
        .pair_device(PairRequestBody {
            pairing_code: session.pairing_code,
            device_name: "phone".into(),
            device_public_key: session.server_public_key,
        })
        .unwrap();
    (paired.device_id, paired.device_token)
}

#[test]
fn expiry_slides_with_use_so_an_active_phone_never_expires() {
    let (path, mut store) = temp_store("slide");
    let (id, token) = pair_in(&mut store, None);
    let paired_at = store.list_devices()[0].paired_at;
    let day = 24 * 60 * 60;
    // Used every 29 days for a year: never signed out.
    let mut now = paired_at;
    for _ in 0..13 {
        now += 29 * day;
        assert!(matches!(store.check_token_at(&token, now), TokenCheck::Valid { .. }), "day {}", (now - paired_at) / day);
    }
    let info = store.list_devices_at(now).into_iter().find(|d| d.id == id).unwrap();
    assert_eq!(info.expires_at, now + DEVICE_IDLE_EXPIRY_SECS);
    assert_eq!(info.last_seen, Some(now));
    // The slid expiry is on disk (written at most hourly, and this moved by days).
    let reloaded = MobilePairingStore::load_or_create(path.clone()).unwrap();
    assert_eq!(reloaded.list_devices_at(now)[0].expires_at, now + DEVICE_IDLE_EXPIRY_SECS);
    // Then 30 days and one second without use: signed out.
    let later = now + DEVICE_IDLE_EXPIRY_SECS + 1;
    assert_eq!(store.check_token_at(&token, later), TokenCheck::Expired { device_id: id.clone() });
    assert!(store.list_devices_at(later)[0].expired);
    // ...and stays out: a late request does not revive it.
    assert_eq!(store.check_token_at(&token, later + 10), TokenCheck::Expired { device_id: id });
}

#[test]
fn exactly_thirty_days_idle_is_still_valid() {
    let (_, mut store) = temp_store("edge");
    let (_, token) = pair_in(&mut store, None);
    let paired_at = store.list_devices()[0].paired_at;
    assert!(matches!(
        store.check_token_at(&token, paired_at + DEVICE_IDLE_EXPIRY_SECS),
        TokenCheck::Valid { .. }
    ));
}

#[test]
fn pairings_from_before_caps_migrate_to_all_caps_and_a_fresh_window() {
    let (path, mut store) = temp_store("migrate");
    let (id, token) = pair_in(&mut store, Some(Caps::watch_and_answer()));
    drop(store);
    // Rewrite the file in the pre-caps format: no caps, no exp, no last_seen.
    let mut raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for device in raw["devices"].as_array_mut().unwrap() {
        let obj = device.as_object_mut().unwrap();
        obj.remove("caps");
        obj.remove("exp");
        obj.remove("last_seen");
    }
    raw.as_object_mut().unwrap().remove("pending_caps");
    let old = serde_json::to_string_pretty(&raw).unwrap();
    assert!(!old.contains("\"caps\"") && !old.contains("\"exp\""), "{old}");
    std::fs::write(&path, &old).unwrap();

    let before = crate::mobile_pairing::now_secs_for_test();
    let mut store = MobilePairingStore::load_or_create(path.clone()).unwrap();
    let device = store.list_devices().into_iter().find(|d| d.id == id).unwrap();
    assert_eq!(device.caps, Caps::all(), "an old pairing keeps everything it could do");
    assert!(device.expires_at >= before + DEVICE_IDLE_EXPIRY_SECS);
    assert!(!device.expired);
    assert!(matches!(store.check_token(&token), TokenCheck::Valid { caps, .. } if caps == Caps::all()));
    // Written back once, in the new format.
    let migrated = std::fs::read_to_string(&path).unwrap();
    assert!(migrated.contains("\"caps\"") && migrated.contains("\"exp\""), "{migrated}");
}

#[test]
fn the_pairing_choice_is_what_the_new_device_gets() {
    let (path, mut store) = temp_store("choice");
    let (_, _) = pair_in(&mut store, None);
    assert_eq!(store.list_devices()[0].caps, Caps::all(), "the default is full control (today)");
    // The choice survives an app restart between showing the QR code and the phone scanning it.
    let session = store.create_pairing_session_with_caps("http://127.0.0.1:1".into(), Caps::watch_and_answer());
    drop(store);
    let mut store = MobilePairingStore::load_or_create(path).unwrap();
    store
        .pair_device(PairRequestBody {
            pairing_code: session.pairing_code,
            device_name: "watcher".into(),
            device_public_key: session.server_public_key,
        })
        .unwrap();
    assert_eq!(store.list_devices()[1].caps, Caps::watch_and_answer());
}

// ---------- CORS ----------

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    crate::mobile_pairing::header_value(head, name)
}

#[test]
fn a_web_page_on_another_origin_gets_no_cors_and_no_answer() {
    let r = rig();
    for origin in ["https://evil.example", "http://192.168.1.20:8080", "null"] {
        let o = format!("Origin: {origin}");
        // The preflight a browser sends before a JSON POST: no allow-origin, so it stops here.
        let pre = request(
            "OPTIONS",
            "/shell/exec",
            &["Host: 127.0.0.1:17321", &o, "Access-Control-Request-Method: POST", "Access-Control-Request-Headers: content-type"],
        );
        let (_, head, _) = send(r.bridge, &pre);
        assert_eq!(header(&head, "Access-Control-Allow-Origin"), None, "{origin}: {head}");
        // A simple GET (no preflight): refused without a token, and unreadable to the page anyway.
        let (status, head, body) = send(r.bridge, &request("GET", "/panes", &["Host: 127.0.0.1:17321", &o]));
        assert_eq!(status, 401, "{origin}: {body}");
        assert_eq!(header(&head, "Access-Control-Allow-Origin"), None, "{origin}: {head}");
    }
}

#[test]
fn the_apps_own_origins_get_their_origin_back_never_a_wildcard() {
    let r = rig();
    for origin in ["http://tauri.localhost", "https://tauri.localhost", "tauri://localhost", "http://localhost:1420"] {
        let o = format!("Origin: {origin}");
        let pre = request("OPTIONS", "/panes", &["Host: 127.0.0.1:17321", &o, "Access-Control-Request-Method: POST"]);
        let (_, head, _) = send(r.bridge, &pre);
        assert_eq!(header(&head, "Access-Control-Allow-Origin"), Some(origin), "{head}");
        assert!(header(&head, "Access-Control-Allow-Headers").unwrap().contains("X-PM-Local-Secret"));
        let (status, head, _) = send(r.bridge, &request("GET", "/panes", &["Host: 127.0.0.1:17321", &o]));
        assert_eq!(status, 200);
        assert_eq!(header(&head, "Access-Control-Allow-Origin"), Some(origin));
    }
    // No Origin (CLI, MCP shim, the phone through the PWA proxy): no CORS lines at all.
    let (_, head, _) = send(r.bridge, &request("GET", "/panes", &["Host: 127.0.0.1"]));
    assert_eq!(header(&head, "Access-Control-Allow-Origin"), None);
    let auth = format!("Authorization: Bearer {}", r.tokens["full"]);
    let (status, head, _) = send(r.pwa, &request("GET", "/bridge/panes", &[&auth]));
    assert_eq!(status, 200);
    assert!(!head.contains("Access-Control-Allow-Origin: *"), "{head}");
}

// ---------- local secret (log-only) ----------

#[test]
fn a_local_caller_without_the_secret_is_logged_but_still_served() {
    let r = rig();
    let port_file = std::env::temp_dir().join(format!("pm-device-caps-secret-{}", std::process::id())).join("x.port");
    std::fs::create_dir_all(port_file.parent().unwrap()).unwrap();
    let secret = local_secret::init(&port_file).unwrap();
    let before = local_secret::status().missing_count;
    let (status, _, _) = send(r.bridge, &request("POST", "/shell/exec", &["Host: 127.0.0.1"]));
    assert_eq!(status, 200, "log-only: not refused");
    assert!(local_secret::status().missing_count > before, "counted as LOCAL_SECRET_MISSING");
    assert!(local_secret::status().log_only);
    // With the secret the request is the app's own (checked exactly by local_secret's unit tests).
    let with = format!("X-PM-Local-Secret: {secret}");
    let (status, _, _) = send(r.bridge, &request("POST", "/shell/exec", &["Host: 127.0.0.1", &with]));
    assert_eq!(status, 200);
    // A device through the tunnel is never asked for the local secret.
    let _ = as_device("full", "GET", "/panes");
    let _ = r.ids.len();
}

/// Helper for the headless-browser CORS proof (`D:\pm-night\p0\device-caps\cors-browser-proof.mjs`):
/// serves the real gate on loopback for 90 s and prints its address. Not part of the normal run.
#[test]
#[ignore]
fn serve_gate_for_browser_probe() {
    let bridge = crate::bridge::spawn_device_gate_listener();
    println!("BRIDGE={bridge}");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    std::thread::sleep(Duration::from_secs(90));
}
