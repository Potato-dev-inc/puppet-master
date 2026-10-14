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
//! - **rate-limits** per device, per team, per invite and per address.
//!
//! What it never sees: names, branches, notes, PR titles, keys or code. Envelopes are end-to-end
//! encrypted and signed by their sender's device; the relay can only check the signature and
//! store, forward, delay or drop. Clients notice drops (per-sender counters).

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pm_relay_protocol::{
    verify_request, Batch, Delivered, Envelope, JoinBox, SignedAcl, SignedBlob, TeamRegistration, AUTH_HEADER, JOIN_TTL_MS,
    MAILBOX_TTL_MS, MAX_ENVELOPE_BYTES, MAX_JOIN_BYTES, MAX_STATE_BYTES, PROTOCOL, SLOTS,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

/// Limits (SAAS-PLAN §3.5), enforced here so self-hosters get them too.
#[derive(Debug, Clone)]
pub struct Limits {
    pub device_per_minute: u32,
    pub device_per_day: u32,
    pub team_per_minute: u32,
    pub joins_per_invite: u32,
    pub joins_per_team_per_day: u32,
    /// Team registrations and join posts per client address per hour.
    pub unauthenticated_per_ip_per_hour: u32,
    /// Longest long-poll wait (seconds).
    pub max_wait_s: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            device_per_minute: 60,
            device_per_day: 5_000,
            team_per_minute: 600,
            joins_per_invite: 5,
            joins_per_team_per_day: 20,
            unauthenticated_per_ip_per_hour: 30,
            max_wait_s: 25,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// SQLite file; `None` keeps everything in memory (tests).
    pub db: Option<PathBuf>,
    pub limits: Limits,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Default)]
struct Window {
    start_ms: u64,
    count: u32,
}

impl Window {
    /// Count one more in a window of `span_ms`; false when over `max`.
    fn hit(&mut self, now: u64, span_ms: u64, max: u32) -> bool {
        if now.saturating_sub(self.start_ms) >= span_ms {
            self.start_ms = now;
            self.count = 0;
        }
        self.count += 1;
        self.count <= max
    }
}

pub struct Relay {
    db: Mutex<Connection>,
    limits: Limits,
    /// Wakes WebSockets and long polls of a team (the payload is the team id).
    wake: broadcast::Sender<String>,
    windows: Mutex<HashMap<String, Window>>,
    /// Signatures already used (replay guard for signed requests) → forget after.
    seen_sigs: Mutex<HashMap<String, u64>>,
}

type Reply = Result<Response, (StatusCode, Json<serde_json::Value>)>;

fn refuse(status: StatusCode, error: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": error })))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    tracing::warn!(error = %e, "relay: internal error");
    refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
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
                 invite TEXT NOT NULL, at_ms INTEGER NOT NULL, body TEXT NOT NULL);",
        )?;
        let (wake, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            db: Mutex::new(conn),
            limits: config.limits.clone(),
            wake,
            windows: Mutex::new(HashMap::new()),
            seen_sigs: Mutex::new(HashMap::new()),
        }))
    }

    fn hit(&self, key: String, span_ms: u64, max: u32) -> bool {
        self.windows.lock().unwrap().entry(key).or_default().hit(now_ms(), span_ms, max)
    }

    fn team_root(&self, team: &str) -> Result<(String, Vec<String>, u64), (StatusCode, Json<serde_json::Value>)> {
        let db = self.db.lock().unwrap();
        let row: Option<(String, String, u64)> = db
            .query_row("SELECT root, acl, acl_version FROM teams WHERE team = ?1", params![team], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()
            .map_err(internal)?;
        let (root, acl, version) = row.ok_or_else(|| refuse(StatusCode::NOT_FOUND, "unknown team"))?;
        let devices: Vec<String> = serde_json::from_str(&acl).unwrap_or_default();
        Ok((root, devices, version))
    }

    /// A signed request from a device on the team's access list. Returns the device id.
    fn authorize(&self, team: &str, method: &str, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
        let header = headers
            .get(AUTH_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| refuse(StatusCode::UNAUTHORIZED, "signed request required"))?;
        let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let now = now_ms();
        let authed = verify_request(header, method, path, body, now).map_err(|why| refuse(StatusCode::UNAUTHORIZED, why))?;
        {
            let mut seen = self.seen_sigs.lock().unwrap();
            seen.retain(|_, until| *until > now);
            if seen.insert(authed.sig.clone(), now + 2 * pm_relay_protocol::AUTH_SKEW_MS + 1_000).is_some() {
                return Err(refuse(StatusCode::UNAUTHORIZED, "request replayed"));
            }
        }
        let (_, devices, _) = self.team_root(team)?;
        if !devices.contains(&authed.device) {
            return Err(refuse(StatusCode::FORBIDDEN, "device not on this team"));
        }
        Ok(authed.device)
    }

    fn batch_after(&self, team: &str, after: u64, limit: u32) -> rusqlite::Result<Batch> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT seq, at_ms, body FROM envelopes WHERE team = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3")?;
        let rows = stmt.query_map(params![team, after, limit], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?, r.get::<_, String>(2)?)))?;
        let mut items = Vec::new();
        for row in rows {
            let (seq, at_ms, body) = row?;
            if let Ok(envelope) = serde_json::from_str::<Envelope>(&body) {
                items.push(Delivered { seq, at_ms, envelope });
            }
        }
        let next = items.last().map(|d| d.seq).unwrap_or(after);
        Ok(Batch { items, next })
    }

    /// Drop envelopes and join requests past their time.
    pub fn sweep(&self) -> rusqlite::Result<usize> {
        let now = now_ms();
        let db = self.db.lock().unwrap();
        let a = db.execute("DELETE FROM envelopes WHERE at_ms < ?1", params![now.saturating_sub(MAILBOX_TTL_MS)])?;
        let b = db.execute("DELETE FROM joins WHERE at_ms < ?1", params![now.saturating_sub(JOIN_TTL_MS)])?;
        Ok(a + b)
    }

    /// Test hook: forget an envelope as if the relay dropped it.
    pub fn drop_envelope(&self, team: &str, seq: u64) -> rusqlite::Result<usize> {
        self.db.lock().unwrap().execute("DELETE FROM envelopes WHERE team = ?1 AND seq = ?2", params![team, seq])
    }

    /// Test hook: every stored byte, to check nothing readable is kept.
    pub fn dump(&self) -> String {
        let db = self.db.lock().unwrap();
        let mut out = String::new();
        for table in ["teams", "state", "envelopes", "joins"] {
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

fn query_u64(uri: &Uri, key: &str) -> Option<u64> {
    uri.query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.parse().ok()).flatten()
    })
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "protocol": PROTOCOL }))
}

async fn register(State(relay): State<Arc<Relay>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, body: Bytes) -> Reply {
    if !relay.hit(format!("ip:{}", addr.ip()), 3_600_000, relay.limits.unauthenticated_per_ip_per_hour) {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let reg: TeamRegistration = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad registration"))?;
    if !reg.verify() {
        return Err(refuse(StatusCode::BAD_REQUEST, "the team id, root key and access list don't match"));
    }
    let db = relay.db.lock().unwrap();
    let exists: Option<String> = db
        .query_row("SELECT root FROM teams WHERE team = ?1", params![reg.team], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if exists.is_some() {
        return Err(refuse(StatusCode::CONFLICT, "team already registered"));
    }
    db.execute(
        "INSERT INTO teams (team, root, salt, acl_version, acl, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![reg.team, reg.root, reg.salt, reg.acl.version, serde_json::to_string(&reg.acl.devices).unwrap_or_default(), now_ms()],
    )
    .map_err(internal)?;
    tracing::info!(team = %reg.team, "relay: team registered");
    Ok(StatusCode::CREATED.into_response())
}

async fn set_acl(State(relay): State<Arc<Relay>>, Path(team): Path<String>, body: Bytes) -> Reply {
    let acl: SignedAcl = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad access list"))?;
    let (root, _, version) = relay.team_root(&team)?;
    if acl.team != team || !acl.verify(&root) {
        return Err(refuse(StatusCode::FORBIDDEN, "not signed by the team's root key"));
    }
    if acl.version <= version {
        return if acl.version == version { Ok(StatusCode::NO_CONTENT.into_response()) } else { Err(refuse(StatusCode::CONFLICT, "older access list")) };
    }
    relay
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE teams SET acl = ?1, acl_version = ?2 WHERE team = ?3",
            params![serde_json::to_string(&acl.devices).unwrap_or_default(), acl.version, team],
        )
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn put_state(State(relay): State<Arc<Relay>>, Path((team, slot)): Path<(String, String)>, uri: Uri, headers: HeaderMap, body: Bytes) -> Reply {
    relay.authorize(&team, "PUT", &uri, &headers, &body)?;
    let blob: SignedBlob = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad state"))?;
    let (root, _, _) = relay.team_root(&team)?;
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
    let device = relay.authorize(&team, "POST", &uri, &headers, &body)?;
    let envelope: Envelope = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad envelope"))?;
    if envelope.team != team || envelope.from != device || !envelope.verify() {
        return Err(refuse(StatusCode::FORBIDDEN, "the envelope is not signed by this device for this team"));
    }
    let limits = &relay.limits;
    if !relay.hit(format!("dev-min:{device}"), 60_000, limits.device_per_minute)
        || !relay.hit(format!("dev-day:{device}"), 86_400_000, limits.device_per_day)
        || !relay.hit(format!("team-min:{team}"), 60_000, limits.team_per_minute)
    {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let seq = {
        let db = relay.db.lock().unwrap();
        let next: u64 = db
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM envelopes WHERE team = ?1", params![team], |r| r.get(0))
            .map_err(internal)?;
        db.execute(
            "INSERT INTO envelopes (team, seq, sender, at_ms, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![team, next, device, now_ms(), String::from_utf8_lossy(&body)],
        )
        .map_err(internal)?;
        next
    };
    let _ = relay.wake.send(team);
    Ok(Json(json!({ "seq": seq })).into_response())
}

async fn get_envelopes(State(relay): State<Arc<Relay>>, Path(team): Path<String>, uri: Uri, headers: HeaderMap) -> Reply {
    relay.authorize(&team, "GET", &uri, &headers, b"")?;
    let after = query_u64(&uri, "after").unwrap_or(0);
    let wait = query_u64(&uri, "wait").unwrap_or(0).min(relay.limits.max_wait_s);
    let mut rx = relay.wake.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
    loop {
        let batch = relay.batch_after(&team, after, 200).map_err(internal)?;
        if !batch.items.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(Json(batch).into_response());
        }
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Ok(woken)) if woken != team => continue,
                Ok(_) => break,             // our team, or lagged: look again
                Err(_) => break,            // deadline
            }
        }
    }
}

async fn ws(State(relay): State<Arc<Relay>>, Path(team): Path<String>, uri: Uri, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Reply {
    relay.authorize(&team, "GET", &uri, &headers, b"")?;
    let after = query_u64(&uri, "after").unwrap_or(0);
    Ok(upgrade.on_upgrade(move |socket| stream_team(socket, relay, team, after)))
}

async fn stream_team(mut socket: WebSocket, relay: Arc<Relay>, team: String, mut cursor: u64) {
    let mut rx = relay.wake.subscribe();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        // Send everything after the cursor.
        match relay.batch_after(&team, cursor, 200) {
            Ok(batch) => {
                for item in batch.items {
                    cursor = item.seq;
                    let Ok(text) = serde_json::to_string(&item) else { continue };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        return;
                    }
                }
            }
            Err(_) => return,
        }
        tokio::select! {
            woken = rx.recv() => {
                match woken {
                    Ok(t) if t != team => continue,
                    _ => continue, // our team, or lagged: send what's new
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

async fn post_join(State(relay): State<Arc<Relay>>, Path(team): Path<String>, ConnectInfo(addr): ConnectInfo<SocketAddr>, body: Bytes) -> Reply {
    if !relay.hit(format!("ip:{}", addr.ip()), 3_600_000, relay.limits.unauthenticated_per_ip_per_hour) {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let join: JoinBox = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad join request"))?;
    join.check_shape().map_err(|why| refuse(StatusCode::BAD_REQUEST, why))?;
    if join.team != team {
        return Err(refuse(StatusCode::BAD_REQUEST, "wrong team"));
    }
    relay.team_root(&team)?;
    if !relay.hit(format!("invite:{team}:{}", join.invite), u64::MAX / 2, relay.limits.joins_per_invite)
        || !relay.hit(format!("joins-day:{team}"), 86_400_000, relay.limits.joins_per_team_per_day)
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
