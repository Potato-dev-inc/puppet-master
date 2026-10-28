//! The running Live Room (SPEC-n32 3.8): one room per app, one thread, one `LiveRoomState` the Tauri commands
//! share. `start` registers the room on the relay, sends the first snapshot and starts the thread `live-room`.
//! The thread ticks the `Publisher` every second, takes Pause / Resume / Stop from the commands and tells the
//! windows about every change (`live://state`, a `LiveStatus`). `stop_all` ends the room from the Stop command, the
//! panic path and the app exit hook. `active.json` (see `active`) lets the next start end a room a crash left.
//!
//! The link (`<relay>/r/#<room>.<read token>.<content key>`) is built here and lives only in this struct's memory;
//! nothing here logs it, writes it to disk or puts it in an event. Nothing here sends a viewer's words anywhere: the
//! app never reads room data from the relay except the two numbers of a `RoomStatus`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use parking_lot::Mutex;
use pm_relay_protocol::b64;
use pm_relay_protocol::room::{read_hash, room_id, RoomRegistration, MAX_ROOM_LIFETIME_MS};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::active::{self, Active};
use super::client::{ClientError, RoomClient};
use super::publisher::{LiveStatus, Publisher, RoomPlan, Sink, Source, StopReason};
use super::seal::Sealer;
use super::source::ClientSink;
use super::{MAX_PANES, MINUTES, TICK, VIEWERS};
use crate::redact::RedactContext;

pub use super::source::WatchSource;

/// The name of the event the windows listen to.
pub const STATE_EVENT: &str = "live://state";

/// The sentence a window gets when a command is not allowed from it.
pub const LIVE_ROOM_WINDOW_ONLY: &str = "Live Room can only be used from the Puppet Master window.";
pub const NO_RELAY: &str = "Live Room needs a relay address. Set it in Settings \u{203a} General \u{203a} Live Room.";
const NOT_RUNNING: &str = "There is no Live Room running.";
const ALREADY_RUNNING: &str = "A Live Room is already running. Stop it before you start another.";
const NO_PANES: &str = "Pick at least one pane to share.";
const TOO_MANY: &str = "A Live Room can share up to 6 panes.";
const PANE_GONE: &str = "That pane is not open any more.";
const COMPANION_PRIVATE: &str = "Companion chats stay private.";
const NO_CHAT: &str = "No chat for this pane.";
const BAD_LENGTH: &str = "Pick a length from the list.";
const BAD_VIEWERS: &str = "Pick a viewer limit from the list.";
const REFUSED: &str = "The relay did not accept this Live Room. Check the relay address.";
const NO_THREAD: &str = "The Live Room could not be started on this PC.";

/// A room asks for at most this much of the relay's 8 hour limit, so a clock a minute off cannot refuse it.
const SKEW_MARGIN_MS: u64 = 60_000;
/// How long a stop waits for the room's thread to delete the room on the relay.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// Rooms running in this process. The tray tooltip reads it (`is_live`).
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Whether a Live Room is on right now (paused counts). One word for the tray tooltip.
pub fn is_live() -> bool {
    RUNNING.load(Ordering::SeqCst) > 0
}

/// Whether a command from the window labelled `label` may go on. `allowed` lists the app windows. A web page the
/// app shows (a pane preview) has another label and is refused.
pub fn check_window(label: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&label) {
        Ok(())
    } else {
        Err(LIVE_ROOM_WINDOW_ONLY.to_string())
    }
}

/// What the person picked in the Go live sheet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub pane_ids: Vec<String>,
    pub title: String,
    pub minutes: u32,
    pub max_viewers: u32,
}

/// The answer of a start. The link is not in it: the window asks for it (`live_room_copy_link`) when the person
/// copies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedRoom {
    pub ends_at_ms: u64,
}

/// Tells the windows the room changed.
pub type Emit = Arc<dyn Fn(&LiveStatus) + Send + Sync>;

/// What a start needs from the app.
pub struct Env {
    /// The app data folder (`active.json` goes under it).
    pub data_dir: PathBuf,
    /// The relay address, already checked (`config::effective_relay`).
    pub relay: String,
    pub emit: Emit,
    /// What counts as secret on this PC, as it is now.
    pub ctx: RedactContext,
}

/// The panes a room may share, in the viewers' order (`p1`, `p2`...). Duplicates are dropped. `agent_type_of` gives
/// the agent of an open pane. A companion's pane and a pane with no chat are refused with the reason.
pub fn check_panes(ids: &[String], agent_type_of: impl Fn(&str) -> Option<String>) -> Result<Vec<String>, String> {
    let mut panes: Vec<String> = Vec::new();
    for id in ids {
        if !panes.contains(id) {
            panes.push(id.clone());
        }
    }
    if panes.is_empty() {
        return Err(NO_PANES.to_string());
    }
    if panes.len() > MAX_PANES {
        return Err(TOO_MANY.to_string());
    }
    for id in &panes {
        if crate::companion::service::is_pane_id(id) {
            return Err(COMPANION_PRIVATE.to_string());
        }
        let agent = agent_type_of(id).ok_or_else(|| PANE_GONE.to_string())?;
        if crate::session_chat::Opening::for_agent(&agent).is_none() {
            return Err(NO_CHAT.to_string());
        }
    }
    Ok(panes)
}

/// What a command tells the room's thread.
enum Cmd {
    Pause,
    Resume,
    Stop(StopReason),
}

/// What the commands and the thread share.
struct Shared {
    status: Mutex<LiveStatus>,
    /// The link, until the room ends.
    link: Mutex<Option<String>>,
    done: StdMutex<bool>,
    ended: Condvar,
}

impl Shared {
    fn finish(&self) {
        *self.link.lock() = None;
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.ended.notify_all();
    }

    fn is_done(&self) -> bool {
        *self.done.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait_done(&self, limit: Duration) {
        let deadline = Instant::now() + limit;
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else { return };
            done = self.ended.wait_timeout(done, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

struct Running {
    tx: Sender<Cmd>,
    shared: Arc<Shared>,
}

#[derive(Default)]
enum Slot {
    #[default]
    Off,
    Starting,
    Running(Running),
}

#[derive(Default)]
struct Inner {
    slot: Mutex<Slot>,
}

/// The one Live Room of the app. Cheap to clone: every clone is the same room.
#[derive(Clone, Default)]
pub struct LiveRoomState {
    inner: Arc<Inner>,
}

impl LiveRoomState {
    /// Starts a room on `panes` (already checked) read through `source`. Registers it on the relay, sends the first
    /// snapshot, then runs the room on its own thread. Refuses a second room.
    pub fn start<S: Source + Send + 'static>(
        &self,
        env: &Env,
        req: &StartRequest,
        panes: Vec<String>,
        source: S,
    ) -> Result<StartedRoom, String> {
        self.start_ticking(env, req, panes, source, TICK)
    }

    /// As `start`, with the thread's tick given (tests use a short one).
    pub(super) fn start_ticking<S: Source + Send + 'static>(
        &self,
        env: &Env,
        req: &StartRequest,
        panes: Vec<String>,
        source: S,
        tick: Duration,
    ) -> Result<StartedRoom, String> {
        if !MINUTES.contains(&req.minutes) {
            return Err(BAD_LENGTH.to_string());
        }
        if !VIEWERS.contains(&req.max_viewers) {
            return Err(BAD_VIEWERS.to_string());
        }
        if env.relay.trim().is_empty() {
            return Err(NO_RELAY.to_string());
        }
        self.claim()?;
        let result = self.launch(env, req, panes, source, tick);
        if result.is_err() {
            *self.inner.slot.lock() = Slot::Off;
        }
        result
    }

    /// Takes the slot for a start. A room that ended by itself leaves its slot behind; that one is free.
    fn claim(&self) -> Result<(), String> {
        let mut slot = self.inner.slot.lock();
        match &*slot {
            Slot::Running(running) if !running.shared.is_done() => return Err(ALREADY_RUNNING.to_string()),
            Slot::Starting => return Err(ALREADY_RUNNING.to_string()),
            _ => {}
        }
        *slot = Slot::Starting;
        Ok(())
    }

    fn launch<S: Source + Send + 'static>(
        &self,
        env: &Env,
        req: &StartRequest,
        panes: Vec<String>,
        source: S,
        tick: Duration,
    ) -> Result<StartedRoom, String> {
        let now = now_ms();
        let ends_at = (now + u64::from(req.minutes) * 60_000).min(now + MAX_ROOM_LIFETIME_MS - SKEW_MARGIN_MS);
        let key = new_room_key();
        let mut token = [0u8; 16];
        OsRng.fill_bytes(&mut token);
        let sealer = Sealer::new_random();
        let client = RoomClient::new(&env.relay, key.clone())?;
        let room = room_id(key.verifying_key().as_bytes());
        let link = build_link(&env.relay, &room, &b64(&token), &sealer.key_b64());

        // The file comes first: a crash between the file and the registration costs only a call to a room that is
        // not there. A crash the other way round would leave a room nobody can end.
        active::write(&env.data_dir, &Active::new(&env.relay, &key, ends_at))?;
        let registration = RoomRegistration::signed(&key, ends_at, req.max_viewers, read_hash(&token));
        if let Err(error) = client.register(&registration) {
            active::remove_if(&env.data_dir, &room);
            return Err(describe(error));
        }

        let plan = RoomPlan { room_id: room.clone(), title: req.title.clone(), pane_ids: panes, ends_at_ms: ends_at };
        let mut publisher = Publisher::new(plan, sealer, source, ClientSink::new(client), env.ctx.clone(), now);
        if let Err(error) = publisher.start(now_ms()) {
            publisher.stop(StopReason::Owner);
            active::remove_if(&env.data_dir, &room);
            return Err(describe(error));
        }

        let initial = publisher.status();
        let shared = Arc::new(Shared {
            status: Mutex::new(initial.clone()),
            link: Mutex::new(Some(link)),
            done: StdMutex::new(false),
            ended: Condvar::new(),
        });
        let (tx, rx) = mpsc::channel();
        let cell = Arc::new(StdMutex::new(Some(publisher)));
        let (for_thread, shared_for_thread) = (cell.clone(), shared.clone());
        let (emit, dir, room_for_thread) = (env.emit.clone(), env.data_dir.clone(), room.clone());
        RUNNING.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new().name("live-room".into()).spawn(move || {
            let publisher = for_thread.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(publisher) = publisher {
                run(publisher, rx, shared_for_thread, emit, Loop { dir, room: room_for_thread, tick });
            }
        });
        if spawned.is_err() {
            RUNNING.fetch_sub(1, Ordering::SeqCst);
            if let Some(mut publisher) = cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
                publisher.stop(StopReason::Owner);
            }
            active::remove_if(&env.data_dir, &room);
            return Err(NO_THREAD.to_string());
        }
        (env.emit)(&initial);
        *self.inner.slot.lock() = Slot::Running(Running { tx, shared });
        tracing::info!("live room started");
        Ok(StartedRoom { ends_at_ms: ends_at })
    }

    /// Ends the room now, whatever the reason, and waits a moment for the relay to wipe it. Called by the Stop
    /// command, the panic path and the app exit hook. Does nothing when no room is running.
    pub fn stop_all(&self, reason: StopReason) {
        let running = {
            let mut slot = self.inner.slot.lock();
            match std::mem::take(&mut *slot) {
                Slot::Running(running) => Some(running),
                other => {
                    *slot = other;
                    None
                }
            }
        };
        if let Some(running) = running {
            let _ = running.tx.send(Cmd::Stop(reason));
            running.shared.wait_done(STOP_WAIT);
        }
    }

    pub fn pause(&self) -> Result<(), String> {
        self.send(Cmd::Pause)
    }

    pub fn resume(&self) -> Result<(), String> {
        self.send(Cmd::Resume)
    }

    fn send(&self, cmd: Cmd) -> Result<(), String> {
        match &*self.inner.slot.lock() {
            Slot::Running(running) if !running.shared.is_done() => {
                running.tx.send(cmd).map_err(|_| NOT_RUNNING.to_string())
            }
            _ => Err(NOT_RUNNING.to_string()),
        }
    }

    /// The room as the bar shows it. A room that ended by itself still answers, with the reason, until the next
    /// start or stop; otherwise the answer is "off".
    pub fn status(&self) -> LiveStatus {
        match &*self.inner.slot.lock() {
            Slot::Running(running) => running.shared.status.lock().clone(),
            _ => off_status(),
        }
    }

    /// The link of the running room. The caller copies it and must not log it.
    pub fn link(&self) -> Result<String, String> {
        match &*self.inner.slot.lock() {
            Slot::Running(running) => running.shared.link.lock().clone().ok_or_else(|| NOT_RUNNING.to_string()),
            _ => Err(NOT_RUNNING.to_string()),
        }
    }
}

/// Ends a room a crash left behind (see `active`). Runs on its own thread: it may wait for the network.
pub fn cleanup_at_boot(data_dir: PathBuf) {
    let _ = std::thread::Builder::new().name("live-room-cleanup".into()).spawn(move || {
        active::cleanup_leftover(&data_dir, now_ms());
    });
}

fn off_status() -> LiveStatus {
    LiveStatus { live: false, paused: false, viewers: 0, ends_at_ms: 0, pane_ids: Vec::new(), hidden: 0, stopped: None }
}

/// `<relay>/r/#<room>.<read token>.<content key>`. The secret parts are all after the `#`, so no request line, log
/// or `Referer` carries them.
pub(super) fn build_link(relay: &str, room: &str, token_b64: &str, key_b64: &str) -> String {
    format!("{}/r/#{room}.{token_b64}.{key_b64}", relay.trim_end_matches('/'))
}

fn new_room_key() -> SigningKey {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    SigningKey::from_bytes(&seed)
}

/// The fixed sentence for a failed start. The relay's own words never reach the person.
fn describe(error: ClientError) -> String {
    match error {
        ClientError::Gone(_) => REFUSED.to_string(),
        other => other.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// What the thread needs besides the publisher.
struct Loop {
    dir: PathBuf,
    room: String,
    tick: Duration,
}

/// The thread of one room: waits for a command or the next tick, publishes the status when it changed, and cleans
/// up when the room ends, whatever the reason.
fn run<S: Source, K: Sink>(
    mut publisher: Publisher<S, K>,
    rx: Receiver<Cmd>,
    shared: Arc<Shared>,
    emit: Emit,
    lp: Loop,
) {
    let mut last = shared.status.lock().clone();
    let mut next_tick = Instant::now() + lp.tick;
    loop {
        let wait = next_tick.saturating_duration_since(Instant::now());
        let mut stop = None;
        match rx.recv_timeout(wait) {
            Ok(Cmd::Pause) => publisher.pause(now_ms()),
            Ok(Cmd::Resume) => publisher.resume(now_ms()),
            Ok(Cmd::Stop(why)) => stop = Some(why),
            Err(RecvTimeoutError::Timeout) => {}
            // The state was dropped: the app is going away.
            Err(RecvTimeoutError::Disconnected) => stop = Some(StopReason::AppQuit),
        }
        if let Some(why) = stop {
            publisher.stop(why);
        } else if Instant::now() >= next_tick {
            next_tick = Instant::now() + lp.tick;
            publisher.tick(now_ms());
        }
        let status = publisher.status();
        let ended = status.stopped.is_some();
        if ended {
            // Before the last event, so a window or the tray that reacts to it sees no room.
            RUNNING.fetch_sub(1, Ordering::SeqCst);
        }
        if status != last {
            *shared.status.lock() = status.clone();
            emit(&status);
            last = status;
        }
        if ended {
            break;
        }
    }
    active::remove_if(&lp.dir, &lp.room);
    shared.finish();
    tracing::info!("live room ended");
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
