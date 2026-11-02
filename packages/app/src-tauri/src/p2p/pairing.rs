//! Pairing two computers. The result is an entry in the signed trust list (`trust.rs`) on both sides, with
//! every capability off.
//!
//! **Roles.** The computer that *shows* a code is the host; the one that *enters* it (or picks the host from the
//! "found nearby" list) is the joiner. The joiner dials the host's pairing capability (`pm/pair/1`), which the
//! link layer admits only while the host has a code on screen, and only on a direct (LAN) path.
//!
//! **How the code authenticates the exchange** (no PAKE; the code is short and single use, so the rules around it
//! carry the weight):
//! 1. J -> H `hello` {nonce_j, binding_j, name_j}. The host checks the signed `NodeBinding` and that its node id is
//!    the one the encrypted link was made with.
//! 2. H -> J `challenge` {nonce_h, binding_h, name_h}. The joiner checks the same, and that it is the node it dialed.
//! 3. Both derive `K` = stretched HMAC of the code, salted with the host's node id. The transcript is both bindings
//!    and both nonces. J -> H `proof` = HMAC(K, "join" | transcript). The host sends nothing derived from the code
//!    before this checks out, so a stranger who connects learns nothing to guess against offline.
//! 4. H -> J `accept` = HMAC(K, "host" | transcript): the host knew the code too. A wrong proof counts as a wrong
//!    code. The code is single use: a good proof consumes it.
//! 5. **Both people confirm on their own screen** ("<Name> wants to pair. Code 4F7Q-9K2M. Allow / No"). Only then
//!    each side sends `decision` {yes, HMAC(K, role | yes | transcript)}. A side trusts the other only after it said
//!    yes itself AND the other's yes arrived with a valid MAC. Until then nothing is trusted: the permission check
//!    of the link layer still refuses every other capability.
//!
//! **Limits.** A code lives 2 minutes. 3 wrong codes or 3 "No" answers lock pairing for 10 minutes (the shown code
//! is withdrawn, joining and inbound sessions are refused). One session at a time. The sheet only appears after a
//! correct proof, so a stranger without the code cannot pop it up. The answer comes only through
//! `PairManager::answer`, which the desktop window's command calls: agents, MCP and the phone have no route to it
//! (`guard_tests.rs`).
//!
//! **Weak spot, on purpose said out loud:** if the joiner picks a "nearby" computer, the node id it dials was
//! announced on the LAN and is not yet authenticated. A fake host could then collect the joiner's proof and try
//! codes offline (the stretch makes each try costly). The host person's sheet and the code shown on both sheets
//! are the defence: pairing never completes without both people saying yes.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use rand::Rng;
use serde_json::{json, Value};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

use super::alpn;
use super::capability::{self, Capability, CapabilityHandler, RelayPolicy};
use super::discovery::{Announcement, Discovery};
use super::endpoint::{BiStream, Link, NodeId, P2pError, PeerAddr};
use super::identity::{DeviceIdentity, NodeBinding};
use super::trust::{clean_name, TrustList};
use super::P2p;
use crate::team::keystore::KeyStore;

pub const CODE_TTL_MS: u64 = 120_000;
pub const LOCKOUT_MS: u64 = 600_000;
pub const MAX_STRIKES: u32 = 3;
const CODE_LEN: usize = 8;
/// No 0/O/1/I: a code read off one screen and typed on another is not mixed up.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const STRETCH_ROUNDS: u32 = 5_000;
const STEP_TIMEOUT: Duration = Duration::from_secs(20);
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);
const FRAME_MAX: usize = 4096;
const TICKET_PREFIX: &str = "pm1:";

type HmacSha256 = Hmac<Sha256>;

// ----- clock -----

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
}

/// A clock a test moves by hand.
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

// ----- errors -----

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairError {
    #[error("FEATURE_OFF: My computers is switched off in Labs")]
    Off,
    #[error("Pairing is paused until {0}. Too many wrong codes or answers.")]
    Locked(u64),
    #[error("Check the code: it has 8 letters and numbers.")]
    BadCode,
    #[error("Check the address from the other computer.")]
    BadTicket,
    #[error("That computer is not nearby any more.")]
    NotNearby,
    #[error("Could not reach that computer. Keep PM open on both and stay on the same network.")]
    Unreachable,
    #[error("The code was not accepted. Check it, or ask for a new one.")]
    WrongCode,
    #[error("The other computer could not prove who it is.")]
    BadIdentity,
    #[error("The other computer is busy or has no code open.")]
    Refused,
    #[error("The other computer said no.")]
    TheyDeclined,
    #[error("You said no.")]
    YouDeclined,
    #[error("Nobody answered in time.")]
    TimedOut,
    #[error("The other computer did not finish.")]
    Dropped,
    #[error("Already pairing.")]
    Busy,
    #[error("Could not save: {0}")]
    Save(String),
    #[error("No such question.")]
    NoSuchQuestion,
    #[error("{0}")]
    Other(String),
}

impl From<P2pError> for PairError {
    fn from(e: P2pError) -> Self {
        match e {
            P2pError::Off => PairError::Off,
            P2pError::Identity(m) => PairError::Other(m),
            _ => PairError::Unreachable,
        }
    }
}

// ----- code, ticket, keys -----

/// "4F7Q-9K2M" -> "4F7Q9K2M". `None` when it is not a code.
pub fn normalize_code(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '-').map(|c| c.to_ascii_uppercase()).collect();
    if s.len() == CODE_LEN && s.bytes().all(|b| ALPHABET.contains(&b)) {
        Some(s)
    } else {
        None
    }
}

pub fn display_code(code: &str) -> String {
    format!("{}-{}", &code[..4], &code[4..])
}

fn new_code() -> String {
    let mut rng = rand::rngs::OsRng;
    (0..CODE_LEN).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// What the host shows next to the code: its node id and the addresses to try.
pub fn make_ticket(node: &NodeId, addrs: &[SocketAddr]) -> String {
    let mut t = format!("{TICKET_PREFIX}{}", node.to_hex());
    for a in addrs {
        t.push('@');
        t.push_str(&a.to_string());
    }
    t
}

pub fn parse_ticket(raw: &str) -> Option<PeerAddr> {
    let rest = raw.trim().strip_prefix(TICKET_PREFIX)?;
    let mut parts = rest.split('@');
    let id = NodeId(unhex::<32>(parts.next()?)?);
    let mut direct = Vec::new();
    for p in parts {
        direct.push(SocketAddr::from_str(p).ok()?);
    }
    Some(PeerAddr { id, direct })
}

fn mac(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).expect("hmac takes any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

fn mac_ok(key: &[u8; 32], parts: &[&[u8]], given: &[u8; 32]) -> bool {
    mac(key, parts).ct_eq(given).into()
}

/// The code is stretched so each offline try costs real work, and salted with the host's id.
fn derive_key(code: &str, host: &NodeId) -> [u8; 32] {
    let mut k = mac(&[0u8; 32], &[b"pm/pair/kdf/1", &host.0, code.as_bytes()]);
    for i in 0..STRETCH_ROUNDS {
        k = mac(&k, &[&i.to_be_bytes(), code.as_bytes()]);
    }
    k
}

struct Hello {
    nonce: [u8; 16],
    binding: NodeBinding,
    name: String,
}

fn binding_bytes(b: &NodeBinding) -> Vec<u8> {
    let mut v = Vec::with_capacity(136);
    v.extend_from_slice(&b.node_id.0);
    v.extend_from_slice(&b.device_pub);
    v.extend_from_slice(&b.issued_at.to_be_bytes());
    v.extend_from_slice(&b.signature);
    v
}

fn transcript(joiner: &Hello, host: &Hello) -> Vec<u8> {
    let mut t = b"pm/pair/transcript/1".to_vec();
    t.extend_from_slice(&binding_bytes(&joiner.binding));
    t.extend_from_slice(&binding_bytes(&host.binding));
    t.extend_from_slice(&joiner.nonce);
    t.extend_from_slice(&host.nonce);
    t
}

fn hello_json(kind: &str, h: &Hello) -> Value {
    json!({
        "t": kind,
        "nonce": hex(&h.nonce),
        "name": h.name,
        "binding": {
            "node": h.binding.node_id.to_hex(),
            "device": hex(&h.binding.device_pub),
            "issued": h.binding.issued_at,
            "sig": hex(&h.binding.signature),
        },
    })
}

fn parse_hello(v: &Value, kind: &str) -> Option<Hello> {
    if v.get("t")?.as_str()? != kind {
        return None;
    }
    let b = v.get("binding")?;
    let binding = NodeBinding {
        node_id: NodeId(unhex::<32>(b.get("node")?.as_str()?)?),
        device_pub: unhex::<32>(b.get("device")?.as_str()?)?,
        issued_at: b.get("issued")?.as_u64()?,
        signature: unhex::<64>(b.get("sig")?.as_str()?)?,
    };
    Some(Hello { nonce: unhex::<16>(v.get("nonce")?.as_str()?)?, binding, name: clean_name(v.get("name")?.as_str()?) })
}

// ----- frames -----

async fn send_frame(s: &mut BiStream, v: &Value) -> Result<(), PairError> {
    let bytes = v.to_string().into_bytes();
    s.send.write_all(&(bytes.len() as u32).to_be_bytes()).await.map_err(|_| PairError::Dropped)?;
    s.send.write_all(&bytes).await.map_err(|_| PairError::Dropped)?;
    s.send.flush().await.map_err(|_| PairError::Dropped)
}

async fn recv_frame(s: &mut BiStream, wait: Duration) -> Result<Value, PairError> {
    let read = async {
        let mut len = [0u8; 4];
        s.recv.read_exact(&mut len).await.map_err(|_| PairError::Dropped)?;
        let len = u32::from_be_bytes(len) as usize;
        if len > FRAME_MAX {
            return Err(PairError::Dropped);
        }
        let mut buf = vec![0u8; len];
        s.recv.read_exact(&mut buf).await.map_err(|_| PairError::Dropped)?;
        serde_json::from_slice::<Value>(&buf).map_err(|_| PairError::Dropped)
    };
    tokio::time::timeout(wait, read).await.map_err(|_| PairError::TimedOut)?
}

// ----- state -----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// This computer showed the code.
    Host,
    /// This computer typed the code.
    Joiner,
}

#[derive(Default)]
struct Limiter {
    wrongs: u32,
    nos: u32,
    locked_until: u64,
}

struct HostCode {
    code: String,
    expires_at: u64,
    node: NodeId,
}

struct PendingAsk {
    id: u64,
    role: Role,
    name: String,
    code: String,
    expires_at: u64,
    tx: Option<oneshot::Sender<bool>>,
}

#[derive(Default)]
struct State {
    host: Option<HostCode>,
    /// A session (inbound or outbound) is running.
    busy: bool,
    limiter: Limiter,
    pending: Option<PendingAsk>,
    joining: Option<String>,
    message: Option<String>,
    next_id: u64,
}

/// Called with the new view whenever anything the sheets show changes.
pub type PairNotifier = Arc<dyn Fn(&Value) + Send + Sync>;

pub struct PairManager {
    p2p: Arc<P2p>,
    trust: Arc<TrustList>,
    keys: KeyStore,
    clock: Arc<dyn Clock>,
    discovery: Arc<dyn Discovery>,
    name: String,
    state: Mutex<State>,
    notifier: Mutex<Option<PairNotifier>>,
}

/// Resets `busy` however a session ends.
struct BusyGuard(Arc<PairManager>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.with(|s| s.busy = false);
        self.0.changed();
    }
}

struct HostHandler(Arc<PairManager>);

#[async_trait]
impl CapabilityHandler for HostHandler {
    async fn serve(&self, peer: NodeId, link: Arc<dyn Link>) {
        let _ = self.0.clone().serve_host(peer, &link).await;
        // Let the last frame arrive before the link goes away.
        tokio::time::sleep(Duration::from_millis(20)).await;
        link.close(0, "done");
    }
}

impl PairManager {
    /// Registers the pairing capability on the P2p service's registry. Starts nothing.
    pub fn new(
        p2p: Arc<P2p>,
        trust: Arc<TrustList>,
        keys: KeyStore,
        clock: Arc<dyn Clock>,
        discovery: Arc<dyn Discovery>,
        name: &str,
    ) -> Result<Arc<Self>, P2pError> {
        let m = Arc::new(Self {
            p2p,
            trust,
            keys,
            clock,
            discovery,
            name: clean_name(name),
            state: Mutex::new(State::default()),
            notifier: Mutex::new(None),
        });
        m.p2p.registry().register(Capability::new(capability::PAIR, alpn::PAIR, RelayPolicy::Never, Arc::new(HostHandler(m.clone()))))?;
        Ok(m)
    }

    pub fn set_notifier(&self, n: PairNotifier) {
        *self.notifier.lock().unwrap_or_else(|p| p.into_inner()) = Some(n);
    }

    pub fn trust(&self) -> &Arc<TrustList> {
        &self.trust
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn changed(&self) {
        let n = self.notifier.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(n) = n {
            n(&self.view());
        }
    }

    fn say(&self, text: &str) {
        self.with(|s| s.message = Some(text.to_string()));
        self.changed();
    }

    fn identity(&self) -> Result<DeviceIdentity, PairError> {
        DeviceIdentity::load(&self.keys).map_err(PairError::from)
    }

    fn locked_until(&self) -> Option<u64> {
        let now = self.clock.now_ms();
        self.with(|s| {
            if s.limiter.locked_until > now {
                Some(s.limiter.locked_until)
            } else {
                if s.limiter.locked_until != 0 {
                    s.limiter = Limiter::default();
                }
                None
            }
        })
    }

    /// Counts a wrong code (`wrong`) or a "No" and trips the lockout at the third. A trip withdraws the shown code.
    fn strike(&self, wrong: bool) {
        let now = self.clock.now_ms();
        let tripped = self.with(|s| {
            let n = if wrong { &mut s.limiter.wrongs } else { &mut s.limiter.nos };
            *n += 1;
            if *n >= MAX_STRIKES {
                s.limiter.locked_until = now + LOCKOUT_MS;
                true
            } else {
                false
            }
        });
        if tripped {
            self.cancel_hosting();
        }
        self.changed();
    }

    // ----- the view the sheets draw -----

    pub fn view(&self) -> Value {
        let now = self.clock.now_ms();
        let (host, pending, joining, message, lock) = self.with(|s| {
            let host = s.host.as_ref().filter(|h| h.expires_at > now).map(|h| (h.code.clone(), h.expires_at, h.node));
            let pending = s.pending.as_ref().map(|p| {
                json!({ "id": p.id, "role": if p.role == Role::Host { "host" } else { "joiner" }, "name": p.name, "code": display_code(&p.code), "expires_at": p.expires_at })
            });
            let lock = (s.limiter.locked_until > now).then_some(s.limiter.locked_until);
            (host, pending, s.joining.clone(), s.message.clone(), lock)
        });
        let hosting = host.map(|(code, expires_at, node)| {
            json!({ "code": display_code(&code), "ticket": make_ticket(&node, &self.p2p.local_addrs()), "expires_at": expires_at })
        });
        json!({
            "hosting": hosting,
            "pending": pending,
            "joining": joining,
            "message": message,
            "locked_until": lock,
            "tampered": self.trust.tampered(),
            "devices": self.devices_view(),
        })
    }

    pub fn devices_view(&self) -> Value {
        Value::Array(
            self.trust
                .list()
                .iter()
                .map(|d| {
                    json!({
                        "node": d.node.to_hex(),
                        "name": d.name,
                        "added_at": d.added_at,
                        // Whether it is reachable is the desk link's to say (DL-7). Until then: unknown.
                        "online": Value::Null,
                        "allowed": d.allowed.iter().collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )
    }

    /// Computers showing a code on this network now, minus this one and the ones already paired.
    pub fn nearby(&self) -> Value {
        let me = self.p2p.node_id();
        Value::Array(
            self.discovery
                .nearby()
                .into_iter()
                .filter(|a| Some(a.node) != me && !self.trust.is_trusted(&a.node))
                .map(|a| json!({ "node": a.node.to_hex(), "name": a.name }))
                .collect(),
        )
    }

    // ----- host side -----

    /// Shows a code. Opens pairing for strangers until it is used, cancelled or 2 minutes pass.
    pub async fn start_hosting(self: &Arc<Self>) -> Result<Value, PairError> {
        if let Some(until) = self.locked_until() {
            return Err(PairError::Locked(until));
        }
        let transport = self.p2p.want(capability::PAIR).await?;
        let now = self.clock.now_ms();
        let code = new_code();
        let node = transport.id();
        self.with(|s| {
            s.host = Some(HostCode { code, expires_at: now + CODE_TTL_MS, node });
            s.message = None;
        });
        self.trust.set_pairing_open(true);
        self.discovery.announce(Announcement { node, name: self.name.clone(), addrs: self.p2p.local_addrs() });
        self.changed();
        Ok(self.view())
    }

    /// Withdraws the code and closes pairing.
    pub fn cancel_hosting(&self) {
        let node = self.with(|s| s.host.take().map(|h| h.node));
        self.trust.set_pairing_open(false);
        if let Some(n) = node {
            self.discovery.withdraw(&n);
        }
        self.changed();
    }

    async fn serve_host(self: Arc<Self>, peer: NodeId, link: &Arc<dyn Link>) -> Result<(), PairError> {
        let mut stream = link.accept_bi().await.map_err(|_| PairError::Dropped)?;
        let refuse = |why: &'static str| json!({ "t": "refused", "why": why });

        // Gate: locked, no code, expired, busy.
        if self.locked_until().is_some() {
            return send_frame(&mut stream, &refuse("locked")).await;
        }
        let now = self.clock.now_ms();
        let open = self.with(|s| s.host.as_ref().map(|h| h.expires_at > now).unwrap_or(false));
        if !open {
            self.cancel_hosting();
            return send_frame(&mut stream, &refuse("expired")).await;
        }
        let claimed = self.with(|s| {
            if s.busy {
                false
            } else {
                s.busy = true;
                true
            }
        });
        if !claimed {
            return send_frame(&mut stream, &refuse("busy")).await;
        }
        let _busy = BusyGuard(self.clone());

        // 1-2: hellos and bindings.
        let first = recv_frame(&mut stream, STEP_TIMEOUT).await?;
        let Some(joiner) = parse_hello(&first, "hello") else {
            return send_frame(&mut stream, &refuse("binding")).await;
        };
        if !joiner.binding.verify() || joiner.binding.node_id != peer {
            return send_frame(&mut stream, &refuse("binding")).await;
        }
        let identity = self.identity()?;
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill(&mut nonce);
        let me = Hello { nonce, binding: identity.binding(), name: self.name.clone() };
        send_frame(&mut stream, &hello_json("challenge", &me)).await?;
        let transcript = transcript(&joiner, &me);

        // 3-4: the proof. The code is read only now, and a good proof consumes it.
        let proof = recv_frame(&mut stream, STEP_TIMEOUT).await?;
        let given = proof.get("mac").and_then(Value::as_str).and_then(unhex::<32>);
        let now = self.clock.now_ms();
        let code = self.with(|s| s.host.as_ref().filter(|h| h.expires_at > now).map(|h| h.code.clone()));
        let Some(code) = code else {
            return send_frame(&mut stream, &refuse("expired")).await;
        };
        let key = derive_key(&code, &identity.id());
        let good = proof.get("t").and_then(Value::as_str) == Some("proof")
            && given.map(|g| mac_ok(&key, &[b"join", &transcript], &g)).unwrap_or(false);
        if !good {
            self.strike(true);
            return send_frame(&mut stream, &refuse("wrong")).await;
        }
        self.cancel_hosting();
        let accept = mac(&key, &[b"host", &transcript]);
        send_frame(&mut stream, &json!({ "t": "accept", "mac": hex(&accept) })).await?;

        // 5: both people decide.
        let outcome = self.confirm_and_finish(Role::Host, &mut stream, &key, &transcript, &joiner, &code).await;
        if let Err(e) = &outcome {
            self.say(&e.to_string());
        }
        outcome
    }

    // ----- joiner side -----

    /// Enters a code for a computer given by its ticket (hex of the id is looked up in the nearby list).
    pub async fn join(self: &Arc<Self>, target: &str, code: &str) -> Result<String, PairError> {
        if !self.p2p.labs() {
            return Err(PairError::Off);
        }
        if let Some(until) = self.locked_until() {
            return Err(PairError::Locked(until));
        }
        let code = normalize_code(code).ok_or(PairError::BadCode)?;
        let peer = if let Some(id) = unhex::<32>(target.trim()) {
            let a = self.discovery.nearby().into_iter().find(|a| a.node.0 == id).ok_or(PairError::NotNearby)?;
            PeerAddr { id: a.node, direct: a.addrs }
        } else {
            parse_ticket(target).ok_or(PairError::BadTicket)?
        };
        let claimed = self.with(|s| {
            if s.busy {
                false
            } else {
                s.busy = true;
                s.joining = Some("Looking for the other computer… Keep PM open on both computers.".into());
                s.message = None;
                true
            }
        });
        if !claimed {
            return Err(PairError::Busy);
        }
        let _busy = BusyGuard(self.clone());
        self.changed();
        let result = self.run_join(&peer, &code).await;
        if result.is_ok() {
            self.p2p.addrs().remember(peer.id, &peer.direct);
        }
        self.with(|s| s.joining = None);
        match &result {
            Ok(name) => self.say(&format!("Paired with {name}. Nothing is shared yet: turn on what you want below.")),
            Err(e) => self.say(&e.to_string()),
        }
        result
    }

    async fn run_join(self: &Arc<Self>, peer: &PeerAddr, code: &str) -> Result<String, PairError> {
        let link = self.p2p.dial(peer, capability::PAIR).await?;
        let result = self.run_join_on(&link, peer, code).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        link.close(0, "done");
        result
    }

    async fn run_join_on(self: &Arc<Self>, link: &Arc<dyn Link>, peer: &PeerAddr, code: &str) -> Result<String, PairError> {
        if link.peer() != peer.id {
            return Err(PairError::BadIdentity);
        }
        let identity = self.identity()?;
        let mut stream = link.open_bi().await.map_err(|_| PairError::Unreachable)?;
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill(&mut nonce);
        let me = Hello { nonce, binding: identity.binding(), name: self.name.clone() };
        send_frame(&mut stream, &hello_json("hello", &me)).await?;

        let reply = recv_frame(&mut stream, STEP_TIMEOUT).await?;
        if reply.get("t").and_then(Value::as_str) == Some("refused") {
            return Err(self.refusal(&reply));
        }
        let host = parse_hello(&reply, "challenge").ok_or(PairError::BadIdentity)?;
        if !host.binding.verify() || host.binding.node_id != peer.id {
            return Err(PairError::BadIdentity);
        }
        let transcript = transcript(&me, &host);
        let key = derive_key(code, &peer.id);
        let proof = mac(&key, &[b"join", &transcript]);
        send_frame(&mut stream, &json!({ "t": "proof", "mac": hex(&proof) })).await?;

        let answer = recv_frame(&mut stream, STEP_TIMEOUT).await?;
        if answer.get("t").and_then(Value::as_str) == Some("refused") {
            return Err(self.refusal(&answer));
        }
        let given = answer.get("mac").and_then(Value::as_str).and_then(unhex::<32>);
        if answer.get("t").and_then(Value::as_str) != Some("accept") || !given.map(|g| mac_ok(&key, &[b"host", &transcript], &g)).unwrap_or(false) {
            return Err(PairError::BadIdentity);
        }
        self.with(|s| s.joining = Some(format!("Waiting for {} to confirm…", host.name)));
        self.changed();
        self.confirm_and_finish(Role::Joiner, &mut stream, &key, &transcript, &host, code).await?;
        Ok(host.name)
    }

    fn refusal(&self, frame: &Value) -> PairError {
        match frame.get("why").and_then(Value::as_str) {
            Some("wrong") => {
                self.strike(true);
                PairError::WrongCode
            }
            Some("locked") | Some("expired") | Some("busy") => PairError::Refused,
            _ => PairError::BadIdentity,
        }
    }

    // ----- both sides: the two answers -----

    async fn confirm_and_finish(
        self: &Arc<Self>,
        role: Role,
        stream: &mut BiStream,
        key: &[u8; 32],
        transcript: &[u8],
        other: &Hello,
        code: &str,
    ) -> Result<(), PairError> {
        let mine: &[u8] = if role == Role::Host { b"h" } else { b"j" };
        let theirs: &[u8] = if role == Role::Host { b"j" } else { b"h" };
        let yes = self.ask(role, &other.name, code).await;
        let tag = |who: &[u8], yes: bool| mac(key, &[b"decision", who, &[yes as u8], transcript]);
        // Best effort: the other side may already have said no and gone. Its answer is still read below.
        let _sent = send_frame(stream, &json!({ "t": "decision", "yes": yes, "mac": hex(&tag(mine, yes)) })).await;
        if !yes {
            self.strike(false);
            return Err(PairError::YouDeclined);
        }
        let frame = recv_frame(stream, CONFIRM_TIMEOUT).await?;
        let their_yes = frame.get("yes").and_then(Value::as_bool).ok_or(PairError::Dropped)?;
        let given = frame.get("mac").and_then(Value::as_str).and_then(unhex::<32>).ok_or(PairError::Dropped)?;
        if frame.get("t").and_then(Value::as_str) != Some("decision") || !mac_ok(key, &[b"decision", theirs, &[their_yes as u8], transcript], &given) {
            return Err(PairError::BadIdentity);
        }
        if !their_yes {
            return Err(PairError::TheyDeclined);
        }
        // Both said yes, both MACs hold: now, and only now, the other computer is trusted (every capability off).
        let now_secs = self.clock.now_ms() / 1000;
        self.trust
            .add(other.binding.node_id, other.binding.device_pub, &other.name, now_secs)
            .map_err(|e| PairError::Save(e.to_string()))?;
        self.with(|s| s.limiter = Limiter::default());
        Ok(())
    }

    /// Puts the question on this computer's screen and waits for the person.
    async fn ask(self: &Arc<Self>, role: Role, name: &str, code: &str) -> bool {
        let (tx, rx) = oneshot::channel();
        let now = self.clock.now_ms();
        self.with(|s| {
            s.next_id += 1;
            let id = s.next_id;
            s.joining = None;
            s.pending = Some(PendingAsk {
                id,
                role,
                name: name.to_string(),
                code: code.to_string(),
                expires_at: now + CONFIRM_TIMEOUT.as_millis() as u64,
                tx: Some(tx),
            });
        });
        self.changed();
        // If this wait is dropped (the other computer left, the session was aborted) the question must not stay on
        // screen with an Allow that can no longer reach anyone.
        struct ClearOnDrop(Arc<PairManager>);
        impl Drop for ClearOnDrop {
            fn drop(&mut self) {
                self.0.with(|s| s.pending = None);
                self.0.changed();
            }
        }
        let _clear = ClearOnDrop(self.clone());
        let answer = tokio::time::timeout(CONFIRM_TIMEOUT, rx).await;
        matches!(answer, Ok(Ok(true)))
    }

    /// The person's answer to the sheet. The only way a question is answered; the desktop window's command is the
    /// only caller (see `guard_tests.rs`).
    pub fn answer(&self, id: u64, allow: bool) -> Result<(), PairError> {
        let tx = self.with(|s| match s.pending.as_mut() {
            Some(p) if p.id == id => p.tx.take(),
            _ => None,
        });
        tx.ok_or(PairError::NoSuchQuestion)?.send(allow).map_err(|_| PairError::NoSuchQuestion)
    }

    /// Removes a paired computer. The person confirms in the UI first.
    pub fn remove(&self, node: &NodeId) -> Result<(), PairError> {
        self.trust.remove(node).map_err(|e| PairError::Save(e.to_string()))?;
        self.changed();
        Ok(())
    }

    pub fn set_permission(&self, node: &NodeId, capability: &str, on: bool) -> Result<(), PairError> {
        self.trust.set_permission(node, capability, on).map_err(|e| PairError::Save(e.to_string()))?;
        self.changed();
        Ok(())
    }
}

/// Capability ids the page may switch, for the command layer.
pub fn permissionable() -> BTreeSet<&'static str> {
    capability::PERMISSIONABLE.iter().copied().collect()
}
