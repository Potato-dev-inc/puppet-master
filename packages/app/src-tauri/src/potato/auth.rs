//! Who may call `/potato/brain/*`, and what a brain can never do (spec section 2.1). This is a new
//! caller on the local bridge, so nothing here loosens an existing check; it only adds refusals.
//!
//! A `/potato/brain/*` request is served only when ALL of these hold, and otherwise the answer is 404
//! (the route does not exist for anyone else, a phone's bearer token never reaches it):
//!
//! 1. Potato Bot is on (the Labs switch);
//! 2. the peer is loopback;
//! 3. `mobile_pairing::requires_pairing_token` is false: no forwarder header, no foreign `Host`;
//! 4. there is no `Origin` header at all (a web page cannot reach it, not even one on localhost);
//! 5. `X-PM-Potato-Token` matches a running bot's token, compared in constant time;
//! 6. on Windows, the process that owns the client end of the socket is that bot's node process (or a
//!    process in its tree: a launcher shim may start the real node as a child). The owner comes from the
//!    OS table (`companion::peer::SystemTcp`), never from a header the caller writes.
//!
//! The brain has no admin powers:
//!
//! - the token on any other route is 403 `POTATO_SCOPE` (the pattern of the hook token);
//! - while a brain runs, a token-less local request whose TCP owner is the brain, or is in a brain's
//!   process tree, is 403 `POTATO_SCOPE` too. The brain itself is always checked; its descendants are
//!   checked for requests that change something (non-GET) and for card routes, because reading the
//!   process table costs a few milliseconds (spec 2.1, point 3);
//! - the brain's environment has no port file, no local secret and no `PUPPET_MASTER_*` variable
//!   (`env.rs`), so it has nothing to build a token-less request from anyway.
//!
//! The decisions are pure (`check`, `scope`), over a fake TCP owner and a fake process table in the
//! tests. The real bridge reaches them through `gate`.

use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::companion::guard::is_card_route;
use crate::companion::peer::{Peer, SystemTcp, TcpOwner};
use crate::mobile_pairing::{header_value, requires_pairing_token};
use crate::pty::agent_proc::ProcEntry;
use crate::pty::pane_parent::descends_from;

pub const TOKEN_HEADER: &str = "X-PM-Potato-Token";
pub const SCOPE_CODE: &str = "POTATO_SCOPE";

/// A new secret for one spawn: 32 random bytes as hex.
pub fn mint_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[derive(Clone)]
struct Entry {
    bot: String,
    token_hash: [u8; 32],
    pid: Option<u32>,
}

/// The running brains: one entry per bot, with the hash of its token (never the token) and its pid.
pub struct Brains {
    enabled: std::sync::atomic::AtomicBool,
    entries: Mutex<Vec<Entry>>,
}

impl Default for Brains {
    fn default() -> Self {
        Self::new()
    }
}

impl Brains {
    pub fn new() -> Self {
        Self { enabled: std::sync::atomic::AtomicBool::new(false), entries: Mutex::new(Vec::new()) }
    }

    /// Potato Bot on or off. Off: every brain route is 404.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// A new token for the bot replaces the old one. The pid follows with `set_pid`.
    pub fn register(&self, bot: &str, token: &str) {
        let mut entries = self.entries.lock();
        entries.retain(|e| e.bot != bot);
        entries.push(Entry { bot: bot.to_string(), token_hash: digest(token), pid: None });
    }

    pub fn set_pid(&self, bot: &str, pid: u32) {
        if let Some(entry) = self.entries.lock().iter_mut().find(|e| e.bot == bot) {
            entry.pid = Some(pid);
        }
    }

    pub fn unregister(&self, bot: &str) {
        self.entries.lock().retain(|e| e.bot != bot);
    }

    pub fn any_running(&self) -> bool {
        !self.entries.lock().is_empty()
    }

    fn pids(&self) -> Vec<u32> {
        self.entries.lock().iter().filter_map(|e| e.pid).collect()
    }

    /// The entry the token belongs to. Every entry is compared, in constant time, so the time does not
    /// say which bot (or how much of the token) matched.
    fn find(&self, presented: &str) -> Option<Entry> {
        let want = digest(presented);
        let entries = self.entries.lock();
        let mut found = None;
        for entry in entries.iter() {
            if bool::from(entry.token_hash.ct_eq(&want)) {
                found = Some(entry.clone());
            }
        }
        found
    }
}

/// The process-wide registry the real bridge uses.
pub fn brains() -> &'static Arc<Brains> {
    static BRAINS: OnceLock<Arc<Brains>> = OnceLock::new();
    BRAINS.get_or_init(|| Arc::new(Brains::new()))
}

/// Whether these path segments (after the `bridge` prefix is dropped) are a brain route.
pub fn is_brain_path(segments: &[&str]) -> bool {
    let segments = crate::mobile_pairing::normalize_bridge_segments(segments);
    segments.len() >= 2 && segments[0] == "potato" && segments[1] == "brain"
}

fn refuse_scope() -> (u16, Value) {
    (403, json!({ "code": SCOPE_CODE, "error": "the Potato Bot token is good for the brain routes only" }))
}

fn not_found() -> (u16, Value) {
    (404, json!({ "error": "not found" }))
}

/// What the gate says about a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Not about a brain: carry on with the normal checks.
    Pass,
    /// An authenticated brain on a brain route: this bot.
    Brain(String),
    Refuse(u16, Value),
}

/// What the gate reads from a request, and the two OS lookups it may need.
pub struct Request<'a> {
    pub method: &'a str,
    pub segments: &'a [&'a str],
    pub headers: &'a str,
    pub peer_loopback: bool,
    pub peer: Option<Peer>,
}

pub struct Os<'a> {
    pub owner: &'a dyn TcpOwner,
    pub procs: &'a dyn Fn() -> Option<Vec<ProcEntry>>,
    /// Whether the TCP owner must be the brain (Windows). Where the OS cannot say who owns a socket, the
    /// token and the loopback checks stand alone.
    pub require_owner: bool,
}

/// The brain route check: all six conditions, or 404.
pub fn check(brains: &Brains, req: &Request, os: &Os) -> Result<String, ()> {
    if !brains.enabled() || !req.peer_loopback {
        return Err(());
    }
    if requires_pairing_token(req.headers, true) || header_value(req.headers, "Origin").is_some() {
        return Err(());
    }
    let token = header_value(req.headers, TOKEN_HEADER).filter(|t| !t.is_empty()).ok_or(())?;
    let entry = brains.find(token).ok_or(())?;
    if os.require_owner {
        let peer = req.peer.ok_or(())?;
        let brain_pid = entry.pid.ok_or(())?;
        let owner = os.owner.owner_pid(&peer).ok_or(())?;
        if owner != brain_pid {
            // A launcher shim may start the real node as its child: the tree counts, the header does not.
            let procs = (os.procs)().ok_or(())?;
            if !descends_from(&procs, &[brain_pid], owner) {
                return Err(());
            }
        }
    }
    Ok(entry.bot)
}

/// The whole decision for one request.
pub fn decide(brains: &Brains, req: &Request, os: &Os) -> Verdict {
    if is_brain_path(req.segments) {
        return match check(brains, req, os) {
            Ok(bot) => Verdict::Brain(bot),
            Err(()) => {
                let (status, body) = not_found();
                Verdict::Refuse(status, body)
            }
        };
    }
    // The token on any other route.
    if header_value(req.headers, TOKEN_HEADER).is_some() {
        let (status, body) = refuse_scope();
        return Verdict::Refuse(status, body);
    }
    // A token-less local request from the brain's own process (or its tree): the brain has no way in
    // that is not the token.
    if brains.any_running() && !requires_pairing_token(req.headers, req.peer_loopback) {
        if let Some(owner) = req.peer.and_then(|peer| os.owner.owner_pid(&peer)) {
            let pids = brains.pids();
            let path = format!("/{}", req.segments.join("/"));
            let deep = req.method != "GET" || is_card_route(req.method, &path);
            let in_tree = pids.contains(&owner) || (deep && (os.procs)().is_some_and(|procs| descends_from(&procs, &pids, owner)));
            if in_tree {
                let (status, body) = refuse_scope();
                return Verdict::Refuse(status, body);
            }
        }
    }
    Verdict::Pass
}

/// The real gate: the process-wide registry, the OS's own tables.
pub fn gate(brains: &Brains, method: &str, segments: &[&str], headers: &str, peer_loopback: bool, peer: Option<Peer>) -> Verdict {
    let req = Request { method, segments, headers, peer_loopback, peer };
    let os = Os { owner: &SystemTcp, procs: &crate::pty::agent_proc::snapshot, require_owner: cfg!(windows) };
    decide(brains, &req, &os)
}
