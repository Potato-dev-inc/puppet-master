//! The control client: the argv each action becomes (the shapes the guest allows), the bearer token on
//! every request, and the replies, against a tiny fake service on a loopback port picked by the OS.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread::JoinHandle;

use base64::Engine;
use serde_json::{json, Value};

use super::control::*;
use super::secrets::ControlToken;
use super::FakeSandbox;

pub(super) struct Seen {
    pub request_line: String,
    pub authorization: Option<String>,
    pub body: Value,
}

/// Answers exactly one request with `status` and `body`, then reports what it saw.
pub(super) fn serve(status: u16, body: String) -> (u16, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        let (head_end, length) = loop {
            let n = stream.read(&mut buf).unwrap();
            raw.extend_from_slice(&buf[..n]);
            if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..at]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                    .unwrap_or(0);
                break (at + 4, length);
            }
            assert!(n > 0, "the client hung up before the request ended");
        };
        while raw.len() < head_end + length {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            raw.extend_from_slice(&buf[..n]);
        }
        let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
        let request_line = head.lines().next().unwrap().to_string();
        let authorization = head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("authorization").then(|| v.trim().to_string())
        });
        let body_json = serde_json::from_slice(&raw[head_end..head_end + length]).unwrap_or(Value::Null);
        let reply = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(reply.as_bytes()).unwrap();
        Seen { request_line, authorization, body: body_json }
    });
    (port, handle)
}

fn client(port: u16) -> (ControlClient, Arc<ControlToken>) {
    let token = Arc::new(ControlToken::generate());
    (ControlClient::new(port, token.clone()), token)
}

fn png_b64() -> String {
    base64::engine::general_purpose::STANDARD.encode(FakeSandbox::picture().png)
}

pub(super) fn observation_json() -> Value {
    json!({
        "image": png_b64(),
        "mimeType": "image/png",
        "width": 9999,
        "height": 9999,
        crate::potato::wire_names::GUEST_POINTER_FIELD: { "x": 12, "y": 34 },
        "activeWindow": { "id": "123", "title": "Chrome\u{7} - Shop" }
    })
}

fn argv(parts: &[&str]) -> Step {
    Step::Argv(parts.iter().map(|p| p.to_string()).collect())
}

#[test]
fn every_action_becomes_a_shape_the_guest_allows() {
    let d = "DISPLAY=:99";
    assert_eq!(steps_for(&Action::Move { x: 5, y: 6 }).unwrap(), vec![argv(&["env", d, "xdotool", "mousemove", "--", "5", "6"])]);
    assert_eq!(
        steps_for(&Action::Click { x: 5, y: 6, button: Button::Left }).unwrap(),
        vec![argv(&["env", d, "xdotool", "mousemove", "--", "5", "6", "click", "1"])]
    );
    assert_eq!(
        steps_for(&Action::Click { x: 5, y: 6, button: Button::Right }).unwrap(),
        vec![argv(&["env", d, "xdotool", "mousemove", "--", "5", "6", "click", "3"])]
    );
    assert_eq!(
        steps_for(&Action::DoubleClick { x: 1, y: 2 }).unwrap(),
        vec![
            argv(&["env", d, "xdotool", "mousemove", "--", "1", "2", "click", "1"]),
            Step::Wait(60),
            argv(&["env", d, "xdotool", "mousemove", "--", "1", "2", "click", "1"]),
        ]
    );
    assert_eq!(
        steps_for(&Action::Drag { from: (1, 2), to: (30, 40) }).unwrap(),
        vec![
            argv(&["env", d, "xdotool", "mousemove", "--", "1", "2", "mousedown", "1"]),
            argv(&["env", d, "xdotool", "mousemove", "--", "30", "40"]),
            argv(&["env", d, "xdotool", "mouseup", "1"]),
        ]
    );
    assert_eq!(
        steps_for(&Action::Scroll { x: 7, y: 8, direction: Direction::Down, amount: 3 }).unwrap(),
        vec![argv(&["env", d, "xdotool", "mousemove", "--", "7", "8", "click", "--repeat", "3", "5"])]
    );
    assert_eq!(
        steps_for(&Action::Scroll { x: 7, y: 8, direction: Direction::Up, amount: 500 }).unwrap(),
        vec![argv(&["env", d, "xdotool", "mousemove", "--", "7", "8", "click", "--repeat", "20", "4"])],
        "the wheel count is held to the guest's limit"
    );
    assert_eq!(
        steps_for(&Action::Scroll { x: 0, y: 0, direction: Direction::Left, amount: 0 }).unwrap(),
        vec![argv(&["env", d, "xdotool", "mousemove", "--", "0", "0", "click", "--repeat", "1", "6"])]
    );
    assert_eq!(
        steps_for(&Action::Type { text: "-rf hello".into() }).unwrap(),
        vec![argv(&["env", d, "xdotool", "type", "--clearmodifiers", "--", "-rf hello"])]
    );
    assert_eq!(
        steps_for(&Action::Key { combo: "ctrl+Return".into() }).unwrap(),
        vec![argv(&["env", d, "xdotool", "key", "--clearmodifiers", "ctrl+Return"])]
    );
    assert_eq!(steps_for(&Action::Wait { ms: 250 }).unwrap(), vec![Step::Wait(250)]);
    assert_eq!(steps_for(&Action::Wait { ms: 999_999 }).unwrap(), vec![Step::Wait(5000)]);
    assert_eq!(
        steps_for(&Action::Launch { app: "chrome".into(), arg: None }).unwrap(),
        vec![argv(&["env", d, "pm-sbx-focus-or-launch", "chrome"])]
    );
    assert_eq!(
        steps_for(&Action::Launch { app: "chrome".into(), arg: Some("https://example.com".into()) }).unwrap(),
        vec![argv(&["env", d, "pm-sbx-focus-or-launch", "chrome", "https://example.com"])]
    );
}

#[test]
fn an_action_the_guest_would_refuse_is_not_sent() {
    let bad = [
        Action::Move { x: -1, y: 0 },
        Action::Click { x: 0, y: 20_000, button: Button::Left },
        Action::Drag { from: (0, 0), to: (-5, 5) },
        Action::Type { text: String::new() },
        Action::Type { text: "a\0b".into() },
        Action::Type { text: "x".repeat(2001) },
        Action::Key { combo: String::new() },
        Action::Key { combo: "ctrl c".into() },
        Action::Key { combo: "ctrl+;".into() },
        Action::Key { combo: "--help".into() },
        Action::Launch { app: "Chrome".into(), arg: None },
        Action::Launch { app: "chrome; rm".into(), arg: None },
        Action::Launch { app: String::new(), arg: None },
        Action::Launch { app: "chrome".into(), arg: Some("--no-sandbox".into()) },
        Action::Launch { app: "chrome".into(), arg: Some(String::new()) },
    ];
    for action in bad {
        assert!(matches!(steps_for(&action), Err(ControlError::Invalid(_))), "{action:?}");
    }
    // 65 steps are too many for one request; 64 are fine
    assert!(plan(&vec![Action::Wait { ms: 1 }; 64]).is_ok());
    assert!(matches!(plan(&vec![Action::Wait { ms: 1 }; 65]), Err(ControlError::Invalid(_))));
}

#[test]
fn act_sends_the_token_and_the_steps_and_reads_the_picture() {
    let reply = json!({ "completed": 2, "observation": observation_json() }).to_string();
    let (port, seen) = serve(200, reply);
    let (client, token) = client(port);
    let result = client
        .act(&[Action::Click { x: 10, y: 20, button: Button::Left }, Action::Key { combo: "Return".into() }], 300, true)
        .unwrap();
    let seen = seen.join().unwrap();
    assert_eq!(seen.request_line, "POST /v1/desktop HTTP/1.1");
    assert_eq!(seen.authorization.as_deref(), Some(format!("Bearer {}", token.expose()).as_str()));
    assert_eq!(seen.body["display"], ":99");
    assert_eq!(seen.body["settleMs"], 300);
    assert_eq!(seen.body["observe"], true);
    assert_eq!(seen.body["steps"][0]["argv"][3], "mousemove");
    assert_eq!(seen.body["steps"][1]["argv"][3], "key");
    assert_eq!(result.completed, 2);
    let obs = result.observation.unwrap();
    assert_eq!((obs.width, obs.height), (160, 100), "the size comes from the picture, not from the JSON");
    assert_eq!(obs.pointer, Some((12, 34)));
    assert_eq!(obs.active_window_title.as_deref(), Some("Chrome - Shop"));
    assert_eq!(&obs.png[..4], b"\x89PNG");
}

#[test]
fn a_screenshot_is_a_post_to_the_screenshot_route() {
    let (port, seen) = serve(200, json!({ "observation": observation_json() }).to_string());
    let (client, _) = client(port);
    let obs = client.screenshot().unwrap();
    assert_eq!(obs.width, 160);
    assert_eq!(seen.join().unwrap().request_line, "POST /v1/screenshot HTTP/1.1");
}

#[test]
fn act_without_observation_returns_none() {
    let (port, _seen) = serve(200, json!({ "completed": 1 }).to_string());
    let (client, _) = client(port);
    let result = client.act(&[Action::Wait { ms: 1 }], 0, false).unwrap();
    assert_eq!(result.completed, 1);
    assert!(result.observation.is_none());
}

#[test]
fn replies_map_to_errors() {
    for (status, body, check) in [
        (401, json!({ "error": "unauthorized" }).to_string(), ControlError::Unauthorized),
        (400, json!({ "error": "unsupported computer action" }).to_string(), ControlError::Rejected("unsupported computer action".into())),
        (500, json!({ "error": "computer action failed" }).to_string(), ControlError::Failed("computer action failed".into())),
        (404, "not json".to_string(), ControlError::Failed(String::new())),
    ] {
        let (port, _seen) = serve(status, body);
        let (client, _) = client(port);
        assert_eq!(client.screenshot().unwrap_err(), check, "status {status}");
    }
}

#[test]
fn a_reply_that_is_not_the_protocol_is_a_bad_reply() {
    for body in [
        "not json".to_string(),
        json!({}).to_string(),
        json!({ "observation": {} }).to_string(),
        json!({ "observation": { "image": "!!!not base64!!!" } }).to_string(),
        json!({ "observation": { "image": base64::engine::general_purpose::STANDARD.encode(b"this is not a png at all, just text") } }).to_string(),
    ] {
        let (port, _seen) = serve(200, body.clone());
        let (client, _) = client(port);
        assert!(matches!(client.screenshot(), Err(ControlError::BadReply(_))), "{body}");
    }
}

#[test]
fn a_service_that_is_not_there_is_unreachable() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let (client, _) = client(port);
    let error = client.screenshot().unwrap_err();
    assert!(matches!(error, ControlError::Unreachable(_)), "{error:?}");
    assert_eq!(error.code(), "SANDBOX_UNREACHABLE");
}

#[test]
fn an_invalid_action_sends_nothing() {
    // No server is listening; an invalid action must fail before any connection is tried.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let (client, _) = client(port);
    let error = client.act(&[Action::Move { x: -3, y: 0 }], 0, true).unwrap_err();
    assert!(matches!(error, ControlError::Invalid(_)));
}

#[test]
fn the_token_is_random_hex_and_never_printed() {
    let a = ControlToken::generate();
    let b = ControlToken::generate();
    assert_eq!(a.expose().len(), 64);
    assert!(a.expose().bytes().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a.expose(), b.expose());
    let shown = format!("{a:?}");
    assert!(!shown.contains(a.expose()), "{shown}");
    let b = Arc::new(b);
    let client = ControlClient::new(DEFAULT_PORT, b.clone());
    assert!(!format!("{client:?}").contains(b.expose()), "the client's Debug hides the token");
}

#[test]
fn errors_have_wire_words_and_plain_sentences() {
    let all = [
        ControlError::Unreachable("x".into()),
        ControlError::Unauthorized,
        ControlError::Rejected("why".into()),
        ControlError::Failed("why".into()),
        ControlError::BadReply("x".into()),
        ControlError::Invalid("why"),
    ];
    for error in all {
        assert!(error.code().starts_with("SANDBOX_"));
        assert!(error.message().ends_with('.'));
    }
}
