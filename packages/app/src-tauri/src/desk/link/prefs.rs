// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT: `modifiers` and `scroll` in its config.
//! What the person chose for each paired computer about keys and scrolling, saved beside the layout.
//!
//! DevHop had one global setting for each. Here it is per computer, because a Mac laptop and a Linux box on the
//! same desk want different answers.
//!
//! - **Keys** (`alt_cmd`, `win_option`): when that computer drives this one and it is a different kind of computer
//!   (Windows and Mac), Alt becomes Command and the Windows key becomes Option, or not. Both on by default.
//! - **Scrolling when I use it** (`natural`, `speed`): the wheel is flipped and scaled on its way out.
//! - **Scrolling when it uses me** (`reverse_incoming`): the wheel is flipped on its way in.
//! - **Pointer speed when I use it** (`pointer_speed`): how far its pointer moves per step of my mouse (see
//!   `pointer_speed.rs`). Windows pixels and Mac points differ, so the default is 1.0 and the person sets it.
//!
//! Plain JSON in the app data folder (`desk/peers.json`), like the layout. Only choices that differ from the
//! defaults are written, so a computer you never touched leaves no trace.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use desk_core::{ModifierMap, ScrollNorm};
use serde::{Deserialize, Serialize};

use super::pointer_speed::{self, POINTER_SPEED_MAX, POINTER_SPEED_MIN};
use super::{lock, DeskLink, Inner};
use crate::p2p::endpoint::NodeId;

pub const SPEED_MIN: f64 = 0.25;
pub const SPEED_MAX: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PeerPrefs {
    /// Windows Alt <-> Mac Command.
    pub alt_cmd: bool,
    /// Windows key <-> Mac Option.
    pub win_option: bool,
    /// Flip the wheel when I use that computer.
    pub natural: bool,
    /// How much faster (or slower) the wheel is when I use that computer. 1.0 = as is.
    pub speed: f64,
    /// Flip the wheel when that computer uses this one.
    pub reverse_incoming: bool,
    /// How far that computer's pointer moves for each step of my mouse, from 0.25 to 3. 1.0 = as is.
    pub pointer_speed: f64,
}

impl Default for PeerPrefs {
    fn default() -> Self {
        Self { alt_cmd: true, win_option: true, natural: false, speed: 1.0, reverse_incoming: false, pointer_speed: 1.0 }
    }
}

impl PeerPrefs {
    pub fn modifier_map(&self) -> ModifierMap {
        ModifierMap { alt_cmd: self.alt_cmd, win_option: self.win_option }
    }

    /// The wheel on its way out to that computer.
    pub fn scroll_out(&self) -> ScrollNorm {
        ScrollNorm { natural: self.natural, speed: self.speed }
    }

    /// The wheel on its way in from that computer.
    pub fn scroll_in(&self) -> ScrollNorm {
        ScrollNorm { natural: self.reverse_incoming, speed: 1.0 }
    }

    /// Why these cannot be used, if they cannot.
    pub fn check(&self) -> Result<(), String> {
        if !self.speed.is_finite() || self.speed < SPEED_MIN || self.speed > SPEED_MAX {
            return Err(format!("Scroll speed must be between {SPEED_MIN} and {SPEED_MAX}."));
        }
        if !pointer_speed::is_valid(self.pointer_speed) {
            return Err(format!("Pointer speed must be between {POINTER_SPEED_MIN} and {POINTER_SPEED_MAX}."));
        }
        Ok(())
    }
}

/// Where the choices are kept. Keys are node ids in hex.
pub trait PrefsStore: Send + Sync {
    fn load(&self) -> BTreeMap<String, PeerPrefs>;
    fn save(&self, prefs: &BTreeMap<String, PeerPrefs>) -> Result<(), String>;
}

#[derive(Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct MemPrefsStore(Mutex<BTreeMap<String, PeerPrefs>>);

impl PrefsStore for MemPrefsStore {
    fn load(&self) -> BTreeMap<String, PeerPrefs> {
        lock(&self.0).clone()
    }
    fn save(&self, prefs: &BTreeMap<String, PeerPrefs>) -> Result<(), String> {
        *lock(&self.0) = prefs.clone();
        Ok(())
    }
}

pub struct FilePrefsStore {
    pub path: PathBuf,
}

impl PrefsStore for FilePrefsStore {
    fn load(&self) -> BTreeMap<String, PeerPrefs> {
        let Ok(text) = std::fs::read_to_string(&self.path) else { return BTreeMap::new() };
        let mut map: BTreeMap<String, PeerPrefs> = serde_json::from_str(&text).unwrap_or_default();
        // A hand-edited file with a speed that cannot be used gets the normal speed, not a frozen wheel or pointer.
        for p in map.values_mut() {
            if p.speed.is_nan() || !(SPEED_MIN..=SPEED_MAX).contains(&p.speed) {
                p.speed = 1.0;
            }
            if !pointer_speed::is_valid(p.pointer_speed) {
                p.pointer_speed = 1.0;
            }
        }
        map
    }
    fn save(&self, prefs: &BTreeMap<String, PeerPrefs>) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

/// The choices in memory plus the store behind them.
pub(super) struct Prefs {
    store: Arc<dyn PrefsStore>,
    map: BTreeMap<String, PeerPrefs>,
}

impl Prefs {
    pub(super) fn new(store: Arc<dyn PrefsStore>) -> Self {
        let map = store.load();
        Self { store, map }
    }

    fn get(&self, node: &str) -> PeerPrefs {
        self.map.get(node).copied().unwrap_or_default()
    }
}

impl Inner {
    /// What the person chose for `peer`; the defaults for a computer never touched.
    pub(super) fn peer_prefs(&self, peer: &NodeId) -> PeerPrefs {
        lock(&self.prefs).get(&peer.to_hex())
    }
}

impl DeskLink {
    /// Keep the per-computer choices in `store` from now on (loads what it holds).
    pub fn use_prefs_store(&self, store: Arc<dyn PrefsStore>) {
        *lock(&self.inner.prefs) = Prefs::new(store);
        self.inner.notify();
    }

    /// Change some of a computer's choices. Refused with a reason when the result is not usable or cannot be saved;
    /// then nothing changed.
    pub fn update_peer_prefs(&self, node: &NodeId, change: impl FnOnce(&mut PeerPrefs)) -> Result<(), String> {
        let hex = node.to_hex();
        {
            let mut prefs = lock(&self.inner.prefs);
            let mut next = prefs.get(&hex);
            change(&mut next);
            next.check()?;
            let mut map = prefs.map.clone();
            if next == PeerPrefs::default() {
                map.remove(&hex);
            } else {
                map.insert(hex, next);
            }
            prefs.store.save(&map).map_err(|e| format!("Could not save the choice: {e}"))?;
            prefs.map = map;
        }
        self.inner.notify();
        Ok(())
    }
}
