//! Asking the server: a tiny local HTTP server stands in for OpenCode, so the real requests, the session choice and
//! the model windows are exercised.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::json;

use super::*;

const TURN: &str = include_str!("../fixtures/api_messages_turn.json");

/// Answers `GET <path>` with the body `routes` gives for it (404 for other paths), until dropped.
struct FakeServer {
    base_url: String,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    fn start(routes: impl Fn(&str) -> Option<String> + Send + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base_url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let path = path.split('?').next().unwrap_or("/").to_string();
                let (status, body) = match routes(&path) {
                    Some(body) => ("200 OK", body),
                    None => ("404 Not Found", "{}".to_string()),
                };
                let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        FakeServer { base_url, stop }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn pane() -> PaneFacts {
    PaneFacts { cwd: PathBuf::from(r"C:\Users\you\proj"), started_at: SystemTime::now(), provider_session_id: None, home: None }
}

fn endpoint(server: &FakeServer, session: Option<&str>) -> ApiEndpoint {
    ApiEndpoint { base_url: server.base_url.clone(), session_id: session.map(str::to_string) }
}

#[test]
fn a_native_pane_reads_its_own_session_and_the_model_window() {
    let providers = json!({"providers": [{"id": "opencode-go", "key": "never-kept", "models": {"deepseek-v4.1-flash": {"limit": {"context": 128000}}}}]});
    let server = FakeServer::start(move |path| match path {
        "/session/ses_mine/message" => Some(TURN.to_string()),
        "/config/providers" => Some(providers.to_string()),
        _ => None,
    });
    let items = API.fetch(&pane(), &endpoint(&server, Some("ses_mine"))).unwrap();
    assert!(matches!(items.first(), Some(ChatItem::User { .. })));
    assert!(matches!(items.last(), Some(ChatItem::TurnEnd { .. })));
    let windows: Vec<_> = items.iter().filter_map(|item| if let ChatItem::Usage { context_window, .. } = item { Some(*context_window) } else { None }).collect();
    assert_eq!(windows, vec![Some(128_000); 5]);
}

#[test]
fn a_server_that_has_no_model_list_still_gives_the_conversation() {
    let server = FakeServer::start(|path| (path == "/session/s1/message").then(|| TURN.to_string()));
    let items = API.fetch(&pane(), &endpoint(&server, Some("s1"))).unwrap();
    assert!(items.iter().any(|item| matches!(item, ChatItem::Usage { context_window: None, .. })));
}

#[test]
fn a_tui_pane_finds_its_session_among_the_folders_sessions() {
    let started = pane().started_at.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let sessions = json!([
        {"id": "ses_old", "time": {"created": 1, "updated": started - 60_000}},
        {"id": "ses_helper", "parentID": "ses_new", "time": {"created": started, "updated": started + 9_000}},
        {"id": "ses_new", "time": {"created": started + 100, "updated": started + 5_000}},
    ]);
    let server = FakeServer::start(move |path| match path {
        "/session" => Some(sessions.to_string()),
        "/session/status" => Some("{}".to_string()),
        "/session/ses_new/message" => Some(TURN.to_string()),
        _ => None,
    });
    let items = API.fetch(&pane(), &endpoint(&server, None)).unwrap();
    assert!(!items.is_empty());
}

#[test]
fn a_tui_pane_with_no_session_of_its_own_yet_is_an_error_so_the_screen_is_read() {
    let started = pane().started_at.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let sessions = json!([{"id": "ses_old", "time": {"created": 1, "updated": started - 60_000}}]);
    let server = FakeServer::start(move |path| (path == "/session").then(|| sessions.to_string()));
    let error = API.fetch(&pane(), &endpoint(&server, None)).unwrap_err();
    assert!(error.contains("no session yet"), "{error}");
}

#[test]
fn the_session_that_is_working_wins_over_a_newer_one_of_another_pane() {
    let started = pane().started_at.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let sessions = json!([
        {"id": "ses_other", "time": {"created": started, "updated": started + 9_000}},
        {"id": "ses_mine", "time": {"created": started, "updated": started + 5_000}},
    ]);
    let server = FakeServer::start(move |path| match path {
        "/session" => Some(sessions.to_string()),
        "/session/status" => Some(json!({"ses_mine": {"type": "busy"}}).to_string()),
        "/session/ses_mine/message" => Some(TURN.to_string()),
        _ => None,
    });
    assert!(API.fetch(&pane(), &endpoint(&server, None)).is_ok());
}

#[test]
fn a_server_that_is_not_there_is_an_error() {
    let dead = {
        let server = FakeServer::start(|_| None);
        server.base_url.clone()
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    let endpoint = ApiEndpoint { base_url: dead, session_id: Some("s".into()) };
    assert!(API.fetch(&pane(), &endpoint).is_err());
}

#[test]
fn the_tui_is_told_a_port_unless_the_person_chose_one() {
    let args = prepare("pane-chat-api-1", &[]);
    assert_eq!(args[0], "--port");
    let port: u16 = args[1].parse().unwrap();
    assert_eq!(ports().lock().get("pane-chat-api-1"), Some(&port));
    assert_eq!(&args[2..], ["--hostname", "127.0.0.1"]);

    for chosen in [vec!["--port".to_string(), "4000".to_string()], vec!["--hostname=0.0.0.0".to_string()]] {
        assert!(prepare("pane-chat-api-1", &chosen).is_empty());
        assert!(ports().lock().get("pane-chat-api-1").is_none(), "an old port is forgotten");
    }
}

#[test]
fn only_the_context_size_is_kept_of_the_model_list() {
    let table = window_table(&json!({"providers": [
        {"id": "a", "models": {"m1": {"limit": {"context": 1000}}, "m2": {"limit": {"context": 0}}, "m3": {}}},
        {"id": "b", "options": {"apiKey": "secret"}, "models": {"m1": {"limit": {"context": 2000}}}},
        {"models": {"x": {"limit": {"context": 5}}}},
    ]}));
    assert_eq!(table.len(), 2);
    assert_eq!(table.get("a/m1"), Some(&1000));
    assert_eq!(table.get("b/m1"), Some(&2000));
}

// ---- two OpenCode panes in one folder (safety net) ----
//
// Every OpenCode pane serves its own API, but the folder's sessions are shared: each server lists the sessions of
// every pane in the folder. A pane must not show the conversation of the pane next to it.

fn started_ms() -> u64 {
    pane().started_at.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// A server for one pane: the folder's session list (the same for every pane), what this pane's server works on, and
/// the messages of the sessions it can read (`/session/<id>/message`).
fn pane_server(list: serde_json::Value, busy: serde_json::Value, readable: &'static [&'static str]) -> FakeServer {
    FakeServer::start(move |path| match path {
        "/session" => Some(list.to_string()),
        "/session/status" => Some(busy.to_string()),
        other => readable.iter().any(|id| other == format!("/session/{id}/message")).then(|| TURN.to_string()),
    })
}

/// Registers `server` as the API of the TUI pane `pane_id`; the pane's entry goes when the guard drops.
struct Served(&'static str);

impl Drop for Served {
    fn drop(&mut self) {
        ports().lock().remove(self.0);
    }
}

fn serve_pane(pane_id: &'static str, server: &FakeServer) -> Served {
    let port: u16 = server.base_url.rsplit(':').next().unwrap().parse().unwrap();
    ports().lock().insert(pane_id.to_string(), port);
    Served(pane_id)
}

#[test]
fn a_pane_that_has_not_sent_anything_does_not_show_the_session_a_working_pane_next_to_it_holds() {
    let started = started_ms();
    let list = json!([{"id": "ses_b1", "time": {"created": started + 100, "updated": started + 9_000}}]);
    // B works on ses_b1; A's server knows no work at all. Both list the same shared folder.
    let server_b = pane_server(list.clone(), json!({"ses_b1": {"type": "busy"}}), &["ses_b1"]);
    let server_a = pane_server(list, json!({}), &["ses_b1"]);
    let _server_a = serve_pane("pane-safety-oc-1a", &server_a);
    let _server_b = serve_pane("pane-safety-oc-1b", &server_b);

    let error = API.fetch(&pane(), &endpoint(&server_a, None)).unwrap_err();
    assert!(error.contains("no session yet"), "A has no session of its own: its chat falls back to the screen. {error}");
    assert!(API.fetch(&pane(), &endpoint(&server_b, None)).is_ok(), "B reads the session its server works on");
}

#[test]
fn two_panes_with_a_session_each_read_their_own_whoever_touched_theirs_last() {
    let started = started_ms();
    let list = json!([
        {"id": "ses_b2", "time": {"created": started + 100, "updated": started + 9_000}},
        {"id": "ses_a2", "time": {"created": started + 200, "updated": started + 3_000}},
    ]);
    // A's server can read only ses_a2 and B's only ses_b2: whichever session a pane picks, the other fails to load.
    let server_a = pane_server(list.clone(), json!({}), &["ses_a2"]);
    let server_b = pane_server(list, json!({"ses_b2": {"type": "busy"}}), &["ses_b2"]);
    let _server_a = serve_pane("pane-safety-oc-2a", &server_a);
    let _server_b = serve_pane("pane-safety-oc-2b", &server_b);

    assert!(API.fetch(&pane(), &endpoint(&server_a, None)).is_ok(), "A (idle, older session) reads ses_a2, not B's newer one");
    assert!(API.fetch(&pane(), &endpoint(&server_b, None)).is_ok(), "B reads ses_b2");
    // A again, a moment later: still its own.
    assert!(API.fetch(&pane(), &endpoint(&server_a, None)).is_ok());
}

#[test]
fn the_session_another_pane_showed_before_is_not_taken_by_a_pane_with_none() {
    let started = started_ms();
    let list = json!([{"id": "ses_b3", "time": {"created": started + 100, "updated": started + 4_000}}]);
    let server_a = pane_server(list.clone(), json!({}), &["ses_b3"]);
    let server_b = pane_server(list, json!({}), &["ses_b3"]);
    let _server_a = serve_pane("pane-safety-oc-3a", &server_a);
    let _server_b = serve_pane("pane-safety-oc-3b", &server_b);
    // B is idle but showed ses_b3 before (its chat was open): that session is B's.
    assert!(API.fetch(&pane(), &endpoint(&server_b, None)).is_ok());
    let error = API.fetch(&pane(), &endpoint(&server_a, None)).unwrap_err();
    assert!(error.contains("no session yet"), "{error}");
}
