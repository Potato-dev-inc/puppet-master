// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The engine thread: the only owner of the desk state. Commands, hook events and a watchdog tick all arrive
//! on one bounded channel; the engine answers each command with a fresh [`Snapshot`].
//!
//! It talks to the PC only through [`DeskHost`]. The hook thread is the host's own thread with its own message
//! loop (never the engine's, never the app's main thread), and only ever `try_send`s into this channel.
//!
//! Hooks are installed only while ALL of these hold: the Labs switch is on, the person turned "Screen hops" on,
//! and hops are not paused. Anything else removes them and releases everything.

pub mod config;
pub mod hops;
pub mod hotkeys;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use desk_core::{HopAction, HopMode, PositionMemory};
use serde::{Deserialize, Serialize};

use self::config::{Config, ConfigStore};
use self::hotkeys::{compile, key_texts, problems_text, Binding, DeskAction, HotkeyConfig, Problem};
use super::host::{DeskHost, DisplayInfo, HookHealth, HostEvent, HostSink};
use super::port::{LinkAction, LinkPort};

/// What the page can ask for. Wire form: `{ "type": "hop_to", "screen": 2 }`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cmd {
    /// Re-read screens and pointer. Changes nothing else, and keeps the last error.
    Refresh,
    HopNext,
    HopPrev,
    /// 1-based screen number in reading order.
    HopTo { screen: u8 },
    FindPointer,
    SetHotkeys { hotkeys: HotkeyConfig },
    ResetHotkeys,
    /// The "Screen hops" switch.
    SetHopsEnabled { enabled: bool },
    /// Pause releases everything (hooks off); resuming installs them again if hops are on.
    Pause { paused: bool },
    SetHopMode { mode: HopMode },
    /// Put the pointer in the middle of the screen it is on.
    Center,
    /// Keep the pointer on the screen it is on, or free it.
    SetPointerLock { locked: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Status {
    /// `off`, `on`, `starting`, `paused`, `labs_off`, `error`, `unsupported`.
    pub state: &'static str,
    pub hops_enabled: bool,
    pub paused: bool,
    pub hooks_installed: bool,
    /// The pointer is held on one screen (and cannot cross to another computer) until it is freed.
    pub pointer_locked: bool,
    /// How many times the watchdog had to put a dropped hook back.
    pub hook_reinstalls: u32,
    /// Stays until the next command that works. Polling does not clear it.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Screen {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub primary: bool,
    /// 1-based, the number "go to screen N" uses.
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    pub screen_id: Option<String>,
    pub screen_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BindingView {
    /// `next`, `prev`, `screen:3`, `find`, `pause`.
    pub action: String,
    pub keys: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HotkeysView {
    pub config: HotkeyConfig,
    pub defaults: HotkeyConfig,
    pub bindings: Vec<BindingView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Find {
    /// Goes up on every "find my pointer"; the page draws the ring when it changes.
    pub seq: u64,
    pub x: i32,
    pub y: i32,
    pub screen_index: Option<usize>,
    /// For a screen reader, since the ring is only visual.
    pub announce: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Snapshot {
    /// Goes up whenever anything below changes.
    pub seq: u64,
    pub status: Status,
    pub screens: Vec<Screen>,
    pub pointer: Option<Pointer>,
    pub hop_mode: HopMode,
    pub hotkeys: HotkeysView,
    pub find: Option<Find>,
}

pub type Notifier = Arc<dyn Fn(&Snapshot) + Send + Sync>;

enum Msg {
    Cmd(Cmd, mpsc::Sender<Result<Snapshot, String>>),
    Host(HostEvent),
    Reconcile,
    Stop(mpsc::Sender<()>),
}

/// The sink given to a host. It never blocks the hook thread: a full channel drops the event.
fn make_sink(tx: SyncSender<Msg>) -> HostSink {
    Arc::new(move |event| {
        let _ = tx.try_send(Msg::Host(event));
    })
}

pub struct EngineParts {
    pub host: Arc<dyn DeskHost>,
    pub store: Arc<dyn ConfigStore>,
    /// The Labs switch. Read on every decision.
    pub labs: Arc<AtomicBool>,
    pub notifier: Arc<Mutex<Option<Notifier>>>,
    /// Watchdog period.
    pub tick: Duration,
    /// Where jump/return-home go and where the pointer lock is published for the link.
    pub port: Arc<LinkPort>,
}

pub struct EngineHandle {
    tx: SyncSender<Msg>,
    #[allow(dead_code)]
    snapshot: Arc<Mutex<Snapshot>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

const COMMAND_WAIT: Duration = Duration::from_secs(5);
/// How often a locked pointer is looked at.
const LOCK_LOOK: Duration = Duration::from_millis(25);

impl EngineHandle {
    pub fn start(parts: EngineParts) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<Msg>(64);
        let loaded = parts.store.load();
        let bindings = compile(&loaded.config.hotkeys).unwrap_or_default();
        publish_link_chords(&parts.port, &bindings);
        let mut state = State {
            port: parts.port,
            lock: None,
            host: parts.host,
            store: parts.store,
            labs: parts.labs,
            notifier: parts.notifier,
            sink: make_sink(tx.clone()),
            cfg: loaded.config,
            bindings,
            installed: false,
            memory: PositionMemory::default(),
            displays: Vec::new(),
            pointer: None,
            find: None,
            find_seq: 0,
            error: loaded.note,
            reinstalls: 0,
            seq: 0,
            published: None,
            ticks: 0,
            retry_at_tick: 0,
            fail_streak: 0,
        };
        state.refresh();
        let snapshot = Arc::new(Mutex::new(state.snapshot()));
        let shared = snapshot.clone();
        let tick = parts.tick;
        let join = thread::Builder::new()
            .name("desk-engine".into())
            .spawn(move || run(state, rx, tick, shared))
            .map_err(|e| e.to_string())?;
        let handle = Self { tx, snapshot, join: Mutex::new(Some(join)) };
        handle.reconcile();
        Ok(handle)
    }

    /// The last published snapshot, without asking the engine thread.
    #[allow(dead_code)]
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn command(&self, cmd: Cmd) -> Result<Snapshot, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx.send(Msg::Cmd(cmd, reply_tx)).map_err(|_| "My computers is stopped".to_string())?;
        match reply_rx.recv_timeout(COMMAND_WAIT) {
            Ok(r) => r,
            Err(_) => Err("My computers did not answer in time".to_string()),
        }
    }

    /// Ask the engine to look at the Labs switch and the hooks again (called when settings change).
    pub fn reconcile(&self) {
        let _ = self.tx.try_send(Msg::Reconcile);
    }

    /// Release everything and end the engine thread. Waits for it.
    pub fn shutdown(&self) {
        let (done_tx, done_rx) = mpsc::channel();
        if self.tx.send(Msg::Stop(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(COMMAND_WAIT);
        }
        if let Some(j) = self.join.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = j.join();
        }
    }
}

/// The hub matches the return-home and pause chords itself, on the keys it captures while the pointer is on another
/// computer (the keyboard hook does not see those keys).
fn publish_link_chords(port: &LinkPort, bindings: &[Binding]) {
    let chord = |action: DeskAction| bindings.iter().find(|b| b.action == action).map(|b| b.chord);
    port.set_return_chord(chord(DeskAction::ReturnHome));
    port.set_pause_chord(chord(DeskAction::Pause));
}

struct State {
    port: Arc<LinkPort>,
    /// The pointer is held on one screen (not saved: a restart starts free).
    lock: Option<desk_core::PhysRect>,
    host: Arc<dyn DeskHost>,
    store: Arc<dyn ConfigStore>,
    labs: Arc<AtomicBool>,
    notifier: Arc<Mutex<Option<Notifier>>>,
    sink: HostSink,
    cfg: Config,
    bindings: Vec<Binding>,
    installed: bool,
    memory: PositionMemory,
    displays: Vec<DisplayInfo>,
    pointer: Option<Pointer>,
    find: Option<Find>,
    find_seq: u64,
    error: Option<String>,
    reinstalls: u32,
    seq: u64,
    published: Option<Snapshot>,
    ticks: u64,
    retry_at_tick: u64,
    fail_streak: u32,
}

fn run(mut st: State, rx: Receiver<Msg>, tick: Duration, shared: Arc<Mutex<Snapshot>>) {
    let mut last_watch = Instant::now();
    loop {
        let wait = if st.lock.is_some() { tick.min(LOCK_LOOK) } else { tick };
        match rx.recv_timeout(wait) {
            Ok(Msg::Cmd(cmd, reply)) => {
                let result = st.handle(cmd);
                st.publish(&shared);
                let _ = reply.send(result.map(|_| st.snapshot()));
            }
            Ok(Msg::Host(event)) => {
                st.on_host(event);
                st.publish(&shared);
            }
            Ok(Msg::Reconcile) => {
                st.retry_at_tick = 0;
                st.reconcile();
                st.publish(&shared);
            }
            Ok(Msg::Stop(done)) => {
                st.teardown();
                st.publish(&shared);
                let _ = done.send(());
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                // While the pointer is locked the wait is short; the watchdog still runs once per `tick`.
                st.keep_in_lock();
                if st.lock.is_none() || last_watch.elapsed() >= tick {
                    last_watch = Instant::now();
                    st.ticks += 1;
                    st.watchdog();
                    st.publish(&shared);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                st.teardown();
                return;
            }
        }
    }
}

impl State {
    fn labs_on(&self) -> bool {
        self.labs.load(Ordering::SeqCst)
    }

    fn want_hooks(&self) -> bool {
        self.labs_on() && self.cfg.hops_enabled && !self.cfg.paused
    }

    // ---- hooks -------------------------------------------------------------------------------------------

    fn reconcile(&mut self) {
        let want = self.want_hooks();
        if want && !self.installed {
            self.host.set_hotkeys(self.bindings.clone());
            match self.host.install_hooks(self.sink.clone()) {
                Ok(()) => {
                    self.installed = true;
                    self.fail_streak = 0;
                    if self.error.as_deref().is_some_and(|e| e.starts_with("Keys are not working")) {
                        self.error = None;
                    }
                }
                Err(e) => {
                    self.fail_streak = self.fail_streak.saturating_add(1);
                    self.retry_at_tick = self.ticks + u64::from(self.fail_streak.min(30));
                    self.error = Some(format!("Keys are not working: {e}"));
                }
            }
        } else if !want && self.installed {
            self.host.remove_hooks();
            self.host.release_all();
            self.clear_lock();
            self.installed = false;
        }
    }

    /// Put back a hook that Windows dropped, and retry a failed install with a growing wait.
    fn watchdog(&mut self) {
        if !self.want_hooks() {
            if self.installed {
                self.reconcile();
            }
            return;
        }
        if self.installed {
            if self.host.hook_health() != HookHealth::Healthy {
                tracing::warn!("desk: keyboard hook was dropped; installing it again");
                self.host.remove_hooks();
                self.installed = false;
                self.reinstalls = self.reinstalls.saturating_add(1);
                self.reconcile();
            }
        } else if self.ticks >= self.retry_at_tick {
            self.reconcile();
        }
    }

    fn teardown(&mut self) {
        self.host.remove_hooks();
        self.host.release_all();
        self.clear_lock();
        self.installed = false;
    }

    // ---- commands ----------------------------------------------------------------------------------------

    fn handle(&mut self, cmd: Cmd) -> Result<(), String> {
        // A command that works clears the last error first, so an error it raises itself (a hook that
        // would not install) is kept. Polling (`Refresh`) never clears it.
        if !matches!(cmd, Cmd::Refresh) {
            self.error = None;
        }
        let result = self.apply(cmd);
        if let Err(e) = &result {
            self.error = Some(e.clone());
        }
        result
    }

    fn apply(&mut self, cmd: Cmd) -> Result<(), String> {
        if !self.labs_on() {
            return Err("FEATURE_OFF: My computers is switched off in Labs".into());
        }
        match cmd {
            Cmd::Refresh => {
                self.refresh();
                Ok(())
            }
            Cmd::HopNext => self.hop(HopAction::Next),
            Cmd::HopPrev => self.hop(HopAction::Prev),
            Cmd::HopTo { screen } => self.hop(HopAction::Number(screen)),
            Cmd::FindPointer => self.find_pointer(),
            Cmd::SetHotkeys { hotkeys } => self.set_hotkeys(hotkeys),
            Cmd::ResetHotkeys => self.set_hotkeys(HotkeyConfig::default()),
            Cmd::SetHopsEnabled { enabled } => {
                self.cfg.hops_enabled = enabled;
                self.save();
                self.retry_at_tick = 0;
                self.reconcile();
                Ok(())
            }
            Cmd::Pause { paused } => {
                self.cfg.paused = paused;
                if paused {
                    // Everything back to the system now, whether or not a hook was up.
                    self.reconcile();
                    self.host.release_all();
                    self.clear_lock();
                } else {
                    self.retry_at_tick = 0;
                    self.reconcile();
                }
                Ok(())
            }
            Cmd::Center => self.hop(HopAction::Center),
            Cmd::SetPointerLock { locked } => self.set_lock(locked),
            Cmd::SetHopMode { mode } => {
                self.cfg.hop_mode = mode;
                self.save();
                Ok(())
            }
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.store.save(&self.cfg) {
            self.error = Some(format!("Could not save settings: {e}"));
        }
    }

    fn set_hotkeys(&mut self, hotkeys: HotkeyConfig) -> Result<(), String> {
        let bindings: Vec<Binding> = compile(&hotkeys).map_err(|p: Vec<Problem>| problems_text(&p))?;
        self.cfg.hotkeys = hotkeys;
        publish_link_chords(&self.port, &bindings);
        self.bindings = bindings;
        self.host.set_hotkeys(self.bindings.clone());
        self.save();
        Ok(())
    }

    fn refresh(&mut self) {
        match self.host.displays() {
            Ok(d) => self.displays = d,
            Err(e) => {
                self.displays.clear();
                if self.host.supported() {
                    self.error = Some(format!("Could not list screens: {e}"));
                }
            }
        }
        let layout = hops::layout_of(&self.displays);
        self.pointer = self.host.cursor_pos().ok().map(|(x, y)| {
            let here = hops::locate(&layout, x, y);
            Pointer {
                x,
                y,
                screen_id: here.map(|m| m.id.0.clone()),
                screen_index: here.and_then(|m| hops::index_of(&layout, &m.id)),
            }
        });
    }

    fn hop(&mut self, action: HopAction) -> Result<(), String> {
        if self.cfg.paused {
            return Err("Screen hops are paused".into());
        }
        if self.lock.is_some() && !matches!(action, HopAction::Center) {
            return Err("The pointer is locked to this screen. Press the lock keys again to free it".into());
        }
        let displays = self.host.displays()?;
        let pointer = self.host.cursor_pos()?;
        let layout = hops::layout_of(&displays);
        let target = hops::plan(&layout, self.cfg.hop_mode, &mut self.memory, pointer, action).map_err(|e| e.to_string())?;
        self.host.move_cursor(target.x, target.y)?;
        self.refresh();
        Ok(())
    }

    /// Hold the pointer on the screen it is on (and keep it from crossing to another computer), or free it.
    /// No OS clip (the guard tests forbid it): the engine puts the pointer back a few times a second instead.
    fn set_lock(&mut self, locked: bool) -> Result<(), String> {
        if !locked {
            self.clear_lock();
            return Ok(());
        }
        let displays = self.host.displays()?;
        let (x, y) = self.host.cursor_pos()?;
        let layout = hops::layout_of(&displays);
        let here = hops::locate(&layout, x, y).ok_or("No screens found")?;
        self.lock = Some(here.bounds);
        self.port.set_locked(true);
        Ok(())
    }

    /// Free the pointer. Safe when it was not locked.
    fn clear_lock(&mut self) {
        self.lock = None;
        self.port.set_locked(false);
    }

    /// While locked: a pointer that got out (a fast move, a game, the screen under it going away) is put back on
    /// the nearest point of the locked screen.
    fn keep_in_lock(&mut self) {
        let Some(b) = self.lock else { return };
        let Ok((x, y)) = self.host.cursor_pos() else { return };
        let (cx, cy) = (x.clamp(b.x, b.right() - 1), y.clamp(b.y, b.bottom() - 1));
        if (cx, cy) != (x, y) {
            let _ = self.host.move_cursor(cx, cy);
        }
    }

    fn find_pointer(&mut self) -> Result<(), String> {
        self.refresh();
        let (x, y) = self.host.cursor_pos()?;
        let layout = hops::layout_of(&self.displays);
        let index = hops::locate(&layout, x, y).and_then(|m| hops::index_of(&layout, &m.id));
        self.find_seq += 1;
        let announce = match index {
            Some(i) if layout.monitors.len() > 1 => format!("Your pointer is on screen {i}"),
            _ => "Your pointer is here".to_string(),
        };
        self.find = Some(Find { seq: self.find_seq, x, y, screen_index: index, announce });
        Ok(())
    }

    fn on_host(&mut self, event: HostEvent) {
        let HostEvent::Hotkey(action) = event;
        // A late event after pause or Labs-off is dropped.
        if !self.want_hooks() {
            return;
        }
        let result = match action {
            DeskAction::Next => self.hop(HopAction::Next),
            DeskAction::Prev => self.hop(HopAction::Prev),
            DeskAction::Screen(n) => self.hop(HopAction::Number(n)),
            DeskAction::Find => self.find_pointer(),
            DeskAction::Pause => self.apply(Cmd::Pause { paused: true }),
            DeskAction::Center => self.hop(HopAction::Center),
            DeskAction::LockPointer => self.set_lock(self.lock.is_none()),
            DeskAction::Jump(n) => self.port.request(LinkAction::Jump(n)),
            DeskAction::ReturnHome => self.port.request(LinkAction::ReturnHome),
        };
        match result {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(e),
        }
    }

    // ---- snapshot ----------------------------------------------------------------------------------------

    fn status_state(&self) -> &'static str {
        if !self.host.supported() {
            "unsupported"
        } else if !self.labs_on() {
            "labs_off"
        } else if self.cfg.paused {
            "paused"
        } else if !self.cfg.hops_enabled {
            "off"
        } else if self.installed {
            "on"
        } else if self.error.is_some() {
            "error"
        } else {
            "starting"
        }
    }

    fn snapshot(&self) -> Snapshot {
        let layout = hops::layout_of(&self.displays);
        let screens = hops::order(&layout)
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let m = layout.monitor(id)?;
                let d = self.displays.iter().find(|d| d.id == id.0)?;
                Some(Screen {
                    id: id.0.clone(),
                    name: m.name.clone(),
                    x: m.bounds.x,
                    y: m.bounds.y,
                    width: m.bounds.width,
                    height: m.bounds.height,
                    scale: m.scale,
                    primary: d.primary,
                    index: i + 1,
                })
            })
            .collect();
        let bindings = key_texts(&self.cfg.hotkeys)
            .into_iter()
            .map(|(a, keys)| BindingView { action: a.name(), keys })
            .collect();
        Snapshot {
            seq: self.seq,
            status: Status {
                state: self.status_state(),
                hops_enabled: self.cfg.hops_enabled,
                paused: self.cfg.paused,
                hooks_installed: self.installed,
                pointer_locked: self.lock.is_some(),
                hook_reinstalls: self.reinstalls,
                error: self.error.clone(),
            },
            screens,
            pointer: self.pointer.clone(),
            hop_mode: self.cfg.hop_mode,
            hotkeys: HotkeysView { config: self.cfg.hotkeys.clone(), defaults: HotkeyConfig::default(), bindings },
            find: self.find.clone(),
        }
    }

    /// Store the snapshot and tell the page, only when something changed.
    fn publish(&mut self, shared: &Arc<Mutex<Snapshot>>) {
        let mut snap = self.snapshot();
        let same = self.published.as_ref().is_some_and(|p| {
            let mut a = p.clone();
            a.seq = snap.seq;
            a == snap
        });
        if same {
            return;
        }
        self.seq += 1;
        snap.seq = self.seq;
        self.published = Some(snap.clone());
        *shared.lock().unwrap_or_else(|p| p.into_inner()) = snap.clone();
        let notifier = self.notifier.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(n) = notifier {
            n(&snap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hook thread calls the sink. When the engine is busy and the channel is full, the sink must drop the
    /// event and return at once (DevHop waited up to 5 ms per key inside the hook, and Windows removes a slow hook).
    #[test]
    fn the_sink_never_blocks_on_a_full_channel() {
        let (tx, _rx) = mpsc::sync_channel::<Msg>(1);
        let sink = make_sink(tx);
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            sink(HostEvent::Hotkey(DeskAction::Next));
        }
        assert!(start.elapsed() < Duration::from_millis(500), "sink blocked: {:?}", start.elapsed());
    }

    #[test]
    fn the_sink_drops_events_after_the_engine_is_gone() {
        let (tx, rx) = mpsc::sync_channel::<Msg>(4);
        let sink = make_sink(tx);
        drop(rx);
        sink(HostEvent::Hotkey(DeskAction::Find));
    }
}
