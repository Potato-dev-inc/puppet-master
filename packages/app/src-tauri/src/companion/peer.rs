//! A companion's shell must not answer a card by plain HTTP either.
//!
//! `guard.rs` stops a companion's MCP session. But a companion (an agent with a shell) can also run
//! `curl http://127.0.0.1:<port>/questions/<id>/answer` over loopback, with no MCP session at all. Such a
//! request is "the local app's own tooling" to the bridge, so it needs another way to be told apart: the
//! TCP connection. For a loopback request the app asks the OS which process owns the other end of the
//! socket (`TcpOwner`, on Windows `GetExtendedTcpTable`), then walks that process up its parents with the
//! same code the recall check uses (`caller::decide`). A process in a companion pane's tree is refused with
//! `COMPANION_CANNOT_ANSWER`.
//!
//! Which requests: the loopback routes that answer or dismiss a card, or type into a pane (`guard::
//! is_card_route`). Not these: the desktop window (Tauri commands, no HTTP: `commands/questions.rs`), a
//! paired phone (a bearer token: `requires_pairing_token`), and every route that is not about a card.
//!
//! When the caller cannot be identified (no process table, a socket the OS table does not show, another
//! OS): refuse only when the card belongs to a pane that is a companion's, or whose parent chain holds a
//! companion pane (its helpers): fail closed for companion helpers, and today's behaviour for the rest.
//! The decisions are pure (`decide`), over a fake process table and a fake TCP owner in the tests.

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

use super::caller::{companion_panes, decide, CompanionPane};
use super::guard;
use super::service::is_pane_id;
use crate::operations::OperationError;
use crate::pty::agent_proc::ProcEntry;
use crate::pty::PaneRegistry;

/// Both ends of an accepted connection, as the server sees them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    /// The server's own side (`TcpStream::local_addr`).
    pub local: SocketAddr,
    /// The client's side (`TcpStream::peer_addr`).
    pub remote: SocketAddr,
}

/// Which process owns the client end of a connection.
pub trait TcpOwner {
    /// The process id that owns the socket whose address is `peer.remote` and whose far end is `peer.local`.
    /// `None` when the OS does not say.
    fn owner_pid(&self, peer: &Peer) -> Option<u32>;
}

/// The OS's own table. Windows only today (`GetExtendedTcpTable`); elsewhere nobody is identified, which is
/// the "cannot identify" path of the module doc.
pub struct SystemTcp;

#[cfg(windows)]
impl TcpOwner for SystemTcp {
    fn owner_pid(&self, peer: &Peer) -> Option<u32> {
        windows_owner(peer)
    }
}

#[cfg(not(windows))]
impl TcpOwner for SystemTcp {
    fn owner_pid(&self, _peer: &Peer) -> Option<u32> {
        None
    }
}

#[cfg(windows)]
fn windows_owner(peer: &Peer) -> Option<u32> {
    use std::net::IpAddr;

    #[repr(C)]
    struct Row {
        state: u32,
        local_addr: u32,
        local_port: u32,
        remote_addr: u32,
        remote_port: u32,
        pid: u32,
    }
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(table: *mut core::ffi::c_void, size: *mut u32, order: i32, family: u32, class: u32, reserved: u32) -> u32;
    }
    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_ALL: u32 = 5;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    // Only IPv4 loopback: the bridge listens on 127.0.0.1.
    let (IpAddr::V4(client), IpAddr::V4(server)) = (peer.remote.ip(), peer.local.ip()) else { return None };

    // A u32 buffer keeps the rows aligned.
    let mut words: Vec<u32> = vec![0; 1024];
    for _ in 0..4 {
        let mut size = (words.len() * 4) as u32;
        // SAFETY: `words` holds `size` writable bytes; the call fills it or reports the size it needs.
        let code = unsafe { GetExtendedTcpTable(words.as_mut_ptr().cast(), &mut size, 0, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0) };
        if code == ERROR_INSUFFICIENT_BUFFER {
            words = vec![0; (size as usize).div_ceil(4) + 64];
            continue;
        }
        if code != 0 {
            return None;
        }
        let count = words[0] as usize;
        let rows = words.get(1..)?;
        if rows.len() < count * 6 {
            return None;
        }
        let port = |raw: u32| u16::from_be((raw & 0xFFFF) as u16);
        let addr = |raw: u32| std::net::Ipv4Addr::from(u32::from_be(raw));
        for n in 0..count {
            // SAFETY: `rows` holds `count` rows of six u32 each (checked above); `Row` is six u32.
            let row = unsafe { &*(rows.as_ptr().add(n * 6) as *const Row) };
            // The client's own socket: its address is the peer address, its far end is our listener.
            if addr(row.local_addr) == client
                && port(row.local_port) == peer.remote.port()
                && addr(row.remote_addr) == server
                && port(row.remote_port) == peer.local.port()
            {
                return Some(row.pid);
            }
        }
        return None;
    }
    None
}

/// Who is on the other end of a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    /// A process inside this companion pane's tree.
    Companion(String),
    /// A process the OS named and that is in no companion pane.
    Other,
    /// Not known.
    Unknown,
}

/// The owner's process id, and then the pane tree it sits in.
pub fn identify(owner: &dyn TcpOwner, peer: &Peer, panes: &[CompanionPane], procs: Option<&[ProcEntry]>) -> Who {
    let Some(pid) = owner.owner_pid(peer) else { return Who::Unknown };
    // Without the process table nobody can be placed in a pane tree.
    let Some(procs) = procs else { return Who::Unknown };
    match decide(panes, Some(procs), Some(pid), None).verified {
        Some(pane) => Who::Companion(pane),
        None => Who::Other,
    }
}

/// Whether `pane` is a companion's pane or has one up its parent chain. `parent_of` answers for a pane id.
pub fn in_companion_tree(parent_of: &dyn Fn(&str) -> Option<String>, pane: &str) -> bool {
    let mut current = pane.to_string();
    for _ in 0..16 {
        if is_pane_id(&current) {
            return true;
        }
        match parent_of(&current) {
            Some(parent) if parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

/// The verdict. `target_in_tree` says whether the pane the request is about is a companion's or its helper's.
pub fn decide_request(who: &Who, target_in_tree: bool) -> Result<(), OperationError> {
    match who {
        Who::Companion(_) => Err(guard::refusal()),
        Who::Other => Ok(()),
        Who::Unknown if target_in_tree => Err(guard::refusal()),
        Who::Unknown => Ok(()),
    }
}

/// The pane a card-route request is about, when the request says: the pane in `/panes/{id}/...`, the pane in
/// the body (`/agents/answer`), the pane of the question in `/questions/{id}/...`.
pub fn target_pane(path: &str, body: &Value, question_pane: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let parts: Vec<&str> = path.split('?').next().unwrap_or(path).trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["panes", id, ..] => Some((*id).to_string()),
        ["questions", id, _] => question_pane(id),
        ["agents", "answer"] => body.get("pane_id").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// The router's check for a request from a local (token-less) caller, over the real process table. `peer` is the
/// connection; `None` when it could not be read.
pub fn check(
    registry: &Arc<Mutex<PaneRegistry>>,
    owner: &dyn TcpOwner,
    peer: Option<Peer>,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<(), OperationError> {
    check_with(registry, owner, &crate::pty::agent_proc::snapshot, peer, method, path, body)
}

/// `check` with the process table passed in (the tests give it a fake one).
pub fn check_with(
    registry: &Arc<Mutex<PaneRegistry>>,
    owner: &dyn TcpOwner,
    snapshot: &dyn Fn() -> Option<Vec<ProcEntry>>,
    peer: Option<Peer>,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<(), OperationError> {
    if !guard::is_card_route(method, path) {
        return Ok(());
    }
    let panes = companion_panes(registry);
    let who = match peer {
        // The table is read only when there is a companion pane the caller could be in.
        Some(peer) if !panes.is_empty() => {
            let procs = snapshot();
            identify(owner, &peer, &panes, procs.as_deref())
        }
        Some(_) => Who::Other,
        None => Who::Unknown,
    };
    match who {
        // A process the OS named that is not in a companion pane: today's behaviour.
        Who::Other => Ok(()),
        Who::Companion(_) => decide_request(&who, false),
        // Not known: only a companion's own tree is refused, by the pane the request is about.
        Who::Unknown => {
            let question_pane = |id: &str| {
                let question = crate::questions::store().get(id, crate::questions::now_ms())?;
                question.pane_id.or_else(|| question.asker.as_deref().and_then(crate::mcp_sessions::claimed_session_pane))
            };
            let in_tree = target_pane(path, body, &question_pane).is_some_and(|pane| {
                let parent_of = |id: &str| registry.lock().panes.get(id).and_then(|p| p.info.parent_pane_id.clone());
                in_companion_tree(&parent_of, &pane)
            });
            decide_request(&who, in_tree)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeTcp(Option<u32>);
    impl TcpOwner for FakeTcp {
        fn owner_pid(&self, _peer: &Peer) -> Option<u32> {
            self.0
        }
    }

    fn peer() -> Peer {
        Peer { local: "127.0.0.1:17360".parse().unwrap(), remote: "127.0.0.1:50123".parse().unwrap() }
    }

    fn proc(pid: u32, parent: u32) -> ProcEntry {
        ProcEntry { pid, parent, image: "x.exe".into() }
    }

    /// Mochi's pane 100 -> agent 101 -> its shell 102 -> curl 103. A helper pane 400 -> 401. An ordinary agent 300 -> 301.
    fn table() -> Vec<ProcEntry> {
        vec![proc(100, 1), proc(101, 100), proc(102, 101), proc(103, 102), proc(300, 1), proc(301, 300), proc(400, 1), proc(401, 400)]
    }

    fn mochi() -> Vec<CompanionPane> {
        vec![CompanionPane { id: "companion-mochi".into(), roots: vec![100], exited: false }]
    }

    #[test]
    fn a_curl_in_the_companions_shell_is_the_companion() {
        // The socket belongs to curl (103), a grandchild of the agent in Mochi's pane.
        let who = identify(&FakeTcp(Some(103)), &peer(), &mochi(), Some(&table()));
        assert_eq!(who, Who::Companion("companion-mochi".into()));
        assert_eq!(decide_request(&who, false).unwrap_err().code, "COMPANION_CANNOT_ANSWER");
        // Also the pane's own process.
        assert_eq!(identify(&FakeTcp(Some(100)), &peer(), &mochi(), Some(&table())), Who::Companion("companion-mochi".into()));
    }

    #[test]
    fn any_other_process_is_other_and_passes_whatever_the_target() {
        for pid in [301, 401, 4242] {
            let who = identify(&FakeTcp(Some(pid)), &peer(), &mochi(), Some(&table()));
            assert_eq!(who, Who::Other, "{pid}");
            assert!(decide_request(&who, true).is_ok(), "a process that is not the companion's may answer, even a companion's helper");
        }
    }

    #[test]
    fn an_unidentified_caller_is_refused_only_for_a_companions_own_tree() {
        let unknown_owner = identify(&FakeTcp(None), &peer(), &mochi(), Some(&table()));
        let no_table = identify(&FakeTcp(Some(103)), &peer(), &mochi(), None);
        for who in [unknown_owner, no_table] {
            assert_eq!(who, Who::Unknown);
            assert_eq!(decide_request(&who, true).unwrap_err().code, "COMPANION_CANNOT_ANSWER");
            assert!(decide_request(&who, false).is_ok(), "today's behaviour for every other card");
        }
    }

    #[test]
    fn the_parent_chain_finds_a_companion_above_a_helper() {
        let parents: HashMap<&str, &str> = [("helper", "companion-mochi"), ("grand", "helper"), ("loop-a", "loop-b"), ("loop-b", "loop-a"), ("plain", "other")].into();
        let parent_of = |id: &str| parents.get(id).map(|p| p.to_string());
        assert!(in_companion_tree(&parent_of, "companion-mochi"), "the companion's own pane");
        assert!(in_companion_tree(&parent_of, "helper"));
        assert!(in_companion_tree(&parent_of, "grand"));
        assert!(!in_companion_tree(&parent_of, "plain"));
        assert!(!in_companion_tree(&parent_of, "unknown-pane"));
        assert!(!in_companion_tree(&parent_of, "loop-a"), "a malformed loop ends");
    }

    #[test]
    fn the_target_of_each_card_route_is_found() {
        let question_pane = |id: &str| (id == "q1").then(|| "helper".to_string());
        let none = serde_json::json!({});
        assert_eq!(target_pane("/panes/h1/key", &none, &question_pane).as_deref(), Some("h1"));
        assert_eq!(target_pane("/panes/h1/opencode/question/reply", &none, &question_pane).as_deref(), Some("h1"));
        assert_eq!(target_pane("/panes/h1/messages", &none, &question_pane).as_deref(), Some("h1"));
        assert_eq!(target_pane("/questions/q1/answer", &none, &question_pane).as_deref(), Some("helper"));
        assert_eq!(target_pane("/questions/q2/dismiss", &none, &question_pane), None);
        assert_eq!(target_pane("/agents/answer", &serde_json::json!({ "pane_id": "h2" }), &question_pane).as_deref(), Some("h2"));
        assert_eq!(target_pane("/agents/answer", &serde_json::json!({ "handle": "x" }), &question_pane), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_table_names_this_process_as_the_owner_of_its_own_loopback_client() {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        // What the bridge sees for this connection.
        let seen = Peer { local: server_side.local_addr().unwrap(), remote: server_side.peer_addr().unwrap() };
        assert_eq!(seen.remote, client.local_addr().unwrap());
        assert_eq!(SystemTcp.owner_pid(&seen), Some(std::process::id()));
        // A connection that does not exist has no owner.
        let ghost = Peer { local: seen.local, remote: "127.0.0.1:1".parse().unwrap() };
        assert_eq!(SystemTcp.owner_pid(&ghost), None);
    }

    // ---- the whole request check, over a registry, a fake process table and a fake TCP owner ----

    fn registry() -> Arc<Mutex<PaneRegistry>> {
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        {
            let mut reg = registry.lock();
            let mut mochi = PaneRegistry::test_pane_stub("companion-mochi");
            mochi.info.pid = 100;
            reg.panes.insert("companion-mochi".into(), mochi);
            // A helper the companion started, and an ordinary agent pane of the person's.
            let mut helper = PaneRegistry::test_pane_stub("helper-1");
            helper.info.pid = 400;
            helper.info.parent_pane_id = Some("companion-mochi".into());
            reg.panes.insert("helper-1".into(), helper);
            let mut ordinary = PaneRegistry::test_pane_stub("ordinary-1");
            ordinary.info.pid = 300;
            reg.panes.insert("ordinary-1".into(), ordinary);
        }
        registry
    }

    fn run(owner: Option<u32>, peer_known: bool, method: &str, path: &str, body: Value) -> Result<(), OperationError> {
        let procs = || Some(table());
        check_with(&registry(), &FakeTcp(owner), &procs, peer_known.then(peer), method, path, &body)
    }

    const CARD_ROUTES: [&str; 6] = [
        "/questions/q-none/answer",
        "/questions/q-none/dismiss",
        "/agents/answer",
        "/panes/helper-1/key",
        "/panes/helper-1/input",
        "/panes/helper-1/messages",
    ];

    #[test]
    fn a_curl_from_the_companions_shell_is_refused_on_every_card_route() {
        let helper_body = serde_json::json!({ "pane_id": "helper-1" });
        for path in CARD_ROUTES.into_iter().chain(["/panes/helper-1/opencode/question/reply", "/panes/helper-1/opencode/permissions/r1/reply"]) {
            let error = run(Some(103), true, "POST", path, helper_body.clone()).expect_err(path);
            assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}");
        }
    }

    #[test]
    fn a_process_that_is_not_the_companions_still_answers_and_other_routes_are_untouched() {
        // The person's own tooling, or an ordinary agent, over plain loopback: as before.
        for path in CARD_ROUTES {
            assert!(run(Some(301), true, "POST", path, serde_json::json!({})).is_ok(), "{path}");
        }
        // The companion's shell may still use everything that is not about a card.
        for (method, path) in [("GET", "/panes"), ("POST", "/agents/run"), ("GET", "/questions"), ("POST", "/panes/helper-1/detach"), ("GET", "/panes/helper-1/messages")] {
            assert!(run(Some(103), true, method, path, serde_json::json!({})).is_ok(), "{method} {path}");
        }
    }

    #[test]
    fn when_nobody_can_say_who_calls_only_a_companions_own_tree_is_refused() {
        for (owner, peer_known) in [(None, true), (Some(103), false)] {
            // A helper's pane (parent chain holds the companion), and the companion's own pane.
            for path in ["/panes/helper-1/key", "/panes/helper-1/messages", "/panes/companion-mochi/input"] {
                let error = run(owner, peer_known, "POST", path, serde_json::json!({})).expect_err(path);
                assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}");
            }
            assert!(run(owner, peer_known, "POST", "/agents/answer", serde_json::json!({ "pane_id": "helper-1" })).is_err());
            // Everything else keeps today's behaviour.
            for path in ["/panes/ordinary-1/key", "/panes/gone-pane/key", "/questions/q-none/answer"] {
                assert!(run(owner, peer_known, "POST", path, serde_json::json!({})).is_ok(), "{path}");
            }
            assert!(run(owner, peer_known, "POST", "/agents/answer", serde_json::json!({ "handle": "h" })).is_ok());
        }
    }

    #[test]
    fn an_ask_user_card_of_a_companions_helper_is_refused_to_an_unidentified_caller_and_the_desktop_still_answers_it() {
        use crate::questions::{self, Choice, NewQuestion};
        let ask = |pane: Option<&str>| {
            questions::store()
                .create(
                    NewQuestion {
                        question: "Deploy now?".into(),
                        choices: vec![Choice { id: "yes".into(), label: "Yes".into() }],
                        pane_id: pane.map(str::to_string),
                        ..Default::default()
                    },
                    questions::now_ms(),
                )
                .unwrap()
        };
        let helper_card = ask(Some("helper-1"));
        let other_card = ask(Some("ordinary-1"));
        let path = |q: &questions::Question| format!("/questions/{}/answer", q.id);
        // A loopback caller nobody could identify: the companion's helper's card is refused, the other is not.
        assert_eq!(run(None, true, "POST", &path(&helper_card), serde_json::json!({})).unwrap_err().code, "COMPANION_CANNOT_ANSWER");
        assert!(run(None, true, "POST", &path(&other_card), serde_json::json!({})).is_ok());
        // The companion's own shell: refused for both.
        assert!(run(Some(103), true, "POST", &path(&other_card), serde_json::json!({})).is_err());
        // The desktop window answers through its Tauri command, `questions::store().answer(.., "desktop", ..)`, which never
        // passes through the bridge check: the card is still answered.
        let done = questions::store().answer(&helper_card.id, Some("yes"), None, "desktop", questions::now_ms()).unwrap();
        assert!(done.accepted);
        assert_eq!(done.question.id, helper_card.id);
        questions::store().answer(&other_card.id, Some("yes"), None, "desktop", questions::now_ms()).unwrap();
    }
}
