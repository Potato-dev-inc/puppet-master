//! The hub's answers to hotkeys (see `desk::port`): jump to a computer by number, and bring the pointer home.
//! A child of `hub` so it can use the hub's own state; kept apart so `hub.rs` stays about the link itself.

use super::{Chords, Hub, Inner, Side};
use crate::desk::link::geometry::side_name;

impl Hub {
    /// Cross to the Nth paired computer (the order the page lists them) without moving the mouse: the pointer
    /// enters at the middle of the edge facing this computer. Says why on the page when it cannot.
    pub(super) fn jump_to(&mut self, n: u8) {
        self.error = None;
        if self.paused {
            return self.fail("Mouse sharing is paused");
        }
        if self.inner.pointer_locked() {
            return self.fail("The pointer is locked to this screen. Free it first");
        }
        if self.remote.is_some() || self.controlled.is_some() {
            return self.fail("The mouse is already on another computer");
        }
        let devices = self.inner.trust.list();
        let Some(device) = usize::from(n).checked_sub(1).and_then(|i| devices.get(i)) else {
            return self.fail(&format!("There is no computer {n}: {} paired", devices.len()));
        };
        let (peer, name) = (device.node, device.name.clone());
        if !self.live.contains_key(&peer) {
            return self.fail(&format!("{name} is not connected"));
        }
        let Ok(pos) = self.inner.host.cursor_pos() else { return self.fail("Could not find the pointer") };
        let side: Side = self.inner.side_of(&peer);
        if !self.cross_to(peer, side, 0.5, pos) && self.error.is_none() {
            self.fail(&format!("Could not move the mouse to {name} (a button may be held, or its mouse switch is off)"));
        }
        self.edge_candidate = None;
        let _ = side_name; // the entry edge is named on the wire by `cross_to`
    }

    /// Bring the pointer back from the other computer, or take it back from one using this computer. Nothing to do
    /// when it is already here.
    pub(super) fn return_home(&mut self) {
        if self.remote.is_some() || self.controlled.is_some() {
            self.stop_all();
        }
    }

    /// Whether the key just pressed completes the return-home chord (matched on the keys captured while the pointer
    /// is away, which the keyboard hook does not see).
    /// (Takes the parts, not `&self`: the caller holds a borrow of the live links.)
    pub(super) fn return_chord_pressed(inner: &Inner, chords: &Chords, hid: u16) -> bool {
        inner.port().is_some_and(|p| p.is_return_home(&chords.down, hid))
    }
}
