//! The relay's protocol, end to end over real HTTP and WebSocket on a loopback port.
//!
//! The same tests run against any relay that speaks the protocol: set `PM_RELAY_TEST_URL` (for
//! example `http://127.0.0.1:8787`, a `wrangler dev` of `relay/worker` started with `TEST_HOOKS=1`)
//! and they use it instead of the in-process axum relay. The few things only a test may do (drop an
//! envelope, tighten a limit) go through the relay's `/v1/_test/{team}/...` hooks then.

use ed25519_dalek::SigningKey;
use pm_relay::{spawn_local, Config, Limits};
use pm_relay_protocol::*;
use std::time::{Duration, Instant};

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

#[derive(Clone)]
struct Team {
    base: String,
    team: String,
    root: SigningKey,
    salt: Vec<u8>,
    device: SigningKey,
    /// The in-process relay (its test hooks), or `None` when `PM_RELAY_TEST_URL` points elsewhere.
    local: Option<std::sync::Arc<pm_relay::Relay>>,
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// A salt no other test (or earlier run against a long-lived relay) has used.
fn fresh_salt() -> Vec<u8> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("salt-{nanos}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)).into_bytes()
}

fn setup(limits: Limits) -> Team {
    let remote = std::env::var("PM_RELAY_TEST_URL").ok().map(|u| u.trim_end_matches('/').to_string());
    let (base, local) = match remote {
        Some(url) => (url, None),
        None => {
            let (addr, relay) = spawn_local(Config { db: None, limits: limits.clone(), ..Config::default() });
            (format!("http://{addr}"), Some(relay))
        }
    };
    let root = key(1);
    let salt = fresh_salt();
    let team = team_id(root.verifying_key().as_bytes(), &salt);
    let t = Team { base, team, root, salt, device: key(2), local };
    let reg = TeamRegistration {
        team: t.team.clone(),
        root: key_id(&t.root),
        salt: b64(&t.salt),
        acl: SignedAcl::signed(&t.team, 1, vec![key_id(&t.device)], &t.root),
    };
    let r = ureq::post(&format!("{}/v1/teams", t.base)).send_bytes(&serde_json::to_vec(&reg).unwrap()).unwrap();
    assert_eq!(r.status(), 201);
    if t.local.is_none() {
        // A remote relay has its own limits; tighten this team's through the test hook.
        let tight = serde_json::json!({
            "devicePerMinute": limits.device_per_minute,
            "devicePerDay": limits.device_per_day,
            "teamPerMinute": limits.team_per_minute,
            "deviceRequestsPerMinute": limits.device_requests_per_minute,
            "joinsPerInvite": limits.joins_per_invite,
            "joinsPerTeamPerDay": limits.joins_per_team_per_day,
            "joinsPerAddressPerHour": limits.joins_per_address_per_hour,
            "teamMaxEnvelopes": limits.team_max_envelopes,
            "teamMaxBytes": limits.team_max_bytes,
            "socketsPerDevice": limits.sockets_per_device,
        });
        let r = ureq::post(&format!("{}/v1/_test/{}/limits", t.base, t.team)).send_json(tight).expect("the relay's test hooks are on");
        assert_eq!(r.status(), 200);
    }
    t
}

/// Let `ms` of time pass for the relay's expiry (the mailbox's 7 days, the join requests' day, the
/// rate-limit windows) and run its cleanup.
fn age(t: &Team, ms: u64) {
    match &t.local {
        Some(relay) => {
            relay.sweep_at(now_ms() + ms).unwrap();
        }
        None => {
            let r = ureq::post(&format!("{}/v1/_test/{}/age?ms={ms}", t.base, t.team)).send_bytes(&[]).unwrap();
            assert_eq!(r.status(), 200);
        }
    }
}

/// What the relay holds in memory for rate limits and replay protection: (windows, remembered
/// request signatures).
fn memory(t: &Team) -> (usize, usize) {
    match &t.local {
        Some(relay) => {
            let (windows, sigs, _) = relay.memory();
            (windows, sigs)
        }
        None => {
            let state: serde_json::Value =
                ureq::get(&format!("{}/v1/_test/{}/alarm", t.base, t.team)).call().unwrap().into_json().unwrap();
            (state["windows"].as_u64().unwrap() as usize, state["seen"].as_u64().unwrap() as usize)
        }
    }
}

/// Forget an envelope as if the relay dropped it.
fn drop_envelope(t: &Team, seq: u64) {
    match &t.local {
        Some(relay) => {
            relay.drop_envelope(&t.team, seq).unwrap();
        }
        None => {
            let r = ureq::post(&format!("{}/v1/_test/{}/drop?seq={seq}", t.base, t.team)).send_bytes(&[]).unwrap();
            assert_eq!(r.status(), 200);
        }
    }
}

fn status<T>(r: Result<T, ureq::Error>) -> u16
where
    T: Into<ureq::Response>,
{
    match r {
        Ok(resp) => resp.into().status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("transport error: {e}"),
    }
}

fn signed(t: &Team, device: &SigningKey, method: &str, path: &str, body: &[u8]) -> Result<ureq::Response, ureq::Error> {
    let header = auth_header(device, method, path, now_ms(), body);
    let req = ureq::request(method, &format!("{}{}", t.base, path)).set(AUTH_HEADER, &header);
    if method == "GET" {
        req.call()
    } else {
        req.send_bytes(body)
    }
}

fn envelope(t: &Team, device: &SigningKey, seq: u64) -> Envelope {
    let mut env = Envelope {
        v: 1,
        id: format!("01J{:023}", seq),
        team: t.team.clone(),
        epoch: 1,
        from: String::new(),
        seq,
        nonce: b64(&[seq as u8; 24]),
        ct: b64(b"ciphertext the relay cannot read"),
        sig: String::new(),
    };
    env.sign_with(device);
    env
}

fn post(t: &Team, device: &SigningKey, env: &Envelope) -> u16 {
    status(signed(t, device, "POST", &format!("/v1/teams/{}/envelopes", t.team), &serde_json::to_vec(env).unwrap()))
}

fn poll(t: &Team, after: u64, wait: u64) -> Batch {
    let r = signed(t, &t.device, "GET", &format!("/v1/teams/{}/envelopes?after={after}&wait={wait}", t.team), b"").unwrap();
    serde_json::from_reader(r.into_reader()).unwrap()
}

#[test]
fn registration_binds_the_team_to_its_root_key() {
    let t = setup(Limits::default());
    let health: serde_json::Value = ureq::get(&format!("{}/v1/health", t.base)).call().unwrap().into_json().unwrap();
    assert_eq!(health["protocol"], PROTOCOL);
    // The same team again: refused.
    let again = TeamRegistration {
        team: t.team.clone(),
        root: key_id(&t.root),
        salt: b64(&t.salt),
        acl: SignedAcl::signed(&t.team, 1, vec![key_id(&key(9))], &t.root),
    };
    assert_eq!(status(ureq::post(&format!("{}/v1/teams", t.base)).send_bytes(&serde_json::to_vec(&again).unwrap())), 409);
    // Another key claiming a team id it doesn't hash to: refused.
    let thief = key(7);
    let fake = TeamRegistration {
        team: team_id(t.root.verifying_key().as_bytes(), b"other"),
        root: key_id(&thief),
        salt: b64(b"other"),
        acl: SignedAcl::signed(&team_id(t.root.verifying_key().as_bytes(), b"other"), 1, vec![], &thief),
    };
    assert_eq!(status(ureq::post(&format!("{}/v1/teams", t.base)).send_bytes(&serde_json::to_vec(&fake).unwrap())), 400);
}

#[test]
fn only_listed_devices_post_and_read_and_requests_cannot_be_replayed() {
    let t = setup(Limits::default());
    let outsider = key(5);
    // No signature at all.
    let body = serde_json::to_vec(&envelope(&t, &t.device, 1)).unwrap();
    assert_eq!(status(ureq::post(&format!("{}/v1/teams/{}/envelopes", t.base, t.team)).send_bytes(&body)), 401);
    // A device that is not on the access list.
    assert_eq!(post(&t, &outsider, &envelope(&t, &outsider, 1)), 403);
    // A listed device posting an envelope signed by someone else.
    assert_eq!(post(&t, &t.device, &envelope(&t, &outsider, 1)), 403);
    // The real thing.
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 1)), 200);
    // The exact same signed request again: refused.
    let path = format!("/v1/teams/{}/envelopes", t.team);
    let header = auth_header(&t.device, "POST", &path, now_ms(), &body);
    let first = ureq::post(&format!("{}{}", t.base, path)).set(AUTH_HEADER, &header).send_bytes(&body);
    let second = ureq::post(&format!("{}{}", t.base, path)).set(AUTH_HEADER, &header).send_bytes(&body);
    assert_eq!(status(first), 200);
    assert_eq!(status(second), 401);
    // Reading needs the access list too.
    let r = signed(&t, &outsider, "GET", &format!("/v1/teams/{}/envelopes?after=0", t.team), b"");
    assert_eq!(status(r), 403);
}

#[test]
fn the_mailbox_hands_out_by_cursor_and_long_polls_wake_up() {
    let t = setup(Limits::default());
    for seq in 1..=3 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    let all = poll(&t, 0, 0);
    assert_eq!(all.items.iter().map(|d| d.envelope.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(all.next, 3);
    assert!(poll(&t, 3, 0).items.is_empty());
    // A waiting poll returns as soon as something arrives.
    let t2 = t.clone();
    let started = Instant::now();
    let waiter = std::thread::spawn(move || poll(&t2, 3, 10));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 200);
    let got = waiter.join().unwrap();
    assert_eq!(got.items.len(), 1);
    assert!(started.elapsed() < Duration::from_secs(5), "woke up early, not at the deadline");
    // A dropped envelope leaves a hole the cursor skips over (clients see the gap by counters).
    drop_envelope(&t, 2);
    assert_eq!(poll(&t, 0, 0).items.iter().map(|d| d.envelope.seq).collect::<Vec<_>>(), vec![1, 3, 4]);
}

#[test]
fn the_websocket_sends_the_backlog_then_live_envelopes() {
    let t = setup(Limits::default());
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 1)), 200);
    let path = format!("/v1/teams/{}/ws?after=0", t.team);
    let url = format!("{}{}", t.base.replace("http://", "ws://"), path);
    use tungstenite::client::IntoClientRequest;
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(AUTH_HEADER, auth_header(&t.device, "GET", &path, now_ms(), b"").parse().unwrap());
    let (mut socket, _) = tungstenite::connect(request).unwrap();
    let read = |socket: &mut tungstenite::WebSocket<_>| -> Delivered {
        loop {
            if let tungstenite::Message::Text(text) = socket.read().unwrap() {
                return serde_json::from_str(&text).unwrap();
            }
        }
    };
    assert_eq!(read(&mut socket).envelope.seq, 1);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 2)), 200);
    assert_eq!(read(&mut socket).envelope.seq, 2);
    // Taken off the access list: the open socket is closed.
    let acl = SignedAcl::signed(&t.team, 2, vec![key_id(&key(9))], &t.root);
    assert_eq!(status(ureq::put(&format!("{}/v1/teams/{}/acl", t.base, t.team)).send_bytes(&serde_json::to_vec(&acl).unwrap())), 204);
    let closed = loop {
        match socket.read() {
            Ok(tungstenite::Message::Close(_)) | Err(_) => break true,
            Ok(_) => continue,
        }
    };
    assert!(closed, "a removed device's socket closes");
    // Without a signature the upgrade is refused.
    let bare = format!("{}{}", t.base.replace("http://", "ws://"), path);
    assert!(tungstenite::connect(bare).is_err());
}

#[test]
fn access_lists_and_state_need_the_root_key_and_never_go_back() {
    let t = setup(Limits::default());
    let newcomer = key(3);
    // An access list signed by a device instead of the root: refused.
    let forged = SignedAcl::signed(&t.team, 2, vec![key_id(&t.device), key_id(&newcomer)], &t.device);
    assert_eq!(status(ureq::put(&format!("{}/v1/teams/{}/acl", t.base, t.team)).send_bytes(&serde_json::to_vec(&forged).unwrap())), 403);
    assert_eq!(post(&t, &newcomer, &envelope(&t, &newcomer, 1)), 403);
    // Signed by the root: the newcomer is in.
    let acl = SignedAcl::signed(&t.team, 2, vec![key_id(&t.device), key_id(&newcomer)], &t.root);
    assert_eq!(status(ureq::put(&format!("{}/v1/teams/{}/acl", t.base, t.team)).send_bytes(&serde_json::to_vec(&acl).unwrap())), 204);
    assert_eq!(post(&t, &newcomer, &envelope(&t, &newcomer, 1)), 200);
    // An older list can't be replayed to lock them out again.
    let old = SignedAcl::signed(&t.team, 1, vec![key_id(&t.device)], &t.root);
    assert_eq!(status(ureq::put(&format!("{}/v1/teams/{}/acl", t.base, t.team)).send_bytes(&serde_json::to_vec(&old).unwrap())), 409);

    let path = format!("/v1/teams/{}/state/keys", t.team);
    assert_eq!(status(signed(&t, &t.device, "GET", &path, b"")), 204, "no state yet");
    let blob = SignedBlob::signed(&t.team, "keys", 2, b"sealed keys", &t.root);
    assert_eq!(status(signed(&t, &t.device, "PUT", &path, &serde_json::to_vec(&blob).unwrap())), 204);
    let got: SignedBlob = serde_json::from_reader(signed(&t, &newcomer, "GET", &path, b"").unwrap().into_reader()).unwrap();
    assert_eq!(got, blob);
    let by_device = SignedBlob::signed(&t.team, "keys", 3, b"evil", &t.device);
    assert_eq!(status(signed(&t, &t.device, "PUT", &path, &serde_json::to_vec(&by_device).unwrap())), 403);
    let older = SignedBlob::signed(&t.team, "keys", 1, b"old", &t.root);
    assert_eq!(status(signed(&t, &t.device, "PUT", &path, &serde_json::to_vec(&older).unwrap())), 409);
}

#[test]
fn join_requests_are_open_to_post_but_limited_and_read_by_members_only() {
    let t = setup(Limits::default());
    let join = JoinBox { team: t.team.clone(), invite: "INVITE1".into(), enc: b64(b"enc"), ct: b64(b"sealed"), at_ms: now_ms() };
    let url = format!("{}/v1/teams/{}/joins", t.base, t.team);
    for _ in 0..5 {
        assert_eq!(status(ureq::post(&url).send_bytes(&serde_json::to_vec(&join).unwrap())), 202);
    }
    assert_eq!(status(ureq::post(&url).send_bytes(&serde_json::to_vec(&join).unwrap())), 429, "5 tries per invite");
    assert_eq!(status(ureq::get(&url).call()), 401);
    let joins: Vec<JoinBox> =
        serde_json::from_reader(signed(&t, &t.device, "GET", &format!("/v1/teams/{}/joins", t.team), b"").unwrap().into_reader()).unwrap();
    assert_eq!(joins.len(), 5);
}

#[test]
fn a_chatty_device_is_slowed_down() {
    let t = setup(Limits { device_per_minute: 3, ..Limits::default() });
    for seq in 1..=3 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 429);
}

#[test]
fn the_mailbox_counter_never_restarts_after_a_quiet_week() {
    let t = setup(Limits::default());
    for seq in 1..=3 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    let cursor = poll(&t, 0, 0).next;
    assert_eq!(cursor, 3);
    // Nobody posts for more than the mailbox TTL: the cleanup empties the mailbox.
    age(&t, MAILBOX_TTL_MS + 60_000);
    assert!(poll(&t, 0, 0).items.is_empty(), "everything expired");
    // The next envelope must land after the cursor every client already holds.
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 200);
    let after = poll(&t, cursor, 0);
    assert_eq!(after.items.len(), 1, "a client at cursor {cursor} still receives new mail");
    assert!(after.items[0].seq > cursor);
}

#[test]
fn a_cursor_past_the_relays_counter_is_told_to_start_again() {
    // A restored or replaced relay database: the client's cursor is ahead of the team's counter.
    let t = setup(Limits::default());
    for seq in 1..=2 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    let batch = poll(&t, 812, 10);
    assert!(batch.reset, "the relay says the cursor is ahead of it");
    assert_eq!(batch.head, Some(2));
    assert_eq!(batch.items.iter().map(|d| d.seq).collect::<Vec<_>>(), vec![1, 2], "and starts again from the beginning");
    assert_eq!(batch.next, 2);
    // A cursor that is not ahead: no reset, the head is reported.
    let fine = poll(&t, 2, 0);
    assert!(!fine.reset && fine.items.is_empty());
    assert_eq!(fine.head, Some(2));
    // Over the WebSocket: a {"reset":true,"head":2} frame, then the mailbox from the start.
    let mut socket = open_ws(&t, &t.device, 812);
    let first = read_text(&mut socket);
    assert_eq!(serde_json::from_str::<Resync>(&first).unwrap(), Resync { reset: true, head: 2 });
    let item: Delivered = serde_json::from_str(&read_text(&mut socket)).unwrap();
    assert_eq!(item.seq, 1);
}

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn ws_request(t: &Team, device: &SigningKey, after: u64) -> tungstenite::handshake::client::Request {
    use tungstenite::client::IntoClientRequest;
    let path = format!("/v1/teams/{}/ws?after={after}", t.team);
    let mut request = format!("{}{}", t.base.replace("http://", "ws://"), path).into_client_request().unwrap();
    request.headers_mut().insert(AUTH_HEADER, auth_header(device, "GET", &path, now_ms(), b"").parse().unwrap());
    request
}

fn open_ws(t: &Team, device: &SigningKey, after: u64) -> Ws {
    tungstenite::connect(ws_request(t, device, after)).unwrap().0
}

fn read_text(socket: &mut Ws) -> String {
    loop {
        if let tungstenite::Message::Text(text) = socket.read().unwrap() {
            return text.to_string();
        }
    }
}

fn put_acl(t: &Team, version: u64, devices: Vec<String>) -> u16 {
    let acl = SignedAcl::signed(&t.team, version, devices, &t.root);
    status(ureq::put(&format!("{}/v1/teams/{}/acl", t.base, t.team)).send_bytes(&serde_json::to_vec(&acl).unwrap()))
}

#[test]
fn a_new_device_reads_only_from_when_it_was_added() {
    let t = setup(Limits::default());
    for seq in 1..=3 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    let newcomer = key(3);
    assert_eq!(put_acl(&t, 2, vec![key_id(&t.device), key_id(&newcomer)]), 204);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 200);
    let path = format!("/v1/teams/{}/envelopes?after=0&wait=0", t.team);
    let theirs: Batch = serde_json::from_reader(signed(&t, &newcomer, "GET", &path, b"").unwrap().into_reader()).unwrap();
    assert_eq!(theirs.items.iter().map(|d| d.seq).collect::<Vec<_>>(), vec![4], "nothing from before they joined");
    // Their WebSocket too.
    let mut socket = open_ws(&t, &newcomer, 0);
    let item: Delivered = serde_json::from_str(&read_text(&mut socket)).unwrap();
    assert_eq!(item.seq, 4);
    // Devices that were there keep the whole mailbox; a re-signed list doesn't move anyone.
    assert_eq!(put_acl(&t, 3, vec![key_id(&t.device), key_id(&newcomer)]), 204);
    assert_eq!(poll(&t, 0, 0).items.len(), 4);
}

#[test]
fn strangers_requests_are_not_remembered_and_windows_are_pruned() {
    let t = setup(Limits::default());
    let before = memory(&t);
    // A flood of requests signed by throwaway keys, each with a fresh signature.
    for i in 0..50u8 {
        let stranger = SigningKey::from_bytes(&[100 + i; 32]);
        let r = signed(&t, &stranger, "GET", &format!("/v1/teams/{}/state/keys", t.team), b"");
        assert_eq!(status(r), 403);
    }
    assert_eq!(memory(&t).1, before.1, "no signature of a device off the access list is stored");
    // A member's request is remembered (replay guard), and windows are forgotten once expired.
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 1)), 200);
    let (windows, sigs) = memory(&t);
    assert!(windows > 0 && sigs > before.1);
    age(&t, 2 * 86_400_000);
    assert_eq!(memory(&t).0, 0, "expired rate-limit windows are pruned");
}

#[test]
fn a_team_has_a_storage_quota() {
    let t = setup(Limits { team_max_envelopes: 2, ..Limits::default() });
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 1)), 200);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 2)), 200);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 3)), 507, "the mailbox is full");
    // Expiry frees the quota.
    age(&t, MAILBOX_TTL_MS + 60_000);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 3)), 200);
    // Bytes count too.
    let small = setup(Limits { team_max_bytes: 100, ..Limits::default() });
    assert_eq!(post(&small, &small.device, &envelope(&small, &small.device, 1)), 507);
}

fn register_from(base: &str, seed: u8, forwarded: &str) -> u16 {
    let root = key(seed);
    let team = team_id(root.verifying_key().as_bytes(), b"s");
    let reg = TeamRegistration { team: team.clone(), root: key_id(&root), salt: b64(b"s"), acl: SignedAcl::signed(&team, 1, vec![], &root) };
    status(ureq::post(&format!("{base}/v1/teams")).set("x-forwarded-for", forwarded).send_bytes(&serde_json::to_vec(&reg).unwrap()))
}

// Axum only: the trusted-proxy setting (`Config::trusted_proxies`, X-Forwarded-For) is how the axum relay
// finds the client behind a reverse proxy. The Worker has no such setting: Cloudflare sets
// `CF-Connecting-IP`, which a client cannot forge (see relay/worker/test/worker.test.mjs).
#[test]
fn per_address_limits_read_forwarded_addresses_only_from_trusted_proxies() {
    let limits = Limits { unauthenticated_per_ip_per_hour: 1, ..Limits::default() };
    let trusted = vec![pm_relay::Cidr::parse("127.0.0.1").unwrap()];
    let (addr, _) = spawn_local(Config { db: None, limits: limits.clone(), trusted_proxies: trusted });
    let base = format!("http://{addr}");
    // Behind the (trusted) proxy every client has its own budget.
    assert_eq!(register_from(&base, 11, "203.0.113.1"), 201);
    assert_eq!(register_from(&base, 12, "203.0.113.2"), 201);
    assert_eq!(register_from(&base, 13, "203.0.113.1"), 429, "the same client again");
    // Without trusting the proxy the header is ignored: everyone is the peer.
    let (plain, _) = spawn_local(Config { db: None, limits, ..Config::default() });
    let base = format!("http://{plain}");
    assert_eq!(register_from(&base, 21, "203.0.113.1"), 201);
    assert_eq!(register_from(&base, 22, "203.0.113.2"), 429, "a spoofed header buys nothing");
}

#[test]
fn join_limits_are_per_team_and_address_and_invite_counters_expire() {
    let t = setup(Limits { joins_per_address_per_hour: 3, ..Limits::default() });
    let url = format!("{}/v1/teams/{}/joins", t.base, t.team);
    let join = |invite: &str| JoinBox { team: t.team.clone(), invite: invite.into(), enc: b64(b"enc"), ct: b64(b"sealed"), at_ms: now_ms() };
    for i in 0..3 {
        assert_eq!(status(ureq::post(&url).send_bytes(&serde_json::to_vec(&join(&format!("JUNK{i}"))).unwrap())), 202);
    }
    assert_eq!(status(ureq::post(&url).send_bytes(&serde_json::to_vec(&join("REAL")).unwrap())), 429, "one address is capped for this team");
    // Another team on the same relay is not affected by that address's flood.
    let root2 = key(40);
    let salt2 = fresh_salt();
    let team2 = team_id(root2.verifying_key().as_bytes(), &salt2);
    let reg = TeamRegistration { team: team2.clone(), root: key_id(&root2), salt: b64(&salt2), acl: SignedAcl::signed(&team2, 1, vec![], &root2) };
    assert_eq!(status(ureq::post(&format!("{}/v1/teams", t.base)).send_bytes(&serde_json::to_vec(&reg).unwrap())), 201);
    let other = JoinBox { team: team2.clone(), invite: "INV".into(), enc: b64(b"enc"), ct: b64(b"sealed"), at_ms: now_ms() };
    assert_eq!(status(ureq::post(&format!("{}/v1/teams/{team2}/joins", t.base)).send_bytes(&serde_json::to_vec(&other).unwrap())), 202);
    // A day later the counters are gone (the per-invite one used to last forever).
    age(&t, 2 * 86_400_000);
    assert_eq!(memory(&t).0, 0);
}

#[test]
fn websockets_are_capped_per_device_and_client_frames_are_small() {
    let t = setup(Limits { sockets_per_device: 1, ..Limits::default() });
    let mut first = open_ws(&t, &t.device, 0);
    assert!(tungstenite::connect(ws_request(&t, &t.device, 0)).is_err(), "a second socket for the same device is refused");
    // A big client message closes the socket instead of being buffered.
    let _ = first.send(tungstenite::Message::Text("x".repeat(64 * 1024).into()));
    let closed = loop {
        match first.read() {
            Ok(tungstenite::Message::Close(_)) | Err(_) => break true,
            Ok(_) => continue,
        }
    };
    assert!(closed);
    // The slot is free again once the socket is gone.
    let deadline = Instant::now() + Duration::from_secs(5);
    while tungstenite::connect(ws_request(&t, &t.device, 0)).is_err() {
        assert!(Instant::now() < deadline, "the socket slot is released");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_device_that_was_removed_is_refused_even_by_a_poll_it_already_had_open() {
    let t = setup(Limits::default());
    let newcomer = key(3);
    assert_eq!(put_acl(&t, 2, vec![key_id(&t.device), key_id(&newcomer)]), 204);
    let waiting = {
        let (base, team, newcomer) = (t.base.clone(), t.team.clone(), newcomer.clone());
        std::thread::spawn(move || {
            let path = format!("/v1/teams/{team}/envelopes?after=0&wait=20");
            let header = auth_header(&newcomer, "GET", &path, now_ms(), b"");
            let started = Instant::now();
            let r = ureq::get(&format!("{base}{path}")).set(AUTH_HEADER, &header).call();
            (status(r), started.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(500));
    // The root takes the newcomer off the list while their long poll is waiting.
    assert_eq!(put_acl(&t, 3, vec![key_id(&t.device)]), 204);
    let (code, took) = waiting.join().unwrap();
    assert_eq!(code, 403, "the waiting poll is told it is no longer on the team");
    assert!(took < Duration::from_secs(10), "and at once, not when the wait runs out");
    // New requests from them are refused too, and the team's other device is unaffected.
    assert_eq!(post(&t, &newcomer, &envelope(&t, &newcomer, 1)), 403);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 1)), 200);
}

#[test]
fn a_device_has_a_cap_on_signed_requests_reads_included() {
    let t = setup(Limits { device_requests_per_minute: 5, ..Limits::default() });
    for _ in 0..5 {
        assert_eq!(status(signed(&t, &t.device, "GET", &format!("/v1/teams/{}/state/keys", t.team), b"")), 204);
    }
    assert_eq!(status(signed(&t, &t.device, "GET", &format!("/v1/teams/{}/state/keys", t.team), b"")), 429, "the sixth in a minute");
    // The cap is per device: another listed device still gets through.
    let other = key(3);
    assert_eq!(put_acl(&t, 2, vec![key_id(&t.device), key_id(&other)]), 204);
    assert_eq!(status(signed(&t, &other, "GET", &format!("/v1/teams/{}/state/keys", t.team), b"")), 204);
}

#[test]
fn a_full_mailbox_says_507_in_the_relays_words() {
    let t = setup(Limits { team_max_envelopes: 1, ..Limits::default() });
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 1)), 200);
    let path = format!("/v1/teams/{}/envelopes", t.team);
    let body = serde_json::to_vec(&envelope(&t, &t.device, 2)).unwrap();
    match signed(&t, &t.device, "POST", &path, &body) {
        Err(ureq::Error::Status(507, resp)) => {
            let said: serde_json::Value = resp.into_json().unwrap();
            assert_eq!(said, serde_json::json!({ "error": "the team's mailbox is full" }));
        }
        other => panic!("expected 507, got {:?}", other.map(|r| r.status())),
    }
}

/// A WebSocket upgrade written by hand so that a refusal's whole body can be read (a WebSocket
/// client library keeps only the part of it that arrived with the headers).
fn refused_upgrade(t: &Team, device: &SigningKey, after: u64) -> (u16, serde_json::Value) {
    use std::io::{Read, Write};
    let path = format!("/v1/teams/{}/ws?after={after}", t.team);
    let authority = t.base.trim_start_matches("http://");
    let mut stream = std::net::TcpStream::connect(authority).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let lines = [
        format!("GET {path} HTTP/1.1"),
        format!("Host: {authority}"),
        format!("{AUTH_HEADER}: {}", auth_header(device, "GET", &path, now_ms(), b"")),
        "Connection: Upgrade".to_string(),
        "Upgrade: websocket".to_string(),
        "Sec-WebSocket-Version: 13".to_string(),
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==".to_string(),
    ];
    let request = format!("{}\r\n\r\n", lines.join("\r\n"));
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 1024];
    let (head_end, length) = loop {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "the relay closed the connection without an answer");
        raw.extend_from_slice(&chunk[..n]);
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..at]).to_lowercase();
            let length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
            break (at + 4, length);
        }
    };
    while raw.len() < head_end + length {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "the body ended early");
        raw.extend_from_slice(&chunk[..n]);
    }
    let code: u16 = String::from_utf8_lossy(&raw[..12]).split(' ').nth(1).unwrap().parse().unwrap();
    (code, serde_json::from_slice(&raw[head_end..head_end + length]).unwrap_or(serde_json::Value::Null))
}

#[test]
fn a_socket_over_the_cap_is_refused_in_the_relays_words() {
    let t = setup(Limits { sockets_per_device: 2, ..Limits::default() });
    let _a = open_ws(&t, &t.device, 0);
    let _b = open_ws(&t, &t.device, 0);
    let (code, said) = refused_upgrade(&t, &t.device, 0);
    assert_eq!(code, 429);
    assert_eq!(said, serde_json::json!({ "error": "too many open connections for this device" }));
    // Another device has its own count.
    let other = key(3);
    assert_eq!(put_acl(&t, 2, vec![key_id(&t.device), key_id(&other)]), 204);
    let _c = open_ws(&t, &other, 0);
}

#[test]
fn a_removed_and_re_added_device_starts_again_at_the_head() {
    let t = setup(Limits::default());
    let other = key(3);
    for seq in 1..=2 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    assert_eq!(put_acl(&t, 2, vec![key_id(&t.device), key_id(&other)]), 204);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 3)), 200);
    let path = format!("/v1/teams/{}/envelopes?after=0&wait=0", t.team);
    let first: Batch = serde_json::from_reader(signed(&t, &other, "GET", &path, b"").unwrap().into_reader()).unwrap();
    assert_eq!(first.items.iter().map(|d| d.seq).collect::<Vec<_>>(), vec![3]);
    assert_eq!(first.head, Some(3));
    // Off the list and back on: their join point is now.
    assert_eq!(put_acl(&t, 3, vec![key_id(&t.device)]), 204);
    assert_eq!(status(signed(&t, &other, "GET", &path, b"")), 403);
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 200);
    assert_eq!(put_acl(&t, 4, vec![key_id(&t.device), key_id(&other)]), 204);
    let again: Batch = serde_json::from_reader(signed(&t, &other, "GET", &path, b"").unwrap().into_reader()).unwrap();
    assert!(again.items.is_empty(), "nothing from while they were away");
    assert_eq!(again.head, Some(4));
    assert_eq!(again.next, 4, "the cursor moves up to where they may read from");
    // A client that was ahead of the relay starts again at its join point (not at 0).
    let ahead = format!("/v1/teams/{}/envelopes?after=900&wait=0", t.team);
    let reset: Batch = serde_json::from_reader(signed(&t, &other, "GET", &ahead, b"").unwrap().into_reader()).unwrap();
    assert!(reset.reset);
    assert_eq!((reset.next, reset.head), (4, Some(4)), "next is max(head, join point) when there is nothing to send");
    assert!(reset.items.is_empty());
}
