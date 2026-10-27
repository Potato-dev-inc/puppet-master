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
fn hex_bearer_carries_base64_tokens() {
    // "ab+/=" as hex: the characters browsers refuse in a subprotocol.
    assert_eq!(
        bearer_from_protocols("pm-events, pm-bearer-hex.61622b2f3d"),
        Some("ab+/=".into())
    );
    assert_eq!(bearer_from_protocols("pm-bearer-hex.6"), None);
    assert_eq!(bearer_from_protocols("pm-bearer-hex.zz"), None);
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
fn ping_is_answered_with_the_same_t() {
    let ping: Value = serde_json::from_str(r#"{"type":"ping","t":12.5}"#).unwrap();
    let pong: Value = serde_json::from_str(&pong_reply(&ping)).unwrap();
    assert_eq!(pong, json!({"type": "pong", "t": 12.5}));
    let bare: Value = serde_json::from_str(r#"{"type":"ping"}"#).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&pong_reply(&bare)).unwrap(), json!({"type": "pong", "t": null}));
}

/// A pane whose screen shows a real Claude bash approval dialog.
fn claude_pane_showing_a_dialog(id: &str) -> Arc<Mutex<PaneRegistry>> {
    const DIALOG: &str = include_str!("../tui/claude/fixtures/bash_simple_4_options_2_1_299.txt");
    let mut registry = PaneRegistry::new();
    let mut pane = PaneRegistry::test_pane_stub(id);
    pane.info.agent_type = "claude".into();
    pane.screen = Arc::new(Mutex::new(vt100::Parser::new(40, 120, 0)));
    let mut bytes = b"\x1b[2J\x1b[H".to_vec();
    // The capture asks about `npm test` (low risk); the same dialog for a deleting command outside the project is higher.
    bytes.extend(DIALOG.replace("npm test", "rm -rf ~/Documents").replace('\n', "\r\n").as_bytes());
    pane.screen.lock().process(&bytes);
    registry.panes.insert(id.into(), pane);
    Arc::new(Mutex::new(registry))
}

/// An MCP session that took a pane over types a key into that pane while it shows a dialog. The
/// HTTP route holds the keys for a person (`raw_keys_gate_for`); the socket's `input` frame must
/// hold them too. The socket write itself needs an `AppHandle`, which unit tests cannot build,
/// so this checks the gate every socket write passes before it.
#[test]
fn a_socket_input_from_a_session_into_a_dialog_is_held_like_the_http_route() {
    use crate::agent_authority::config::{with_config, Config};
    const TODAY: Config = Config { ask_before_agent_control: false, agents_may_merge: true };
    const ASKING: Config = Config { ask_before_agent_control: true, agents_may_merge: true };
    let session = uuid::Uuid::new_v4().to_string();
    let pane = format!("socket-dialog-{}", uuid::Uuid::new_v4());
    let registry = claude_pane_showing_a_dialog(&pane);
    // The session took the pane over before the person's "ask first" setting was on.
    with_config(TODAY, || crate::mcp_sessions::take_over_pane(&session, &pane, true).unwrap());
    crate::mcp_sessions::set_mode(&session, "shell").unwrap();
    let message = json!({"type": "input", "pane_id": pane, "text": "1", "append_newline": false});
    let keys = typed_text_keys("1", false);
    with_config(ASKING, || {
        let (_, http) = crate::bridge::routes::panes::raw_keys_gate_for(Some(&session), &registry, &pane, &keys)
            .expect_err("HTTP holds the session's keys into a dialog");
        assert_eq!(http["code"], "CONSENT_PENDING", "{http}");
        // The socket's error text is the HTTP error text (the socket sends `error` only).
        let socket = check_socket_input(Some(&session), false, &registry, &pane, &keys, &message)
            .expect_err("the socket must hold the same keys");
        assert_eq!(socket, http["error"], "the socket refuses as HTTP does");
    });
    // A paired phone is a person: never gated here.
    with_config(ASKING, || {
        check_socket_input(None, true, &registry, &pane, &keys, &message).expect("a phone");
    });
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
