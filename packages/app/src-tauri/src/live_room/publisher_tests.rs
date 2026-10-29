//! Tests for `live_room::publisher` (SPEC-n32 LR-9): snapshot and deltas, pause, screen hold-back, the stop reasons at
//! their thresholds, and the privacy check on every frame. The fake source and sink live here. Each frame is opened
//! with the test key, the way a viewer would open it. Planted secrets are built in the test, never written out.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pm_relay_protocol::room::{RoomFrame, RoomStatus};
use serde_json::Value;

use super::{PaneNow, Publisher, RoomPlan, Sink, Source, StopReason, Tick};
use crate::live_room::client::ClientError;
use crate::live_room::pin::ChatShare;
use crate::live_room::seal::{open, Sealer};
use crate::live_room::view::WAITING_LINE;
use crate::pty::status::PaneStatus;
use crate::redact::RedactContext;
use crate::session_chat::model::ChatItem;
use crate::tui::{Activity, ActivityKind};

const ROOM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY: [u8; 32] = [7; 32];
const SALT: [u8; 4] = [1, 2, 3, 4];
const T0: u64 = 1_000_000;
const SEC: u64 = 1_000;
const MIN: u64 = 60 * SEC;
const DELTA_LIMIT: usize = 12 * 1024;

const ENV_SECRET: &str = "pl4nted-env-secret-4242";
const EMAIL: &str = "jane.doe@example.test";
const USER_HOME: &str = concat!("C:\\Users\\", "Jane Doe Smith");
const USER_PATH: &str = concat!("C:\\Users\\", "Jane Doe Smith", "\\proj\\src\\a.rs");

fn api_key() -> String {
    format!("sk-ant-api03-{}", "A1".repeat(20))
}

fn opaque() -> String {
    "Qw3rT9yUi0".repeat(4)
}

fn ctx() -> RedactContext {
    RedactContext { env_values: vec![ENV_SECRET.to_string()], home: Some(USER_HOME.to_string()) }
}

fn agent(text: &str) -> ChatItem {
    ChatItem::Assistant { id: "x".into(), at: None, text: text.into(), model: None, final_answer: false }
}

#[derive(Clone)]
struct PaneSpec {
    id: String,
    status: Option<PaneStatus>,
    pending: bool,
    screen: bool,
    activity: Option<Activity>,
    label: String,
    title: Option<String>,
    chat: ChatShare,
}

fn pane(id: &str, screen: bool) -> PaneSpec {
    PaneSpec {
        id: id.into(),
        status: Some(PaneStatus::Running),
        pending: false,
        screen,
        activity: None,
        label: "Codex".into(),
        title: Some("Fix login".into()),
        chat: ChatShare::Shared,
    }
}

/// What the fake app knows: the panes, their chat logs, and the panes whose chat starts over on the next read.
#[derive(Default)]
struct World {
    panes: Vec<PaneSpec>,
    logs: HashMap<String, Vec<ChatItem>>,
    resets: HashSet<String>,
}

struct FakeSource(Rc<RefCell<World>>);

impl Source for FakeSource {
    fn panes(&mut self) -> Vec<PaneNow> {
        self.0
            .borrow()
            .panes
            .iter()
            .map(|spec| PaneNow {
                pane_id: spec.id.clone(),
                agent_label: spec.label.clone(),
                title: spec.title.clone(),
                status: spec.status,
                activity: spec.activity.clone(),
                pending_card: spec.pending,
                screen_source: spec.screen,
                chat: spec.chat,
            })
            .collect()
    }

    fn items_since(&mut self, pane_id: &str, rev: Option<u64>) -> Option<(u64, bool, Vec<ChatItem>)> {
        let mut world = self.0.borrow_mut();
        let reset = world.resets.remove(pane_id);
        if world.panes.iter().any(|spec| spec.id == pane_id && spec.chat != ChatShare::Shared) {
            return None;
        }
        let log = world.logs.get(pane_id)?;
        let len = log.len();
        let from = if reset { 0 } else { (rev.unwrap_or(0) as usize).min(len) };
        Some((len as u64, reset, log[from..].to_vec()))
    }
}

#[derive(Clone, Copy)]
enum Failure {
    Network,
    Gone,
}

/// What the fake relay got. `fail` makes every call fail the same way.
#[derive(Default)]
struct Wire {
    frames: Vec<RoomFrame>,
    fail: Option<Failure>,
    ended: u32,
    viewers: u32,
    /// How many status calls (heartbeats) the relay got.
    pings: u32,
}

struct FakeSink(Rc<RefCell<Wire>>);

impl FakeSink {
    fn accept(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        let mut wire = self.0.borrow_mut();
        match wire.fail {
            Some(Failure::Network) => Err(ClientError::Network("no answer".into())),
            Some(Failure::Gone) => Err(ClientError::Gone(None)),
            None => {
                wire.frames.push(frame.clone());
                Ok(RoomStatus { viewers: wire.viewers, expires_at_ms: 0 })
            }
        }
    }
}

impl Sink for FakeSink {
    fn ping(&mut self) -> Result<RoomStatus, ClientError> {
        let mut wire = self.0.borrow_mut();
        wire.pings += 1;
        match wire.fail {
            Some(Failure::Network) => Err(ClientError::Network("no answer".into())),
            Some(Failure::Gone) => Err(ClientError::Gone(None)),
            None => Ok(RoomStatus { viewers: wire.viewers, expires_at_ms: 0 }),
        }
    }

    fn snapshot(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        self.accept(frame)
    }

    fn frame(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        self.accept(frame)
    }

    fn end(&mut self) {
        self.0.borrow_mut().ended += 1;
    }
}

struct Rig {
    world: Rc<RefCell<World>>,
    wire: Rc<RefCell<Wire>>,
}

impl Rig {
    fn new(panes: Vec<PaneSpec>) -> Self {
        let world = World { panes, ..Default::default() };
        Self { world: Rc::new(RefCell::new(world)), wire: Rc::default() }
    }

    fn publisher(&self, ends_at_ms: u64) -> Publisher<FakeSource, FakeSink> {
        let pane_ids = self.world.borrow().panes.iter().map(|spec| spec.id.clone()).collect();
        let plan = RoomPlan { room_id: ROOM.into(), title: "Live Room".into(), pane_ids, ends_at_ms };
        Publisher::new(plan, Sealer::from_parts(KEY, SALT), FakeSource(self.world.clone()), FakeSink(self.wire.clone()), ctx(), T0)
    }

    /// Adds a chat row to the pane's log, as the agent would.
    fn say(&self, pane_id: &str, text: &str) {
        let mut world = self.world.borrow_mut();
        world.logs.entry(pane_id.to_string()).or_default().push(agent(text));
    }

    fn set(&self, pane_id: &str, change: impl FnOnce(&mut PaneSpec)) {
        let mut world = self.world.borrow_mut();
        let spec = world.panes.iter_mut().find(|spec| spec.id == pane_id).expect("pane exists");
        change(spec);
    }

    /// Every frame, opened and parsed. Fails the test if one does not open.
    fn bodies(&self) -> Vec<Value> {
        self.wire
            .borrow()
            .frames
            .iter()
            .map(|frame| {
                let plain = open(&KEY, frame).expect("frame opens with the room key");
                serde_json::from_slice(&plain).expect("frame is JSON")
            })
            .collect()
    }

    /// The text of every frame, joined, for "this must not appear anywhere" checks.
    fn everything_sent(&self) -> String {
        self.bodies().iter().map(|body| body.to_string()).collect::<Vec<_>>().join("\n")
    }

    fn kinds(&self) -> Vec<String> {
        self.bodies().iter().map(|body| body["kind"].as_str().unwrap_or_default().to_string()).collect()
    }
}

/// The `text` of every row in one frame, in order.
fn texts(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .map(|items| items.iter().filter_map(|item| item["text"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn all_texts(bodies: &[Value]) -> Vec<String> {
    bodies.iter().flat_map(texts).collect()
}

#[test]
fn start_sends_a_snapshot_of_the_panes_and_their_rows() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.say("pa", "first");
    rig.say("pa", "second");
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("the first snapshot goes out");

    let bodies = rig.bodies();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["kind"], "snap");
    assert_eq!(bodies[0]["v"], 1);
    assert_eq!(bodies[0]["room"]["title"], "Live Room");
    assert_eq!(bodies[0]["room"]["paused"], false);
    assert_eq!(bodies[0]["room"]["hidden"], 0);
    assert_eq!(bodies[0]["panes"][0]["key"], "p1");
    assert_eq!(bodies[0]["panes"][0]["state"], "working");
    assert_eq!(texts(&bodies[0]), vec!["first", "second"]);
    assert!(publisher.status().live);
}

#[test]
fn preview_shows_the_same_rows_as_the_first_snapshot() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.say("pa", "one");
    rig.say("pa", "two");
    let (panes, items) = Publisher::<FakeSource, FakeSink>::preview(
        &mut FakeSource(rig.world.clone()),
        &["pa".to_string()],
        &ctx(),
    );
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");

    let body = &rig.bodies()[0];
    assert_eq!(serde_json::to_value(&items).unwrap(), body["items"]);
    assert_eq!(serde_json::to_value(&panes).unwrap(), body["panes"]);
}

#[test]
fn snapshot_keeps_the_last_sixty_rows_per_pane() {
    let rig = Rig::new(vec![pane("pa", false)]);
    for i in 0..100 {
        rig.say("pa", &format!("row {i}"));
    }
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");

    let shown = texts(&rig.bodies()[0]);
    assert_eq!(shown.len(), 60);
    assert_eq!(shown.first().map(String::as_str), Some("row 40"));
    assert_eq!(shown.last().map(String::as_str), Some("row 99"));
}

#[test]
fn new_rows_go_out_as_a_delta_in_order() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.say("pa", "a");
    rig.say("pa", "b");
    rig.say("pa", "c");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);

    let bodies = rig.bodies();
    assert_eq!(rig.kinds(), vec!["snap", "delta"]);
    assert_eq!(texts(&bodies[1]), vec!["a", "b", "c"]);
}

#[test]
fn a_burst_of_big_rows_arrives_in_order_within_the_frame_limits() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    // Words with spaces: one long run of one character would look like an opaque token and be redacted.
    let long = "lorem ipsum ".repeat(120);
    let expected: Vec<String> = (0..500).map(|i| format!("message {i} {long}")).collect();
    for text in &expected {
        rig.say("pa", text);
    }

    let mut now = T0;
    for _ in 0..200 {
        now += SEC;
        let before = rig.wire.borrow().frames.len();
        assert_eq!(publisher.tick(now), Tick::Continue);
        let sent = rig.wire.borrow().frames.len() - before;
        assert!(sent <= 2, "at most two frames in one tick, got {sent}");
        if all_texts(&rig.bodies()[1..]).len() == expected.len() {
            break;
        }
    }

    let frames = rig.wire.borrow().frames.clone();
    for frame in &frames[1..] {
        let plain = open(&KEY, frame).expect("opens");
        assert!(plain.len() <= DELTA_LIMIT, "delta plaintext {} is over the limit", plain.len());
        let body: Value = serde_json::from_slice(&plain).unwrap();
        assert!(body["items"].as_array().unwrap().len() <= 40);
    }
    assert_eq!(all_texts(&rig.bodies()[1..]), expected);
}

#[test]
fn rows_read_while_paused_never_reach_the_relay() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");

    publisher.pause(T0 + SEC);
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);
    let note = rig.bodies().last().cloned().expect("the paused note");
    assert_eq!(note["kind"], "state");
    assert_eq!(note["room"]["paused"], true);

    rig.say("pa", "private one");
    rig.say("pa", "private two");
    assert_eq!(publisher.tick(T0 + 2 * SEC), Tick::Continue);
    assert_eq!(publisher.status().hidden, 2);
    assert_eq!(rig.bodies().len(), 2, "only the snapshot and the paused note were sent");
    assert!(!rig.everything_sent().contains("private"));

    publisher.resume(T0 + 3 * SEC);
    rig.say("pa", "after resume");
    assert_eq!(publisher.tick(T0 + 3 * SEC), Tick::Continue);
    let snapshot = rig.bodies().last().cloned().expect("the resume snapshot");
    assert_eq!(snapshot["kind"], "snap");
    assert_eq!(snapshot["room"]["paused"], false);
    assert_eq!(snapshot["room"]["hidden"], 2);
    assert_eq!(texts(&snapshot), vec!["after resume"]);
    assert!(!rig.everything_sent().contains("private"));
}

#[test]
fn a_screen_chat_is_held_back_around_a_card_but_a_file_chat_is_not() {
    let rig = Rig::new(vec![pane("screen", true), pane("file", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.set("screen", |spec| spec.pending = true);
    rig.set("file", |spec| spec.pending = true);

    rig.say("screen", "dialog text from screen");
    rig.say("file", "file text while waiting");
    publisher.tick(T0 + SEC);

    rig.set("screen", |spec| spec.pending = false);
    rig.set("file", |spec| spec.pending = false);
    rig.say("screen", "one second after");
    publisher.tick(T0 + 2 * SEC);
    rig.say("screen", "two seconds after");
    publisher.tick(T0 + 3 * SEC);

    let sent = all_texts(&rig.bodies());
    assert!(sent.contains(&"file text while waiting".to_string()));
    assert!(sent.contains(&"two seconds after".to_string()));
    assert!(!sent.contains(&"dialog text from screen".to_string()));
    assert!(!sent.contains(&"one second after".to_string()));
    assert_eq!(publisher.status().hidden, 2);
}

#[test]
fn a_waiting_pane_shows_only_the_fixed_line() {
    let spec = PaneSpec {
        pending: true,
        activity: Some(Activity { kind: ActivityKind::Waiting, text: "Run rm on notes.txt?".into() }),
        ..pane("pa", false)
    };
    let rig = Rig::new(vec![spec]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");

    let body = rig.bodies()[0].clone();
    assert_eq!(body["panes"][0]["state"], "waiting");
    assert_eq!(body["panes"][0]["line"], WAITING_LINE);
    assert!(!rig.everything_sent().contains("notes.txt"));
}

#[test]
fn no_frame_carries_a_planted_secret_or_a_folder_name() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.say("pa", &format!("key {} mail {} env {} run {} at {} end", api_key(), EMAIL, ENV_SECRET, opaque(), USER_PATH));
    publisher.tick(T0 + SEC);

    let sent = rig.everything_sent();
    for secret in [api_key(), EMAIL.to_string(), ENV_SECRET.to_string(), opaque(), "Jane Doe Smith".to_string()] {
        assert!(!sent.contains(&secret), "secret leaked: {secret}");
    }
    assert!(!sent.contains("Users"));
    assert!(sent.contains("a.rs"), "the file name is kept");
}

#[test]
fn viewer_count_from_the_relay_lands_in_status() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.wire.borrow_mut().viewers = 4;
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    assert_eq!(publisher.status().viewers, 4);
}

#[test]
fn stop_ends_the_sink_once_and_reports_the_reason() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    publisher.stop(StopReason::Owner);
    publisher.stop(StopReason::AppQuit);

    assert_eq!(publisher.tick(T0 + SEC), Tick::Stopped(StopReason::Owner));
    assert_eq!(rig.wire.borrow().ended, 1);
    let status = publisher.status();
    assert!(!status.live);
    assert_eq!(status.stopped.as_deref(), Some("owner"));
}

#[test]
fn the_room_expires_at_its_end_and_not_a_tick_before() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + MIN);
    publisher.start(T0).expect("starts");
    for s in 1..60 {
        assert_eq!(publisher.tick(T0 + s * SEC), Tick::Continue, "at {s}s");
    }
    assert_eq!(publisher.tick(T0 + MIN), Tick::Stopped(StopReason::Expired));
    assert_eq!(rig.wire.borrow().ended, 1);
}

#[test]
fn the_room_stops_after_twenty_quiet_minutes_and_not_a_second_before() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 2 * 60 * MIN);
    publisher.start(T0).expect("starts");
    for s in 1..1200 {
        assert_eq!(publisher.tick(T0 + s * SEC), Tick::Continue, "at {s}s");
    }
    assert_eq!(publisher.tick(T0 + 1200 * SEC), Tick::Stopped(StopReason::Idle));
}

#[test]
fn pausing_does_not_reset_the_idle_clock() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 2 * 60 * MIN);
    publisher.start(T0).expect("starts");
    publisher.pause(T0 + SEC);
    rig.say("pa", "hidden while paused");
    for s in 1..1200 {
        assert_eq!(publisher.tick(T0 + s * SEC), Tick::Continue, "at {s}s");
    }
    assert_eq!(publisher.tick(T0 + 1200 * SEC), Tick::Stopped(StopReason::Idle));
}

#[test]
fn a_clock_jump_of_more_than_five_minutes_stops_the_room() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 2 * 60 * MIN);
    publisher.start(T0).expect("starts");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);
    let five = T0 + SEC + 5 * MIN;
    assert_eq!(publisher.tick(five), Tick::Continue, "exactly five minutes is not a jump");
    assert_eq!(publisher.tick(five + 5 * MIN + 1), Tick::Stopped(StopReason::ClockJump));
}

#[test]
fn the_room_stops_when_the_last_shared_pane_closes() {
    let rig = Rig::new(vec![pane("pa", false), pane("pb", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.set("pa", |spec| spec.status = None);
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue, "one pane is still open");
    rig.set("pb", |spec| spec.status = None);
    assert_eq!(publisher.tick(T0 + 2 * SEC), Tick::Stopped(StopReason::AllPanesClosed));
}

#[test]
fn three_failed_calls_over_thirty_seconds_stop_the_room() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.wire.borrow_mut().fail = Some(Failure::Network);
    for s in 1..=31 {
        rig.say("pa", "row");
        let tick = publisher.tick(T0 + s * SEC);
        if s < 31 {
            assert_eq!(tick, Tick::Continue, "at {s}s");
        } else {
            assert_eq!(tick, Tick::Stopped(StopReason::RelayLost));
        }
    }
    assert_eq!(rig.wire.borrow().ended, 1);
}

#[test]
fn the_relay_saying_gone_ends_the_room_at_once() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.wire.borrow_mut().fail = Some(Failure::Gone);
    rig.say("pa", "row");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Stopped(StopReason::RelayEnded));
}

#[test]
fn a_first_snapshot_the_relay_refuses_is_an_error_and_not_live() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.wire.borrow_mut().fail = Some(Failure::Network);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    assert!(publisher.start(T0).is_err());
    assert!(!publisher.status().live);
}

#[test]
fn a_chat_that_starts_over_sends_a_snapshot() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.say("pa", "old 1");
    rig.say("pa", "old 2");
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.world.borrow_mut().resets.insert("pa".to_string());
    rig.say("pa", "fresh");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);

    let last = rig.bodies().last().cloned().expect("a frame");
    assert_eq!(last["kind"], "snap");
    assert_eq!(texts(&last), vec!["old 1", "old 2", "fresh"]);
}

#[test]
fn a_snapshot_goes_out_every_sixty_seconds_and_nothing_between() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    for s in 1..=60 {
        assert_eq!(publisher.tick(T0 + s * SEC), Tick::Continue);
    }
    assert_eq!(rig.kinds(), vec!["snap", "snap"]);
}

#[test]
fn a_pane_state_change_goes_out_as_a_state_frame() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.set("pa", |spec| spec.status = Some(PaneStatus::Idle));
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);

    let last = rig.bodies().last().cloned().expect("a frame");
    assert_eq!(last["kind"], "state");
    assert_eq!(last["panes"][0]["state"], "idle");
    assert_eq!(last["items"].as_array().map(Vec::len), Some(0));
}

fn end_row(ms: Option<u64>) -> ChatItem {
    ChatItem::TurnEnd { id: "e".into(), at: None, outcome: crate::session_chat::model::TurnOutcome::Completed, duration_ms: ms }
}

fn user(text: &str) -> ChatItem {
    ChatItem::User { id: "u".into(), at: None, text: text.into() }
}

/// The `t` tag of every row in a frame.
fn tags(body: &Value) -> Vec<String> {
    body["items"].as_array().map(|rows| rows.iter().filter_map(|row| row["t"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[test]
fn a_late_second_turn_end_is_not_shown_again() {
    let rig = Rig::new(vec![pane("pa", false)]);
    {
        let mut world = rig.world.borrow_mut();
        let log = world.logs.entry("pa".to_string()).or_default();
        log.extend([user("hi"), agent("one"), end_row(None), end_row(Some(5000))]);
    }
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    let first = rig.bodies()[0].clone();
    assert_eq!(tags(&first), vec!["user", "agent", "end"], "the first snapshot has one end row");

    // The next turn: its end row, and then its late length line, arrive in later ticks.
    {
        let mut world = rig.world.borrow_mut();
        let log = world.logs.get_mut("pa").unwrap();
        log.extend([user("again"), agent("two"), end_row(None)]);
    }
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);
    rig.world.borrow_mut().logs.get_mut("pa").unwrap().push(end_row(Some(7000)));
    assert_eq!(publisher.tick(T0 + 2 * SEC), Tick::Continue);
    let sent: Vec<String> = rig.bodies().iter().skip(1).flat_map(tags).collect();
    assert_eq!(sent, vec!["user", "agent", "end"], "one end row for the second turn");
}

#[test]
fn a_turn_end_after_a_new_turn_is_shown() {
    let rig = Rig::new(vec![pane("pa", false)]);
    {
        let mut world = rig.world.borrow_mut();
        let log = world.logs.entry("pa".to_string()).or_default();
        log.extend([user("a"), end_row(None), user("b"), end_row(None)]);
    }
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    assert_eq!(tags(&rig.bodies()[0]), vec!["user", "end", "user", "end"]);
}

#[test]
fn a_pane_that_closes_keeps_its_label_and_title() {
    let rig = Rig::new(vec![pane("pa", false), pane("pb", false)]);
    rig.set("pa", |spec| {
        spec.label = "Claude Code".into();
        spec.title = Some("Fix the build".into());
    });
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    // The source no longer knows the pane at all: it is simply not listed.
    rig.world.borrow_mut().panes.retain(|spec| spec.id != "pa");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);
    for s in 2..=60 {
        publisher.tick(T0 + s * SEC);
    }
    let last_snapshot = rig.bodies().into_iter().rev().find(|body| body["kind"] == "snap").expect("a snapshot");
    let closed = &last_snapshot["panes"][0];
    assert_eq!(closed["state"], "gone");
    assert_eq!(closed["label"], "Claude Code");
    assert_eq!(closed["title"], "Fix the build");
}

#[test]
fn a_pane_reported_closed_with_its_name_is_shown_closed_with_that_name() {
    let rig = Rig::new(vec![pane("pa", false), pane("pb", false)]);
    rig.set("pa", |spec| {
        spec.label = "Claude Code".into();
        spec.status = None;
    });
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    let first = &rig.bodies()[0];
    assert_eq!(first["panes"][0]["state"], "gone");
    assert_eq!(first["panes"][0]["label"], "Claude Code");
    assert_eq!(first["panes"][0]["title"], "Fix login");
}

#[test]
fn a_pane_whose_chat_cannot_be_tied_to_it_shows_a_note_and_no_chat() {
    let rig = Rig::new(vec![pane("pa", false), pane("pb", false)]);
    rig.set("pa", |spec| spec.chat = ChatShare::NotPinned);
    rig.say("pa", "another session's words");
    rig.say("pb", "my words");
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);
    rig.say("pa", "more of another session");
    assert_eq!(publisher.tick(T0 + 2 * SEC), Tick::Continue);

    let all = rig.everything_sent();
    assert!(!all.contains("another session"), "nothing of the unpinned pane's chat: {all}");
    assert!(all.contains("my words"));
    let first = &rig.bodies()[0];
    let notes: Vec<&Value> = first["items"].as_array().unwrap().iter().filter(|row| row["t"] == "note").collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["pane"], "p1");
    assert!(notes[0]["text"].as_str().unwrap().contains("not shared"));
    let notes_in_all = all.matches("is not shared").count();
    assert_eq!(notes_in_all, 1, "told once, not on every tick");
}

#[test]
fn a_pane_that_becomes_pinned_starts_sharing_and_a_changed_session_stops_with_one_note() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.set("pa", |spec| spec.chat = ChatShare::NotPinned);
    rig.say("pa", "foreign");
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    // Pinned: its own chat is shared from now on.
    rig.set("pa", |spec| spec.chat = ChatShare::Shared);
    rig.say("pa", "mine");
    assert_eq!(publisher.tick(T0 + SEC), Tick::Continue);
    // The session changed: no more chat, one note.
    rig.set("pa", |spec| spec.chat = ChatShare::SessionChanged);
    rig.say("pa", "a new session's words");
    assert_eq!(publisher.tick(T0 + 2 * SEC), Tick::Continue);
    assert_eq!(publisher.tick(T0 + 3 * SEC), Tick::Continue);
    let all = rig.everything_sent();
    assert!(all.contains("mine"));
    assert!(!all.contains("a new session"));
    assert_eq!(all.matches("its session changed").count(), 1);
}

#[test]
fn a_quiet_room_asks_the_relay_every_ten_seconds_and_sends_no_frame_for_it() {
    let rig = Rig::new(vec![pane("pa", false)]);
    rig.wire.borrow_mut().viewers = 3;
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    for s in 1..=35 {
        assert_eq!(publisher.tick(T0 + s * SEC), Tick::Continue);
    }
    assert_eq!(rig.wire.borrow().pings, 3, "at 10 s, 20 s and 30 s");
    assert_eq!(rig.kinds(), vec!["snap"], "a status call is not a frame");
    assert_eq!(publisher.status().viewers, 3);
}

#[test]
fn a_quiet_room_stops_thirty_to_forty_seconds_after_the_relay_stops_answering() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    // Quiet for 3 s, then the relay stops answering (killed, or the network is gone).
    let lost_at = 3;
    for s in 1..=lost_at {
        assert_eq!(publisher.tick(T0 + s * SEC), Tick::Continue);
    }
    rig.wire.borrow_mut().fail = Some(Failure::Network);
    let mut stopped_after = None;
    for s in (lost_at + 1)..=120 {
        if publisher.tick(T0 + s * SEC) == Tick::Stopped(StopReason::RelayLost) {
            stopped_after = Some(s - lost_at);
            break;
        }
    }
    let waited = stopped_after.expect("the room stops by itself");
    assert!((30..=40).contains(&waited), "stopped {waited} s after the relay was lost");
    assert_eq!(rig.wire.borrow().ended, 1);
}

#[test]
fn the_relay_saying_gone_to_a_status_call_ends_a_quiet_room() {
    let rig = Rig::new(vec![pane("pa", false)]);
    let mut publisher = rig.publisher(T0 + 20 * MIN);
    publisher.start(T0).expect("starts");
    rig.wire.borrow_mut().fail = Some(Failure::Gone);
    let mut stopped = None;
    for s in 1..=15 {
        if publisher.tick(T0 + s * SEC) == Tick::Stopped(StopReason::RelayEnded) {
            stopped = Some(s);
            break;
        }
    }
    assert_eq!(stopped, Some(10));
}
