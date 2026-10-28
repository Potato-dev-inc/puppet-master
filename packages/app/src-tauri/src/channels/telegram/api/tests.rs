//! HTTP tests against a local fake of the Bot API (a `TcpListener`, the pattern of `push/sender.rs`).
//! No test talks to Telegram.

use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "123456789:TESTtokenTESTtokenTESTtokenTESTtok";

/// One request the fake received: the request line and the JSON body.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    body: Value,
}

/// Answers one connection per canned reply, in order, and records each request.
struct Fake {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn fake(replies: &[(u16, &'static str)]) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let replies = replies.to_vec();
    std::thread::spawn(move || {
        for (status, body) in replies {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let req = read_request(&mut stream);
            log.lock().unwrap().push(req);
            let resp = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    Fake { base, seen }
}

fn read_request(stream: &mut TcpStream) -> Seen {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break buf.len(),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let len = head
        .lines()
        .find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while buf.len() < head_end + len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = serde_json::from_slice(&buf[head_end.min(buf.len())..]).unwrap_or(Value::Null);
    Seen { line: head.lines().next().unwrap_or("").to_string(), body }
}

fn api_for(f: &Fake) -> HttpApi {
    HttpApi::with_base(TOKEN, f.base.clone())
}

fn last_seen(f: &Fake) -> Seen {
    f.seen.lock().unwrap().last().cloned().expect("the fake saw a request")
}

fn message(text: &str) -> WireMessage {
    WireMessage { text: crate::redact::text(text), keyboard: Vec::new() }
}

fn button(label: &str, data: &str) -> WireButton {
    WireButton { label: label.into(), data: data.into() }
}

#[test]
fn get_me_posts_to_the_bot_path_and_reads_the_bot() {
    let f = fake(&[(200, r#"{"ok":true,"result":{"id":123456789,"is_bot":true,"first_name":"Test Bot","username":"test_pm_bot"}}"#)]);
    let me = api_for(&f).get_me().unwrap();
    assert_eq!(me, BotInfo { id: 123456789, username: "test_pm_bot".into() });
    assert_eq!(last_seen(&f).line, format!("POST /bot{TOKEN}/getMe HTTP/1.1"));
}

#[test]
fn webhook_url_empty_is_none_and_a_set_url_is_some() {
    let f = fake(&[
        (200, r#"{"ok":true,"result":{"url":"","has_custom_certificate":false}}"#),
        (200, r#"{"ok":true,"result":{"url":"https://example.invalid/hook"}}"#),
    ]);
    let api = api_for(&f);
    assert_eq!(api.webhook_url().unwrap(), None);
    assert_eq!(api.webhook_url().unwrap(), Some("https://example.invalid/hook".to_string()));
}

#[test]
fn get_updates_maps_the_batch_and_sends_the_poll_settings() {
    let batch = r#"{"ok":true,"result":[
        {"update_id":1001,"message":{"message_id":55,"from":{"id":1001,"is_bot":false,"first_name":"Test","username":"test_user"},"chat":{"id":1001,"type":"private"},"date":1761600000,"text":"/status"}},
        {"update_id":1002,"edited_message":{"message_id":55,"chat":{"id":1001,"type":"private"},"date":1761600000,"text":"/stop"}},
        {"update_id":1003,"callback_query":{"id":"4382","from":{"id":1001,"is_bot":false},"message":{"message_id":77,"chat":{"id":1001,"type":"private"},"date":1761600200},"data":"g:AbCdEfGhIjKlMnOpQrStUv"}}
    ]}"#;
    let f = fake(&[(200, batch)]);
    let updates = api_for(&f).get_updates(Some(1001), POLL_TIMEOUT_S).unwrap();
    assert_eq!(updates.len(), 3);
    assert_eq!(updates[0].update_id, 1001);
    assert!(matches!(&updates[0].kind, UpdateKind::Message(m) if m.text.as_deref() == Some("/status") && m.chat_private));
    assert_eq!(updates[1].kind, UpdateKind::Other);
    assert!(matches!(&updates[2].kind, UpdateKind::Callback(c) if c.message_id == Some(77)));

    let body = last_seen(&f).body;
    assert_eq!(body["offset"], json!(1001));
    assert_eq!(body["timeout"], json!(50));
    assert_eq!(body["allowed_updates"], json!(["message", "callback_query"]));
}

#[test]
fn get_updates_without_an_offset_omits_it() {
    let f = fake(&[(200, r#"{"ok":true,"result":[]}"#)]);
    assert!(api_for(&f).get_updates(None, 5).unwrap().is_empty());
    let body = last_seen(&f).body;
    assert!(body.get("offset").is_none());
    assert_eq!(body["timeout"], json!(5));
}

#[test]
fn get_updates_with_a_result_that_is_not_a_list_is_a_parse_error() {
    let f = fake(&[(200, r#"{"ok":true,"result":{"update_id":1}}"#)]);
    assert_eq!(api_for(&f).get_updates(None, 5), Err(ApiError::Parse));
}

#[test]
fn send_is_plain_text_with_no_preview_protected_and_a_reply_target() {
    let f = fake(&[(200, r#"{"ok":true,"result":{"message_id":88,"date":1,"chat":{"id":1001,"type":"private"}}}"#)]);
    let out = WireMessage {
        text: crate::redact::text("Mochi says hi"),
        keyboard: vec![vec![button("Deny", "g:0123456789abcdefghijkl"), button("Allow once…", "g:mnopqrstuvwxyz012345")]],
    };
    assert_eq!(api_for(&f).send(1001, &out, Some(55)), Ok(88));

    let seen = last_seen(&f);
    assert_eq!(seen.line, format!("POST /bot{TOKEN}/sendMessage HTTP/1.1"));
    let b = &seen.body;
    assert_eq!(b["chat_id"], json!(1001));
    assert_eq!(b["text"], json!("Mochi says hi"));
    assert_eq!(b["link_preview_options"], json!({"is_disabled": true}));
    assert_eq!(b["protect_content"], json!(true));
    assert_eq!(b["reply_to_message_id"], json!(55));
    assert!(b.get("parse_mode").is_none(), "plain text only: no parse_mode");
    assert_eq!(
        b["reply_markup"]["inline_keyboard"],
        json!([[
            {"text": "Deny", "callback_data": "g:0123456789abcdefghijkl"},
            {"text": "Allow once…", "callback_data": "g:mnopqrstuvwxyz012345"}
        ]])
    );
}

#[test]
fn send_without_buttons_or_reply_has_neither_key() {
    let f = fake(&[(200, r#"{"ok":true,"result":{"message_id":3}}"#)]);
    assert_eq!(api_for(&f).send(1001, &message("plain"), None), Ok(3));
    let b = last_seen(&f).body;
    assert!(b.get("reply_markup").is_none());
    assert!(b.get("reply_to_message_id").is_none());
}

#[test]
fn send_without_a_message_id_in_the_answer_is_a_parse_error() {
    let f = fake(&[(200, r#"{"ok":true,"result":true}"#)]);
    assert_eq!(api_for(&f).send(1001, &message("x"), None), Err(ApiError::Parse));
}

#[test]
fn edit_keeps_plain_text_and_drops_the_buttons_when_there_are_none() {
    let f = fake(&[(200, r#"{"ok":true,"result":true}"#), (200, r#"{"ok":true,"result":true}"#)]);
    let api = api_for(&f);
    api.edit(1001, 88, &message("Allowed once")).unwrap();
    let b = last_seen(&f).body;
    assert_eq!(last_seen(&f).line, format!("POST /bot{TOKEN}/editMessageText HTTP/1.1"));
    assert_eq!(b["message_id"], json!(88));
    assert_eq!(b["link_preview_options"], json!({"is_disabled": true}));
    assert!(b.get("parse_mode").is_none());
    assert!(b.get("reply_markup").is_none(), "no buttons: omit reply_markup so Telegram removes them");

    let with_buttons = WireMessage { text: crate::redact::text("Allow once?"), keyboard: vec![vec![button("Yes", "g:x")]] };
    api.edit(1001, 88, &with_buttons).unwrap();
    assert!(last_seen(&f).body.get("reply_markup").is_some());
}

#[test]
fn answer_callback_sends_the_id_and_the_optional_note() {
    let f = fake(&[(200, r#"{"ok":true,"result":true}"#), (200, r#"{"ok":true,"result":true}"#)]);
    let api = api_for(&f);
    api.answer_callback("4382", None).unwrap();
    assert_eq!(last_seen(&f).body, json!({"callback_query_id": "4382"}));
    api.answer_callback("4382", Some(&crate::redact::text("This button expired."))).unwrap();
    assert_eq!(last_seen(&f).body, json!({"callback_query_id": "4382", "text": "This button expired."}));
}

#[test]
fn set_commands_sends_command_and_description_pairs() {
    let f = fake(&[(200, r#"{"ok":true,"result":true}"#)]);
    api_for(&f).set_commands(&[("help", "What I can do"), ("stop", "Turn Telegram off")]).unwrap();
    let b = last_seen(&f).body;
    assert_eq!(
        b["commands"],
        json!([
            {"command": "help", "description": "What I can do"},
            {"command": "stop", "description": "Turn Telegram off"}
        ])
    );
}

#[test]
fn error_statuses_map_to_api_errors() {
    let cases: [(u16, &'static str, ApiError); 8] = [
        (401, r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#, ApiError::Unauthorized),
        (404, r#"{"ok":false,"error_code":404,"description":"Not Found"}"#, ApiError::Unauthorized),
        (403, r#"{"ok":false,"error_code":403,"description":"Forbidden: bot was blocked by the user"}"#, ApiError::Forbidden),
        (409, r#"{"ok":false,"error_code":409,"description":"Conflict"}"#, ApiError::Conflict),
        (429, r#"{"ok":false,"error_code":429,"description":"Too Many Requests","parameters":{"retry_after":7}}"#, ApiError::RateLimited { retry_after_s: 7 }),
        (429, r#"{"ok":false,"error_code":429,"description":"Too Many Requests"}"#, ApiError::RateLimited { retry_after_s: 30 }),
        (500, r#"{"ok":false,"error_code":500,"description":"Internal"}"#, ApiError::Bad { status: 500 }),
        (502, "<html>bad gateway</html>", ApiError::Bad { status: 502 }),
    ];
    let f = fake(&cases.iter().map(|(s, b, _)| (*s, *b)).collect::<Vec<_>>());
    let api = api_for(&f);
    for (_, _, expected) in cases {
        assert_eq!(api.get_me(), Err(expected));
    }
}

#[test]
fn a_huge_retry_after_is_capped() {
    let f = fake(&[(429, r#"{"ok":false,"error_code":429,"parameters":{"retry_after":99999}}"#)]);
    assert_eq!(api_for(&f).get_me(), Err(ApiError::RateLimited { retry_after_s: 3600 }));
}

#[test]
fn an_answer_that_is_not_json_is_a_parse_error() {
    let f = fake(&[(200, "<html>ok</html>"), (200, r#"{"ok":true,"result":{}}"#), (200, r#"{"ok":false}"#)]);
    let api = api_for(&f);
    assert_eq!(api.get_me(), Err(ApiError::Parse));
    assert_eq!(api.get_me(), Err(ApiError::Parse), "getMe without an id or username");
    assert_eq!(api.get_me(), Err(ApiError::Parse), "ok false without an error_code");
}

#[test]
fn a_redirect_is_not_followed() {
    let f = fake(&[(302, "")]);
    assert_eq!(api_for(&f).get_me(), Err(ApiError::Bad { status: 302 }));
    assert_eq!(f.seen.lock().unwrap().len(), 1);
}

#[test]
fn network_failure_maps_to_network_and_never_shows_the_token() {
    // A port nothing listens on: the connect fails. The ureq error would name the URL with the token.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let api = HttpApi::with_base(TOKEN, format!("http://127.0.0.1:{port}"));
    let err = api.get_me().unwrap_err();
    assert_eq!(err, ApiError::Network);
    assert!(!format!("{err}").contains(TOKEN));
    assert!(!format!("{err:?}").contains(TOKEN));
    assert!(!format!("{api:?}").contains(TOKEN));
}

#[test]
fn an_error_body_that_echoes_the_token_never_reaches_the_error() {
    let body = r#"{"ok":false,"error_code":500,"description":"failed at /bot123456789:TESTtokenTESTtokenTESTtokenTESTtok/getMe"}"#;
    let f = fake(&[(500, body), (401, body)]);
    let api = api_for(&f);
    for _ in 0..2 {
        let err = api.get_me().unwrap_err();
        assert!(!format!("{err}").contains(TOKEN), "Display: {err}");
        assert!(!format!("{err:?}").contains(TOKEN), "Debug: {err:?}");
    }
}

#[test]
fn every_api_error_text_is_free_of_the_token() {
    let all = [
        ApiError::Unauthorized,
        ApiError::Conflict,
        ApiError::Forbidden,
        ApiError::RateLimited { retry_after_s: 30 },
        ApiError::Network,
        ApiError::Bad { status: 500 },
        ApiError::Parse,
    ];
    for e in all {
        assert!(!format!("{e}").contains("TEST"), "{e}");
        assert!(!format!("{e:?}").contains("TEST"), "{e:?}");
    }
}

#[test]
fn debug_prints_no_fields() {
    let api = HttpApi::new(TOKEN);
    assert_eq!(format!("{api:?}"), "HttpApi(<hidden>)");
}

#[test]
fn the_base_is_telegrams_unless_a_debug_build_names_a_loopback_port() {
    let telegram = "https://api.telegram.org";
    assert_eq!(pick_base(false, Some("http://127.0.0.1:18443")), telegram, "release builds never move the base");
    assert_eq!(pick_base(true, Some("http://127.0.0.1:18443")), "http://127.0.0.1:18443");
    assert_eq!(pick_base(true, Some("http://127.0.0.1:18443/")), "http://127.0.0.1:18443");
    assert_eq!(pick_base(true, None), telegram);
    assert_eq!(pick_base(true, Some("https://evil.example")), telegram);
    assert_eq!(pick_base(true, Some("http://127.0.0.1.evil.example:80")), telegram);
    assert_eq!(pick_base(true, Some("http://localhost:18443")), telegram);
    assert_eq!(pick_base(true, Some("http://10.0.0.5:80")), telegram);
    assert_eq!(pick_base(true, Some("http://127.0.0.1:0")), telegram);
    assert_eq!(pick_base(true, Some("http://127.0.0.1:18443/x")), telegram);
    assert_eq!(pick_base(true, Some("http://127.0.0.1")), telegram, "a port is required");
}
