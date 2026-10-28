//! Live Room over real HTTP on a loopback port (SPEC-n32 §3.7, §4 #5-#7, #9, #11).
//!
//! The owner signs with the room key (`x-pm-auth`), viewers send the read token and a seat in headers.
//! Set `PM_RELAY_TEST_URL` to run the calls against another relay; the tests that move the clock need
//! the in-process relay and return early there.

use ed25519_dalek::SigningKey;
use pm_relay::{spawn_local, Config, Limits, Relay};
use pm_relay_protocol::room::{read_hash, FrameKind, RoomFrame, RoomRegistration, ROOM_READ_HEADER, ROOM_SEAT_HEADER};
use pm_relay_protocol::{auth_header, b64, key_id};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MINUTE: u64 = 60 * 1000;
const HOUR: u64 = 60 * MINUTE;

fn real_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// A relay under test: its base URL, and the in-process relay (for its clock and dump) when there is one.
struct Ctx {
    base: String,
    local: Option<Arc<Relay>>,
}

impl Ctx {
    fn now(&self) -> u64 {
        match &self.local {
            Some(relay) => relay.room_now(),
            None => real_now(),
        }
    }

    fn advance(&self, ms: u64) {
        self.local.as_ref().expect("clock tests run in-process").advance_room_clock(ms);
    }
}

fn setup(limits: Limits) -> Ctx {
    match std::env::var("PM_RELAY_TEST_URL").ok().map(|u| u.trim_end_matches('/').to_string()) {
        Some(base) => Ctx { base, local: None },
        None => {
            let (addr, relay) = spawn_local(Config { db: None, limits, ..Config::default() });
            Ctx { base: format!("http://{addr}"), local: Some(relay) }
        }
    }
}

/// The status and the body of an answer (ureq returns an error for every non-2xx).
fn answer(result: Result<ureq::Response, ureq::Error>) -> (u16, String) {
    match result {
        Ok(resp) => (resp.status(), resp.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, resp)) => (code, resp.into_string().unwrap_or_default()),
        Err(e) => panic!("network error: {e}"),
    }
}

fn frame(room: &str, seq: u64, kind: FrameKind, ct_len: usize) -> RoomFrame {
    RoomFrame { v: 1, room: room.into(), seq, kind, nonce: b64(&[7u8; 12]), ct: b64(&vec![9u8; ct_len]) }
}

/// The token a room was registered with: 16 bytes.
fn token(seed: u8) -> [u8; 16] {
    [seed; 16]
}

fn register_with(ctx: &Ctx, owner: &SigningKey, ttl_ms: u64, max_viewers: u32, read: &[u8]) -> (u16, String) {
    let reg = RoomRegistration::signed(owner, ctx.now() + ttl_ms, max_viewers, read_hash(read));
    answer(ureq::post(&format!("{}/v1/rooms", ctx.base)).send_bytes(&serde_json::to_vec(&reg).unwrap()))
}

/// Registers a room for `owner` and returns its id.
fn register(ctx: &Ctx, owner: &SigningKey, read: &[u8]) -> String {
    let reg = RoomRegistration::signed(owner, ctx.now() + HOUR, 10, read_hash(read));
    let (status, body) = answer(ureq::post(&format!("{}/v1/rooms", ctx.base)).send_bytes(&serde_json::to_vec(&reg).unwrap()));
    assert_eq!(status, 201, "registration: {body}");
    reg.room
}

/// An owner call, signed by `signer` over the path and the body.
fn owner(ctx: &Ctx, signer: &SigningKey, method: &str, path: &str, body: &[u8]) -> (u16, String) {
    let header = auth_header(signer, method, path, ctx.now(), body);
    let url = format!("{}{}", ctx.base, path);
    let req = match method {
        "GET" => ureq::get(&url),
        "PUT" => ureq::put(&url),
        "POST" => ureq::post(&url),
        "DELETE" => ureq::delete(&url),
        other => panic!("method {other}"),
    };
    answer(req.set("x-pm-auth", &header).send_bytes(body))
}

fn post_frame(ctx: &Ctx, signer: &SigningKey, room: &str, f: &RoomFrame) -> (u16, String) {
    owner(ctx, signer, "POST", &format!("/v1/rooms/{room}/frames"), &serde_json::to_vec(f).unwrap())
}

fn put_snapshot(ctx: &Ctx, signer: &SigningKey, room: &str, f: &RoomFrame) -> (u16, String) {
    owner(ctx, signer, "PUT", &format!("/v1/rooms/{room}/snapshot"), &serde_json::to_vec(f).unwrap())
}

/// A viewer call: a GET with the read token and a seat in headers.
fn view(ctx: &Ctx, path: &str, read: &[u8], seat: &str) -> (u16, String) {
    answer(ureq::get(&format!("{}{}", ctx.base, path)).set(ROOM_READ_HEADER, &b64(read)).set(ROOM_SEAT_HEADER, seat).call())
}

fn seat(n: u8) -> String {
    b64(&[n; 16])
}

#[test]
fn a_room_runs_from_registration_to_viewer_reads() {
    let ctx = setup(Limits::default());
    let owner_key = key(3);
    let room = register(&ctx, &owner_key, &token(1));

    let (status, _) = view(&ctx, &format!("/v1/rooms/{room}/snapshot"), &token(1), &seat(1));
    assert_eq!(status, 204, "no snapshot yet");

    let (status, body) = put_snapshot(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Snap, 40));
    assert_eq!(status, 200, "snapshot: {body}");
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 2, FrameKind::Delta, 40)).0, 200);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 3, FrameKind::State, 40)).0, 200);

    let (status, body) = view(&ctx, &format!("/v1/rooms/{room}/snapshot"), &token(1), &seat(1));
    assert_eq!(status, 200);
    let snap: RoomFrame = serde_json::from_str(&body).unwrap();
    assert_eq!((snap.seq, snap.kind), (1, FrameKind::Snap));

    let (status, body) = view(&ctx, &format!("/v1/rooms/{room}/frames?after=1&wait=0"), &token(1), &seat(1));
    assert_eq!(status, 200);
    let batch: serde_json::Value = serde_json::from_str(&body).unwrap();
    let seqs: Vec<u64> = batch["frames"].as_array().unwrap().iter().map(|f| f["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, vec![2, 3]);
    assert_eq!(batch["next"], 3);
    assert!(batch["ended"].is_null());

    let (status, body) = view(&ctx, &format!("/v1/rooms/{room}/frames?after=3&wait=0"), &token(1), &seat(1));
    assert_eq!(status, 200);
    let batch: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(batch["frames"].as_array().unwrap().len(), 0);
    assert_eq!(batch["next"], 3);

    let (status, body) = owner(&ctx, &owner_key, "GET", &format!("/v1/rooms/{room}/status"), b"");
    assert_eq!(status, 200);
    let st: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(st["viewers"], 1, "the viewer seat is active");
}

#[test]
fn a_new_snapshot_drops_the_deltas_before_it() {
    let ctx = setup(Limits::default());
    let owner_key = key(4);
    let room = register(&ctx, &owner_key, &token(2));
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Delta, 40)).0, 200);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 2, FrameKind::Delta, 40)).0, 200);
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 3, FrameKind::Snap, 40)).0, 200);

    let (status, body) = view(&ctx, &format!("/v1/rooms/{room}/frames?after=0&wait=0"), &token(2), &seat(2));
    assert_eq!(status, 200);
    let batch: serde_json::Value = serde_json::from_str(&body).unwrap();
    let seqs: Vec<u64> = batch["frames"].as_array().unwrap().iter().map(|f| f["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, vec![3], "only the snapshot is left");
    assert_eq!(batch["next"], 3);
}

#[test]
fn a_wrong_token_an_unknown_room_and_a_bad_token_all_answer_404() {
    let ctx = setup(Limits::default());
    let owner_key = key(5);
    let room = register(&ctx, &owner_key, &token(6));
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Snap, 40)).0, 200);

    let snap = format!("/v1/rooms/{room}/snapshot");
    assert_eq!(view(&ctx, &snap, &token(7), &seat(1)).0, 404, "wrong token");
    assert_eq!(view(&ctx, &snap, &[6u8; 15], &seat(1)).0, 404, "a token of the wrong size");
    let unknown = "bbbbbbbbbbbbbbbbbbbbbbbbbb";
    assert_eq!(view(&ctx, &format!("/v1/rooms/{unknown}/snapshot"), &token(6), &seat(1)).0, 404, "unknown room");
    assert_eq!(view(&ctx, "/v1/rooms/BBBBBBBBBBBBBBBBBBBBBBBBBB/snapshot", &token(6), &seat(1)).0, 404, "bad room id");
    assert_eq!(view(&ctx, &snap, &token(6), &seat(1)).0, 200, "the right token opens it");
}

#[test]
fn viewers_cannot_write_and_owners_must_sign_with_the_room_key() {
    let ctx = setup(Limits::default());
    let owner_key = key(8);
    let room = register(&ctx, &owner_key, &token(9));
    let body = serde_json::to_vec(&frame(&room, 1, FrameKind::Delta, 40)).unwrap();

    let post = ureq::post(&format!("{}/v1/rooms/{room}/frames", ctx.base))
        .set(ROOM_READ_HEADER, &b64(&token(9)))
        .set(ROOM_SEAT_HEADER, &seat(1));
    assert_eq!(answer(post.send_bytes(&body)).0, 401, "a viewer POST has no signature");
    let put = ureq::put(&format!("{}/v1/rooms/{room}/snapshot", ctx.base))
        .set(ROOM_READ_HEADER, &b64(&token(9)))
        .set(ROOM_SEAT_HEADER, &seat(1));
    assert_eq!(answer(put.send_bytes(&body)).0, 401, "a viewer PUT has no signature");

    let stranger = key(10);
    assert_eq!(post_frame(&ctx, &stranger, &room, &frame(&room, 1, FrameKind::Delta, 40)).0, 403, "another key");
    assert_eq!(put_snapshot(&ctx, &stranger, &room, &frame(&room, 1, FrameKind::Snap, 40)).0, 403);
    assert_eq!(owner(&ctx, &stranger, "DELETE", &format!("/v1/rooms/{room}"), b"").0, 403);
    assert_eq!(owner(&ctx, &stranger, "GET", &format!("/v1/rooms/{room}/status"), b"").0, 403);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Delta, 40)).0, 200, "the owner still can");
}

#[test]
fn a_viewer_get_with_a_body_is_refused() {
    let ctx = setup(Limits::default());
    let owner_key = key(11);
    let room = register(&ctx, &owner_key, &token(12));
    let url = format!("{}/v1/rooms/{room}/snapshot", ctx.base);
    let (status, _) =
        answer(ureq::get(&url).set(ROOM_READ_HEADER, &b64(&token(12))).set(ROOM_SEAT_HEADER, &seat(1)).send_bytes(b"x"));
    assert_eq!(status, 413);
}

#[test]
fn oversize_bodies_are_refused_before_they_are_parsed() {
    let ctx = setup(Limits::default());
    let owner_key = key(13);
    let room = register(&ctx, &owner_key, &token(14));
    // A snapshot over 96 KiB, and a frame over 24 KiB: both 413 from the body limit, before the signature check.
    let big_snap = vec![b' '; 97 * 1024];
    assert_eq!(owner(&ctx, &owner_key, "PUT", &format!("/v1/rooms/{room}/snapshot"), &big_snap).0, 413);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Delta, 25 * 1024)).0, 413);
    // Small enough frames still pass.
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Delta, 40)).0, 200);
}

#[test]
fn a_frame_whose_sequence_number_does_not_rise_is_refused() {
    let ctx = setup(Limits::default());
    let owner_key = key(15);
    let room = register(&ctx, &owner_key, &token(16));
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 5, FrameKind::Delta, 40)).0, 200);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 5, FrameKind::Delta, 40)).0, 409, "same seq");
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 4, FrameKind::Delta, 40)).0, 409, "older seq");
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 5, FrameKind::Snap, 40)).0, 409, "snapshot must rise too");
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 6, FrameKind::Delta, 40)).0, 200);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 7, FrameKind::Snap, 40)).0, 400, "frames are delta or state");
}

#[test]
fn owner_frames_are_limited_to_a_burst_of_ten_then_two_a_second() {
    let ctx = setup(Limits::default());
    let owner_key = key(17);
    let room = register(&ctx, &owner_key, &token(18));
    let started = Instant::now();
    let mut accepted = 0u64;
    for seq in 1..=30u64 {
        if post_frame(&ctx, &owner_key, &room, &frame(&room, seq, FrameKind::Delta, 40)).0 == 200 {
            accepted += 1;
        }
    }
    let refill = (started.elapsed().as_secs_f64() * 2.0) as u64;
    assert!(accepted >= 10, "the first ten go through");
    assert!(accepted <= 10 + refill + 1, "then only 2 a second: accepted {accepted}");
    assert!(accepted < 30, "at least one call is limited");
}

#[test]
fn stopping_a_room_wipes_it_and_answers_410_stopped() {
    let ctx = setup(Limits::default());
    let owner_key = key(19);
    let room = register(&ctx, &owner_key, &token(20));
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Snap, 40)).0, 200);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 2, FrameKind::Delta, 40)).0, 200);

    assert_eq!(owner(&ctx, &owner_key, "DELETE", &format!("/v1/rooms/{room}"), b"").0, 204);
    let (status, body) = view(&ctx, &format!("/v1/rooms/{room}/snapshot"), &token(20), &seat(1));
    assert_eq!(status, 410);
    assert_eq!(body, "{\"ended\":\"stopped\"}");
    assert_eq!(view(&ctx, &format!("/v1/rooms/{room}/frames?after=0&wait=0"), &token(20), &seat(1)).0, 410, "frames gone");
    assert_eq!(view(&ctx, &format!("/v1/rooms/{room}/snapshot"), &token(21), &seat(1)).0, 404, "a wrong token still gets 404");
    assert_eq!(owner(&ctx, &owner_key, "DELETE", &format!("/v1/rooms/{room}"), b"").0, 410, "stopping twice");
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 3, FrameKind::Delta, 40)).0, 410);

    // A stopped room's id is never reused, even with a fresh registration.
    let again = RoomRegistration::signed(&owner_key, real_now() + HOUR, 10, read_hash(&token(20)));
    let (status, _) = answer(ureq::post(&format!("{}/v1/rooms", ctx.base)).send_bytes(&serde_json::to_vec(&again).unwrap()));
    assert_eq!(status, 409);
}

#[test]
fn expiry_answers_410_expired_then_404_after_the_tombstone() {
    let ctx = setup(Limits::default());
    if ctx.local.is_none() {
        return; // needs the in-process clock
    }
    let owner_key = key(22);
    let (status, _) = register_with(&ctx, &owner_key, 2 * MINUTE, 10, &token(23));
    assert_eq!(status, 201);
    let room = pm_relay_protocol::room::room_id(owner_key.verifying_key().as_bytes());
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Snap, 40)).0, 200);

    ctx.advance(3 * MINUTE);
    let (status, body) = view(&ctx, &format!("/v1/rooms/{room}/snapshot"), &token(23), &seat(1));
    assert_eq!(status, 410);
    assert_eq!(body, "{\"ended\":\"expired\"}");

    ctx.advance(HOUR + MINUTE);
    assert_eq!(view(&ctx, &format!("/v1/rooms/{room}/snapshot"), &token(23), &seat(1)).0, 404, "the tombstone is gone");
}

#[test]
fn max_viewers_gives_429_and_a_quiet_seat_frees_its_place_after_40_seconds() {
    let ctx = setup(Limits::default());
    if ctx.local.is_none() {
        return; // needs the in-process clock
    }
    let owner_key = key(24);
    let read = token(25);
    let (status, _) = register_with(&ctx, &owner_key, HOUR, 1, &read);
    assert_eq!(status, 201);
    let room = pm_relay_protocol::room::room_id(owner_key.verifying_key().as_bytes());
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Snap, 40)).0, 200);

    let snap = format!("/v1/rooms/{room}/snapshot");
    assert_eq!(view(&ctx, &snap, &read, &seat(1)).0, 200, "the first seat");
    let (status, body) = view(&ctx, &snap, &read, &seat(2));
    assert_eq!(status, 429);
    assert_eq!(body, "{\"full\":true}");
    assert_eq!(view(&ctx, &snap, &read, &seat(1)).0, 200, "the seat already in the room keeps its place");

    ctx.advance(41 * 1000);
    assert_eq!(view(&ctx, &snap, &read, &seat(2)).0, 200, "the first seat went quiet and frees its place");
}

#[test]
fn one_address_may_hold_four_seats_in_a_room() {
    let ctx = setup(Limits::default());
    let owner_key = key(26);
    let read = token(27);
    let room = register(&ctx, &owner_key, &read);
    let snap = format!("/v1/rooms/{room}/snapshot");
    for n in 1..=4u8 {
        assert_eq!(view(&ctx, &snap, &read, &seat(n)).0, 204, "seat {n}");
    }
    assert_eq!(view(&ctx, &snap, &read, &seat(5)).0, 429, "a fifth seat from the same address");
}

#[test]
fn a_long_poll_wakes_when_a_frame_arrives() {
    let ctx = setup(Limits::default());
    let owner_key = key(28);
    let read = token(29);
    let room = register(&ctx, &owner_key, &read);
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &frame(&room, 1, FrameKind::Snap, 40)).0, 200);

    let url = format!("{}/v1/rooms/{room}/frames?after=1&wait=5", ctx.base);
    let read_b64 = b64(&read);
    let waiter = std::thread::spawn(move || {
        let started = Instant::now();
        let (status, body) = answer(ureq::get(&url).set(ROOM_READ_HEADER, &read_b64).set(ROOM_SEAT_HEADER, &seat(1)).call());
        (status, body, started.elapsed())
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 2, FrameKind::Delta, 40)).0, 200);

    let (status, body, took) = waiter.join().unwrap();
    assert_eq!(status, 200);
    let batch: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(batch["frames"].as_array().unwrap().len(), 1);
    assert!(took < Duration::from_secs(4), "woke at once, not at the 5 s wait: {took:?}");
}

#[test]
fn registration_is_limited_per_address_per_hour() {
    let ctx = setup(Limits::default());
    for seed in 30..40u8 {
        let (status, body) = register_with(&ctx, &key(seed), HOUR, 10, &token(seed));
        assert_eq!(status, 201, "registration {seed}: {body}");
    }
    let (status, _) = register_with(&ctx, &key(40), HOUR, 10, &token(40));
    assert_eq!(status, 429, "the eleventh from one address");
}

#[test]
fn a_registration_must_be_signed_by_the_room_key_and_be_in_time() {
    let ctx = setup(Limits::default());
    let owner_key = key(41);
    let mut reg = RoomRegistration::signed(&owner_key, ctx.now() + HOUR, 10, read_hash(&token(41)));
    reg.max_viewers = 11;
    let (status, _) = answer(ureq::post(&format!("{}/v1/rooms", ctx.base)).send_bytes(&serde_json::to_vec(&reg).unwrap()));
    assert_eq!(status, 400, "a changed field breaks the signature");
    let (status, _) = register_with(&ctx, &owner_key, 9 * HOUR, 10, &token(41));
    assert_eq!(status, 400, "longer than eight hours");
    let (status, _) = register_with(&ctx, &owner_key, HOUR, 51, &token(41));
    assert_eq!(status, 400, "more than fifty viewers");
}

#[test]
fn the_viewer_page_is_served_with_a_strict_policy() {
    let ctx = setup(Limits::default());
    let resp = ureq::get(&format!("{}/r/", ctx.base)).call().expect("the viewer page");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.header("content-type"), Some("text/html; charset=utf-8"));
    let csp = resp.header("content-security-policy").expect("a CSP").to_string();
    for part in [
        "default-src 'none'",
        "script-src 'self'",
        "style-src 'self'",
        "connect-src 'self'",
        "frame-ancestors 'none'",
        "form-action 'none'",
    ] {
        assert!(csp.contains(part), "{part} in {csp}");
    }
    assert_eq!(resp.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(resp.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(resp.header("cross-origin-opener-policy"), Some("same-origin"));
    let body = resp.into_string().unwrap();
    assert!(!body.contains("<form") && !body.contains("<input") && !body.contains("<textarea"), "no inputs on the page");

    let js = ureq::get(&format!("{}/r/viewer.js", ctx.base)).call().expect("the viewer script");
    assert_eq!(js.status(), 200);
    assert!(js.header("content-security-policy").is_some());
    assert_eq!(ureq::get(&format!("{}/r/viewer.css", ctx.base)).call().expect("the viewer style").status(), 200);
}

#[test]
fn the_relay_dump_holds_ids_and_sizes_only() {
    let ctx = setup(Limits::default());
    let Some(relay) = ctx.local.clone() else {
        return; // the dump is a hook of the in-process relay
    };
    let owner_key = key(42);
    let read = token(43);
    let room = register(&ctx, &owner_key, &read);
    let secret = "SECRET-CIPHERTEXT-MARKER-0123456789";
    let mut f = frame(&room, 1, FrameKind::Snap, 40);
    f.ct = b64(secret.as_bytes());
    assert_eq!(put_snapshot(&ctx, &owner_key, &room, &f).0, 200);
    assert_eq!(post_frame(&ctx, &owner_key, &room, &frame(&room, 2, FrameKind::Delta, 40)).0, 200);

    let dump = relay.dump();
    assert!(dump.contains(&room), "the room id is listed");
    assert!(!dump.contains(&b64(secret.as_bytes())), "no ciphertext");
    assert!(!dump.contains(&b64(&read)), "no read token");
    assert!(!dump.contains(&key_id(&owner_key)), "no room key");
    assert!(!dump.contains(&read_hash(&read)), "no read hash");
}
