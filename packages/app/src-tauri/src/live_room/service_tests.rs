//! Tests for `live_room::service` (SPEC-n32 LR-12): the pane checks, the link, the window check, and a whole room on a
//! relay on this computer (`pm_relay::spawn_local`) with a fake chat source: start, a viewer's read, pause, stop,
//! a room that ends by itself, and a start the relay refuses. Nothing leaves the loopback address. Planted secrets
//! are built here from pieces, never written out.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pm_relay_protocol::b64;
use pm_relay_protocol::room::RoomFrame;

use super::*;
use crate::live_room::pin::ChatShare;
use crate::live_room::publisher::PaneNow;
use crate::live_room::seal::open;
use crate::pty::status::PaneStatus;
use crate::session_chat::model::ChatItem;

const ENV_SECRET: &str = "pl4nted-env-secret-4242";
const EMAIL: &str = "jane.doe@example.test";
const USER_HOME: &str = concat!("C:\\Users\\", "Jane Doe Smith");
const USER_PATH: &str = concat!("C:\\Users\\", "Jane Doe Smith", "\\proj\\src\\a.rs");
const PANE: &str = "pane-a";
const TICK_FAST: Duration = Duration::from_millis(20);

fn api_key() -> String {
    format!("sk-ant-api03-{}", "A1".repeat(20))
}

fn ctx() -> RedactContext {
    RedactContext { env_values: vec![ENV_SECRET.to_string()], home: Some(USER_HOME.to_string()) }
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-lr12-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn relay() -> (SocketAddr, String) {
    let addr = pm_relay::spawn_local(pm_relay::Config::default()).0;
    (addr, format!("http://{addr}"))
}

/// The windows' side: every status the room announced.
fn recorder() -> (Emit, Arc<Mutex<Vec<LiveStatus>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    (Arc::new(move |status: &LiveStatus| log.lock().push(status.clone())), seen)
}

fn env(dir: &PathBuf, relay: &str) -> (Env, Arc<Mutex<Vec<LiveStatus>>>) {
    let (emit, seen) = recorder();
    (Env { data_dir: dir.clone(), relay: relay.to_string(), emit, ctx: ctx() }, seen)
}

fn request(minutes: u32, viewers: u32) -> StartRequest {
    StartRequest { pane_ids: vec![PANE.to_string()], title: "Live Room".into(), minutes, max_viewers: viewers }
}

/// What the fake app shows: the pane (or none), and its chat.
#[derive(Default)]
struct World {
    gone: bool,
    log: Vec<ChatItem>,
}

#[derive(Clone, Default)]
struct FakeSource(Arc<Mutex<World>>);

impl Source for FakeSource {
    fn panes(&mut self) -> Vec<PaneNow> {
        if self.0.lock().gone {
            return Vec::new();
        }
        vec![PaneNow {
            pane_id: PANE.to_string(),
            agent_label: "Codex".into(),
            title: Some("Fix login".into()),
            status: Some(PaneStatus::Running),
            activity: None,
            pending_card: false,
            screen_source: false,
            chat: ChatShare::Shared,
        }]
    }

    fn items_since(&mut self, pane_id: &str, rev: Option<u64>) -> Option<(u64, bool, Vec<ChatItem>)> {
        if pane_id != PANE {
            return None;
        }
        let world = self.0.lock();
        let from = (rev.unwrap_or(0) as usize).min(world.log.len());
        Some((world.log.len() as u64, false, world.log[from..].to_vec()))
    }
}

fn user(text: &str) -> ChatItem {
    ChatItem::User { id: "u".into(), at: None, text: text.into() }
}

/// A hang detector, not a time limit: the room ticks and talks to a relay on its own threads, and a machine running
/// the whole suite stretches every one of those steps.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The three secret parts of a link.
struct Parts {
    room: String,
    token: String,
    key: [u8; 32],
}

fn parts(link: &str, relay: &str) -> Parts {
    let fragment = link.strip_prefix(&format!("{relay}/r/#")).expect("link = <relay>/r/#<fragment>");
    let pieces: Vec<&str> = fragment.split('.').collect();
    assert_eq!(pieces.len(), 3, "room.token.key");
    Parts {
        room: pieces[0].to_string(),
        token: pieces[1].to_string(),
        key: pm_relay_protocol::unb64(pieces[2]).expect("key b64").try_into().expect("32-byte key"),
    }
}

/// What a browser does: GET the snapshot with the read token and a seat, then open it with the key.
fn viewer_snapshot(relay: &str, parts: &Parts, token: &str) -> Result<String, u16> {
    let url = format!("{relay}/v1/rooms/{}/snapshot", parts.room);
    match ureq::get(&url).set("x-pm-room-read", token).set("x-pm-room-seat", &b64(&[9u8; 16])).call() {
        Ok(response) if response.status() == 200 => {
            let frame: RoomFrame = response.into_json().expect("frame json");
            Ok(String::from_utf8(open(&parts.key, &frame).expect("frame opens with the link's key")).unwrap())
        }
        Ok(response) => Err(response.status()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(_) => Err(0),
    }
}

#[test]
fn panes_are_checked_in_order_without_duplicates() {
    let known = |id: &str| (id != "closed").then(|| "claude".to_string());
    let ids = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    assert_eq!(check_panes(&ids(&["a", "b", "a"]), known).unwrap(), ids(&["a", "b"]));
    assert_eq!(check_panes(&[], known).unwrap_err(), NO_PANES);
    assert_eq!(check_panes(&ids(&["1", "2", "3", "4", "5", "6", "7"]), known).unwrap_err(), TOO_MANY);
    assert_eq!(check_panes(&ids(&["closed"]), known).unwrap_err(), PANE_GONE);
    assert_eq!(check_panes(&ids(&["a", "companion-x"]), known).unwrap_err(), COMPANION_PRIVATE);
}

#[test]
fn a_pane_whose_agent_has_no_chat_is_refused() {
    let shell = |_: &str| Some("powershell".to_string());
    assert_eq!(check_panes(&["a".to_string()], shell).unwrap_err(), NO_CHAT);
}

#[test]
fn the_link_keeps_every_secret_after_the_hash() {
    let link = build_link("https://relay.example.test/", "roomid", "tok", "key");
    assert_eq!(link, "https://relay.example.test/r/#roomid.tok.key");
    let (head, fragment) = link.split_once('#').unwrap();
    assert_eq!(fragment, "roomid.tok.key");
    for secret in ["roomid", "tok", "key"] {
        assert!(!head.contains(secret));
    }
}

#[test]
fn only_the_named_app_windows_pass_the_window_check() {
    let allowed = ["cockpit", "main"];
    assert!(check_window("cockpit", &allowed).is_ok());
    assert!(check_window("main", &allowed).is_ok());
    for label in ["pane-preview-x", "preview-1", "detached-pane-3", "settings", "askbar", ""] {
        assert_eq!(check_window(label, &allowed), Err(LIVE_ROOM_WINDOW_ONLY.to_string()), "{label}");
    }
}

#[test]
fn no_room_means_an_off_status_and_no_link() {
    let state = LiveRoomState::default();
    assert_eq!(state.status(), off_status());
    assert!(state.link().is_err());
    assert!(state.pause().is_err());
    assert!(state.resume().is_err());
    state.stop_all(StopReason::Owner);
}

#[test]
fn a_start_needs_a_relay_and_a_listed_length_and_viewer_limit() {
    let dir = temp_dir();
    let state = LiveRoomState::default();
    let (mut env, _) = env(&dir, "");
    let source = FakeSource::default();
    assert_eq!(state.start(&env, &request(60, 10), vec![PANE.into()], source.clone()).unwrap_err(), NO_RELAY);
    env.relay = "http://127.0.0.1:9".into();
    assert_eq!(state.start(&env, &request(45, 10), vec![PANE.into()], source.clone()).unwrap_err(), BAD_LENGTH);
    assert_eq!(state.start(&env, &request(60, 7), vec![PANE.into()], source).unwrap_err(), BAD_VIEWERS);
    assert!(!active::path(&dir).exists());
}

#[test]
fn a_room_runs_from_start_to_stop_and_the_viewer_reads_clean_text() {
    let (_addr, relay) = relay();
    let dir = temp_dir();
    let (env, seen) = env(&dir, &relay);
    let source = FakeSource::default();
    source.0.lock().log.push(user(&format!(
        "Fix the login bug. key {} mail {EMAIL} file {USER_PATH} env {ENV_SECRET}",
        api_key()
    )));
    let state = LiveRoomState::default();

    let before = now_ms();
    let started = state.start_ticking(&env, &request(60, 5), vec![PANE.into()], source.clone(), TICK_FAST).expect("start");
    assert!(started.ends_at_ms >= before + 60 * 60_000 && started.ends_at_ms <= now_ms() + 60 * 60_000);
    assert!(is_live());

    // The link: the room, the read token and the content key, all after the hash.
    let link = state.link().expect("link");
    let link_parts = parts(&link, &relay);
    assert_eq!(link_parts.room.len(), 26);

    // active.json names the room and holds the room key, never the content key or the read token.
    let file = std::fs::read_to_string(active::path(&dir)).expect("active.json");
    assert!(file.contains(&link_parts.room));
    assert!(!file.contains(&link_parts.token));
    assert!(!file.contains(&b64(&link_parts.key)));

    // A viewer with the link reads the first snapshot, and only clean text is in it.
    let text = viewer_snapshot(&relay, &link_parts, &link_parts.token).expect("snapshot");
    assert!(text.contains("Fix the login bug"), "{text}");
    assert!(text.contains("Live Room"), "the title: {text}");
    for secret in [api_key().as_str(), EMAIL, "Jane Doe Smith", "Users\\", ENV_SECRET, link_parts.room.as_str()] {
        assert!(!text.contains(secret), "the snapshot leaks {secret}: {text}");
    }
    // A wrong read token looks like a room that is not there.
    assert_eq!(viewer_snapshot(&relay, &link_parts, &b64(&[1u8; 16])), Err(404));

    // One room at a time.
    let again = state.start_ticking(&env, &request(60, 5), vec![PANE.into()], source.clone(), TICK_FAST);
    assert_eq!(again.unwrap_err(), ALREADY_RUNNING);

    // Pause and resume reach the room and the windows hear about them.
    assert!(!state.status().paused);
    state.pause().expect("pause");
    wait_until("paused", || state.status().paused);
    state.resume().expect("resume");
    wait_until("resumed", || !state.status().paused);

    // New chat reaches the viewers as a frame the relay holds (the status shows the room is working).
    source.0.lock().log.push(user("one more thing"));
    assert!(state.status().live);

    // Stop: the room is wiped on the relay, the file and the link are gone, the windows are told why.
    state.stop_all(StopReason::Owner);
    assert_eq!(state.status(), off_status());
    assert!(state.link().is_err());
    wait_until("the last event", || seen.lock().last().is_some_and(|s| s.stopped.is_some()));
    let last = seen.lock().last().cloned().unwrap();
    assert!(!last.live);
    assert_eq!(last.stopped.as_deref(), Some("owner"));
    assert!(seen.lock().first().is_some_and(|s| s.live), "the first event says the room is live");
    wait_until("active.json gone", || !active::path(&dir).exists());
    assert_eq!(viewer_snapshot(&relay, &link_parts, &link_parts.token), Err(410));

    // The slot is free for the next room, which gets a new link.
    let next = state.start_ticking(&env, &request(30, 5), vec![PANE.into()], source, TICK_FAST).expect("second room");
    assert!(next.ends_at_ms > 0);
    let second = state.link().expect("second link");
    assert_ne!(parts(&second, &relay).room, link_parts.room);
    state.stop_all(StopReason::Owner);
}

#[test]
fn a_room_whose_panes_all_closed_ends_by_itself_and_frees_the_slot() {
    let (_addr, relay) = relay();
    let dir = temp_dir();
    let (env, seen) = env(&dir, &relay);
    let source = FakeSource::default();
    let state = LiveRoomState::default();
    state.start_ticking(&env, &request(60, 5), vec![PANE.into()], source.clone(), TICK_FAST).expect("start");
    source.0.lock().gone = true;
    wait_until("the room to end", || seen.lock().last().is_some_and(|s| s.stopped.as_deref() == Some("all_panes_closed")));
    // Until the next start the status still says why.
    assert_eq!(state.status().stopped.as_deref(), Some("all_panes_closed"));
    // The last event goes out just before the slot moves to its ended state, so wait for that step.
    wait_until("the link to drop", || state.link().is_err());
    assert!(state.pause().is_err());
    wait_until("active.json gone", || !active::path(&dir).exists());
    source.0.lock().gone = false;
    state.start_ticking(&env, &request(60, 5), vec![PANE.into()], source, TICK_FAST).expect("a new room");
    state.stop_all(StopReason::Owner);
}

#[test]
fn a_window_that_hears_the_room_has_ended_finds_no_link_and_no_command_taken() {
    // The last status event goes out from the room's own thread; a window or the tray reacting to it at once asks
    // for the link or presses pause. Both must already be refused (the ending was published first, the cleanup
    // after it, so a test that looked a moment later passed and one that looked at once did not).
    let (_addr, relay) = relay();
    let dir = temp_dir();
    let state = LiveRoomState::default();
    let heard: Arc<Mutex<Vec<(bool, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let emit: Emit = {
        let (state, heard) = (state.clone(), heard.clone());
        Arc::new(move |status: &LiveStatus| {
            if status.stopped.is_some() {
                heard.lock().push((state.link().is_err(), state.pause().is_err()));
            }
        })
    };
    let env = Env { data_dir: dir.clone(), relay, emit, ctx: ctx() };
    let source = FakeSource::default();
    state.start_ticking(&env, &request(60, 5), vec![PANE.into()], source.clone(), TICK_FAST).expect("start");
    source.0.lock().gone = true;
    wait_until("the room to end", || !heard.lock().is_empty());
    assert_eq!(*heard.lock(), [(true, true)], "(no link, pause refused) at the moment of the last event");
    // The slot is free as soon as the windows heard it, before the room's thread has cleaned up (this start came
    // back "already running" on a starved machine).
    source.0.lock().gone = false;
    state.start_ticking(&env, &request(60, 5), vec![PANE.into()], source, TICK_FAST).expect("a new room at once");
    state.stop_all(StopReason::Owner);
    wait_until("active.json gone", || !active::path(&dir).exists());
}

#[test]
fn a_relay_that_refuses_leaves_no_file_and_no_room() {
    // Nothing listens on this address: the connection is refused at once.
    let dir = temp_dir();
    let (env, seen) = env(&dir, "http://127.0.0.1:1");
    let state = LiveRoomState::default();
    let error = state.start_ticking(&env, &request(60, 5), vec![PANE.into()], FakeSource::default(), TICK_FAST).unwrap_err();
    assert!(!error.contains("127.0.0.1"), "the address is not echoed: {error}");
    assert!(!active::path(&dir).exists(), "no file is left for a room that never started");
    assert!(seen.lock().is_empty(), "no event for a start that failed");
    assert_eq!(state.status(), off_status());
    // The slot was given back: a second start is tried, not refused as "already running".
    let again = state.start_ticking(&env, &request(60, 5), vec![PANE.into()], FakeSource::default(), TICK_FAST).unwrap_err();
    assert_ne!(again, ALREADY_RUNNING);
}

#[test]
fn the_limit_of_eight_hours_is_asked_with_a_margin() {
    let (_addr, relay) = relay();
    let dir = temp_dir();
    let (env, _) = env(&dir, &relay);
    let state = LiveRoomState::default();
    let started = state
        .start_ticking(&env, &request(480, 5), vec![PANE.into()], FakeSource::default(), TICK_FAST)
        .expect("8 hours is accepted by the relay");
    assert!(started.ends_at_ms < now_ms() + MAX_ROOM_LIFETIME_MS);
    state.stop_all(StopReason::Owner);
}
