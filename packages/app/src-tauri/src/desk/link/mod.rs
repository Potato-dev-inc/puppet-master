//! My computers, phases DL-7 and DL-9: the link between two paired computers, edge crossing, and the clipboard.
//!
//! The mouse and keyboard of one computer drive another: push the pointer through the edge facing it and it appears
//! there; type, click and scroll there; push back and it returns. Copy here, paste there, on the same crossing.
//!
//! **What this is built on**: the PC link (`p2p/`). This module registers two capabilities there, `desk/input`
//! (`pm/desk/1`, direct only) and `desk/clipboard` (`pm/desk-clipboard/1`), and nothing else touches the network.
//! A computer may use them only if it is paired AND the person turned that computer's switch on ("Mouse and
//! keyboard", "Clipboard"). Turning a switch off, removing the computer, or turning Labs off closes the link within
//! 100 ms. The endpoint is not even listening for a capability no switch wants.
//!
//! **Safety** (each has a test in `tests.rs`):
//! - Every key and button pressed on a computer for the other one is released when control ends, the link ends, or
//!   the other side goes quiet for 250 ms (the heartbeat is 100 ms; 3 s of silence closes the link).
//! - Ctrl+Alt+Esc (panic) and Scroll Lock (pause) are matched on the raw keys while the pointer is away: the pointer
//!   comes home and everything is released, on both computers.
//! - The pointer always comes home when Puppet Master quits or Labs is switched off (`DeskLink::shutdown`).
//! - Nothing here is reachable by agents, MCP or the phone, and no agent can start sharing (`guard_tests.rs`).
//! - Elevated windows (UIPI): Windows ignores injected input aimed at a window that runs as admin. The page says
//!   so: "Can't control windows that run as admin."
//!
//! **Tests** use the in-memory net and `MockRemote` only. They never open a socket, move the real pointer, inject
//! a key, install a hook, register a hotkey or touch the real clipboard.
//!
//! Layout: `hub` (decisions), `io` (one link's tasks), `clip_link` (clipboard streams), `clipboard` (chunk rules),
//! `geometry` (edges), `arrange` (where each computer sits in one shared space, and where the pointer crosses),
//! `remote` (the OS trait), `wire` (framing), `commands` (the page's commands).
//!
//! **The layout**: each paired computer is placed at a spot relative to this one (`arrange::Spot`). The two computers
//! send each other their screens and the placement (`StreamMsg::Layout`, newest revision wins, ties go to the lower
//! node id) so both agree; moving a computer on either one updates the other. The pointer crosses only where screens
//! touch and appears at the same place in the shared space.

pub mod arrange;
pub mod clip_link;
pub mod clipboard;
pub mod commands;
pub mod geometry;
pub mod hub;
pub mod io;
pub mod remote;
pub mod wire;

#[cfg(test)]
pub mod mock;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use desk_core::Side;
use desk_proto::OsKind;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::p2p::alpn;
use crate::p2p::capability::{self, Capability, CapabilityHandler, Permissions, RelayPolicy};
use crate::p2p::endpoint::{Link, NodeId, P2pError};
use crate::p2p::session::Reach;
use crate::p2p::trust::TrustList;
use crate::p2p::P2p;

use self::arrange::{collide, default_spot, dominant_side, localize, shifted, size_of, Shape, Spot};
use self::geometry::{parse_side, side_name};
use self::hub::{Hub, Msg};
use self::io::LinkShared;
use self::remote::RemoteHost;

pub(super) const CLOSE_PROTOCOL: u32 = 20;
pub(super) const CLOSE_DUPLICATE: u32 = 21;
pub(super) const CLOSE_REPLACED: u32 = 22;
pub(super) const CLOSE_DEAD: u32 = 23;
pub(super) const CLOSE_STOPPED: u32 = 24;

/// The honest line about elevated windows.
pub const ADMIN_NOTE: &str = "Can't control windows that run as admin.";
pub const TRAY_ON: &str = "Mouse sharing is on";

/// Where each paired computer sits relative to this one ("right" until the person says otherwise).
pub trait LinkStore: Send + Sync {
    fn load(&self) -> BTreeMap<String, String>;
    fn save(&self, sides: &BTreeMap<String, String>) -> Result<(), String>;
}

#[derive(Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct MemLinkStore(Mutex<BTreeMap<String, String>>);

impl LinkStore for MemLinkStore {
    fn load(&self) -> BTreeMap<String, String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn save(&self, sides: &BTreeMap<String, String>) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = sides.clone();
        Ok(())
    }
}

/// `desk/link.json` in the app data folder: public node ids and the words left/right/top/bottom. No secrets.
pub struct FileLinkStore {
    pub path: std::path::PathBuf,
}

impl LinkStore for FileLinkStore {
    fn load(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    fn save(&self, sides: &BTreeMap<String, String>) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(sides).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

/// What this computer remembers about one paired computer's place in the layout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PeerRec {
    /// Where it sits relative to this computer. `None` until anyone moved it (then `arrange::default_spot`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot: Option<Spot>,
    /// When that was decided (milliseconds since 1970 on the computer where it was decided). 0 = never.
    #[serde(default)]
    pub rev: u64,
    /// Its screens the last time it told us (in its own local space), so the layout can be drawn while it is off.
    #[serde(default)]
    pub screens: Vec<Shape>,
}

/// The store keeps one string per computer: JSON of [`PeerRec`], or one of the old words left/right/top/bottom.
fn decode_store(raw: BTreeMap<String, String>) -> BTreeMap<String, PeerRec> {
    let mut out = BTreeMap::new();
    for (node, text) in raw {
        let rec = if text.trim_start().starts_with('{') {
            serde_json::from_str(&text).unwrap_or_default()
        } else {
            match parse_side(&text) {
                Some(side) => PeerRec { spot: Some(Spot::Side(side)), rev: 1, screens: Vec::new() },
                None => PeerRec::default(),
            }
        };
        out.insert(node, rec);
    }
    out
}

fn encode_store(peers: &BTreeMap<String, PeerRec>) -> BTreeMap<String, String> {
    peers.iter().filter_map(|(k, v)| serde_json::to_string(v).ok().map(|s| (k.clone(), s))).collect()
}

fn next_rev(prev: u64) -> u64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    now.max(prev + 1)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeviceLink {
    pub node: String,
    pub name: String,
    /// `online`, `connecting`, `offline`, or `off` (the person has not turned "Mouse and keyboard" on).
    pub state: &'static str,
    /// `left`, `right`, `top`, `bottom`: where it sits relative to this computer.
    pub side: &'static str,
    /// The pointer is on this computer right now.
    pub mouse_here: bool,
    /// This computer is being driven by that one right now.
    pub controls_me: bool,
    pub input_on: bool,
    pub clipboard_on: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LinkView {
    pub seq: u64,
    pub devices: Vec<DeviceLink>,
    /// "Mouse is on <Name>": the pointer left this computer for that one.
    pub mouse_on: Option<String>,
    /// That computer is using this one.
    pub controlled_by: Option<String>,
    pub paused: bool,
    /// At least one computer has "Mouse and keyboard" on and Labs is on.
    pub sharing: bool,
    /// The tray's status line, only while sharing.
    pub tray: Option<&'static str>,
    pub admin_note: &'static str,
    pub error: Option<String>,
    /// Why the last copy did not go across.
    pub clipboard_note: Option<String>,
    pub supported: bool,
}

/// One screen in the shared space (this computer's local space, so this computer starts at 0, 0).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutScreen {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutComputer {
    /// Empty for this computer, the node id of a paired one.
    pub node: String,
    pub name: String,
    pub me: bool,
    pub online: bool,
    /// False until it has told us its screens (then this computer's own screens are drawn as a stand-in).
    pub known: bool,
    /// Top-left corner of the box around its screens, and the size of that box.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub screens: Vec<LayoutScreen>,
    /// Zero until someone moved it.
    pub rev: u64,
}

/// Every computer's screens in one coordinate space. This computer's box starts at (0, 0).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutView {
    pub computers: Vec<LayoutComputer>,
    /// Edges closer than this (shared-space pixels) count as touching, so the page can snap to it.
    pub touch_gap: i32,
}

pub type LayoutNotifier = Arc<dyn Fn(&LayoutView) + Send + Sync>;

static SHARING: AtomicBool = AtomicBool::new(false);

/// Whether the tray's tooltip should say "Mouse sharing is on".
pub fn sharing_on() -> bool {
    SHARING.load(Ordering::SeqCst)
}

pub type Notifier = Arc<dyn Fn(&LinkView) + Send + Sync>;

#[derive(Default)]
struct Mirror {
    reach: HashMap<NodeId, Reach>,
    mouse_on: Option<NodeId>,
    controlled_by: Option<NodeId>,
    paused: bool,
    error: Option<String>,
    seq: u64,
    last: Option<LinkView>,
    last_layout: Option<LayoutView>,
}

#[derive(Default)]
struct Emergency {
    capturing: bool,
    home: (i32, i32),
}

#[derive(Default)]
struct ClipState {
    /// Fingerprint of the last copy written here from another computer.
    applied: Option<u64>,
    sent: HashMap<NodeId, u64>,
}

pub(super) struct Inner {
    pub(super) p2p: Arc<P2p>,
    pub(super) trust: Arc<TrustList>,
    pub(super) host: Arc<dyn RemoteHost>,
    pub(super) self_name: String,
    pub(super) self_os: OsKind,
    clock0: Instant,
    next_link: AtomicU64,
    next_token: AtomicU64,
    labs: AtomicBool,
    hub: Mutex<Option<(mpsc::UnboundedSender<Msg>, JoinHandle<()>)>>,
    store: Arc<dyn LinkStore>,
    peers: Mutex<BTreeMap<String, PeerRec>>,
    /// This computer's screens in local space and where that space's origin is on the desktop. Kept by the hub.
    shapes: Mutex<(Vec<Shape>, (i32, i32))>,
    mirror: Mutex<Mirror>,
    notifier: Mutex<Option<Notifier>>,
    layout_notifier: Mutex<Option<LayoutNotifier>>,
    emergency: Mutex<Emergency>,
    stats: Mutex<HashMap<NodeId, Arc<LinkShared>>>,
    clip: Mutex<ClipState>,
    clip_links: Mutex<HashMap<NodeId, Arc<dyn Link>>>,
    clip_note: Mutex<Option<String>>,
    reconciling: AtomicBool,
    wanted: Mutex<(bool, bool)>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Inner {
    pub(super) fn now_ms(&self) -> u64 {
        self.clock0.elapsed().as_millis() as u64
    }

    pub(super) fn labs(&self) -> bool {
        self.labs.load(Ordering::SeqCst)
    }

    pub(super) fn node_hex(&self) -> String {
        self.p2p.node_id().map(|n| n.to_hex()).unwrap_or_default()
    }

    pub(super) fn hub_tx(&self) -> Option<mpsc::UnboundedSender<Msg>> {
        lock(&self.hub).as_ref().map(|(tx, _)| tx.clone())
    }

    pub(super) fn new_link_id(&self) -> u64 {
        self.next_link.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub(super) fn next_token(&self) -> u64 {
        self.next_token.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub(super) fn register_stats(&self, peer: NodeId, s: Arc<LinkShared>) {
        lock(&self.stats).insert(peer, s);
    }

    /// Paired, and the person turned "Mouse and keyboard" on for it.
    pub(super) fn input_allowed(&self, peer: &NodeId) -> bool {
        self.labs() && self.trust.allowed(peer, capability::DESK_INPUT)
    }

    pub(super) fn clipboard_allowed(&self, peer: &NodeId) -> bool {
        self.labs() && self.trust.allowed(peer, capability::DESK_CLIPBOARD)
    }

    /// This computer's screens in local space, and the desktop position of that space's origin.
    pub(super) fn my_shapes(&self) -> (Vec<Shape>, (i32, i32)) {
        let cached = lock(&self.shapes).clone();
        if !cached.0.is_empty() {
            return cached;
        }
        let fresh = self.host.displays().map(|d| localize(&d)).unwrap_or_default();
        if !fresh.0.is_empty() {
            *lock(&self.shapes) = fresh.clone();
        }
        fresh
    }

    pub(super) fn set_my_shapes(&self, s: (Vec<Shape>, (i32, i32))) -> bool {
        let mut cur = lock(&self.shapes);
        let changed = *cur != s;
        *cur = s;
        changed
    }

    /// Where `hex` sits relative to this computer, and when that was decided.
    fn spot_of(&self, peers: &BTreeMap<String, PeerRec>, hex: &str) -> (Spot, u64) {
        match peers.get(hex) {
            Some(PeerRec { spot: Some(s), rev, .. }) => (*s, *rev),
            _ => (default_spot(&self.node_hex(), hex), 0),
        }
    }

    /// The pointer rests at `pos` (this computer's local space): where does it go on `peer`?
    pub(super) fn depart_to(&self, peer: &NodeId, mine: &[Shape], pos: (i32, i32), sides: &[Side]) -> Option<arrange::Departure> {
        let hex = peer.to_hex();
        let peers = lock(&self.peers);
        let (spot, _) = self.spot_of(&peers, &hex);
        let theirs = peers.get(&hex).map(|r| r.screens.as_slice()).filter(|s| !s.is_empty()).unwrap_or(mine);
        let at = spot.resolve(size_of(mine), size_of(theirs));
        arrange::depart(mine, theirs, at, pos, sides)
    }

    /// Width and height of the box around `peer`'s screens (this computer's own size until it has told us).
    pub(super) fn peer_box(&self, peer: &NodeId) -> (i32, i32) {
        let told = lock(&self.peers).get(&peer.to_hex()).map(|r| size_of(&r.screens)).filter(|s| s.0 > 0);
        told.unwrap_or_else(|| size_of(&self.my_shapes().0))
    }

    /// The layout message for `peer`: our screens, and where we sit relative to it.
    pub(super) fn layout_bytes(&self, peer: &NodeId) -> Option<Vec<u8>> {
        let screens = self.my_shapes().0;
        if screens.is_empty() {
            return None;
        }
        let (spot, rev) = self.spot_of(&lock(&self.peers), &peer.to_hex());
        Some(arrange::encode_msg(&arrange::LayoutMsg { v: arrange::LAYOUT_V, screens, spot: spot.mirror(), rev }))
    }

    /// A layout message arrived. Returns whether our answer is newer, so it should be said back.
    pub(super) fn apply_layout(&self, peer: &NodeId, m: arrange::LayoutMsg) -> bool {
        let hex = peer.to_hex();
        let peer_lower = hex < self.node_hex();
        let (changed, reply) = {
            let mut peers = lock(&self.peers);
            let (local, local_rev) = self.spot_of(&peers, &hex);
            let rec = peers.entry(hex).or_default();
            let mut changed = rec.screens != m.screens;
            rec.screens = m.screens;
            // `m.spot` is where the sender sits relative to us: exactly what we keep for it.
            let theirs = m.spot;
            let (mut adopt, mut reply) = (false, false);
            if m.rev > local_rev {
                adopt = true;
            } else if m.rev == local_rev {
                if local_rev > 0 && theirs != local {
                    adopt = peer_lower;
                    reply = !peer_lower;
                }
            } else {
                reply = true;
            }
            if adopt && (rec.spot != Some(theirs) || rec.rev != m.rev) {
                rec.spot = Some(theirs);
                rec.rev = m.rev;
                changed = true;
            }
            (changed, reply)
        };
        if changed {
            self.persist();
            self.notify();
            self.notify_layout();
        }
        reply
    }

    fn persist(&self) {
        let _ = self.store.save(&encode_store(&lock(&self.peers)));
    }

    pub(super) fn build_layout(&self) -> LayoutView {
        let (mine, _) = self.my_shapes();
        let devices = self.trust.list();
        let m = lock(&self.mirror);
        let peers = lock(&self.peers);
        let (mw, mh) = size_of(&mine);
        let screens_at = |shapes: &[Shape], by: (i32, i32)| -> Vec<LayoutScreen> {
            shifted(shapes, by)
                .into_iter()
                .map(|s| LayoutScreen { id: s.id, name: s.name, x: s.rect.x, y: s.rect.y, width: s.rect.width, height: s.rect.height, primary: s.primary })
                .collect()
        };
        let mut computers = vec![LayoutComputer {
            node: String::new(),
            name: self.self_name.clone(),
            me: true,
            online: true,
            known: !mine.is_empty(),
            x: 0,
            y: 0,
            width: mw,
            height: mh,
            screens: screens_at(&mine, (0, 0)),
            rev: 0,
        }];
        for d in devices.iter().filter(|d| d.allowed.contains(capability::DESK_INPUT)) {
            let hex = d.node.to_hex();
            let told = peers.get(&hex).map(|r| r.screens.clone()).filter(|s| !s.is_empty());
            let known = told.is_some();
            let theirs = told.unwrap_or_else(|| mine.clone());
            let (spot, rev) = self.spot_of(&peers, &hex);
            let (tw, th) = size_of(&theirs);
            let at = spot.resolve((mw, mh), (tw, th));
            computers.push(LayoutComputer {
                node: hex,
                name: d.name.clone(),
                me: false,
                online: m.reach.get(&d.node).copied() == Some(Reach::Online),
                known,
                x: at.0,
                y: at.1,
                width: tw,
                height: th,
                screens: screens_at(&theirs, at),
                rev,
            });
        }
        LayoutView { computers, touch_gap: desk_core::MAX_GAP_PX }
    }

    pub(super) fn notify_layout(&self) {
        let view = self.build_layout();
        let send = {
            let mut m = lock(&self.mirror);
            if m.last_layout.as_ref() == Some(&view) {
                false
            } else {
                m.last_layout = Some(view.clone());
                true
            }
        };
        if send {
            let n = lock(&self.layout_notifier).clone();
            if let Some(n) = n {
                n(&view);
            }
        }
    }

    pub(super) fn set_emergency(&self, capturing: bool, home: (i32, i32)) {
        let mut e = lock(&self.emergency);
        e.capturing = capturing;
        e.home = home;
    }

    /// Gives the pointer and keyboard back, whatever the hub is doing. Synchronous, safe from anywhere, safe twice.
    pub(super) fn emergency_home(&self) {
        let (capturing, home) = {
            let mut e = lock(&self.emergency);
            let was = (e.capturing, e.home);
            e.capturing = false;
            was
        };
        if capturing {
            self.host.stop_capture();
            let _ = self.host.move_cursor(home.0, home.1);
        }
        self.host.release_injected();
    }

    pub(super) fn set_clip_note(&self, note: Option<String>) {
        let changed = {
            let mut n = lock(&self.clip_note);
            let changed = *n != note;
            *n = note;
            changed
        };
        if changed {
            self.notify();
        }
    }

    /// Whether this copy is worth sending to `peer`: it did not come from another computer and was not sent already.
    pub(super) fn clip_is_new(&self, peer: &NodeId, fp: u64) -> bool {
        let c = lock(&self.clip);
        c.applied != Some(fp) && c.sent.get(peer) != Some(&fp)
    }

    pub(super) fn clip_sent(&self, peer: &NodeId, fp: u64) {
        lock(&self.clip).sent.insert(*peer, fp);
    }

    pub(super) fn clip_applied(&self, fp: u64) {
        lock(&self.clip).applied = Some(fp);
    }

    pub(super) fn cached_clip_link(&self, peer: &NodeId) -> Option<Arc<dyn Link>> {
        lock(&self.clip_links).get(peer).cloned()
    }

    pub(super) fn cache_clip_link(&self, peer: NodeId, link: Arc<dyn Link>) {
        lock(&self.clip_links).insert(peer, link);
    }

    pub(super) fn forget_clip_link(&self, peer: &NodeId) {
        lock(&self.clip_links).remove(peer);
    }

    /// The plain word for where `peer` sits: by where its middle is.
    fn side_name_of(&self, peer: &NodeId) -> &'static str {
        let (mine, _) = self.my_shapes();
        let peers = lock(&self.peers);
        let hex = peer.to_hex();
        let (spot, _) = self.spot_of(&peers, &hex);
        let theirs = peers.get(&hex).map(|r| r.screens.as_slice()).filter(|s| !s.is_empty()).unwrap_or(mine.as_slice());
        let (ms, ts) = (size_of(&mine), size_of(theirs));
        side_name(dominant_side(ms, ts, spot.resolve(ms, ts)))
    }

    /// Called by the hub after every message.
    pub(super) fn publish(&self, reach: HashMap<NodeId, Reach>, mouse_on: Option<NodeId>, controlled_by: Option<NodeId>, paused: bool, error: Option<String>) {
        {
            let mut m = lock(&self.mirror);
            m.reach = reach;
            m.mouse_on = mouse_on;
            m.controlled_by = controlled_by;
            m.paused = paused;
            m.error = error;
        }
        self.notify();
    }

    pub(super) fn build_view(&self) -> LinkView {
        let m = lock(&self.mirror);
        let devices = self.trust.list();
        let name_of = |n: &Option<NodeId>| n.and_then(|n| devices.iter().find(|d| d.node == n).map(|d| d.name.clone()));
        let mut views = Vec::new();
        for d in &devices {
            let input_on = d.allowed.contains(capability::DESK_INPUT);
            let state = if !input_on {
                "off"
            } else {
                match m.reach.get(&d.node).copied().unwrap_or(Reach::Offline) {
                    Reach::Online => "online",
                    Reach::Connecting => "connecting",
                    Reach::Offline => "offline",
                }
            };
            views.push(DeviceLink {
                node: d.node.to_hex(),
                name: d.name.clone(),
                state,
                side: self.side_name_of(&d.node),
                mouse_here: m.mouse_on == Some(d.node),
                controls_me: m.controlled_by == Some(d.node),
                input_on,
                clipboard_on: d.allowed.contains(capability::DESK_CLIPBOARD),
            });
        }
        let sharing = self.labs() && views.iter().any(|d| d.input_on);
        LinkView {
            seq: m.seq,
            mouse_on: name_of(&m.mouse_on),
            controlled_by: name_of(&m.controlled_by),
            paused: m.paused,
            sharing,
            tray: sharing.then_some(TRAY_ON),
            admin_note: ADMIN_NOTE,
            error: m.error.clone(),
            clipboard_note: lock(&self.clip_note).clone(),
            supported: self.host.supported(),
            devices: views,
        }
    }

    /// Tells the page, when something it shows changed.
    pub(super) fn notify(&self) {
        let mut view = self.build_view();
        let send = {
            let mut m = lock(&self.mirror);
            let same = m.last.as_ref().is_some_and(|p| {
                let mut a = p.clone();
                a.seq = view.seq;
                a == view
            });
            if same {
                false
            } else {
                m.seq += 1;
                view.seq = m.seq;
                m.last = Some(view.clone());
                true
            }
        };
        if send {
            if SHARING.swap(view.sharing, Ordering::SeqCst) != view.sharing {
                #[cfg(not(test))]
                crate::tray::request_refresh();
            }
            let n = lock(&self.notifier).clone();
            if let Some(n) = n {
                n(&view);
            }
        }
    }

    /// Makes the endpoint listen for the capabilities some switch wants, and for no others.
    pub(super) fn reconcile_later(self: &Arc<Self>) {
        let devices = self.trust.list();
        let input = devices.iter().any(|d| d.allowed.contains(capability::DESK_INPUT));
        let clip = devices.iter().any(|d| d.allowed.contains(capability::DESK_CLIPBOARD));
        let want = (input && self.labs(), clip && self.labs());
        let settled = *lock(&self.wanted) == want && (self.p2p.is_running() || want == (false, false));
        if settled || self.reconciling.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            for (id, on) in [(capability::DESK_INPUT, want.0), (capability::DESK_CLIPBOARD, want.1)] {
                if on {
                    let _ = me.p2p.want(id).await;
                } else {
                    me.p2p.unwant(id).await;
                }
            }
            *lock(&me.wanted) = want;
            me.reconciling.store(false, Ordering::SeqCst);
        });
    }
}

struct InputHandler(Weak<Inner>);
struct ClipboardHandler(Weak<Inner>);

#[async_trait]
impl CapabilityHandler for InputHandler {
    async fn serve(&self, peer: NodeId, link: Arc<dyn Link>) {
        match self.0.upgrade() {
            Some(inner) => io::run_link(inner, peer, link, false).await,
            None => link.close(CLOSE_STOPPED, "bye"),
        }
    }
}

#[async_trait]
impl CapabilityHandler for ClipboardHandler {
    async fn serve(&self, peer: NodeId, link: Arc<dyn Link>) {
        match self.0.upgrade() {
            Some(inner) => clip_link::serve_incoming(inner, peer, link).await,
            None => link.close(CLOSE_STOPPED, "bye"),
        }
    }
}

/// The desk link service. Built once; starts nothing until `start`.
pub struct DeskLink {
    inner: Arc<Inner>,
}

impl DeskLink {
    /// Registers `desk/input` and `desk/clipboard` on the PC link. Opens no socket, reads no key, starts no task.
    pub fn new(p2p: Arc<P2p>, trust: Arc<TrustList>, host: Arc<dyn RemoteHost>, store: Arc<dyn LinkStore>, name: &str) -> Result<Arc<Self>, P2pError> {
        let peers = decode_store(store.load());
        let inner = Arc::new(Inner {
            p2p: p2p.clone(),
            trust,
            host,
            self_name: crate::p2p::trust::clean_name(name),
            self_os: io::os_here(),
            clock0: Instant::now(),
            next_link: AtomicU64::new(0),
            next_token: AtomicU64::new(0),
            labs: AtomicBool::new(false),
            hub: Mutex::new(None),
            store,
            peers: Mutex::new(peers),
            shapes: Mutex::new((Vec::new(), (0, 0))),
            mirror: Mutex::new(Mirror::default()),
            notifier: Mutex::new(None),
            layout_notifier: Mutex::new(None),
            emergency: Mutex::new(Emergency::default()),
            stats: Mutex::new(HashMap::new()),
            clip: Mutex::new(ClipState::default()),
            clip_links: Mutex::new(HashMap::new()),
            clip_note: Mutex::new(None),
            reconciling: AtomicBool::new(false),
            wanted: Mutex::new((false, false)),
        });
        let weak = Arc::downgrade(&inner);
        p2p.registry().register(Capability::new(capability::DESK_INPUT, alpn::DESK, RelayPolicy::Never, Arc::new(InputHandler(weak.clone()))))?;
        p2p.registry().register(Capability::new(capability::DESK_CLIPBOARD, alpn::DESK_CLIPBOARD, RelayPolicy::Never, Arc::new(ClipboardHandler(weak))))?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn set_notifier(&self, n: Notifier) {
        *lock(&self.inner.notifier) = Some(n);
    }

    pub fn set_layout_notifier(&self, n: LayoutNotifier) {
        *lock(&self.inner.layout_notifier) = Some(n);
    }

    /// The Labs switch. Off is the kill switch: links close, the pointer comes home, the endpoint stops listening.
    pub fn apply_settings(&self, labs: bool) {
        self.inner.labs.store(labs, Ordering::SeqCst);
        if !labs {
            self.stop();
        }
    }

    /// Starts the hub when Labs is on (once). Needs a tokio runtime.
    pub fn start(&self) {
        if !self.inner.labs() {
            return;
        }
        let mut slot = lock(&self.inner.hub);
        if slot.is_some() {
            return;
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let hub = Hub::new(self.inner.clone(), tx.clone());
        *slot = Some((tx, tokio::spawn(hub.run(rx))));
    }

    /// The hub ends, every link closes, the pointer comes home.
    pub fn stop(&self) {
        let hub = lock(&self.inner.hub).take();
        self.inner.emergency_home();
        if let Some((tx, _task)) = hub {
            let _ = tx.send(Msg::Shutdown);
        }
        lock(&self.inner.clip_links).clear();
        *lock(&self.inner.wanted) = (false, false);
    }

    /// App quit: same as `stop`. The pointer is home before this returns.
    pub fn shutdown(&self) {
        self.stop();
    }

    pub fn view(&self) -> LinkView {
        self.inner.build_view()
    }

    /// Where a computer sits relative to this one, as a snapped placement: touching on that side, top edges aligned.
    pub fn set_side(&self, node: &NodeId, side: &str) -> Result<(), String> {
        let side = parse_side(side).ok_or_else(|| "Pick left, right, above or below.".to_string())?;
        self.place(node, Spot::Side(side))
    }

    /// Puts a computer's screens at (x, y): the top-left corner of the box around them, in the space where this
    /// computer's own box starts at (0, 0). The other computer is told, and agrees.
    pub fn set_position(&self, node: &NodeId, x: i32, y: i32) -> Result<(), String> {
        if x.abs() > arrange::MAX_COORD || y.abs() > arrange::MAX_COORD {
            return Err("That is too far away.".to_string());
        }
        // Its own box, not the box of the whole layout: the page sends where the box is.
        self.place(node, Spot::At { x, y })
    }

    /// Everything the layout page draws.
    pub fn layout(&self) -> LayoutView {
        self.inner.build_layout()
    }

    fn place(&self, node: &NodeId, spot: Spot) -> Result<(), String> {
        let hex = node.to_hex();
        {
            let mut peers = lock(&self.inner.peers);
            let (mine, _) = self.inner.my_shapes();
            // Screens never lie on top of each other, this computer's or another paired computer's.
            let screens_of = |peers: &BTreeMap<String, PeerRec>, h: &str| -> Vec<Shape> {
                peers.get(h).map(|r| r.screens.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| mine.clone())
            };
            let theirs = screens_of(&peers, &hex);
            let at = spot.resolve(size_of(&mine), size_of(&theirs));
            let placed = shifted(&theirs, at);
            if collide(&mine, &placed) {
                return Err("That would put the screens on top of each other.".to_string());
            }
            for other in peers.keys().filter(|k| **k != hex) {
                let is_paired = self.inner.trust.list().iter().any(|d| d.node.to_hex() == *other && d.allowed.contains(capability::DESK_INPUT));
                if !is_paired {
                    continue;
                }
                let o = screens_of(&peers, other);
                let (o_spot, _) = self.inner.spot_of(&peers, other);
                let o_at = o_spot.resolve(size_of(&mine), size_of(&o));
                if collide(&shifted(&o, o_at), &placed) {
                    return Err("That would put the screens on top of another computer.".to_string());
                }
            }
            let rec = peers.entry(hex).or_default();
            rec.spot = Some(spot);
            rec.rev = next_rev(rec.rev);
        }
        self.inner.store.save(&encode_store(&lock(&self.inner.peers))).map_err(|e| format!("Could not save the layout: {e}"))?;
        self.inner.notify();
        self.inner.notify_layout();
        self.send(Msg::LayoutChanged);
        Ok(())
    }

    /// Pause sharing (the pointer comes home) or resume.
    pub fn pause(&self, paused: bool) {
        self.send(Msg::Pause(paused));
    }

    /// The panic key's effect from the page: pointer home, everything released, on both computers.
    pub fn panic(&self) {
        self.inner.emergency_home();
        self.send(Msg::Panic);
    }

    /// Sends this computer's clipboard to that computer now, as it does when the pointer crosses (the soak tool uses it).
    pub fn push_clipboard(&self, peer: &NodeId) {
        clip_link::spawn_send(&self.inner, *peer);
    }

    /// A switch changed or a computer was removed: look now instead of at the next upkeep.
    pub fn permission_changed(&self) {
        self.send(Msg::Kick);
    }

    fn send(&self, m: Msg) {
        if let Some(tx) = self.inner.hub_tx() {
            let _ = tx.send(m);
        }
    }

    /// Heartbeats received from a computer and the longest gap between two, in milliseconds (the soak reads this).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn beat_stats(&self, peer: &NodeId) -> Option<(u64, u64)> {
        lock(&self.inner.stats).get(peer).map(|s| (s.beats_rx.load(Ordering::SeqCst), s.max_beat_gap.load(Ordering::SeqCst)))
    }
}

impl Drop for DeskLink {
    fn drop(&mut self) {
        self.stop();
    }
}
