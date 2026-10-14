//! The relay's protocol, end to end over real HTTP and WebSocket on a loopback port.

use ed25519_dalek::SigningKey;
use pm_relay::{spawn_local, Config, Limits};
use pm_relay_protocol::*;
use std::time::{Duration, Instant};

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

struct Team {
    base: String,
    team: String,
    root: SigningKey,
    salt: Vec<u8>,
    device: SigningKey,
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn setup(limits: Limits) -> (Team, std::sync::Arc<pm_relay::Relay>) {
    let (addr, relay) = spawn_local(Config { db: None, limits });
    let root = key(1);
    let salt = b"salt-1".to_vec();
    let team = team_id(root.verifying_key().as_bytes(), &salt);
    let t = Team { base: format!("http://{addr}"), team, root, salt, device: key(2) };
    let reg = TeamRegistration {
        team: t.team.clone(),
        root: key_id(&t.root),
        salt: b64(&t.salt),
        acl: SignedAcl::signed(&t.team, 1, vec![key_id(&t.device)], &t.root),
    };
    let r = ureq::post(&format!("{}/v1/teams", t.base)).send_bytes(&serde_json::to_vec(&reg).unwrap()).unwrap();
    assert_eq!(r.status(), 201);
    (t, relay)
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
    let (t, _) = setup(Limits::default());
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
    let (t, _) = setup(Limits::default());
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
    let (t, relay) = setup(Limits::default());
    for seq in 1..=3 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    let all = poll(&t, 0, 0);
    assert_eq!(all.items.iter().map(|d| d.envelope.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(all.next, 3);
    assert!(poll(&t, 3, 0).items.is_empty());
    // A waiting poll returns as soon as something arrives.
    let t2 = Team { base: t.base.clone(), team: t.team.clone(), root: t.root.clone(), salt: t.salt.clone(), device: t.device.clone() };
    let started = Instant::now();
    let waiter = std::thread::spawn(move || poll(&t2, 3, 10));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 200);
    let got = waiter.join().unwrap();
    assert_eq!(got.items.len(), 1);
    assert!(started.elapsed() < Duration::from_secs(5), "woke up early, not at the deadline");
    // A dropped envelope leaves a hole the cursor skips over (clients see the gap by counters).
    relay.drop_envelope(&t.team, 2).unwrap();
    assert_eq!(poll(&t, 0, 0).items.iter().map(|d| d.envelope.seq).collect::<Vec<_>>(), vec![1, 3, 4]);
}

#[test]
fn the_websocket_sends_the_backlog_then_live_envelopes() {
    let (t, _) = setup(Limits::default());
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
    // Without a signature the upgrade is refused.
    let bare = format!("{}{}", t.base.replace("http://", "ws://"), path);
    assert!(tungstenite::connect(bare).is_err());
}

#[test]
fn access_lists_and_state_need_the_root_key_and_never_go_back() {
    let (t, _) = setup(Limits::default());
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
    let (t, _) = setup(Limits::default());
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
    let (t, _) = setup(Limits { device_per_minute: 3, ..Limits::default() });
    for seq in 1..=3 {
        assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, seq)), 200);
    }
    assert_eq!(post(&t, &t.device, &envelope(&t, &t.device, 4)), 429);
}
