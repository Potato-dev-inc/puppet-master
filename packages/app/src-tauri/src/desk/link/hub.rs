// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-engine runtime: control machine wiring, remote injection, panic and pause; PLAN.md N3.1 event-driven loop.)
//! The hub: the one owner of "who has the mouse". Link tasks and the capture thread send it messages; it decides and
//! answers through each link's queues. It never awaits a stream and it has no sleep: it wakes for a message, for the
//! 10 ms edge look (only while a computer is linked), or for the 250 ms upkeep.
//!
//! Two roles at once, per computer pair:
//! - **Holding the mouse** (`remote`): the pointer left through an edge. Input is captured here (the host hides and
//!   holds the pointer) and sent to the other computer: keys and buttons on the control stream, motion as datagrams.
//!   Back home on `Return`, on panic (Ctrl+Alt+Esc), on pause (the `pause` hotkey, Ctrl+Alt+P by default), on 250 ms of silence, or when the link ends.
//! - **Being controlled** (`controlled`): the other computer's input is injected. Every key and button pressed is
//!   remembered and released when control ends for any reason, the link breaks, or the peer goes quiet.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use desk_core::{translate_hid, ControlEvent, ControlMachine, HostOs, Side};
use desk_proto::{Datagram, OsKind, StreamMsg};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, MissedTickBehavior};

use crate::p2p::capability;
use crate::p2p::endpoint::{Link, NodeId};
use crate::p2p::session::{DialLedger, Reach, Sessions};

use super::arrange::{self, localize, outward, Shape, ALL_SIDES};
use super::geometry::{entry_point, from_milli, inward, milli, onto_screens, parse_side, side_name, union};
use super::local_use::{self, Arming};
use super::pushback::PushBack;
use super::ratelog::RateLimit;
use super::remote::{Capture, Inject};
use super::pointer_speed::PointerSpeed;
use super::{clip_link, io, Inner, CLOSE_REPLACED, CLOSE_STOPPED};
use crate::desk::host::DisplayInfo;

/// How often the pointer is looked at while a computer is linked.
const EDGE_POLL: Duration = Duration::from_millis(10);
/// Upkeep: dials, switches, screens.
const UPKEEP: Duration = Duration::from_millis(250);
/// The pointer must rest on the edge for this many looks (20 ms) before it crosses: brushing past does not count.
const DWELL_LOOKS: u32 = 2;
/// Most keys remembered as held on the controlled side.
const MAX_HELD: usize = 256;
/// After the other computer said no, the pointer does not try it again for this long (it would only be refused again
/// every 20 ms while the person keeps pushing).
const REFUSED_PAUSE_MS: u64 = 2_000;
/// After this computer hands the pointer back, or the other computer lets go of it, this computer does not take the
/// pointer straight back across the same edge. The other computer may leave its pointer resting on that edge.
const RETURN_PAUSE_MS: u64 = 1_000;
/// After the step-off that follows a hand-back, the pointer is looked at again this long later: posted mouse events
/// still applied on the other computer can have put it back on the edge (see `recheck_step_off`).
const STEP_OFF_RECHECK_MS: u64 = 150;
/// "desk: not crossing" is written at most once a second for each reason.
const NOT_CROSSING_EVERY_MS: u64 = 1_000;
/// The page is handed a new view at most this often. A change inside that window is handed over at the next tick.
const PUBLISH_EVERY_MS: u64 = 50;
/// Longest refusal reason taken from the other computer.
const MAX_REASON_CHARS: usize = 300;

/// The first 8 hex digits of a node id: enough to tell computers apart in a log, nothing secret.
fn short(peer: &NodeId) -> String {
    peer.to_hex().chars().take(8).collect()
}

mod actions;

pub struct InstallReq {
    pub peer: NodeId,
    pub link: Arc<dyn Link>,
    pub link_id: u64,
    pub dialed: bool,
    pub os: OsKind,
    pub ctl: mpsc::UnboundedSender<StreamMsg>,
    pub dg: mpsc::Sender<Datagram>,
    pub reply: oneshot::Sender<bool>,
}

pub enum Msg {
    Handshaking { peer: NodeId, on: bool },
    Install(Box<InstallReq>),
    Frame { peer: NodeId, link_id: u64, msg: StreamMsg },
    Motion { peer: NodeId, link_id: u64, dx: i32, dy: i32 },
    Stalled { peer: NodeId, link_id: u64, gap_ms: u64 },
    Ended { peer: NodeId, link_id: u64 },
    Capture(Capture),
    DialResult { peer: NodeId, ok: bool },
    Pause(bool),
    /// Hotkey: cross to the Nth paired computer.
    Jump(u8),
    /// Hotkey: bring the pointer back from the other computer.
    ReturnHome,
    Panic,
    Kick,
    /// The placement of a computer changed here: tell the linked computers.
    LayoutChanged,
    Shutdown,
}

struct Live {
    #[allow(dead_code)]
    link_id: u64,
    link: Arc<dyn Link>,
    ctl: mpsc::UnboundedSender<StreamMsg>,
    dg: mpsc::Sender<Datagram>,
    os: OsKind,
    motion_seq: u32,
    /// The part of a motion step not yet sent to that computer (see `pointer_speed.rs`).
    pointer: PointerSpeed,
}

/// The pointer is on `peer`.
struct Remote {
    peer: NodeId,
    /// Where the pointer comes back to when nothing says otherwise: inside the edge it left through.
    home: (i32, i32),
    side: Side,
}

/// Why the pointer came home on this computer. The log says it in one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Home {
    Edge,
    Stall { gap_ms: u64 },
    Ended,
    Replaced,
    Leave,
    Refused,
    Panic,
    Pause,
    Hotkey,
    /// Both computers crossed at once; this one lets the other keep its crossing.
    Crossed,
}

/// Why the pointer does not cross, for the log (see `poll_edge`).
#[derive(Clone, Copy)]
enum Block {
    Paused,
    Holding(NodeId),
    ControlledBy(NodeId),
    Locked,
    Waiting(NodeId),
    Unarmed,
    ButtonHeld(NodeId),
    InputOff(NodeId),
}

impl Block {
    /// The limiter's key (one line per second per key) and the words after `reason=`.
    fn text(self) -> (&'static str, String) {
        match self {
            Block::Paused => ("paused", "paused".to_string()),
            Block::Holding(p) => ("holding", format!("holding the mouse peer={}", short(&p))),
            Block::ControlledBy(p) => ("controlled", format!("controlled by peer={}", short(&p))),
            Block::Locked => ("locked", "pointer locked".to_string()),
            Block::Waiting(p) => ("waiting", format!("waiting after a refusal peer={}", short(&p))),
            Block::Unarmed => ("unarmed", "waiting for the pointer to leave the edge".to_string()),
            Block::ButtonHeld(p) => ("button", format!("button held peer={}", short(&p))),
            Block::InputOff(p) => ("input", format!("input off peer={}", short(&p))),
        }
    }
}

impl Home {
    fn text(self) -> String {
        match self {
            Home::Edge => "edge".to_string(),
            Home::Stall { gap_ms } => format!("stall gap_ms={gap_ms}"),
            Home::Ended => "ended".to_string(),
            Home::Replaced => "replaced".to_string(),
            Home::Leave => "leave".to_string(),
            Home::Refused => "refused".to_string(),
            Home::Panic => "panic".to_string(),
            Home::Pause => "pause".to_string(),
            Home::Hotkey => "hotkey".to_string(),
            Home::Crossed => "crossed".to_string(),
        }
    }
}

/// `peer` is using this computer.
struct Controlled {
    peer: NodeId,
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
    /// Pushes at the entry edge in a row (see `pushback`).
    pushback: PushBack,
    /// Where this computer last put the pointer (the entry placement, then each injected move), and when the last
    /// remote motion was handled. A pointer found elsewhere, with no fresh motion, is a person here (`local_use`).
    expected: (i32, i32),
    last_motion: u64,
}

#[derive(Default)]
struct Chords {
    down: BTreeSet<u16>,
}

const HID_ESC: u16 = 0x29;

impl Chords {
    /// Ctrl+Alt+Esc is the panic key. Matched on the raw keys, so it works whatever else is held, and whichever side
    /// of the keyboard the modifiers are on. Returns true for that press. The pause key is the configured `pause`
    /// hotkey, matched by `Hub::pause_chord_pressed` (not Scroll Lock any more).
    fn on_key(&mut self, hid: u16, down: bool) -> bool {
        if !down {
            self.down.remove(&hid);
            return false;
        }
        self.down.insert(hid);
        let ctrl = self.down.contains(&0xE0) || self.down.contains(&0xE4);
        let alt = self.down.contains(&0xE2) || self.down.contains(&0xE6);
        hid == HID_ESC && ctrl && alt
    }
}

fn host_os(os: OsKind) -> HostOs {
    match os {
        OsKind::Macos => HostOs::Macos,
        OsKind::Windows | OsKind::Other => HostOs::Windows,
    }
}

pub struct Hub {
    inner: Arc<Inner>,
    tx: mpsc::UnboundedSender<Msg>,
    sessions: Sessions,
    live: HashMap<NodeId, Live>,
    ledger: DialLedger,
    control: ControlMachine,
    remote: Option<Remote>,
    controlled: Option<Controlled>,
    paused: bool,
    chords: Chords,
    displays: Vec<DisplayInfo>,
    /// `displays` in local space (the box around them starts at 0, 0) and where that space's origin is.
    shapes: Vec<Shape>,
    origin: (i32, i32),
    edge_candidate: Option<(NodeId, u32)>,
    /// A computer that said no to the pointer, and until when it is not tried again.
    refused_until: HashMap<NodeId, u64>,
    /// Which peers the pointer must first be seen away from the edges for (see `local_use::Arming`).
    arming: Arming,
    /// A step-off was made through this side of this computer; the pointer is checked again at this time (ms).
    step_off: Option<(Side, u64)>,
    /// Keeps "desk: not crossing" to one line a second for each reason.
    not_crossing: RateLimit,
    upkeeps: u64,
    error: Option<String>,
    /// When the page was last handed a view, and whether a change is waiting for the next tick.
    published_at: Option<u64>,
    publish_waiting: bool,
}

impl Hub {
    pub fn new(inner: Arc<Inner>, tx: mpsc::UnboundedSender<Msg>) -> Self {
        Self {
            inner,
            tx,
            sessions: Sessions::new(),
            live: HashMap::new(),
            ledger: DialLedger::new(),
            control: ControlMachine::new(),
            remote: None,
            controlled: None,
            paused: false,
            chords: Chords::default(),
            displays: Vec::new(),
            shapes: Vec::new(),
            origin: (0, 0),
            edge_candidate: None,
            refused_until: HashMap::new(),
            arming: Arming::default(),
            step_off: None,
            not_crossing: RateLimit::new(NOT_CROSSING_EVERY_MS),
            upkeeps: 0,
            error: None,
            published_at: None,
            publish_waiting: false,
        }
    }

    pub async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        let mut poll = interval(EDGE_POLL);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut upkeep = interval(UPKEEP);
        upkeep.set_missed_tick_behavior(MissedTickBehavior::Delay);
        self.refresh_displays();
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Shutdown) | None => break,
                    Some(m) => self.on_msg(m),
                },
                _ = poll.tick(), if !self.live.is_empty() => self.poll_edge(),
                _ = upkeep.tick() => self.upkeep(),
            }
        }
        self.teardown();
    }

    fn ms(&self) -> u64 {
        self.inner.now_ms()
    }

    /// This computer's node id (all zeros before the endpoint has one).
    fn local_id(&self) -> NodeId {
        self.inner.p2p.node_id().unwrap_or(NodeId([0; 32]))
    }

    fn refresh_displays(&mut self) {
        if let Ok(d) = self.inner.host.displays() {
            self.displays = d;
            let (shapes, origin) = localize(&self.displays);
            self.shapes = shapes.clone();
            self.origin = origin;
            // New screens change the layout: show it, and tell the linked computers.
            if self.inner.set_my_shapes((shapes, origin)) {
                self.inner.notify_layout();
                self.send_layouts();
            }
        }
    }

    /// Tells `peer` our screens and where we sit relative to it.
    fn send_layout(&self, peer: NodeId) {
        if let (Some(bytes), Some(l)) = (self.inner.layout_bytes(&peer), self.live.get(&peer)) {
            let _ = l.ctl.send(StreamMsg::Layout { bytes });
        }
    }

    fn send_layouts(&self) {
        let peers: Vec<NodeId> = self.live.keys().copied().collect();
        for p in peers {
            self.send_layout(p);
        }
    }

    /// The page shows `what` ("Mouse sharing hit a problem: ..."), and the log says it too.
    fn fail(&mut self, what: &str) {
        tracing::warn!(reason = what, "desk: mouse sharing hit a problem");
        self.error = Some(what.to_string());
    }

    /// The name the person gave a paired computer, for plain-words messages.
    fn peer_name(&self, peer: &NodeId) -> String {
        self.inner.trust.list().iter().find(|d| d.node == *peer).map_or_else(|| "The other computer".to_string(), |d| d.name.clone())
    }

    /// Says no to the pointer `peer` tried to send here: the reason goes to that computer (shown on its page) and
    /// into this computer's log, then `Leave` sends its pointer home.
    fn refuse_enter(&mut self, peer: NodeId, reason: &str, show_here: bool) {
        tracing::warn!(peer = short(&peer).as_str(), reason = reason, "desk: did not let the pointer in");
        if show_here {
            self.error = Some(reason.to_string());
        }
        if let Some(l) = self.live.get(&peer) {
            let _ = l.ctl.send(StreamMsg::Refused { reason: reason.to_string() });
            let _ = l.ctl.send(StreamMsg::Leave);
        }
    }

    // ---- messages ----------------------------------------------------------------------------------------

    fn on_msg(&mut self, msg: Msg) {
        // Motion changes what the page shows only when it moves the pointer on or off a computer.
        let motion = matches!(msg, Msg::Motion { .. } | Msg::Capture(Capture::Motion { .. }));
        let shown = self.shown();
        match msg {
            Msg::Handshaking { peer, on } => {
                if on {
                    self.sessions.handshake_started(&peer);
                } else {
                    self.sessions.handshake_ended(&peer);
                }
            }
            Msg::Install(req) => self.on_install(*req),
            Msg::Frame { peer, link_id, msg } => {
                if self.sessions.is_current(&peer, link_id) {
                    self.on_frame(peer, msg);
                }
            }
            Msg::Motion { peer, link_id, dx, dy } => {
                if self.sessions.is_current(&peer, link_id) {
                    self.on_motion(peer, dx, dy);
                }
            }
            Msg::Stalled { peer, link_id, gap_ms } => {
                if self.sessions.is_current(&peer, link_id) {
                    if self.remote.as_ref().is_some_and(|r| r.peer == peer) || self.controlled.as_ref().is_some_and(|c| c.peer == peer) {
                        tracing::info!(peer = short(&peer).as_str(), "desk: the other computer went quiet, the pointer comes home");
                    }
                    self.session_lost(&peer, Home::Stall { gap_ms });
                }
            }
            Msg::Ended { peer, link_id } => {
                if self.sessions.link_ended(&peer, link_id) {
                    tracing::info!(peer = short(&peer).as_str(), "desk: link to a computer ended");
                    self.live.remove(&peer);
                    self.session_lost(&peer, Home::Ended);
                }
            }
            Msg::Capture(c) => self.on_capture(c),
            Msg::DialResult { peer, ok } => {
                let now = self.ms();
                if ok {
                    self.ledger.succeeded(&peer);
                } else {
                    self.ledger.failed(&peer, now);
                }
            }
            Msg::Pause(p) => {
                self.paused = p;
                if p {
                    self.stop_all(Home::Pause);
                }
            }
            Msg::Panic => self.stop_all(Home::Panic),
            Msg::Jump(n) => self.jump_to(n),
            Msg::ReturnHome => self.return_home(),
            Msg::Kick => self.upkeep(),
            Msg::LayoutChanged => self.send_layouts(),
            Msg::Shutdown => {}
        }
        if !motion {
            self.publish();
        } else if self.shown() != shown {
            self.publish_capped();
        }
    }

    fn on_install(&mut self, req: InstallReq) {
        let local = self.inner.p2p.node_id().unwrap_or(NodeId([0; 32]));
        if !self.inner.labs() || !self.inner.input_allowed(&req.peer) {
            tracing::warn!(peer = short(&req.peer).as_str(), labs = self.inner.labs(), "desk: refused a link: Mouse and keyboard is off for that computer here");
            req.link.close(CLOSE_STOPPED, "bye");
            let _ = req.reply.send(false);
            return;
        }
        let (action, replaced) = self.sessions.install(&req.peer, req.link_id, req.dialed, &local, self.ms());
        use crate::p2p::session::InstallAction::*;
        match action {
            RejectNew => {
                let _ = req.reply.send(false);
            }
            Accept | ReplaceExisting => {
                if replaced.is_some() {
                    // A session on the old link ends: everything held on it is let go before the new one starts.
                    self.session_lost(&req.peer, Home::Replaced);
                    if let Some(old) = self.live.remove(&req.peer) {
                        old.link.close(CLOSE_REPLACED, "bye");
                    }
                }
                self.live.insert(
                    req.peer,
                    Live { link_id: req.link_id, link: req.link, ctl: req.ctl, dg: req.dg, os: req.os, motion_seq: 0, pointer: PointerSpeed::default() },
                );
                self.ledger.succeeded(&req.peer);
                let mac = matches!(req.os, OsKind::Macos);
                tracing::info!(peer = short(&req.peer).as_str(), dialed = req.dialed, peer_is_mac = mac, "desk: linked with a computer");
                let _ = req.reply.send(true);
                // First thing on a new link: our screens and where we think the two computers sit.
                self.send_layout(req.peer);
            }
        }
    }

    // ---- the edge ----------------------------------------------------------------------------------------

    fn poll_edge(&mut self) {
        self.flush_publish();
        if !self.inner.labs() {
            self.edge_candidate = None;
            return;
        }
        let Ok(pos) = self.inner.host.cursor_pos() else { return };
        let now = self.ms();
        self.recheck_step_off(pos, now);
        // A person is using this computer's own mouse while it is being controlled: the pointer goes back to them.
        // This runs before the "controlled by" return below, which would otherwise ignore the pointer for good.
        if self.controlled.is_some() && self.hand_back_if_used_here(pos, now) {
            return;
        }
        // The pointer seen away from every edge: every computer may be crossed to again.
        if union(&self.displays).is_some_and(|u| local_use::away_from_edges(pos, u, local_use::ARM_MARGIN)) {
            self.arming.saw_away();
        }
        let local = (pos.0 - self.origin.0, pos.1 - self.origin.1);
        self.refused_until.retain(|_, until| *until > now);
        let blocked = if self.paused {
            Some(Block::Paused)
        } else if let Some(r) = &self.remote {
            Some(Block::Holding(r.peer))
        } else if let Some(c) = &self.controlled {
            Some(Block::ControlledBy(c.peer))
        } else if self.inner.pointer_locked() {
            Some(Block::Locked)
        } else {
            None
        };
        if let Some(why) = blocked {
            self.edge_candidate = None;
            // Only worth a line when the pointer rests on an edge that faces a connected computer.
            let on_edge = self.live.keys().any(|p| self.inner.depart_to(p, &self.shapes, local, &ALL_SIDES).is_some());
            if on_edge {
                self.say_not_crossing(why);
            }
            return;
        }
        let mut hit = None;
        let mut waiting = None;
        let mut unarmed = false;        for peer in self.live.keys() {
            let Some(dep) = self.inner.depart_to(peer, &self.shapes, local, &ALL_SIDES) else { continue };
            if self.refused_until.contains_key(peer) {
                waiting.get_or_insert(*peer);
                continue;
            }
            if !self.arming.armed(peer) {
                unarmed = true;
                continue;
            }
            hit = Some((*peer, dep));
            break;
        }
        let Some((peer, dep)) = hit else {
            self.edge_candidate = None;
            if let Some(p) = waiting {
                self.say_not_crossing(Block::Waiting(p));
            } else if unarmed {
                self.say_not_crossing(Block::Unarmed);
            }
            return;
        };
        let looks = match self.edge_candidate {
            Some((p, n)) if p == peer => n + 1,
            _ => 1,
        };
        self.edge_candidate = Some((peer, looks));
        if looks < DWELL_LOOKS {
            return;
        }
        if !self.inner.input_allowed(&peer) {
            return self.say_not_crossing(Block::InputOff(peer));
        }
        if self.inner.host.buttons_held() {
            return self.say_not_crossing(Block::ButtonHeld(peer));
        }
        if self.cross_to(peer, &dep, pos) {
            self.edge_candidate = None;
            self.publish();
        }
    }

    /// Writes why the pointer is not crossing, at most once a second for each reason.
    fn say_not_crossing(&mut self, why: Block) {
        let (key, text) = why.text();
        if self.not_crossing.allow(key, self.ms()) {
            tracing::info!("desk: not crossing reason={text}");
        }
    }

    fn cross_to(&mut self, peer: NodeId, dep: &arrange::Departure, pos: (i32, i32)) -> bool {
        let side = dep.side;
        if !self.inner.input_allowed(&peer) || self.inner.host.buttons_held() {
            return false;
        }
        let Some(live) = self.live.get(&peer) else { return false };
        let tx = self.tx.clone();
        let sink: super::remote::CaptureSink = Arc::new(move |c| {
            let _ = tx.send(Msg::Capture(c));
        });
        if let Err(e) = self.inner.host.start_capture(sink, pos) {
            self.fail(&format!("Could not take the mouse: {e}"));
            return false;
        }
        let home = inward(pos, side);
        self.inner.set_emergency(true, home);
        let entry = side.opposite();
        // The exact arrival point travels in the id; the fractions are the same point along the other computer's box.
        let theirs = self.inner.peer_box(&peer);
        let frac = |v: i32, len: i32| milli(f64::from(v) / f64::from((len - 1).max(1)));
        let (nx, ny) = (frac(dep.to.0, theirs.0), frac(dep.to.1, theirs.1));
        let _ = live.ctl.send(StreamMsg::Enter { monitor_id: arrange::enter_tag(entry, dep.to), nx_milli: nx, ny_milli: ny });
        self.control.handle(ControlEvent::CrossTo { peer: desk_core::DeviceId(peer.to_hex()), button_held: false });
        self.remote = Some(Remote { peer, home, side });
        self.error = None;
        tracing::info!(peer = short(&peer).as_str(), side = side_name(side), "desk: the pointer left for another computer");
        clip_link::spawn_send(&self.inner, peer);
        true
    }

    // ---- holding the mouse -------------------------------------------------------------------------------

    fn on_capture(&mut self, c: Capture) {
        let Some(r) = &self.remote else { return };
        let peer = r.peer;
        let Some(live) = self.live.get_mut(&peer) else { return };
        match c {
            Capture::Key { hid, down } => {
                if self.chords.on_key(hid, down) {
                    return self.stop_all(Home::Panic);
                }
                if down && Hub::pause_chord_pressed(&self.inner, &self.chords, hid) {
                    self.paused = true;
                    return self.stop_all(Home::Pause);
                }
                if down && Hub::return_chord_pressed(&self.inner, &self.chords, hid) {
                    return self.go_home(None, Home::Hotkey);
                }
                self.control.handle(if down { ControlEvent::KeyDown(hid) } else { ControlEvent::KeyUp(hid) });
                let _ = live.ctl.send(StreamMsg::Key { hid, down });
            }
            Capture::Button { button, down } => {
                self.control.handle(if down { ControlEvent::ButtonDown(button) } else { ControlEvent::ButtonUp(button) });
                let _ = live.ctl.send(StreamMsg::Button { button, down });
            }
            Capture::Wheel { dx, dy } => {
                let (dx, dy) = desk_core::normalize_wheel(self.inner.peer_prefs(&peer).scroll_out(), dx, dy);
                let _ = live.ctl.send(StreamMsg::Wheel { dx, dy });
            }
            Capture::Motion { dx, dy } => {
                let speed = self.inner.peer_prefs(&peer).pointer_speed;
                let (dx, dy) = live.pointer.scale(speed, dx, dy);
                live.motion_seq = live.motion_seq.wrapping_add(1);
                // A full queue drops motion; it never waits.
                let _ = live.dg.try_send(Datagram::Motion { seq: live.motion_seq, dx, dy, sent_us: self.inner.now_ms() * 1000 });
            }
        }
    }

    /// Back to this computer. `arrival` is where the pointer appears; without it, where it left.
    fn go_home(&mut self, arrival: Option<(i32, i32)>, why: Home) {
        let Some(r) = self.remote.take() else { return };
        let peer = r.peer;
        // The pointer came back from the other computer (or it let go): it must leave the edge before crossing again.
        if matches!(why, Home::Edge | Home::Leave) {
            self.arming.disarm(peer);
        }
        let why = why.text();
        tracing::info!(peer = short(&peer).as_str(), "desk: pointer home reason={why}");
        // The machine keeps the phase in step; what must happen does not depend on what it thinks.
        let _ = self.control.handle(ControlEvent::CrossHome);
        if let Some(l) = self.live.get(&peer) {
            let _ = l.ctl.send(StreamMsg::ReleaseAll);
        }
        self.inner.host.stop_capture();
        self.chords = Chords::default();
        let target = arrival.unwrap_or(r.home);
        if let Err(e) = self.inner.host.move_cursor(target.0, target.1) {
            self.fail(&format!("Could not move the pointer back: {e}"));
        }
        self.inner.set_emergency(false, r.home);
    }

    /// Panic and pause: the pointer is here, nothing is held anywhere, on both computers.
    fn stop_all(&mut self, why: Home) {
        self.go_home(None, why);
        if let Some(c) = self.controlled.as_ref().map(|c| c.peer) {
            // The other computer holds the mouse: tell it to come home too.
            if let Some(l) = self.live.get(&c) {
                let _ = l.ctl.send(StreamMsg::Leave);
            }
            self.release_controlled();
        }
    }

    /// The session with `peer` is over (link ended, replaced, or gone quiet): let go of everything tied to it.
    fn session_lost(&mut self, peer: &NodeId, why: Home) {
        if self.remote.as_ref().is_some_and(|r| &r.peer == peer) {
            self.go_home(None, why);
        }
        if self.controlled.as_ref().is_some_and(|c| &c.peer == peer) {
            // The link may still be up (it went quiet, it was not ended): say so, so the other computer comes home
            // too, instead of staying on `remote` and answering every crossing with "busy".
            if let Some(l) = self.live.get(peer) {
                let _ = l.ctl.send(StreamMsg::Leave);
            }
            self.release_controlled();
        }
    }

    // ---- being controlled --------------------------------------------------------------------------------

    fn on_frame(&mut self, peer: NodeId, msg: StreamMsg) {
        match msg {
            StreamMsg::Enter { monitor_id, nx_milli, ny_milli } => self.on_enter(peer, &monitor_id, nx_milli, ny_milli),
            StreamMsg::Key { hid, down } => {
                let from = self.live.get(&peer).map_or(HostOs::Windows, |l| host_os(l.os));
                let hid = translate_hid(self.inner.peer_prefs(&peer).modifier_map(), from, host_os(io::os_here()), hid);
                self.inject_held(peer, Inject::Key { hid, down }, |c| {
                    c.pushback.reset();
                    if down {
                        if c.keys.len() < MAX_HELD {
                            c.keys.insert(hid);
                        }
                    } else {
                        c.keys.remove(&hid);
                    }
                });
            }
            StreamMsg::Button { button, down } => self.inject_held(peer, Inject::Button { button, down }, |c| {
                c.pushback.reset();
                if down {
                    c.buttons.insert(button);
                } else {
                    c.buttons.remove(&button);
                }
            }),
            StreamMsg::Wheel { dx, dy } => {
                let (dx, dy) = desk_core::normalize_wheel(self.inner.peer_prefs(&peer).scroll_in(), dx, dy);
                self.inject_held(peer, Inject::Wheel { dx, dy }, |_| {})
            }
            StreamMsg::ReleaseAll | StreamMsg::Leave => {
                let leave = matches!(msg, StreamMsg::Leave);
                if self.controlled.as_ref().is_some_and(|c| c.peer == peer) {
                    self.release_controlled();
                    // The other computer let go of its pointer here: do not take it straight back across that edge.
                    self.refused_until.insert(peer, self.ms() + RETURN_PAUSE_MS);
                    self.arming.disarm(peer);
                }
                // "Leave" from the computer we hold: it refuses, or its pointer came home on its own.
                if leave && self.remote.as_ref().is_some_and(|r| r.peer == peer) {
                    self.go_home(None, Home::Leave);
                }
            }
            StreamMsg::Layout { bytes } => {
                if let Some(m) = arrange::decode_msg(&bytes) {
                    if self.inner.apply_layout(&peer, m) {
                        self.send_layout(peer);
                    }
                }
            }
            StreamMsg::ReturnAt { x, y } => {
                if self.remote.as_ref().is_some_and(|r| r.peer == peer) {
                    let side = self.remote.as_ref().map_or(Side::Right, |r| r.side);
                    self.refresh_displays();
                    let at = onto_screens(&self.displays, (x.saturating_add(self.origin.0), y.saturating_add(self.origin.1)), side);
                    self.go_home(Some(at), Home::Edge);
                }
            }
            StreamMsg::Refused { reason } => {
                // The computer we tried to cross to said no (its `Leave` follows and brings the pointer home). Say
                // why on this page, and do not push the pointer at it again for a moment.
                let reason: String = reason.chars().filter(|c| !c.is_control()).take(MAX_REASON_CHARS).collect();
                let name = self.peer_name(&peer);
                self.refused_until.insert(peer, self.ms() + REFUSED_PAUSE_MS);
                // Both crossed at once and that computer kept its own: this one's crossing was cancelled, and it is
                // being used by that computer now. Nothing for the page to complain about.
                let kept_its_own = self.controlled.as_ref().is_some_and(|c| c.peer == peer);
                if !kept_its_own {
                    self.fail(&format!("{name} did not take the mouse: {reason}"));
                }
                if self.remote.as_ref().is_some_and(|r| r.peer == peer) {
                    self.go_home(None, Home::Refused);
                }
            }
            StreamMsg::Return { t_milli } => {
                if self.remote.as_ref().is_some_and(|r| r.peer == peer) {
                    let side = self.remote.as_ref().map_or(Side::Right, |r| r.side);
                    self.refresh_displays();
                    let arrival = entry_point(&self.displays, side, from_milli(t_milli), super::crossing::ARRIVAL_INSET);
                    self.go_home(arrival, Home::Edge);
                }
            }
            // Hello again, clipboard on the wrong stream, drag: not part of this link.
            _ => {}
        }
    }

    fn on_enter(&mut self, peer: NodeId, monitor_id: &str, nx: u16, ny: u16) {
        let me = self.inner.self_name.clone();
        // `left@3,540`: the exact point, in this computer's local space. A plain `left` is an edge and a fraction.
        let exact = arrange::parse_enter_tag(monitor_id);
        let Some(entry) = exact.map(|(s, _)| s).or_else(|| parse_side(monitor_id)) else {
            let why = format!("{me} did not understand where the pointer came in. Update both computers to the same version.");
            return self.refuse_enter(peer, &why, true);
        };
        // Each computer holds the other's pointer: both crossed at once, or one of them missed the other's Leave. Both
        // accepting would leave each controlling the other with nobody holding anything, so one side keeps its crossing:
        // the computer with the lower id. It says no to the other's Enter (with `Refused`, not `Leave`, which would
        // undo its own crossing); the other comes home and takes the Enter.
        let holding_it = self.remote.as_ref().is_some_and(|r| r.peer == peer);
        let mutual = holding_it && self.controlled.as_ref().map_or(true, |c| c.peer == peer);
        if mutual && self.local_id().to_hex() < peer.to_hex() {
            tracing::info!(peer = short(&peer).as_str(), "desk: did not let the pointer in: both crossed at once, this computer keeps its own");
            if let Some(l) = self.live.get(&peer) {
                let _ = l.ctl.send(StreamMsg::Refused { reason: "busy".to_string() });
            }
            return;
        }
        let busy = !mutual && (self.remote.is_some() || self.controlled.as_ref().is_some_and(|c| c.peer != peer));
        if mutual {
            // The other computer kept its crossing: this one's is cancelled before anything else is checked.
            self.go_home(None, Home::Crossed);
        }
        if busy {
            // Both pointers crossed at once, or a third computer has this one: an ordinary race, not a problem.
            tracing::info!(peer = short(&peer).as_str(), "desk: did not let the pointer in: the mouse is busy here");
            // Refused, then Leave: the sender backs off for a while instead of pushing again at every move.
            if let Some(l) = self.live.get(&peer) {
                let _ = l.ctl.send(StreamMsg::Refused { reason: "busy".to_string() });
                let _ = l.ctl.send(StreamMsg::Leave);
            }
            return;
        }
        if self.paused {
            return self.refuse_enter(peer, &format!("Mouse sharing is paused on {me}."), false);
        }
        if !self.inner.labs() {
            return self.refuse_enter(peer, &format!("My computers is switched off in Labs on {me}."), false);
        }
        if !self.inner.input_allowed(&peer) {
            let them = self.peer_name(&peer);
            return self.refuse_enter(peer, &format!("Mouse and keyboard is off for {them} on {me}. Turn it on there under My computers."), true);
        }
        // Asked before anything moves: a computer that cannot send input says so now, instead of taking the pointer
        // and then ignoring every move while the other computer's mouse is swallowed.
        if let Err(e) = self.inner.host.can_be_controlled() {
            return self.refuse_enter(peer, &e, true);
        }
        if self.controlled.is_some() {
            self.release_controlled();
        }
        self.refresh_displays();
        let t = match entry {
            Side::Left | Side::Right => from_milli(ny),
            Side::Top | Side::Bottom => from_milli(nx),
        };
        let at = match exact {
            Some((_, p)) => Some(onto_screens(&self.displays, (p.0.saturating_add(self.origin.0), p.1.saturating_add(self.origin.1)), entry)),
            None => entry_point(&self.displays, entry, t, super::crossing::ARRIVAL_INSET),
        };
        let Some(at) = at else {
            return self.refuse_enter(peer, &format!("{me} could not find a screen to put the pointer on."), true);
        };
        if let Err(e) = self.inner.host.move_cursor(at.0, at.1) {
            return self.refuse_enter(peer, &format!("{me} could not place the pointer: {e}"), true);
        }
        let expected = self.inner.host.cursor_pos().unwrap_or(at);
        self.controlled = Some(Controlled { peer, keys: BTreeSet::new(), buttons: BTreeSet::new(), pushback: PushBack::default(), expected, last_motion: self.ms() });
        self.error = None;
        tracing::info!(peer = short(&peer).as_str(), side = side_name(entry), "desk: another computer is using this one");
    }

    fn inject_held(&mut self, peer: NodeId, ev: Inject, note: impl FnOnce(&mut Controlled)) {
        let Some(c) = self.controlled.as_mut().filter(|c| c.peer == peer) else { return };
        note(c);
        if let Err(e) = self.inner.host.inject(ev) {
            self.fail(&format!("Could not use the keyboard or mouse: {e}"));
        }
    }

    fn on_motion(&mut self, peer: NodeId, dx: i32, dy: i32) {
        if !self.controlled.as_ref().is_some_and(|c| c.peer == peer) {
            return;
        }
        if let Err(e) = self.inner.host.inject(Inject::Move { dx, dy }) {
            // The pointer cannot move here, so being controlled is useless: give the other computer its mouse back,
            // with the reason, rather than leave it swallowing every move.
            let me = self.inner.self_name.clone();
            self.release_controlled();
            return self.refuse_enter(peer, &format!("{me} could not move the pointer: {e}"), true);
        }
        let Ok(pos) = self.inner.host.cursor_pos() else { return };
        let now = self.ms();
        if let Some(c) = self.controlled.as_mut() {
            c.expected = pos;
            c.last_motion = now;
        }
        // Pushing on through an edge that faces the other computer sends the pointer back, to the matching spot, but
        // only after deliberate pushes in a row (`pushback`): one jitter at the edge does nothing.
        let local = (pos.0 - self.origin.0, pos.1 - self.origin.1);
        let pushing = self.inner.depart_to(&peer, &self.shapes, local, &outward((dx, dy)));
        let Some(c) = self.controlled.as_mut() else { return };
        let home = c.pushback.motion(pushing.is_some(), !c.buttons.is_empty());
        if let Some(dep) = pushing.filter(|_| home) {
            let (x, y) = dep.to;
            let held = self.controlled.as_ref().map_or(0, |c| c.buttons.len());
            tracing::info!("desk: returning at={x},{y} delta={dx},{dy} buttons_held={held}");
            if let Some(l) = self.live.get(&peer) {
                let _ = l.ctl.send(StreamMsg::ReturnAt { x, y });
            }
            self.release_controlled();
            // Not straight back across this edge (see RETURN_PAUSE_MS).
            self.refused_until.insert(peer, self.ms() + RETURN_PAUSE_MS);
            self.arming.disarm(peer);
            // Step off the edge. Resting on it, this computer would take the pointer straight back across (when it
            // sees the other computer on this same edge, as after the layout was swapped) and be left holding the
            // mouse the person is not using.
            let off = inward(pos, dep.side);
            let _ = self.inner.host.move_cursor(off.0, off.1);
            self.step_off = Some((dep.side, self.ms() + STEP_OFF_RECHECK_MS));
            clip_link::spawn_send(&self.inner, peer);
        }
    }

    /// A person is using this computer's own mouse (see `local_use::used_here`): the mouse goes back to the other
    /// computer, which is told to come home, and this computer does not take the pointer straight back across.
    fn hand_back_if_used_here(&mut self, pos: (i32, i32), now: u64) -> bool {
        let Some(c) = self.controlled.as_ref() else { return false };
        let since = now.saturating_sub(c.last_motion);
        if !local_use::used_here(c.expected, pos, since, self.inner.host.buttons_held(), c.buttons.len()) {
            return false;
        }
        let peer = c.peer;
        tracing::info!(peer = short(&peer).as_str(), "desk: taking the mouse back: this computer is being used locally");
        if let Some(l) = self.live.get(&peer) {
            let _ = l.ctl.send(StreamMsg::Leave);
        }
        self.release_controlled();
        self.refused_until.insert(peer, now + RETURN_PAUSE_MS);
        self.arming.disarm(peer);
        true
    }

    /// The second step-off: after a hand-back, posted mouse events still applied on this computer can put the pointer
    /// back on the edge it was stepped off. If it is within `EDGE_NEAR` of that edge when the check is due, it is
    /// stepped in again. Nothing happens while this computer is holding or being controlled again.
    fn recheck_step_off(&mut self, pos: (i32, i32), now: u64) {
        let Some((side, due)) = self.step_off else { return };
        if now < due {
            return;
        }
        self.step_off = None;
        if self.remote.is_some() || self.controlled.is_some() {
            return;
        }
        let Some(u) = union(&self.displays) else { return };
        if local_use::near_edge(pos, u, side, local_use::EDGE_NEAR) {
            let off = inward(pos, side);
            let _ = self.inner.host.move_cursor(off.0, off.1);
        }
    }

    /// Key-up and button-up for everything pressed on behalf of the peer.
    fn release_controlled(&mut self) {
        let Some(c) = self.controlled.take() else { return };
        let (keys, buttons) = (c.keys.len(), c.buttons.len());
        tracing::info!("desk: held at end keys={keys} buttons={buttons}");
        for hid in c.keys {
            let _ = self.inner.host.inject(Inject::Key { hid, down: false });
        }
        for button in c.buttons {
            let _ = self.inner.host.inject(Inject::Button { button, down: false });
        }
        self.inner.host.release_injected();
    }

    // ---- upkeep ------------------------------------------------------------------------------------------

    fn upkeep(&mut self) {
        self.flush_publish();
        self.upkeeps += 1;
        if !self.inner.labs() {
            return;
        }
        if self.upkeeps % 4 == 0 {
            self.refresh_displays();
        }
        self.inner.reconcile_later();
        let now = self.ms();
        // Dial every computer whose switch is on and that is not linked or being dialed.
        let want: Vec<NodeId> = self.inner.trust.list().iter().map(|d| d.node).filter(|n| self.inner.input_allowed(n)).collect();
        for peer in &want {
            if self.sessions.reach(peer) == Reach::Offline && self.ledger.begin(peer, now) {
                self.spawn_dial(*peer);
            }
        }
        // A switch that went off, or a computer that was removed.
        for (peer, live) in &self.live {
            if !want.contains(peer) {
                live.link.close(CLOSE_STOPPED, "bye");
            }
        }
        for peer in self.ledger_forget(&want) {
            self.ledger.forget(&peer);
        }
    }

    fn ledger_forget(&self, want: &[NodeId]) -> Vec<NodeId> {
        self.inner.trust.list().iter().map(|d| d.node).filter(|n| !want.contains(n)).collect()
    }

    fn spawn_dial(&self, peer: NodeId) {
        let inner = self.inner.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let addr = inner.p2p.addrs().peer(peer);
            match inner.p2p.dial(&addr, capability::DESK_INPUT).await {
                Ok(link) => io::run_link(inner, peer, link, true).await,
                Err(_) => {
                    // Usually: that computer's app is not running, its My computers page was not opened since it
                    // started, or Mouse and keyboard is off for this computer there.
                    tracing::debug!(peer = short(&peer).as_str(), "desk: could not reach a computer for mouse sharing");
                    let _ = tx.send(Msg::DialResult { peer, ok: false });
                }
            }
        });
    }

    // ---- the page ----------------------------------------------------------------------------------------

    /// What the page shows that the hub decides (the reach of each computer comes from the sessions).
    fn shown(&self) -> (Option<NodeId>, Option<NodeId>, bool, Option<String>) {
        (self.remote.as_ref().map(|r| r.peer), self.controlled.as_ref().map(|c| c.peer), self.paused, self.error.clone())
    }

    /// Hands the page a new view now.
    fn publish(&mut self) {
        let now = self.ms();
        self.publish_now(now);
    }

    /// Like `publish`, but at most once per `PUBLISH_EVERY_MS`: a change inside that window waits for the next tick
    /// (`flush_publish`). Used for what motion changes, which can arrive a thousand times a second.
    fn publish_capped(&mut self) {
        let now = self.ms();
        if self.published_at.is_some_and(|t| now.saturating_sub(t) < PUBLISH_EVERY_MS) {
            self.publish_waiting = true;
            return;
        }
        self.publish_now(now);
    }

    fn flush_publish(&mut self) {
        if self.publish_waiting {
            self.publish_capped();
        }
    }

    fn publish_now(&mut self, now: u64) {
        self.published_at = Some(now);
        self.publish_waiting = false;
        let reach: HashMap<NodeId, Reach> = self.inner.trust.list().iter().map(|d| (d.node, self.sessions.reach(&d.node))).collect();
        self.inner.publish(reach, self.remote.as_ref().map(|r| r.peer), self.controlled.as_ref().map(|c| c.peer), self.paused, self.error.clone());
    }

    fn teardown(&mut self) {
        self.go_home(None, Home::Ended);
        self.release_controlled();
        self.inner.host.stop_capture();
        for (_, l) in self.live.drain() {
            l.link.close(CLOSE_STOPPED, "bye");
        }
        self.inner.publish(HashMap::new(), None, None, self.paused, None);
    }
}
