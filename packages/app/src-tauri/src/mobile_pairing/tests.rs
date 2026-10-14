use super::*;

#[test]
fn public_paths_accept_bridge_prefix() {
    let segments = ["bridge", "health"];
    assert!(is_public_bridge_path(&segments, "GET"));

    let segments = ["bridge", "pair", "session", "CODE12"];
    assert!(is_public_bridge_path(&segments, "GET"));

    let segments = ["bridge", "pair"];
    assert!(is_public_bridge_path(&segments, "POST"));
}

#[test]
fn proxied_bridge_prefix_does_not_bypass_pairing() {
    let segments = ["bridge", "panes"];
    let err = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &segments, "GET")
        .expect_err("panes should require auth");
    assert_eq!(err.0, 401);
}

#[test]
fn local_only_caller_requires_loopback_and_no_proxy_header() {
    assert!(is_local_only_caller("Host: 127.0.0.1\r\n", true));
    assert!(!is_local_only_caller("X-PM-Proxied: 1\r\n", true));
    assert!(!is_local_only_caller("Host: example.com\r\n", false));
}

#[test]
fn invite_route_is_not_public() {
    let segments = ["pair", "invite"];
    assert!(!is_public_bridge_path(&segments, "POST"));
}

#[test]
fn push_act_is_public_but_every_other_push_route_needs_a_bearer() {
    // The Allow/Deny tap carries its own one-shot token instead of a bearer.
    assert!(is_public_bridge_path(&["push", "act"], "POST"));
    assert!(is_public_bridge_path(&["bridge", "push", "act"], "POST"));
    assert!(!is_public_bridge_path(&["push", "act"], "GET"));
    for (method, path) in [("GET", "key"), ("POST", "subscribe"), ("DELETE", "subscribe"), ("POST", "presence")] {
        let err = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &["push", path], method)
            .expect_err("push routes require a paired device");
        assert_eq!(err.0, 401, "{method} /push/{path}");
    }
}

#[test]

fn any_forwarding_header_on_a_loopback_peer_requires_a_token() {
    for header in [
        "X-PM-Proxied: 1",
        "x-pm-proxied: 0",
        "Cf-Connecting-Ip: 203.0.113.9",
        "CF-RAY: 8a1b2c3d-AMS",
        "Cf-Visitor: {\"scheme\":\"https\"}",
        "Cdn-Loop: cloudflare",
        "True-Client-Ip: 203.0.113.9",
        "X-Forwarded-For: 203.0.113.9",
        "X-Forwarded-Host: example.com",
        "X-Forwarded-Proto: https",
        "X-Real-Ip: 203.0.113.9",
        "Forwarded: for=203.0.113.9",
    ] {
        let headers = format!("GET /panes HTTP/1.1\r\nHost: 127.0.0.1:17321\r\n{header}\r\n");
        assert!(requires_pairing_token(&headers, true), "{header}");
        assert!(!is_local_only_caller(&headers, true), "{header}");
    }
}

#[test]
fn local_tooling_without_forwarding_headers_stays_trusted() {
    for headers in [
        "GET /panes HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nAccept: */*\r\n",
        "GET /panes HTTP/1.1\r\nHost: localhost:17321\r\nOrigin: http://tauri.localhost\r\n",
        "GET /panes HTTP/1.1\r\nHost: [::1]:17321\r\n",
        "GET /panes HTTP/1.1\r\nHost: ::1\r\n",
        "GET /panes HTTP/1.1\r\nHost: app.localhost\r\nX-Puppet-Master-Session: s1\r\n",
        // HTTP/1.0-style client with no Host at all
        "GET /panes HTTP/1.0\r\nAccept: */*\r\n",
    ] {
        assert!(!requires_pairing_token(headers, true), "{headers:?}");
    }
    // a non-loopback peer always needs one, whatever it claims
    assert!(requires_pairing_token("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n", false));
}

#[test]
fn a_foreign_host_name_on_a_loopback_peer_requires_a_token() {
    for host in [
        "evil.example",
        "evil.example:17321",
        "localhost.evil.example",
        "127.0.0.1.evil.example",
        "192.168.1.20:17321",
        "",
    ] {
        let headers = format!("GET /panes HTTP/1.1\r\nHost: {host}\r\n");
        assert!(requires_pairing_token(&headers, true), "Host: {host:?}");
    }
    // a second Host header cannot hide behind a loopback first one
    assert!(requires_pairing_token(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nHost: evil.example\r\n",
        true
    ));
}

#[test]
fn forwarded_loopback_request_is_refused_without_a_bearer() {
    let segments = ["panes"];
    for headers in [
        "GET /panes HTTP/1.1\r\nHost: 127.0.0.1\r\nCf-Connecting-Ip: 203.0.113.9\r\n",
        "GET /panes HTTP/1.1\r\nHost: tunnel.example\r\n",
    ] {
        let err = authorize_bridge_request(headers, true, &segments, "GET")
            .expect_err("a forwarded caller needs a token");
        assert_eq!(err.0, 401, "{headers:?}");
    }
    // public paths stay open for the pairing handshake and health checks
    assert!(authorize_bridge_request("Cf-Ray: x\r\n", true, &["health"], "GET").is_ok());
}

#[test]
fn a_web_page_origin_on_a_loopback_peer_requires_a_token() {
    for origin in [
        "https://evil.example",
        "http://evil.example:8080",
        "null",
        "https://localhost.evil.example",
        "file://",
    ] {
        let headers = format!("POST /panes/p1/input HTTP/1.1
Host: 127.0.0.1:17321
Origin: {origin}
");
        assert!(requires_pairing_token(&headers, true), "Origin: {origin}");
    }
    for origin in [
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
        "http://localhost:1420",
        "http://127.0.0.1:1420",
        "http://[::1]:4173",
    ] {
        let headers = format!("POST /panes/p1/input HTTP/1.1
Host: 127.0.0.1:17321
Origin: {origin}
");
        assert!(!requires_pairing_token(&headers, true), "Origin: {origin}");
    }
}

/// Review R12 2.3: the fanout's device mirror held caps only, so a lost phone holding an SSE or WS
/// stream open past its 30 days kept receiving events until it made a new request.
#[test]
fn an_open_stream_stops_when_the_device_expires_without_a_new_request() {
    let dir = std::env::temp_dir().join(format!("pm-stream-exp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = MobilePairingStore::load_or_create(dir.join("pairing.json")).unwrap();
    let session = store.create_pairing_session_with_caps("http://127.0.0.1:1".into(), Caps::all());
    let device = store
        .pair_device(PairRequestBody {
            pairing_code: session.pairing_code,
            device_name: "phone".into(),
            device_public_key: session.server_public_key,
        })
        .unwrap()
        .device_id;
    let now = now_secs();
    assert_eq!(stream_caps_at(&device, now), Some(Caps::all()));
    assert_eq!(stream_caps_at(&device, now + DEVICE_IDLE_EXPIRY_SECS + 60), None, "past its idle expiry the stream closes");
    let _ = std::fs::remove_dir_all(dir);
}
