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
//! Which requests: the loopback routes that answer or dismiss a card, type into a pane, or change a companion
//! (keep or drop a pending memory line, wake, sleep): `guard::is_loopback_guarded_route`. Not these: the desktop
//! window (Tauri commands, no HTTP), a paired phone (a bearer token: `requires_pairing_token`), and every route that
//! is not about a card or a companion.
//!
//! Companion territory is the companion's pane AND every pane started under it (its helpers, by the `parent_pane_id`
//! chain): a helper's shell is the companion's hand too, so its processes count as the companion's.
//!
//! A process the OS names that is in no companion territory is `Other` (today's behaviour) only when its chain of
//! parents is intact up to this app or the person's desktop. An orphan (its parent exited: `Start-Process`, `cmd /c
//! start /b`, a double fork) is not placed: it is treated like a caller nobody could identify.
//!
//! When the caller cannot be identified (no process table, a socket the OS table does not show, another OS, an
//! orphan): refuse only when the request is about a companion's own tree: a pane that is a companion's or has one up
//! its parent chain, a companion's id, the pane a run `handle` or a question names. Fail closed for companion
//! territory, and today's behaviour for the rest. The decisions are pure (`decide`, over a fake process table and a
//! fake TCP owner in the tests).

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

use super::caller::{companion_panes, decide, territory_panes, CompanionPane};
use super::guard;
use super::service::{is_pane_id, pane_id_of};
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

/// Images that start a person's own programs (their desktop shell). A process whose parent chain reaches one of
/// these was started by the person at their desktop; one whose chain breaks first (the parent exited) or ends at a
/// service was not placed anywhere. Not agents: generic.
const DESKTOP_ANCHORS: [&str; 1] = ["explorer.exe"];

/// Whether `pid` is a live process whose chain of parents reaches this app (a pane's process) or the person's
/// desktop shell. A process with a dead parent (`Start-Process`, `cmd /c start /b`, a double fork) breaks the chain
/// and is not placed: Windows keeps the dead parent's pid, which is no longer in the table.
fn placed_in_the_session(procs: &[ProcEntry], pid: u32, app_pid: u32) -> bool {
    let mut current = pid;
    for _ in 0..64 {
        if current == app_pid {
            return true;
        }
        let Some(entry) = procs.iter().find(|p| p.pid == current) else { return false };
        if DESKTOP_ANCHORS.contains(&entry.image.as_str()) {
            return true;
        }
        if entry.parent == current || entry.parent == 0 {
            return false;
        }
        current = entry.parent;
    }
    false
}

/// The owner's process id, and then the pane tree it sits in. `panes` are companion territory (the companion's
/// pane and the panes under it). A process outside it is `Other` only when its chain of parents is intact up to this
/// app or the person's desktop; an orphan (its parent exited) is `Unknown`, because it may have left a tree on purpose.
pub fn identify(owner: &dyn TcpOwner, peer: &Peer, panes: &[CompanionPane], procs: Option<&[ProcEntry]>, app_pid: u32) -> Who {
    let Some(pid) = owner.owner_pid(peer) else { return Who::Unknown };
    // Without the process table nobody can be placed in a pane tree.
    let Some(procs) = procs else { return Who::Unknown };
    match decide(panes, Some(procs), Some(pid), None).verified {
        Some(pane) => Who::Companion(pane),
        None if placed_in_the_session(procs, pid, app_pid) => Who::Other,
        None => Who::Unknown,
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

/// The panes a guarded request is about: the pane in `/panes/{id}/...`, the pane of a companion in
/// `/companions/{id}/...`, the pane in the body of `/agents/answer` and the panes its `handle` resolves to, the pane
/// of the question in `/questions/{id}/...`. More than one when a handle names runs in several projects.
pub fn target_panes(
    path: &str,
    body: &Value,
    question_pane: &dyn Fn(&str) -> Option<String>,
    handle_panes: &dyn Fn(&str, Option<&str>) -> Vec<String>,
) -> Vec<String> {
    let parts: Vec<&str> = path.split('?').next().unwrap_or(path).trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["panes", id, ..] => vec![(*id).to_string()],
        ["companions", id, ..] => vec![pane_id_of(id)],
        ["questions", id, _, ..] => question_pane(id).into_iter().collect(),
        ["agents", "answer", ..] => {
            let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
            let mut panes: Vec<String> = text("pane_id").map(str::to_string).into_iter().collect();
            if let Some(handle) = text("handle") {
                panes.extend(handle_panes(handle, text("project_path")));
            }
            panes
        }
        _ => Vec::new(),
    }
}

/// The panes the run `handle` has (its newest turn in each project that owns it), over the run records.
fn run_panes(registry: &Arc<Mutex<PaneRegistry>>, handle: &str, project: Option<&str>) -> Vec<String> {
    let default_project = registry.lock().project_path.clone();
    let from = project.unwrap_or(&default_project);
    crate::operations::agent_runs_by_project(from, handle)
        .map(|runs| runs.into_iter().filter_map(|run| run.pane_id).collect())
        .unwrap_or_default()
}

/// The companion pane whose own agent is on the other end of this connection, for `companion_recall`. Decided only
/// by the OS: the process that owns the client socket must sit in a live companion pane's own process tree (not a
/// helper's: a helper is not the companion). The `X-Puppet-Master-Pid` header, which the caller writes, plays no part.
/// Where the owner or the process table is unknown, the answer is `None`: fail closed.
pub fn recall_caller_pane(
    registry: &Arc<Mutex<PaneRegistry>>,
    owner: &dyn TcpOwner,
    snapshot: &dyn Fn() -> Option<Vec<ProcEntry>>,
    peer: Option<Peer>,
) -> Option<String> {
    let panes = companion_panes(registry);
    if panes.is_empty() {
        return None;
    }
    let pid = owner.owner_pid(&peer?)?;
    let procs = snapshot()?;
    decide(&panes, Some(&procs), Some(pid), None).verified
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
    let handle_panes = |handle: &str, project: Option<&str>| run_panes(registry, handle, project);
    check_with(registry, owner, &crate::pty::agent_proc::snapshot, &handle_panes, peer, method, path, body)
}

/// `check` with the process table and the run-handle lookup passed in (the tests give it fake ones).
#[allow(clippy::too_many_arguments)]
pub fn check_with(
    registry: &Arc<Mutex<PaneRegistry>>,
    owner: &dyn TcpOwner,
    snapshot: &dyn Fn() -> Option<Vec<ProcEntry>>,
    handle_panes: &dyn Fn(&str, Option<&str>) -> Vec<String>,
    peer: Option<Peer>,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<(), OperationError> {
    if !guard::is_loopback_guarded_route(method, path) {
        return Ok(());
    }
    let panes = territory_panes(registry);
    let who = match peer {
        // The table is read only when there is a companion pane the caller could be in.
        Some(peer) if !panes.is_empty() => {
            let procs = snapshot();
            identify(owner, &peer, &panes, procs.as_deref(), std::process::id())
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
            let in_tree = target_panes(path, body, &question_pane, handle_panes).iter().any(|pane| {
                let parent_of = |id: &str| registry.lock().panes.get(id).and_then(|p| p.info.parent_pane_id.clone());
                in_companion_tree(&parent_of, pane)
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

    /// This app's own process id in the tests (it is not in the table: the panes hang off the desktop shell).
    const APP: u32 = 7;
    /// The person's desktop shell, whose own parent has long exited (as on Windows).
    const DESKTOP: u32 = 50;

    /// Mochi's pane 100 -> agent 101 -> its shell 102 -> curl 103. A helper pane 400 -> 401 -> shell 402 -> curl 403.
    /// An ordinary agent 300 -> 301. A program of the app 500. An orphan: curl 600 whose parent 999 is gone.
    fn table() -> Vec<ProcEntry> {
        let desktop = ProcEntry { pid: DESKTOP, parent: 999_999, image: "explorer.exe".into() };
        vec![
            desktop,
            proc(100, DESKTOP),
            proc(101, 100),
            proc(102, 101),
            proc(103, 102),
            proc(300, DESKTOP),
            proc(301, 300),
            proc(400, DESKTOP),
            proc(401, 400),
            proc(402, 401),
            proc(403, 402),
            proc(500, APP),
            proc(600, 999),
        ]
    }

    /// Mochi's own pane only (as `companion_recall` sees it).
    fn mochi() -> Vec<CompanionPane> {
        vec![CompanionPane { id: "companion-mochi".into(), roots: vec![100], exited: false }]
    }

    /// Mochi's territory: her pane, and the pane of the helper she started.
    fn mochi_territory() -> Vec<CompanionPane> {
        vec![
            CompanionPane { id: "companion-mochi".into(), roots: vec![100], exited: false },
            CompanionPane { id: "companion-mochi".into(), roots: vec![400], exited: false },
        ]
    }

    fn who(owner: Option<u32>, panes: &[CompanionPane]) -> Who {
        identify(&FakeTcp(owner), &peer(), panes, Some(&table()), APP)
    }

    #[test]
    fn a_curl_in_the_companions_shell_is_the_companion() {
        // The socket belongs to curl (103), a grandchild of the agent in Mochi's pane.
        let found = who(Some(103), &mochi());
        assert_eq!(found, Who::Companion("companion-mochi".into()));
        assert_eq!(decide_request(&found, false).unwrap_err().code, "COMPANION_CANNOT_ANSWER");
        // Also the pane's own process.
        assert_eq!(who(Some(100), &mochi()), Who::Companion("companion-mochi".into()));
    }

    #[test]
    fn a_process_whose_chain_is_intact_and_is_in_no_companions_territory_is_other_and_passes() {
        // An ordinary agent (up to the desktop shell) and a program of this app.
        for pid in [301, 500] {
            let found = who(Some(pid), &mochi_territory());
            assert_eq!(found, Who::Other, "{pid}");
            assert!(decide_request(&found, true).is_ok(), "{pid}: the person's own tooling may answer");
        }
    }

    #[test]
    fn a_companions_helper_shell_is_the_companions_hand() {
        // The helper's pane is outside Mochi's own tree, but inside her territory.
        assert_eq!(who(Some(403), &mochi()), Who::Other, "her own pane's tree alone does not hold the helper");
        for pid in [400, 401, 403] {
            assert_eq!(who(Some(pid), &mochi_territory()), Who::Companion("companion-mochi".into()), "{pid}");
        }
    }

    #[test]
    fn an_orphan_or_a_process_not_in_the_table_is_not_placed() {
        // curl 600 started with `Start-Process` and kept the pid of a parent that has exited; 4242 is gone already.
        for pid in [600, 4242] {
            let found = who(Some(pid), &mochi_territory());
            assert_eq!(found, Who::Unknown, "{pid}");
            assert_eq!(decide_request(&found, true).unwrap_err().code, "COMPANION_CANNOT_ANSWER", "{pid}");
            assert!(decide_request(&found, false).is_ok(), "{pid}: still today's behaviour for other cards");
        }
        // A chain that ends at a service or the system, not at the desktop or this app.
        let table = vec![proc(1, 0), proc(2, 1), proc(3, 2)];
        assert_eq!(identify(&FakeTcp(Some(3)), &peer(), &mochi_territory(), Some(&table), APP), Who::Unknown);
    }

    #[test]
    fn an_unidentified_caller_is_refused_only_for_a_companions_own_tree() {
        let unknown_owner = who(None, &mochi_territory());
        let no_table = identify(&FakeTcp(Some(103)), &peer(), &mochi(), None, APP);
        for found in [unknown_owner, no_table] {
            assert_eq!(found, Who::Unknown);
            assert_eq!(decide_request(&found, true).unwrap_err().code, "COMPANION_CANNOT_ANSWER");
            assert!(decide_request(&found, false).is_ok(), "today's behaviour for every other card");
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
    fn the_target_of_each_guarded_route_is_found() {
        let question_pane = |id: &str| (id == "q1").then(|| "helper".to_string());
        // A run handle resolves to its panes: `run-1` is a helper's, `run-2` lives in two projects.
        let handle_panes = |handle: &str, _project: Option<&str>| match handle {
            "run-1" => vec!["helper".to_string()],
            "run-2" => vec!["a".to_string(), "helper".to_string()],
            _ => Vec::new(),
        };
        let none = serde_json::json!({});
        let target = |path: &str, body: &Value| target_panes(path, body, &question_pane, &handle_panes);
        assert_eq!(target("/panes/h1/key", &none), ["h1"]);
        assert_eq!(target("/panes/h1/opencode/question/reply", &none), ["h1"]);
        assert_eq!(target("/panes/h1/messages", &none), ["h1"]);
        assert_eq!(target("/questions/q1/answer", &none), ["helper"]);
        assert!(target("/questions/q2/dismiss", &none).is_empty());
        assert_eq!(target("/agents/answer", &serde_json::json!({ "pane_id": "h2" })), ["h2"]);
        assert_eq!(target("/agents/answer", &serde_json::json!({ "handle": "run-1" })), ["helper"]);
        assert_eq!(target("/agents/answer", &serde_json::json!({ "handle": "run-2", "project_path": "p" })), ["a", "helper"]);
        assert_eq!(target("/agents/answer", &serde_json::json!({ "handle": "run-1", "pane_id": "h2" })), ["h2", "helper"]);
        assert!(target("/agents/answer", &serde_json::json!({ "handle": "nope" })).is_empty());
        // A companion's routes name her pane.
        assert_eq!(target("/companions/mochi/memory/pending/abc", &none), ["companion-mochi"]);
        assert_eq!(target("/companions/mochi/wake", &none), ["companion-mochi"]);
        assert!(target("/health", &none).is_empty());
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

    /// Run records, for the tests: `run-helper` is the helper's, `run-ordinary` an ordinary agent's.
    fn runs(handle: &str, _project: Option<&str>) -> Vec<String> {
        match handle {
            "run-helper" => vec!["helper-1".to_string()],
            "run-ordinary" => vec!["ordinary-1".to_string()],
            _ => Vec::new(),
        }
    }

    fn run(owner: Option<u32>, peer_known: bool, method: &str, path: &str, body: Value) -> Result<(), OperationError> {
        let procs = || Some(table());
        check_with(&registry(), &FakeTcp(owner), &procs, &runs, peer_known.then(peer), method, path, &body)
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

    const REVIEW_ROUTES: [&str; 3] = ["/companions/mochi/memory/pending/item-1", "/companions/mochi/wake", "/companions/mochi/sleep"];

    #[test]
    fn a_curl_from_the_companions_shell_cannot_keep_its_own_memory_or_wake_or_sleep_itself() {
        let keep = serde_json::json!({ "decision": "keep" });
        for path in REVIEW_ROUTES {
            for owner in [103, 403] {
                let error = run(Some(owner), true, "POST", path, keep.clone()).expect_err(path);
                assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{owner} {path}");
            }
        }
        assert!(run(Some(103), true, "DELETE", "/companions/mochi", serde_json::json!({})).is_err());
        // Reading is not the review gate.
        for path in ["/companions", "/companions/mochi/memory", "/companions/mochi/feed"] {
            assert!(run(Some(103), true, "GET", path, serde_json::json!({})).is_ok(), "{path}");
        }
    }

    #[test]
    fn the_person_s_own_tooling_still_reviews_memory_over_loopback_and_an_unknown_caller_is_refused() {
        let keep = serde_json::json!({ "decision": "keep" });
        for path in REVIEW_ROUTES {
            assert!(run(Some(301), true, "POST", path, keep.clone()).is_ok(), "{path}: an ordinary process of the person's");
            for (owner, peer_known) in [(None, true), (Some(103), false), (Some(600), true)] {
                let error = run(owner, peer_known, "POST", path, keep.clone()).expect_err(path);
                assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}: nobody could say who calls");
            }
        }
        // A companion's pane id is a companion's by its name, even one the registry does not hold: fail closed.
        assert!(run(None, true, "POST", "/companions/bit/wake", serde_json::json!({})).is_err());
    }

    #[test]
    fn a_helpers_shell_cannot_answer_for_the_companion_either() {
        let body = serde_json::json!({ "pane_id": "ordinary-1" });
        for path in CARD_ROUTES {
            let error = run(Some(403), true, "POST", path, body.clone()).expect_err(path);
            assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}");
        }
        // The helper's own ordinary peers still answer.
        assert!(run(Some(301), true, "POST", "/panes/helper-1/key", serde_json::json!({})).is_ok());
    }

    #[test]
    fn an_orphaned_curl_is_not_a_free_pass_on_a_companions_tree() {
        // Same as `Start-Process curl`: the OS names the process, its parent is gone.
        for path in ["/panes/helper-1/key", "/panes/companion-mochi/input", "/panes/helper-1/messages"] {
            let error = run(Some(600), true, "POST", path, serde_json::json!({})).expect_err(path);
            assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}");
        }
        // Not a companion's tree: unchanged.
        assert!(run(Some(600), true, "POST", "/panes/ordinary-1/key", serde_json::json!({})).is_ok());
    }

    #[test]
    fn a_run_handle_is_resolved_to_its_pane_before_deciding() {
        // The real handler accepts `handle` and finds the pane through the run record.
        for owner in [None, Some(600)] {
            let helper = run(owner, true, "POST", "/agents/answer", serde_json::json!({ "handle": "run-helper", "prompt_id": "p", "choice": "1" }));
            assert_eq!(helper.unwrap_err().code, "COMPANION_CANNOT_ANSWER");
            assert!(run(owner, true, "POST", "/agents/answer", serde_json::json!({ "handle": "run-ordinary" })).is_ok());
            assert!(run(owner, true, "POST", "/agents/answer", serde_json::json!({ "handle": "unknown-run" })).is_ok());
        }
        // The companion's own tree is refused whatever the target.
        let error = run(Some(103), true, "POST", "/agents/answer", serde_json::json!({ "handle": "run-ordinary" })).unwrap_err();
        assert_eq!(error.code, "COMPANION_CANNOT_ANSWER");
    }

    // ---- recall: who is the companion's own agent ----

    fn recall(owner: Option<u32>, peer_known: bool, table_known: bool) -> Option<String> {
        let procs = || table_known.then(table);
        recall_caller_pane(&registry(), &FakeTcp(owner), &procs, peer_known.then(peer))
    }

    #[test]
    fn recall_is_for_a_process_in_the_companions_own_pane_by_the_real_connection() {
        assert_eq!(recall(Some(103), true, true).as_deref(), Some("companion-mochi"));
        assert_eq!(recall(Some(100), true, true).as_deref(), Some("companion-mochi"));
    }

    #[test]
    fn recall_is_refused_for_everyone_else_and_when_nothing_can_be_checked() {
        // An ordinary agent, a helper of the companion (not the companion), an orphan, a stranger.
        for pid in [301, 403, 600, 4242, 500] {
            assert_eq!(recall(Some(pid), true, true), None, "{pid}");
        }
        // Fail closed: no TCP owner (another OS), no peer, no process table.
        assert_eq!(recall(None, true, true), None);
        assert_eq!(recall(Some(103), false, true), None);
        assert_eq!(recall(Some(103), true, false), None);
        // No companion at all.
        let empty = Arc::new(Mutex::new(PaneRegistry::new()));
        assert_eq!(recall_caller_pane(&empty, &FakeTcp(Some(103)), &|| Some(table()), Some(peer())), None);
    }

    #[test]
    fn a_header_pid_cannot_stand_in_for_the_connection() {
        // The caller says it is process 103 (Mochi's curl) but the OS says its socket belongs to 301.
        let key = "forged-recall-session";
        crate::mcp_sessions::note_session_pid(key, 103);
        assert_eq!(crate::mcp_sessions::session_pid(key), Some(103));
        assert_eq!(recall(Some(301), true, true), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn where_the_os_cannot_say_who_owns_the_socket_recall_is_refused() {
        let procs = || Some(table());
        assert_eq!(recall_caller_pane(&registry(), &SystemTcp, &procs, Some(peer())), None);
    }
}
