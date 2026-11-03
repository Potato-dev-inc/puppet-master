//! The hub's answers to hotkeys (see `desk::port`): jump to a computer by number, and bring the pointer home.
//! A child of `hub` so it can use the hub's own state; kept apart so `hub.rs` stays about the link itself.

use super::{Chords, Home, Hub, Inner, Side};
use crate::desk::link::arrange::ALL_SIDES;

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
        // Leave through the middle of whichever screen edge faces that computer in the layout.
        let mut dep = None;
        'find: for shape in self.shapes.clone() {
            let r = shape.rect;
            for side in ALL_SIDES {
                let at = match side {
                    Side::Right => (r.right() - 1, r.y + r.height as i32 / 2),
                    Side::Left => (r.x, r.y + r.height as i32 / 2),
                    Side::Bottom => (r.x + r.width as i32 / 2, r.bottom() - 1),
                    Side::Top => (r.x + r.width as i32 / 2, r.y),
                };
                if let Some(d) = self.inner.depart_to(&peer, &self.shapes, at, &[side]) {
                    dep = Some(d);
                    break 'find;
                }
            }
        }
        let Some(dep) = dep else { return self.fail(&format!("{name} is not next to any of this computer's screens in the layout")) };
        if !self.cross_to(peer, &dep, pos, "hotkey") && self.error.is_none() {
            self.fail(&format!("Could not move the mouse to {name} (a button may be held, or its mouse switch is off)"));
        }
    }

    /// Bring the pointer back from the other computer, or take it back from one using this computer. Nothing to do
    /// when it is already here.
    pub(super) fn return_home(&mut self) {
        if self.remote.is_some() || self.controlled.is_some() {
            self.stop_all(Home::Hotkey);
        }
    }

    /// Whether the key just pressed completes the return-home chord (matched on the keys captured while the pointer
    /// is away, which the keyboard hook does not see).
    /// (Takes the parts, not `&self`: the caller holds a borrow of the live links.)
    pub(super) fn return_chord_pressed(inner: &Inner, chords: &Chords, hid: u16) -> bool {
        inner.port().is_some_and(|p| p.is_return_home(&chords.down, hid))
    }

    /// Whether the key just pressed completes the pause chord (the configured `pause` hotkey, copied to the port).
    /// Same reason and same rule as `return_chord_pressed`.
    pub(super) fn pause_chord_pressed(inner: &Inner, chords: &Chords, hid: u16) -> bool {
        inner.port().is_some_and(|p| p.is_pause(&chords.down, hid))
    }
}
