//! The Puppet Master team relay: a dumb, untrusted post office for teammates' Puppet Masters
//! (COWORK-PLAN §3 option c1, SAAS-PLAN §3). Self-hostable as one binary; the PM-hosted relay runs
//! the same protocol.
//!
//! What it does:
//! - keeps each team's **mailbox** of sealed envelopes for 7 days and hands them out by cursor,
//!   over a WebSocket (`/ws?after=`) or a long poll (`/envelopes?after=&wait=`);
//! - keeps two small signed **state slots** per team (`roster`, `keys`), opaque to it;
//! - holds **join requests** (sealed to the admin's device) for a day;
//! - **authenticates** every device request by its Ed25519 signature and the team's access list,
//!   which only the team's root key can change (and the team id is bound to that key);
//! - **rate-limits** per device, per team, per invite and per address, and caps what one team
//!   may store.
//!
//! What it never sees: names, branches, notes, PR titles, keys or code. Envelopes are end-to-end
//! encrypted and signed by their sender's device; the relay can only check the signature and
//! store, forward, delay or drop. Clients notice drops (per-sender counters).
//!
//! Mailbox positions (`seq`) come from a per-team counter that never goes back (`teams.next_seq`),
//! so a client's cursor stays valid after every old envelope has expired. A device added to the
//! access list reads only from the position the team was at when it was added (`devices.from_seq`):
//! a new member does not receive what the team said before they joined.

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pm_relay_protocol::{
    verify_request, Batch, Delivered, Envelope, JoinBox, Resync, SignedAcl, SignedBlob, TeamRegistration, AUTH_HEADER,
    AUTH_SKEW_MS, JOIN_TTL_MS, MAILBOX_TTL_MS, MAX_ENVELOPE_BYTES, MAX_JOIN_BYTES, MAX_STATE_BYTES, PROTOCOL, SLOTS,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

const HOUR_MS: u64 = 60 * 60 * 1000;
const DAY_MS: u64 = 24 * HOUR_MS;
/// How long a used request signature is remembered: a signed request is accepted for
/// `AUTH_SKEW_MS` either side of the relay's clock.
pub const SEEN_FOR_MS: u64 = 2 * AUTH_SKEW_MS + 1_000;
/// Largest WebSocket message a client may send (clients only listen; this is for pings).
const MAX_CLIENT_FRAME: usize = 4 * 1024;
/// Envelopes per batch.
const BATCH: u32 = 200;

/// Limits (SAAS-PLAN §3.5), enforced here so self-hosters get them too.
#[derive(Debug, Clone)]
pub struct Limits {
    pub device_per_minute: u32,
    pub device_per_day: u32,
    pub team_per_minute: u32,
    /// Every signed request of one device (reads included), per minute.
    pub device_requests_per_minute: u32,
    pub joins_per_invite: u32,
    pub joins_per_team_per_day: u32,
    /// Join requests per client address per team, per hour. Per team, so one address can't close
    /// joining for every team on the relay.
    pub joins_per_address_per_hour: u32,
    /// Team registrations per client address per hour.
    pub unauthenticated_per_ip_per_hour: u32,
    /// Longest long-poll wait (seconds).
    pub max_wait_s: u64,
    /// What one team may keep in its mailbox at once; more is refused with 507.
    pub team_max_envelopes: u64,
    pub team_max_bytes: u64,
    /// Open WebSockets per device.
    pub sockets_per_device: usize,
    /// Memory caps: rate-limit windows and remembered request signatures. When full, new work is
    /// refused (503) rather than growing without bound.
    pub max_windows: usize,
    pub max_seen_sigs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            device_per_minute: 60,
            device_per_day: 5_000,
            team_per_minute: 600,
            device_requests_per_minute: 300,
            joins_per_invite: 5,
            joins_per_team_per_day: 200,
            joins_per_address_per_hour: 10,
            unauthenticated_per_ip_per_hour: 30,
            max_wait_s: 25,
            team_max_envelopes: 20_000,
            team_max_bytes: 128 * 1024 * 1024,
            sockets_per_device: 4,
            max_windows: 500_000,
            max_seen_sigs: 2_000_000,
        }
    }
}

/// An address range (`10.0.0.0/8`, `::1`, `2001:db8::/32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    net: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(text: &str) -> Result<Self, String> {
        let (addr, prefix) = match text.trim().split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (text.trim(), None),
        };
        let net: IpAddr = addr.parse().map_err(|_| format!("{text:?} is not an address or range"))?;
        let net = net.to_canonical();
        let max = if net.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p.parse::<u8>().ok().filter(|p| *p <= max).ok_or_else(|| format!("{text:?} has a bad prefix length"))?,
            None => max,
        };
        Ok(Self { net, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        match (self.net, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(self.prefix)) };
                (u32::from(net) & mask) == (u32::from(ip) & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(self.prefix)) };
                (u128::from(net) & mask) == (u128::from(ip) & mask)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// SQLite file; `None` keeps everything in memory (tests).
    pub db: Option<PathBuf>,
    pub limits: Limits,
    /// Reverse proxies in front of the relay. Only a request from one of these has its
    /// `X-Forwarded-For` read (right to left, skipping these proxies) for per-address limits;
    /// anyone else's header is ignored.
    pub trusted_proxies: Vec<Cidr>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The client's address: the TCP peer, or, when the peer is a trusted proxy, the right-most
/// `X-Forwarded-For` hop that isn't one (what the nearest trusted proxy saw).
pub fn client_addr(peer: IpAddr, headers: &HeaderMap, trusted: &[Cidr]) -> IpAddr {
    let peer = peer.to_canonical();
    let is_trusted = |ip: IpAddr| trusted.iter().any(|c| c.contains(ip));
    if !is_trusted(peer) {
        return peer;
    }
    let hops: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|hop| hop.trim().parse::<IpAddr>().map(|ip| ip.to_canonical()))
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    hops.iter().rev().copied().find(|ip| !is_trusted(*ip)).unwrap_or(peer)
}

/// The key an address is counted under: IPv4 as is, IPv6 by its /64 (one household or server
/// gets a whole /64, so counting single v6 addresses would let one client rotate around limits).
pub fn address_key(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
    }
}

#[derive(Debug)]
struct Window {
    start_ms: u64,
    span_ms: u64,
    count: u32,
}

/// Request signatures already used, in two generations swapped every [`SEEN_FOR_MS`]: a signature
/// is remembered for at least that long, memory is bounded by the request rate, and nothing is
/// scanned per request.
#[derive(Debug, Default)]
struct SeenSigs {
    current: HashSet<String>,
    previous: HashSet<String>,
    since_ms: u64,
}

impl SeenSigs {
    fn first_use(&mut self, sig: &str, now: u64, cap: usize) -> Result<(), (StatusCode, &'static str)> {
        let age = now.saturating_sub(self.since_ms);
        if age >= SEEN_FOR_MS {
            self.previous = if age >= 2 * SEEN_FOR_MS { HashSet::new() } else { std::mem::take(&mut self.current) };
            self.current.clear();
            self.since_ms = now;
        }
        if self.current.contains(sig) || self.previous.contains(sig) {
            return Err((StatusCode::UNAUTHORIZED, "request replayed"));
        }
        if self.current.len() + self.previous.len() >= cap {
            return Err((StatusCode::SERVICE_UNAVAILABLE, "the relay is busy, try again shortly"));
        }
        self.current.insert(sig.to_string());
        Ok(())
    }

    fn len(&self) -> usize {
        self.current.len() + self.previous.len()
    }
}

/// What a team's listeners wait on: the newest mailbox position and the access-list version.
/// One `watch` channel per team, so a post wakes only that team's sockets and polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signal {
    pub head: u64,
    pub acl_version: u64,
}

pub struct Relay {
    db: Mutex<Connection>,
    limits: Limits,
    trusted_proxies: Vec<Cidr>,
    signals: Mutex<HashMap<String, watch::Sender<Signal>>>,
    windows: Mutex<HashMap<String, Window>>,
    seen_sigs: Mutex<SeenSigs>,
    sockets: Mutex<HashMap<String, usize>>,
}

type Refusal = (StatusCode, Json<serde_json::Value>);
type Reply = Result<Response, Refusal>;

fn refuse(status: StatusCode, error: &str) -> Refusal {
    (status, Json(json!({ "error": error })))
}

fn internal<E: std::fmt::Display>(e: E) -> Refusal {
    tracing::warn!(error = %e, "relay: internal error");
    refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

/// A device request that passed the signature, the access list and the replay guard.
#[derive(Debug, Clone)]
struct Authed {
    device: String,
    /// The mailbox position the device was added at: it reads only after it.
    from_seq: u64,
}

/// Socket count of one device; released when the socket ends.
struct SocketSlot {
    relay: Arc<Relay>,
    device: String,
}

impl Drop for SocketSlot {
    fn drop(&mut self) {
        let mut sockets = self.relay.sockets.lock().unwrap();
        if let Some(n) = sockets.get_mut(&self.device) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                sockets.remove(&self.device);
            }
        }
    }
}

fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
    Ok(names.iter().any(|n| n == column))
}

impl Relay {
    pub fn open(config: &Config) -> rusqlite::Result<Arc<Self>> {
        let conn = match &config.db {
            Some(path) => Connection::open(path)?,
            None => Connection::open_in_memory()?,
        };
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS teams (team TEXT PRIMARY KEY, root TEXT NOT NULL, salt TEXT NOT NULL,
                 acl_version INTEGER NOT NULL, acl TEXT NOT NULL, created_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS state (team TEXT NOT NULL, slot TEXT NOT NULL, version INTEGER NOT NULL,
                 blob TEXT NOT NULL, PRIMARY KEY (team, slot));
             CREATE TABLE IF NOT EXISTS envelopes (team TEXT NOT NULL, seq INTEGER NOT NULL, sender TEXT NOT NULL,
                 at_ms INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY (team, seq));
             CREATE TABLE IF NOT EXISTS joins (id INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL,
                 invite TEXT NOT NULL, at_ms INTEGER NOT NULL, body TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS devices (team TEXT NOT NULL, device TEXT NOT NULL, from_seq INTEGER NOT NULL,
                 PRIMARY KEY (team, device));
             CREATE INDEX IF NOT EXISTS envelopes_at ON envelopes (at_ms);
             CREATE INDEX IF NOT EXISTS joins_at ON joins (at_ms);",
        )?;
        // Relays from before the counter fix: add the columns and start each counter past the
        // newest envelope still stored (the best a restarted counter can do).
        for (column, default) in [("next_seq", 1), ("stored_rows", 0), ("stored_bytes", 0)] {
            if !has_column(&conn, "teams", column)? {
                conn.execute_batch(&format!("ALTER TABLE teams ADD COLUMN {column} INTEGER NOT NULL DEFAULT {default};"))?;
            }
        }
        conn.execute_batch(
            "UPDATE teams SET next_seq = MAX(next_seq,
                 (SELECT COALESCE(MAX(seq), 0) + 1 FROM envelopes e WHERE e.team = teams.team));",
        )?;
        // Relays from before join points: every listed device reads the whole mailbox, as before.
        let teams: Vec<(String, String)> = {
            let mut stmt = conn.prepare(
                "SELECT team, acl FROM teams t WHERE NOT EXISTS (SELECT 1 FROM devices d WHERE d.team = t.team)",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (team, acl) in teams {
            let devices: Vec<String> = serde_json::from_str(&acl).unwrap_or_default();
            for device in devices {
                conn.execute("INSERT OR IGNORE INTO devices (team, device, from_seq) VALUES (?1, ?2, 0)", params![team, device])?;
            }
        }
        recount(&conn)?;
        Ok(Arc::new(Self {
            db: Mutex::new(conn),
            limits: config.limits.clone(),
            trusted_proxies: config.trusted_proxies.clone(),
            signals: Mutex::new(HashMap::new()),
            windows: Mutex::new(HashMap::new()),
            seen_sigs: Mutex::new(SeenSigs::default()),
            sockets: Mutex::new(HashMap::new()),
        }))
    }

    /// Count one more under `key` in a window of `span_ms`; false when over `max` (or when the
    /// relay already tracks `max_windows` keys and none has expired).
    fn hit(&self, key: String, span_ms: u64, max: u32) -> bool {
        let now = now_ms();
        let mut windows = self.windows.lock().unwrap();
        if !windows.contains_key(&key) && windows.len() >= self.limits.max_windows {
            windows.retain(|_, w| now.saturating_sub(w.start_ms) < w.span_ms);
            if windows.len() >= self.limits.max_windows {
                return false;
            }
        }
        let w = windows.entry(key).or_insert(Window { start_ms: now, span_ms, count: 0 });
        if now.saturating_sub(w.start_ms) >= w.span_ms {
            w.start_ms = now;
            w.count = 0;
        }
        w.span_ms = span_ms;
        w.count += 1;
        w.count <= max
    }

    fn addr(&self, peer: SocketAddr, headers: &HeaderMap) -> String {
        address_key(client_addr(peer.ip(), headers, &self.trusted_proxies))
    }

    fn team_root(&self, team: &str) -> Result<(String, u64), Refusal> {
        let db = self.db.lock().unwrap();
        let row: Option<(String, u64)> = db
            .query_row("SELECT root, acl_version FROM teams WHERE team = ?1", params![team], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(internal)?;
        row.ok_or_else(|| refuse(StatusCode::NOT_FOUND, "unknown team"))
    }

    /// The device's join point, or `None` when it isn't on the team's access list.
    fn member_from(&self, team: &str, device: &str) -> rusqlite::Result<Option<u64>> {
        self.db
            .lock()
            .unwrap()
            .query_row("SELECT from_seq FROM devices WHERE team = ?1 AND device = ?2", params![team, device], |r| r.get(0))
            .optional()
    }

    fn signal_now(&self, team: &str) -> rusqlite::Result<Signal> {
        let db = self.db.lock().unwrap();
        let row: Option<(u64, u64)> = db
            .query_row("SELECT next_seq, acl_version FROM teams WHERE team = ?1", params![team], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.map(|(next, acl_version)| Signal { head: next.saturating_sub(1), acl_version }).unwrap_or_default())
    }

    /// Listen to a team's signal (created on first use).
    fn listen(&self, team: &str) -> rusqlite::Result<watch::Receiver<Signal>> {
        if let Some(tx) = self.signals.lock().unwrap().get(team) {
            return Ok(tx.subscribe());
        }
        let now = self.signal_now(team)?;
        let mut signals = self.signals.lock().unwrap();
        Ok(signals.entry(team.to_string()).or_insert_with(|| watch::channel(now).0).subscribe())
    }

    /// Wake a team's listeners (only that team's).
    fn notify(&self, team: &str, change: impl FnOnce(&mut Signal)) {
        if let Some(tx) = self.signals.lock().unwrap().get(team) {
            tx.send_modify(change);
        }
    }

    /// A signed request from a device on the team's access list. In this order: the signature
    /// (no state touched), the access list (one indexed read), then the replay guard, so a
    /// stranger's requests are never remembered.
    fn authorize(&self, team: &str, method: &str, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Result<Authed, Refusal> {
        let header = headers
            .get(AUTH_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| refuse(StatusCode::UNAUTHORIZED, "signed request required"))?;
        let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let now = now_ms();
        let authed = verify_request(header, method, path, body, now).map_err(|why| refuse(StatusCode::UNAUTHORIZED, why))?;
        let Some(from_seq) = self.member_from(team, &authed.device).map_err(internal)? else {
            self.team_root(team)?; // 404 for a team the relay doesn't know
            return Err(refuse(StatusCode::FORBIDDEN, "device not on this team"));
        };
        self.seen_sigs
            .lock()
            .unwrap()
            .first_use(&authed.sig, now, self.limits.max_seen_sigs)
            .map_err(|(status, why)| refuse(status, why))?;
        if !self.hit(format!("dev-req:{}", authed.device), 60_000, self.limits.device_requests_per_minute) {
            return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
        }
        Ok(Authed { device: authed.device, from_seq })
    }

    /// Envelopes after `after` that `device` may read. When `after` is past the team's newest
    /// position the relay's counter is behind the client: start again from its join point and
    /// say so (`reset`).
    fn batch_for(&self, team: &str, after: u64, from_seq: u64, limit: u32) -> rusqlite::Result<Batch> {
        let db = self.db.lock().unwrap();
        let head: u64 = db
            .query_row("SELECT next_seq FROM teams WHERE team = ?1", params![team], |r| r.get::<_, u64>(0))
            .optional()?
            .unwrap_or(1)
            .saturating_sub(1);
        let reset = after > head;
        let start = if reset { from_seq } else { after.max(from_seq) };
        let mut stmt = db.prepare_cached("SELECT seq, at_ms, body FROM envelopes WHERE team = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3")?;
        let rows = stmt.query_map(params![team, start, limit], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?, r.get::<_, String>(2)?)))?;
        let mut items = Vec::new();
        for row in rows {
            let (seq, at_ms, body) = row?;
            if let Ok(envelope) = serde_json::from_str::<Envelope>(&body) {
                items.push(Delivered { seq, at_ms, envelope });
            }
        }
        // Without items the cursor moves to where this device can read from: after a reset that
        // is the head (nothing is newer), never back below the join point.
        let next = items.last().map(|d| d.seq).unwrap_or(if reset { head.max(from_seq) } else { start });
        Ok(Batch { items, next, head: Some(head), reset })
    }

    /// Drop envelopes and join requests past their time, and forget expired rate-limit windows,
    /// listeners nobody holds, and signatures past the replay window.
    pub fn sweep(&self) -> rusqlite::Result<usize> {
        self.sweep_at(now_ms())
    }

    /// [`Relay::sweep`] as if the clock read `now` (tests move it past the mailbox TTL).
    pub fn sweep_at(&self, now: u64) -> rusqlite::Result<usize> {
        let removed = {
            let db = self.db.lock().unwrap();
            let a = db.execute("DELETE FROM envelopes WHERE at_ms < ?1", params![now.saturating_sub(MAILBOX_TTL_MS)])?;
            let b = db.execute("DELETE FROM joins WHERE at_ms < ?1", params![now.saturating_sub(JOIN_TTL_MS)])?;
            if a > 0 {
                recount(&db)?;
            }
            a + b
        };
        self.windows.lock().unwrap().retain(|_, w| now.saturating_sub(w.start_ms) < w.span_ms);
        self.signals.lock().unwrap().retain(|_, tx| tx.receiver_count() > 0);
        Ok(removed)
    }

    /// Test hook: forget an envelope as if the relay dropped it.
    pub fn drop_envelope(&self, team: &str, seq: u64) -> rusqlite::Result<usize> {
        self.db.lock().unwrap().execute("DELETE FROM envelopes WHERE team = ?1 AND seq = ?2", params![team, seq])
    }

    /// Test hook: what the relay holds in memory (rate-limit windows, remembered signatures,
    /// team listeners).
    pub fn memory(&self) -> (usize, usize, usize) {
        (self.windows.lock().unwrap().len(), self.seen_sigs.lock().unwrap().len(), self.signals.lock().unwrap().len())
    }

    /// Test hook: every stored byte, to check nothing readable is kept.
    pub fn dump(&self) -> String {
        let db = self.db.lock().unwrap();
        let mut out = String::new();
        for table in ["teams", "state", "envelopes", "joins", "devices"] {
            let mut stmt = db.prepare(&format!("SELECT * FROM {table}")).unwrap();
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    let mut line = String::new();
                    for i in 0..n {
                        let v: rusqlite::types::Value = r.get(i)?;
                        line.push_str(&format!("{v:?}|"));
                    }
                    Ok(line)
                })
                .unwrap();
            for row in rows.flatten() {
                out.push_str(&row);
                out.push('\n');
            }
        }
        out
    }
}

/// Each team's stored envelope count and bytes, from the rows themselves.
fn recount(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "UPDATE teams SET
             stored_rows = (SELECT COUNT(*) FROM envelopes e WHERE e.team = teams.team),
             stored_bytes = (SELECT COALESCE(SUM(LENGTH(CAST(body AS BLOB))), 0) FROM envelopes e WHERE e.team = teams.team);",
    )
}

fn query_u64(uri: &Uri, key: &str) -> Option<u64> {
    uri.query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.parse().ok()).flatten()
    })
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "protocol": PROTOCOL }))
}

async fn register(State(relay): State<Arc<Relay>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Bytes) -> Reply {
    let addr = relay.addr(peer, &headers);
    if !relay.hit(format!("reg:{addr}"), HOUR_MS, relay.limits.unauthenticated_per_ip_per_hour) {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let reg: TeamRegistration = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad registration"))?;
    if !reg.verify() {
        return Err(refuse(StatusCode::BAD_REQUEST, "the team id, root key and access list don't match"));
    }
    let mut db = relay.db.lock().unwrap();
    let exists: Option<String> = db
        .query_row("SELECT root FROM teams WHERE team = ?1", params![reg.team], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if exists.is_some() {
        return Err(refuse(StatusCode::CONFLICT, "team already registered"));
    }
    let tx = db.transaction().map_err(internal)?;
    tx.execute(
        "INSERT INTO teams (team, root, salt, acl_version, acl, created_ms, next_seq, stored_rows, stored_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 0, 0)",
        params![reg.team, reg.root, reg.salt, reg.acl.version, serde_json::to_string(&reg.acl.devices).unwrap_or_default(), now_ms()],
    )
    .map_err(internal)?;
    for device in &reg.acl.devices {
        tx.execute("INSERT OR IGNORE INTO devices (team, device, from_seq) VALUES (?1, ?2, 0)", params![reg.team, device])
            .map_err(internal)?;
    }
    tx.commit().map_err(internal)?;
    tracing::info!(team = %reg.team, "relay: team registered");
    Ok(StatusCode::CREATED.into_response())
}

async fn set_acl(State(relay): State<Arc<Relay>>, Path(team): Path<String>, body: Bytes) -> Reply {
    let acl: SignedAcl = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad access list"))?;
    let (root, version) = relay.team_root(&team)?;
    if acl.team != team || !acl.verify(&root) {
        return Err(refuse(StatusCode::FORBIDDEN, "not signed by the team's root key"));
    }
    if acl.version <= version {
        return if acl.version == version { Ok(StatusCode::NO_CONTENT.into_response()) } else { Err(refuse(StatusCode::CONFLICT, "older access list")) };
    }
    {
        let mut db = relay.db.lock().unwrap();
        let tx = db.transaction().map_err(internal)?;
        let head: u64 = tx
            .query_row("SELECT next_seq FROM teams WHERE team = ?1", params![team], |r| r.get::<_, u64>(0))
            .map_err(internal)?
            .saturating_sub(1);
        tx.execute(
            "UPDATE teams SET acl = ?1, acl_version = ?2 WHERE team = ?3",
            params![serde_json::to_string(&acl.devices).unwrap_or_default(), acl.version, team],
        )
        .map_err(internal)?;
        let kept: Vec<String> = {
            let mut stmt = tx.prepare("SELECT device FROM devices WHERE team = ?1").map_err(internal)?;
            let rows = stmt.query_map(params![team], |r| r.get::<_, String>(0)).map_err(internal)?;
            rows.collect::<Result<_, _>>().map_err(internal)?
        };
        for gone in kept.iter().filter(|d| !acl.devices.contains(d)) {
            tx.execute("DELETE FROM devices WHERE team = ?1 AND device = ?2", params![team, gone]).map_err(internal)?;
        }
        // A device new to the list reads from here on: not what the team said before it joined.
        for device in &acl.devices {
            tx.execute("INSERT OR IGNORE INTO devices (team, device, from_seq) VALUES (?1, ?2, ?3)", params![team, device, head])
                .map_err(internal)?;
        }
        tx.commit().map_err(internal)?;
    }
    relay.notify(&team, |s| s.acl_version = acl.version);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn put_state(State(relay): State<Arc<Relay>>, Path((team, slot)): Path<(String, String)>, uri: Uri, headers: HeaderMap, body: Bytes) -> Reply {
    relay.authorize(&team, "PUT", &uri, &headers, &body)?;
    let blob: SignedBlob = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad state"))?;
    let (root, _) = relay.team_root(&team)?;
    if blob.team != team || blob.slot != slot || !SLOTS.contains(&slot.as_str()) || !blob.verify(&root) {
        return Err(refuse(StatusCode::FORBIDDEN, "not signed by the team's root key"));
    }
    let db = relay.db.lock().unwrap();
    let current: Option<u64> = db
        .query_row("SELECT version FROM state WHERE team = ?1 AND slot = ?2", params![team, slot], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if current.is_some_and(|v| v > blob.version) {
        return Err(refuse(StatusCode::CONFLICT, "older state"));
    }
    db.execute(
        "INSERT INTO state (team, slot, version, blob) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (team, slot) DO UPDATE SET version = excluded.version, blob = excluded.blob",
        params![team, slot, blob.version, String::from_utf8_lossy(&body)],
    )
    .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn get_state(State(relay): State<Arc<Relay>>, Path((team, slot)): Path<(String, String)>, uri: Uri, headers: HeaderMap) -> Reply {
    relay.authorize(&team, "GET", &uri, &headers, b"")?;
    let blob: Option<String> = relay
        .db
        .lock()
        .unwrap()
        .query_row("SELECT blob FROM state WHERE team = ?1 AND slot = ?2", params![team, slot], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    Ok(match blob {
        Some(blob) => ([(axum::http::header::CONTENT_TYPE, "application/json")], blob).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

async fn post_envelope(State(relay): State<Arc<Relay>>, Path(team): Path<String>, uri: Uri, headers: HeaderMap, body: Bytes) -> Reply {
    let device = relay.authorize(&team, "POST", &uri, &headers, &body)?.device;
    let envelope: Envelope = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad envelope"))?;
    if envelope.team != team || envelope.from != device || !envelope.verify() {
        return Err(refuse(StatusCode::FORBIDDEN, "the envelope is not signed by this device for this team"));
    }
    let limits = &relay.limits;
    if !relay.hit(format!("dev-min:{device}"), 60_000, limits.device_per_minute)
        || !relay.hit(format!("dev-day:{device}"), DAY_MS, limits.device_per_day)
        || !relay.hit(format!("team-min:{team}"), 60_000, limits.team_per_minute)
    {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let text = String::from_utf8_lossy(&body).into_owned();
    let size = text.len() as u64;
    let seq = {
        let mut db = relay.db.lock().unwrap();
        let tx = db.transaction().map_err(internal)?;
        let (next, rows, bytes): (u64, u64, u64) = tx
            .query_row("SELECT next_seq, stored_rows, stored_bytes FROM teams WHERE team = ?1", params![team], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(internal)?;
        if rows >= limits.team_max_envelopes || bytes.saturating_add(size) > limits.team_max_bytes {
            return Err(refuse(StatusCode::INSUFFICIENT_STORAGE, "the team's mailbox is full"));
        }
        // The counter only grows (never MAX(seq)+1 over what is left): a cursor stays valid after
        // every old envelope has expired.
        tx.execute(
            "INSERT INTO envelopes (team, seq, sender, at_ms, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![team, next, device, now_ms(), text],
        )
        .map_err(internal)?;
        tx.execute(
            "UPDATE teams SET next_seq = ?1, stored_rows = stored_rows + 1, stored_bytes = stored_bytes + ?2 WHERE team = ?3",
            params![next + 1, size, team],
        )
        .map_err(internal)?;
        tx.commit().map_err(internal)?;
        next
    };
    relay.notify(&team, |s| s.head = s.head.max(seq));
    Ok(Json(json!({ "seq": seq })).into_response())
}

async fn get_envelopes(State(relay): State<Arc<Relay>>, Path(team): Path<String>, uri: Uri, headers: HeaderMap) -> Reply {
    let authed = relay.authorize(&team, "GET", &uri, &headers, b"")?;
    let after = query_u64(&uri, "after").unwrap_or(0);
    let wait = query_u64(&uri, "wait").unwrap_or(0).min(relay.limits.max_wait_s);
    let mut rx = relay.listen(&team).map_err(internal)?;
    let mut acl_version = rx.borrow_and_update().acl_version;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
    loop {
        let batch = relay.batch_for(&team, after, authed.from_seq, BATCH).map_err(internal)?;
        if !batch.items.is_empty() || batch.reset || tokio::time::Instant::now() >= deadline {
            return Ok(Json(batch).into_response());
        }
        // Sleep until this team's mailbox or access list changes (never on other teams' posts).
        match tokio::time::timeout_at(deadline, rx.changed()).await {
            Ok(Ok(())) => {
                let signal = *rx.borrow_and_update();
                if signal.acl_version != acl_version {
                    acl_version = signal.acl_version;
                    if relay.member_from(&team, &authed.device).map_err(internal)?.is_none() {
                        return Err(refuse(StatusCode::FORBIDDEN, "device not on this team"));
                    }
                }
            }
            Ok(Err(_)) | Err(_) => {} // listener gone, or the deadline: one last look
        }
    }
}

async fn ws(
    State(relay): State<Arc<Relay>>,
    Path(team): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Reply {
    let authed = relay.authorize(&team, "GET", &uri, &headers, b"")?;
    let after = query_u64(&uri, "after").unwrap_or(0);
    let slot = {
        let mut sockets = relay.sockets.lock().unwrap();
        let open = sockets.entry(authed.device.clone()).or_insert(0);
        if *open >= relay.limits.sockets_per_device {
            return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "too many open connections for this device"));
        }
        *open += 1;
        SocketSlot { relay: relay.clone(), device: authed.device.clone() }
    };
    let rx = relay.listen(&team).map_err(internal)?;
    Ok(upgrade
        .max_message_size(MAX_CLIENT_FRAME)
        .max_frame_size(MAX_CLIENT_FRAME)
        .on_upgrade(move |socket| async move {
            stream_team(socket, relay, team, authed, after, rx).await;
            drop(slot);
        }))
}

/// Send everything after `cursor` the device may read; returns the new cursor, or `None` when
/// the socket is gone.
async fn send_backlog(socket: &mut WebSocket, relay: &Relay, team: &str, authed: &Authed, mut cursor: u64) -> Option<u64> {
    loop {
        let batch = relay.batch_for(team, cursor, authed.from_seq, BATCH).ok()?;
        if batch.reset {
            let frame = serde_json::to_string(&Resync { reset: true, head: batch.head.unwrap_or(0) }).ok()?;
            socket.send(Message::Text(frame.into())).await.ok()?;
        }
        let full = batch.items.len() as u32 == BATCH;
        for item in &batch.items {
            let text = serde_json::to_string(item).ok()?;
            socket.send(Message::Text(text.into())).await.ok()?;
        }
        cursor = batch.next;
        if !full {
            return Some(cursor);
        }
    }
}

async fn stream_team(mut socket: WebSocket, relay: Arc<Relay>, team: String, authed: Authed, after: u64, mut rx: watch::Receiver<Signal>) {
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    let mut seen = *rx.borrow_and_update();
    let Some(mut cursor) = send_backlog(&mut socket, &relay, &team, &authed, after).await else { return };
    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    return;
                }
                let signal = *rx.borrow_and_update();
                // A device taken off the access list loses its open socket too, not only new
                // requests. Checked when the list changes, not on every wake.
                if signal.acl_version != seen.acl_version
                    && !matches!(relay.member_from(&team, &authed.device), Ok(Some(_)))
                {
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                }
                if signal.head > cursor {
                    match send_backlog(&mut socket, &relay, &team, &authed, cursor).await {
                        Some(next) => cursor = next,
                        None => return,
                    }
                }
                seen = signal;
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    // Clients only listen: anything they send is ignored and costs no query.
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

async fn post_join(
    State(relay): State<Arc<Relay>>,
    Path(team): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    let join: JoinBox = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad join request"))?;
    join.check_shape().map_err(|why| refuse(StatusCode::BAD_REQUEST, why))?;
    if join.team != team {
        return Err(refuse(StatusCode::BAD_REQUEST, "wrong team"));
    }
    relay.team_root(&team)?;
    let limits = &relay.limits;
    let addr = relay.addr(peer, &headers);
    // Per address and team first, then the team's daily cap, then the invite: a flood from one
    // address can't spend the team's cap, and once the team's cap is spent no new invite ids are
    // counted. An invite's attempts are counted for a join request's lifetime, then forgotten.
    if !relay.hit(format!("join-ip:{team}:{addr}"), HOUR_MS, limits.joins_per_address_per_hour)
        || !relay.hit(format!("joins-day:{team}"), DAY_MS, limits.joins_per_team_per_day)
        || !relay.hit(format!("invite:{team}:{}", join.invite), JOIN_TTL_MS, limits.joins_per_invite)
    {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "too many join requests"));
    }
    relay
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO joins (team, invite, at_ms, body) VALUES (?1, ?2, ?3, ?4)",
            params![team, join.invite, now_ms(), String::from_utf8_lossy(&body)],
        )
        .map_err(internal)?;
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn get_joins(State(relay): State<Arc<Relay>>, Path(team): Path<String>, uri: Uri, headers: HeaderMap) -> Reply {
    relay.authorize(&team, "GET", &uri, &headers, b"")?;
    let db = relay.db.lock().unwrap();
    let mut stmt = db.prepare("SELECT body FROM joins WHERE team = ?1 ORDER BY id").map_err(internal)?;
    let joins: Vec<JoinBox> = stmt
        .query_map(params![team], |r| r.get::<_, String>(0))
        .map_err(internal)?
        .flatten()
        .filter_map(|b| serde_json::from_str(&b).ok())
        .collect();
    Ok(Json(joins).into_response())
}

/// The relay's routes.
pub fn router(relay: Arc<Relay>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/teams", post(register).layer(DefaultBodyLimit::max(8 * 1024)))
        .route("/v1/teams/{team}/acl", put(set_acl).layer(DefaultBodyLimit::max(64 * 1024)))
        .route("/v1/teams/{team}/state/{slot}", put(put_state).get(get_state).layer(DefaultBodyLimit::max(MAX_STATE_BYTES)))
        .route("/v1/teams/{team}/envelopes", post(post_envelope).get(get_envelopes).layer(DefaultBodyLimit::max(MAX_ENVELOPE_BYTES)))
        .route("/v1/teams/{team}/ws", get(ws))
        .route("/v1/teams/{team}/joins", post(post_join).get(get_joins).layer(DefaultBodyLimit::max(MAX_JOIN_BYTES)))
        .with_state(relay)
}

/// Serve on `listener` until the process ends. Sweeps expired mail every 10 minutes.
pub async fn serve(listener: tokio::net::TcpListener, relay: Arc<Relay>) -> std::io::Result<()> {
    let sweeper = relay.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(600));
        loop {
            tick.tick().await;
            if let Ok(n) = sweeper.sweep() {
                if n > 0 {
                    tracing::info!(removed = n, "relay: expired mail removed");
                }
            }
        }
    });
    axum::serve(listener, router(relay).into_make_service_with_connect_info::<SocketAddr>()).await
}

/// A relay on a free loopback port in a background thread, for tests. Returns its address and the
/// relay (for test hooks).
pub fn spawn_local(config: Config) -> (SocketAddr, Arc<Relay>) {
    let relay = Relay::open(&config).expect("open relay store");
    let (tx, rx) = std::sync::mpsc::channel();
    let serving = relay.clone();
    std::thread::Builder::new()
        .name("pm-relay-test".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
                tx.send(listener.local_addr().expect("addr")).expect("report addr");
                let _ = serve(listener, serving).await;
            });
        })
        .expect("relay thread");
    (rx.recv().expect("relay started"), relay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_addresses_are_read_only_from_trusted_proxies() {
        let proxy: IpAddr = "10.0.0.5".parse().unwrap();
        let trusted = vec![Cidr::parse("10.0.0.0/8").unwrap()];
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "6.6.6.6, 203.0.113.9".parse().unwrap());
        // From the proxy: the right-most hop it saw (the left one is whatever the client claimed).
        assert_eq!(client_addr(proxy, &headers, &trusted), "203.0.113.9".parse::<IpAddr>().unwrap());
        // From anyone else, the header is ignored.
        let stranger: IpAddr = "198.51.100.7".parse().unwrap();
        assert_eq!(client_addr(stranger, &headers, &trusted), stranger);
        // No trusted proxies configured: always the peer.
        assert_eq!(client_addr(proxy, &headers, &[]), proxy);
        // A chain of trusted proxies is skipped.
        headers.insert("x-forwarded-for", "203.0.113.9, 10.1.1.1".parse().unwrap());
        assert_eq!(client_addr(proxy, &headers, &trusted), "203.0.113.9".parse::<IpAddr>().unwrap());
        // A garbled header: the peer.
        headers.insert("x-forwarded-for", "not-an-ip".parse().unwrap());
        assert_eq!(client_addr(proxy, &headers, &trusted), proxy);
        // IPv4-mapped IPv6 peers match IPv4 ranges.
        assert!(trusted[0].contains("::ffff:10.0.0.5".parse().unwrap()));
        assert!(Cidr::parse("::1").unwrap().contains("::1".parse().unwrap()));
        assert!(Cidr::parse("10.0.0.0/33").is_err());
    }

    #[test]
    fn ipv6_clients_are_counted_by_their_64() {
        let a = address_key("2001:db8:1:2:aaaa::1".parse().unwrap());
        let b = address_key("2001:db8:1:2:bbbb::9".parse().unwrap());
        assert_eq!(a, b);
        assert_ne!(a, address_key("2001:db8:1:3::1".parse().unwrap()));
        assert_eq!(address_key("192.0.2.1".parse().unwrap()), "192.0.2.1");
    }

    #[test]
    fn used_signatures_are_remembered_for_the_replay_window_then_forgotten() {
        let mut seen = SeenSigs::default();
        let t0 = 1_000_000;
        assert!(seen.first_use("a", t0, 10).is_ok());
        assert!(seen.first_use("a", t0 + 1, 10).is_err());
        // Still refused for the whole window, across one rotation.
        assert!(seen.first_use("a", t0 + SEEN_FOR_MS - 1, 10).is_err());
        assert!(seen.first_use("b", t0 + SEEN_FOR_MS, 10).is_ok());
        assert!(seen.first_use("a", t0 + SEEN_FOR_MS + 1, 10).is_err());
        // Two windows later both generations are gone: memory is bounded by the request rate.
        assert!(seen.first_use("c", t0 + 3 * SEEN_FOR_MS, 10).is_ok());
        assert_eq!(seen.len(), 1);
        // Full: refuse new work instead of growing.
        let mut tiny = SeenSigs::default();
        assert!(tiny.first_use("x", t0, 1).is_ok());
        assert_eq!(tiny.first_use("y", t0, 1).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn an_old_database_keeps_counting_from_its_newest_envelope() {
        let path = std::env::temp_dir().join(format!("pm-relay-migrate-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "CREATE TABLE teams (team TEXT PRIMARY KEY, root TEXT NOT NULL, salt TEXT NOT NULL,
                     acl_version INTEGER NOT NULL, acl TEXT NOT NULL, created_ms INTEGER NOT NULL);
                 CREATE TABLE envelopes (team TEXT NOT NULL, seq INTEGER NOT NULL, sender TEXT NOT NULL,
                     at_ms INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY (team, seq));
                 INSERT INTO teams VALUES ('T', 'root', 'salt', 1, '[\"dev\"]', 0);
                 INSERT INTO envelopes VALUES ('T', 41, 'dev', 0, '{}');",
            )
            .unwrap();
        }
        let relay = Relay::open(&Config { db: Some(path.clone()), ..Config::default() }).unwrap();
        assert_eq!(relay.signal_now("T").unwrap().head, 41, "the counter starts after the newest stored envelope");
        assert_eq!(relay.member_from("T", "dev").unwrap(), Some(0), "listed devices keep reading everything");
        drop(relay);
        let _ = std::fs::remove_file(&path);
    }
}
