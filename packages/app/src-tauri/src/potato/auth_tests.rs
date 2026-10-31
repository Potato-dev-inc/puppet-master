//! Who may call `/potato/brain/*` and what a brain can never do (spec 2.1, 6.6, 7). Two layers:
//! the pure decision over a fake TCP owner and a fake process table, and the real bridge gate on a real
//! socket (the same `read_request` + `gate` the app runs; only the routes behind it are the brain's own).
//!
//! The 404 cases never say why, and the token on any other route is 403 `POTATO_SCOPE`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use super::auth::*;
use super::service_tests::Rig;
use crate::companion::peer::{Peer, TcpOwner};
use crate::pty::agent_proc::ProcEntry;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BRAIN_PID: u32 = 4_000;

struct FakeOwner {
    pid: Option<u32>,
    asked: AtomicUsize,
}

impl FakeOwner {
    fn new(pid: Option<u32>) -> FakeOwner {
        FakeOwner { pid, asked: AtomicUsize::new(0) }
    }
}

impl TcpOwner for FakeOwner {
    fn owner_pid(&self, _peer: &Peer) -> Option<u32> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.pid
    }
}

fn peer() -> Option<Peer> {
    Some(Peer { local: "127.0.0.1:17999".parse().unwrap(), remote: "127.0.0.1:50123".parse().unwrap() })
}

fn proc(pid: u32, parent: u32) -> ProcEntry {
    ProcEntry { pid, parent, image: "x.exe".into() }
}

/// brain node 4000 (parent 1) -> a child 4001 -> a grandchild 4002; an unrelated 5000 (parent 1).
fn table() -> Option<Vec<ProcEntry>> {
    Some(vec![proc(1, 0), proc(BRAIN_PID, 1), proc(4_001, BRAIN_PID), proc(4_002, 4_001), proc(5_000, 1)])
}

fn brains() -> Brains {
    let b = Brains::new();
    b.set_enabled(true);
    b.register("nimbus", TOKEN);
    b.set_pid("nimbus", BRAIN_PID);
    b
}

fn head(method: &str, path: &str, extra: &[String]) -> String {
    let mut text = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:17999\r\n");
    for line in extra {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text
}

fn token_line() -> String {
    format!("X-PM-Potato-Token: {TOKEN}")
}

fn run(b: &Brains, owner: &FakeOwner, method: &str, path: &str, extra: &[String], loopback: bool) -> Verdict {
    let headers = head(method, path, extra);
    let (path, _) = path.split_once('?').unwrap_or((path, ""));
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let procs = table;
    decide(b, &Request { method, segments: &segments, headers: &headers, peer_loopback: loopback, peer: peer() }, &Os { owner, procs: &procs, require_owner: true })
}

fn served(v: Verdict) -> bool {
    matches!(v, Verdict::Brain(ref bot) if bot == "nimbus")
}

fn status(v: &Verdict) -> Option<u16> {
    match v {
        Verdict::Refuse(status, _) => Some(*status),
        _ => None,
    }
}

const INBOX: &str = "/potato/brain/inbox?cursor=0&wait_ms=0";

#[test]
fn a_brain_with_its_token_on_loopback_from_its_own_process_is_served() {
    let b = brains();
    assert!(served(run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", INBOX, &[token_line()], true)));
    assert!(served(run(&b, &FakeOwner::new(Some(BRAIN_PID)), "POST", "/potato/brain/events", &[token_line()], true)));
    // under /bridge too (the router would have stripped it): the same decision
    assert!(served(run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", "/bridge/potato/brain/inbox", &[token_line()], true)));
}

#[test]
fn without_the_token_or_with_a_wrong_one_the_route_is_a_404() {
    let b = brains();
    let own = FakeOwner::new(Some(BRAIN_PID));
    let wrong = [
        vec![],
        vec!["X-PM-Potato-Token: ".to_string()],
        vec![format!("X-PM-Potato-Token: {}", TOKEN.to_uppercase())],
        vec![format!("X-PM-Potato-Token: {}0", TOKEN)],
        vec![format!("X-PM-Potato-Token: {}", &TOKEN[..TOKEN.len() - 1])],
        vec![format!("X-PM-Potato-Token: {}", TOKEN.replacen('0', "1", 1))],
        vec![format!("X-PM-Potato-Token: {}e", &TOKEN[..63])],
        vec![format!("Authorization: Bearer {TOKEN}")],
        vec![format!("X-PM-Hook-Token: {TOKEN}")],
    ];
    for extra in wrong {
        let v = run(&b, &own, "GET", INBOX, &extra, true);
        assert_eq!(status(&v), Some(404), "{extra:?}");
    }
}

#[test]
fn a_forwarder_header_a_foreign_host_or_any_origin_is_a_404_even_with_the_right_token() {
    let b = brains();
    let own = FakeOwner::new(Some(BRAIN_PID));
    let token = token_line();
    let forwarded = [
        "X-PM-Proxied: 1", "Cf-Connecting-Ip: 1.2.3.4", "Cf-Connecting-Ipv6: ::1", "Cf-Ray: abc", "Cf-Visitor: {}", "Cf-Ipcountry: US", "Cdn-Loop: x",
        "True-Client-Ip: 1.2.3.4", "X-Forwarded-For: 1.2.3.4", "X-Forwarded-Host: evil.example", "X-Forwarded-Proto: https", "X-Forwarded-Port: 443", "X-Real-Ip: 1.2.3.4",
        "Forwarded: for=1.2.3.4",
    ];
    for line in forwarded {
        assert_eq!(status(&run(&b, &own, "GET", INBOX, &[token.clone(), line.to_string()], true)), Some(404), "{line}");
    }
    // a foreign Host (a tunnel or a rebinding page): the head helper writes its own Host, so build by hand
    let headers = format!("GET {INBOX} HTTP/1.1\r\nHost: tunnel.example.com\r\n{token}\r\n");
    let segments = ["potato", "brain", "inbox"];
    let v = decide(&b, &Request { method: "GET", segments: &segments, headers: &headers, peer_loopback: true, peer: peer() }, &Os { owner: &own, procs: &table, require_owner: true });
    assert_eq!(status(&v), Some(404), "foreign Host");
    // any Origin at all, including the app's own and a page on localhost (a web page can reach 127.0.0.1)
    for origin in ["tauri://localhost", "http://tauri.localhost", "http://localhost:1420", "http://127.0.0.1:17999", "https://evil.example", "null", "file://"] {
        assert_eq!(status(&run(&b, &own, "GET", INBOX, &[token.clone(), format!("Origin: {origin}")], true)), Some(404), "Origin: {origin}");
    }
}

#[test]
fn a_peer_that_is_not_loopback_is_a_404_and_a_phone_bearer_never_reaches_the_route() {
    let b = brains();
    let own = FakeOwner::new(Some(BRAIN_PID));
    assert_eq!(status(&run(&b, &own, "GET", INBOX, &[token_line()], false)), Some(404));
    // a paired phone: its bearer, with or without the brain token, from outside
    assert_eq!(status(&run(&b, &own, "GET", INBOX, &["Authorization: Bearer some-phone-token".into()], false)), Some(404));
    assert_eq!(status(&run(&b, &own, "GET", INBOX, &["Authorization: Bearer some-phone-token".into()], true)), Some(404));
    assert_eq!(status(&run(&b, &own, "GET", INBOX, &["Authorization: Bearer some-phone-token".into(), token_line()], false)), Some(404));
    // through the phone's proxy
    assert_eq!(status(&run(&b, &own, "GET", INBOX, &["Authorization: Bearer some-phone-token".into(), "X-PM-Proxied: 1".into()], true)), Some(404));
}

#[test]
fn while_potato_bot_is_off_the_routes_are_a_404() {
    let b = brains();
    b.set_enabled(false);
    assert_eq!(status(&run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", INBOX, &[token_line()], true)), Some(404));
    b.set_enabled(true);
    assert!(served(run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", INBOX, &[token_line()], true)));
}

#[test]
fn a_token_from_the_wrong_process_is_a_404() {
    let b = brains();
    // another program that learned the token (it cannot: it is in no environment but the brain's) is not the brain
    for pid in [5_000u32, 1, 4_003, 0] {
        let own = FakeOwner::new(Some(pid));
        assert_eq!(status(&run(&b, &own, "GET", INBOX, &[token_line()], true)), Some(404), "pid {pid}");
    }
    // the OS cannot say who owns the socket: not served
    assert_eq!(status(&run(&b, &FakeOwner::new(None), "GET", INBOX, &[token_line()], true)), Some(404));
    // a start in progress, with no pid yet: not served
    let b = Brains::new();
    b.set_enabled(true);
    b.register("nimbus", TOKEN);
    assert_eq!(status(&run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", INBOX, &[token_line()], true)), Some(404));
}

#[test]
fn a_launcher_shim_that_starts_the_real_process_as_a_child_still_works() {
    let b = brains();
    // the socket belongs to a child or grandchild of the process we started
    assert!(served(run(&b, &FakeOwner::new(Some(4_001)), "GET", INBOX, &[token_line()], true)));
    assert!(served(run(&b, &FakeOwner::new(Some(4_002)), "GET", INBOX, &[token_line()], true)));
}

#[test]
fn where_the_os_cannot_name_the_owner_the_token_and_loopback_stand_alone() {
    let b = brains();
    let headers = head("GET", INBOX, &[token_line()]);
    let segments = ["potato", "brain", "inbox"];
    let none = FakeOwner::new(None);
    let v = decide(&b, &Request { method: "GET", segments: &segments, headers: &headers, peer_loopback: true, peer: peer() }, &Os { owner: &none, procs: &table, require_owner: false });
    assert!(served(v));
}

#[test]
fn each_bot_has_its_own_token_and_a_new_token_replaces_the_old_one() {
    let b = Brains::new();
    b.set_enabled(true);
    b.register("nimbus", "token-for-nimbus-0000000000000000");
    b.register("mochi", "token-for-mochi-00000000000000000");
    b.set_pid("nimbus", 100);
    b.set_pid("mochi", 200);
    let own = |pid| FakeOwner::new(Some(pid));
    let get = |token: &str, pid: u32| {
        let v = run(&b, &own(pid), "GET", INBOX, &[format!("X-PM-Potato-Token: {token}")], true);
        match v {
            Verdict::Brain(bot) => Some(bot),
            _ => None,
        }
    };
    assert_eq!(get("token-for-nimbus-0000000000000000", 100).as_deref(), Some("nimbus"));
    assert_eq!(get("token-for-mochi-00000000000000000", 200).as_deref(), Some("mochi"));
    // nimbus's token from mochi's process
    assert_eq!(get("token-for-nimbus-0000000000000000", 200), None);
    // a restart: the old token is dead
    b.register("nimbus", "second-token-for-nimbus-00000000");
    b.set_pid("nimbus", 101);
    assert_eq!(get("token-for-nimbus-0000000000000000", 100), None);
    assert_eq!(get("second-token-for-nimbus-00000000", 101).as_deref(), Some("nimbus"));
    b.unregister("nimbus");
    assert_eq!(get("second-token-for-nimbus-00000000", 101), None);
}

#[test]
fn a_minted_token_is_64_hex_characters_and_never_repeats() {
    let a = mint_token();
    let b = mint_token();
    assert_eq!(a.len(), 64);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[test]
fn the_token_is_compared_in_constant_time_over_hashes() {
    // The registry keeps the hash of the token, never the token, and compares hashes with `ct_eq`.
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/potato/auth.rs")).unwrap();
    let code = src.split("#[cfg(test)]").next().unwrap();
    assert!(code.contains("use subtle::ConstantTimeEq;") && code.contains(".ct_eq("), "the token is compared with subtle");
    assert!(!code.contains("token_hash == ") && !code.contains("== token") && !code.contains("token =="), "no plain comparison of a token");
    assert!(!code.contains("token: String,\n    token_hash"), "the registry holds no token");
    let b = brains();
    let shown = format!("{:?}", b.any_running());
    assert!(!shown.contains(TOKEN));
}

/// Every route the bridge serves: the tool catalog plus the routes no tool names, with ids filled in.
fn every_route() -> Vec<(String, String)> {
    let mut routes: Vec<(String, String)> = crate::tool_registry::tools()
        .iter()
        .map(|t| {
            let path = t.path.split('/').map(|s| if s.starts_with('{') || s.starts_with(':') { "x" } else { s }).collect::<Vec<_>>().join("/");
            (t.method.to_string(), path)
        })
        .collect();
    for (m, p) in [
        ("GET", "/"), ("GET", "/health"), ("GET", "/events"), ("GET", "/events/ws"), ("GET", "/events/poll"), ("GET", "/panes"), ("POST", "/panes"),
        ("POST", "/panes/x/input"), ("POST", "/panes/x/key"), ("DELETE", "/panes/x"), ("POST", "/pair"), ("GET", "/pair/me"), ("POST", "/push/act"),
        ("GET", "/agents/definitions"), ("POST", "/agents/run"), ("POST", "/agents/answer"), ("POST", "/agents/quick"), ("POST", "/hooks/event"),
        ("GET", "/bridge/panes"), ("POST", "/mcp/mode"), ("PATCH", "/settings"), ("POST", "/questions/x/answer"), ("POST", "/questions/ask"),
        ("GET", "/companions"), ("POST", "/companions/x/wake"), ("GET", "/focus"), ("GET", "/night"), ("GET", "/review-desk"), ("GET", "/team/cards"),
        ("POST", "/team/agent/notify"), ("GET", "/session/context"), ("POST", "/operations/delegate"), ("GET", "/secrets"), ("GET", "/potato/conversations/x"),
        ("POST", "/potato/send"), ("GET", "/potato"), ("POST", "/potato/brain"), ("GET", "/potato/other"),
    ] {
        routes.push((m.to_string(), p.to_string()));
    }
    routes.sort();
    routes.dedup();
    routes
}

fn brain_routes() -> Vec<(&'static str, &'static str)> {
    vec![("GET", "/potato/brain/inbox"), ("POST", "/potato/brain/events")]
}

#[test]
fn the_token_gets_403_on_every_other_route() {
    let b = brains();
    let own = FakeOwner::new(Some(BRAIN_PID));
    let routes = every_route();
    assert!(routes.len() > 60, "the route table is not vacuous: {}", routes.len());
    let mut refused = 0;
    for (method, path) in &routes {
        let v = run(&b, &own, method, path, &[token_line()], true);
        let is_brain = brain_routes().iter().any(|(m, p)| m == method && p == path);
        if is_brain {
            assert!(served(v), "{method} {path}");
            continue;
        }
        match &v {
            // a brain-looking path that is not one of the two routes is the brain's to answer with 404
            Verdict::Brain(_) if path.starts_with("/potato/brain") => {}
            Verdict::Refuse(status, body) if path.starts_with("/potato/brain") => {
                assert_eq!(*status, 404, "{method} {path} {body}");
            }
            Verdict::Refuse(403, body) => {
                assert_eq!(body["code"], "POTATO_SCOPE", "{method} {path}");
                refused += 1;
            }
            other => panic!("{method} {path}: {other:?}"),
        }
    }
    eprintln!("POTATO_SCOPE on {refused} routes");
    assert!(refused > 60);
}

#[test]
fn the_token_is_refused_by_the_bridges_own_authorization_too() {
    // Defence in depth: `authorize_caller` is what `route()`, the WebSocket, the poll and the stream call.
    for (method, path) in every_route() {
        let path = path.split_once('?').map_or(path.as_str(), |(p, _)| p);
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        for headers in [head(&method, path, &[token_line()]), head(&method, path, &[token_line(), "Authorization: Bearer x".into()])] {
            let result = crate::mobile_pairing::authorize_caller(&headers, true, &segments, &method);
            let (status, body) = result.expect_err(&format!("{method} {path}"));
            assert_eq!((status, body["code"].as_str()), (403, Some("POTATO_SCOPE")), "{method} {path}");
            // ... also from outside, where a bearer would otherwise be asked for
            let outside = crate::mobile_pairing::authorize_caller(&headers, false, &segments, &method);
            assert_eq!(outside.expect_err("outside").0, 403, "{method} {path}");
        }
    }
}

#[test]
fn a_brain_caller_has_no_caps_and_never_a_stream() {
    use crate::mobile_pairing::{viewer_for, Cap, Caller, Viewer};
    let brain = Caller::Brain { bot_id: "nimbus".into() };
    for cap in [Cap::Watch, Cap::Answer, Cap::Control] {
        assert!(!brain.may(cap), "{cap:?}");
    }
    // the stream filter treats it as a device that does not exist: it closes at once
    let viewer = viewer_for(&brain);
    assert_eq!(viewer, Viewer::Device(String::new()));
    assert_eq!(viewer.may_receive("{}"), None);
}

#[test]
fn a_token_less_request_from_the_brain_itself_is_a_403_while_it_runs() {
    let b = brains();
    // the brain's own process asking for the pane list, with no token
    let own = FakeOwner::new(Some(BRAIN_PID));
    let v = run(&b, &own, "GET", "/panes", &[], true);
    assert_eq!((status(&v), match &v { Verdict::Refuse(_, b) => b["code"].as_str(), _ => None }), (Some(403), Some("POTATO_SCOPE")));
    for (method, path) in [("GET", "/health"), ("POST", "/agents/run"), ("POST", "/questions/x/answer"), ("POST", "/panes/x/input"), ("GET", "/events")] {
        assert_eq!(status(&run(&b, &own, method, path, &[], true)), Some(403), "{method} {path}");
    }
    // a process in the brain's tree: refused for requests that change something and for card routes
    for pid in [4_001u32, 4_002] {
        let own = FakeOwner::new(Some(pid));
        assert_eq!(status(&run(&b, &own, "POST", "/agents/run", &[], true)), Some(403), "child {pid}");
        assert_eq!(status(&run(&b, &own, "POST", "/panes/x/input", &[], true)), Some(403), "child {pid}");
        assert_eq!(status(&run(&b, &own, "DELETE", "/panes/x", &[], true)), Some(403), "child {pid}");
    }
}

#[test]
fn an_unrelated_local_program_is_untouched_and_so_is_everyone_when_no_brain_runs() {
    let b = brains();
    let stranger = FakeOwner::new(Some(5_000));
    for (method, path) in [("GET", "/panes"), ("POST", "/agents/run"), ("POST", "/panes/x/input")] {
        assert_eq!(run(&b, &stranger, method, path, &[], true), Verdict::Pass, "{method} {path}");
    }
    // no brain running: nobody is asked about their socket at all
    let none = Brains::new();
    none.set_enabled(true);
    let owner = FakeOwner::new(Some(BRAIN_PID));
    assert_eq!(run(&none, &owner, "GET", "/panes", &[], true), Verdict::Pass);
    assert_eq!(run(&none, &owner, "POST", "/agents/run", &[], true), Verdict::Pass);
    assert_eq!(owner.asked.load(Ordering::SeqCst), 0, "the cost is paid only while a brain runs");
    // a phone (outside, or through a proxy) is not the local program: the bearer rules decide, as before
    assert_eq!(run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", "/panes", &["Authorization: Bearer x".into()], false), Verdict::Pass);
    assert_eq!(run(&b, &FakeOwner::new(Some(BRAIN_PID)), "GET", "/panes", &["X-PM-Proxied: 1".into()], true), Verdict::Pass);
    // the OS cannot say who is calling: unchanged behaviour
    assert_eq!(run(&b, &FakeOwner::new(None), "GET", "/panes", &[], true), Verdict::Pass);
}

#[test]
fn a_descendant_is_only_looked_up_for_requests_that_change_something() {
    // The tiering of spec 2.1 point 3: the process table costs milliseconds, so a plain read from a
    // process deeper in the brain's tree is not checked, while the brain itself always is.
    let b = brains();
    let child = FakeOwner::new(Some(4_001));
    assert_eq!(run(&b, &child, "GET", "/panes", &[], true), Verdict::Pass);
    assert_eq!(status(&run(&b, &child, "POST", "/agents/run", &[], true)), Some(403));
}

// ---- the real bridge gate on a real socket ---------------------------------------------------

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

fn request(method: &str, path: &str, extra: &[String], body: &str) -> String {
    let mut text = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    for line in extra {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    text
}

fn json_of(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or(Value::Null)
}

/// Starts the brain's session through a send, and makes this process its process: the service registered a
/// token of its own for the session, which is the one the brain would hold.
fn start_session(rig: &Rig) -> String {
    rig.svc.send("nimbus", "hello").unwrap();
    let token = rig.launcher.spawned.lock()[0].token.clone();
    rig.brains.set_pid("nimbus", std::process::id());
    token
}

/// A listener with the real gate, a service of its own and a brain registered as THIS process: the test
/// is the brain, so the OS table names this process as the socket owner (on Windows).
fn listener(name: &str) -> (SocketAddr, Rig, String) {
    let rig = Rig::new(name);
    let token = mint_token();
    rig.brains.register("nimbus", &token);
    rig.brains.set_pid("nimbus", std::process::id());
    let addr = crate::bridge::spawn_potato_listener(rig.brains.clone(), rig.svc.clone());
    (addr, rig, token)
}

#[test]
fn the_real_gate_answers_the_brain_its_routes_and_nothing_else() {
    let (addr, rig, token) = listener("gate-routes");
    let header = format!("X-PM-Potato-Token: {token}");
    // the long poll with nothing to say, and the post of events
    let (status, _, body) = send(addr, &request("GET", "/potato/brain/inbox?cursor=0&wait_ms=0", &[header.clone()], ""));
    assert_eq!(status, 200, "no session yet: the gate lets the brain in, and the inbox is empty: {body}");
    assert_eq!(json_of(&body)["items"], json!([]));
    // start a session through a send, then fetch
    let token = start_session(&rig);
    let header = format!("X-PM-Potato-Token: {token}");
    let (status, _, body) = send(addr, &request("GET", "/potato/brain/inbox?cursor=0&wait_ms=0", &[header.clone()], ""));
    assert_eq!(status, 200, "{body}");
    let inbox = json_of(&body);
    assert_eq!(inbox["v"], 1);
    assert_eq!(inbox["items"][0]["type"], "send");
    let events = json!({"v":1,"events":[{"v":1,"type":"hello","protocol":1,"version":"t","provider":"stub","packs":["fish"],"surfaces":["app"]},{"v":1,"type":"reply","conversationId":"app:dm:nimbus","inReplyTo":"x","segments":["hi from the brain"]}]}).to_string();
    let (status, _, body) = send(addr, &request("POST", "/potato/brain/events", &[header.clone(), "Content-Type: application/json".into()], &events));
    assert_eq!(status, 200, "{body}");
    assert!(rig.thread().iter().any(|m| m.text == "hi from the brain"));
    // sizes and shapes
    let big = format!("{{\"v\":1,\"events\":[],\"pad\":\"{}\"}}", "x".repeat(70 * 1024));
    assert_eq!(send(addr, &request("POST", "/potato/brain/events", &[header.clone()], &big)).0, 413);
    assert_eq!(send(addr, &request("POST", "/potato/brain/events", &[header.clone()], "not json")).0, 400);
    let many = json!({"v":1,"events":vec![json!({"v":1,"type":"typing","conversationId":"app:dm:nimbus","on":true}); 33]}).to_string();
    assert_eq!(send(addr, &request("POST", "/potato/brain/events", &[header.clone()], &many)).0, 400);
    // another path or method under /potato/brain is a 404
    assert_eq!(send(addr, &request("GET", "/potato/brain/other", &[header.clone()], "")).0, 404);
    assert_eq!(send(addr, &request("DELETE", "/potato/brain/inbox", &[header.clone()], "")).0, 404);
    assert_eq!(send(addr, &request("POST", "/potato/brain/inbox", &[header.clone()], "{}")).0, 404);
}

#[test]
fn the_real_gate_gives_the_token_403_on_every_route_the_bridge_has() {
    let (addr, _rig, token) = listener("gate-403");
    let routes = every_route();
    let mut count = 0;
    for (method, path) in &routes {
        if path.starts_with("/potato/brain") {
            continue;
        }
        let req = request(method, path, &[format!("X-PM-Potato-Token: {token}")], "{}");
        let (status, _, body) = send(addr, &req);
        assert_eq!((status, json_of(&body)["code"].as_str().map(str::to_string)), (403, Some("POTATO_SCOPE".to_string())), "{method} {path}");
        // ... under /bridge as well (a proxy that forwards the prefix)
        let (status, _, _) = send(addr, &request(method, &format!("/bridge{path}"), &[format!("X-PM-Potato-Token: {token}")], "{}"));
        assert_eq!(status, 403, "/bridge{path}");
        count += 1;
    }
    eprintln!("Caller::Brain: 403 on {count} routes through the real gate");
    assert!(count > 60);
}

#[test]
fn the_real_gate_gives_a_404_to_everyone_who_is_not_the_brain() {
    let (addr, rig, token) = listener("gate-404");
    let token = start_session(&rig);
    let path = "/potato/brain/inbox?cursor=0&wait_ms=0";
    let good = format!("X-PM-Potato-Token: {token}");
    for (label, extra) in [
        ("no token", vec![]),
        ("wrong token", vec!["X-PM-Potato-Token: nope".to_string()]),
        ("phone bearer", vec!["Authorization: Bearer some-phone-token".to_string()]),
        ("forwarder", vec![good.clone(), "X-Forwarded-For: 1.2.3.4".into()]),
        ("tunnel header", vec![good.clone(), "Cf-Connecting-Ip: 1.2.3.4".into()]),
        ("proxied", vec![good.clone(), "X-PM-Proxied: 1".into()]),
        ("an app origin", vec![good.clone(), "Origin: tauri://localhost".into()]),
        ("a web page", vec![good.clone(), "Origin: https://evil.example".into()]),
    ] {
        let (status, head, _) = send(addr, &request("GET", path, &extra, ""));
        assert_eq!(status, 404, "{label}");
        // brain routes send no CORS headers, not even the 404 for an app origin
        assert!(!head.to_ascii_lowercase().contains("access-control"), "{label}: {head}");
    }
    // a foreign Host
    let foreign = format!("GET {path} HTTP/1.1\r\nHost: tunnel.example.com\r\n{good}\r\n\r\n");
    assert_eq!(send(addr, &foreign).0, 404);
    // the preflight of a browser: a 404 too, with no CORS headers
    let (status, head, _) = send(addr, &request("OPTIONS", path, &["Origin: tauri://localhost".into(), "Access-Control-Request-Method: GET".into()], ""));
    assert_eq!(status, 404);
    assert!(!head.to_ascii_lowercase().contains("access-control"), "{head}");
    // switched off: a 404 for the right token as well
    rig.brains.set_enabled(false);
    assert_eq!(send(addr, &request("GET", path, &[good.clone()], "")).0, 404);
    // served responses carry no CORS headers either
    rig.brains.set_enabled(true);
    let (status, head, _) = send(addr, &request("GET", path, &[good], ""));
    if cfg!(windows) {
        assert_eq!(status, 200);
    }
    assert!(!head.to_ascii_lowercase().contains("access-control"), "{head}");
}

#[cfg(windows)]
#[test]
fn the_real_gate_refuses_a_token_from_a_process_that_is_not_the_brain() {
    // The OS table names the test process as the owner; the brain is registered as another pid.
    let rig = Rig::new("gate-owner");
    let token = mint_token();
    rig.brains.register("nimbus", &token);
    rig.brains.set_pid("nimbus", std::process::id().wrapping_add(7));
    let addr = crate::bridge::spawn_potato_listener(rig.brains.clone(), rig.svc.clone());
    let (status, _, _) = send(addr, &request("GET", "/potato/brain/inbox?cursor=0&wait_ms=0", &[format!("X-PM-Potato-Token: {token}")], ""));
    assert_eq!(status, 404, "right token, wrong process");
    // ... and the same process, once it is the brain's pid, is served
    rig.brains.set_pid("nimbus", std::process::id());
    let (status, _, _) = send(addr, &request("GET", "/potato/brain/inbox?cursor=0&wait_ms=0", &[format!("X-PM-Potato-Token: {token}")], ""));
    assert_eq!(status, 200);
}

#[cfg(windows)]
#[test]
fn the_real_gate_refuses_a_token_less_request_from_the_brains_own_process() {
    // The test process IS the registered brain here, so its token-less calls to any route are 403.
    let (addr, _rig, _token) = listener("gate-downgrade");
    for (method, path) in [("GET", "/panes"), ("GET", "/health"), ("POST", "/agents/run"), ("GET", "/events/poll")] {
        let (status, _, body) = send(addr, &request(method, path, &[], "{}"));
        assert_eq!((status, json_of(&body)["code"].as_str().map(str::to_string)), (403, Some("POTATO_SCOPE".to_string())), "{method} {path}");
    }
}
