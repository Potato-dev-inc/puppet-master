//! The small meeting point between the screen engine (hotkeys on this computer) and the link hub (other
//! computers). Neither knows the other: the engine asks the port, the hub answers through a handler it plants.
//!
//! - `jump to computer N` and `return home` are requests the hub carries out.
//! - "Pointer locked to this screen" is a flag the hub reads before it lets the pointer cross an edge.
//! - The return-home chord is copied here so the hub can match it on the keys it captures while the pointer is
//!   away (the keyboard hook does not see those keys).
//!
//! Owned by [`super::Desk`], one per desk, so tests never share state.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use desk_core::Chord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// 1-based paired computer number.
    Jump(u8),
    ReturnHome,
}

pub type LinkHandler = Arc<dyn Fn(LinkAction) -> Result<(), String> + Send + Sync>;

#[derive(Default)]
pub struct LinkPort {
    locked: AtomicBool,
    handler: Mutex<Option<LinkHandler>>,
    return_chord: Mutex<Option<Chord>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl LinkPort {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_handler(&self, handler: Option<LinkHandler>) {
        *lock(&self.handler) = handler;
    }

    /// Ask the link to do something. Says why when it cannot.
    pub fn request(&self, action: LinkAction) -> Result<(), String> {
        let handler = lock(&self.handler).clone();
        match handler {
            Some(h) => h(action),
            None => Err("No other computer is connected".into()),
        }
    }

    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    pub fn set_return_chord(&self, chord: Option<Chord>) {
        *lock(&self.return_chord) = chord;
    }

    /// Whether this key press, with these keys already down (HID usages, modifiers 0xE0..=0xE7), is the
    /// return-home chord. The same rule the hook's matcher uses: the modifiers held must be exactly the chord's.
    pub fn is_return_home(&self, down: &BTreeSet<u16>, hid: u16) -> bool {
        let Some(chord) = *lock(&self.return_chord) else { return false };
        let held = |a: u16, b: u16| down.contains(&a) || down.contains(&b);
        desk_core::chord_matches(&chord, held(0xE0, 0xE4), held(0xE2, 0xE6), held(0xE1, 0xE5), held(0xE3, 0xE7), hid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desk_core::parse_chord;

    #[test]
    fn a_request_without_a_link_says_so() {
        let p = LinkPort::new();
        assert!(p.request(LinkAction::ReturnHome).unwrap_err().contains("No other computer"));
    }

    #[test]
    fn a_request_reaches_the_handler() {
        let p = LinkPort::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        p.set_handler(Some(Arc::new(move |a| {
            s.lock().unwrap().push(a);
            Ok(())
        })));
        p.request(LinkAction::Jump(2)).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![LinkAction::Jump(2)]);
    }

    #[test]
    fn the_return_chord_needs_exactly_its_modifiers() {
        let p = LinkPort::new();
        p.set_return_chord(parse_chord("ctrl+alt+shift+h"));
        let h = 0x0B;
        let all: BTreeSet<u16> = [0xE0, 0xE2, 0xE1].into();
        assert!(p.is_return_home(&all, h));
        let right_side: BTreeSet<u16> = [0xE4, 0xE6, 0xE5].into();
        assert!(p.is_return_home(&right_side, h));
        let missing_shift: BTreeSet<u16> = [0xE0, 0xE2].into();
        assert!(!p.is_return_home(&missing_shift, h));
        let extra_gui: BTreeSet<u16> = [0xE0, 0xE2, 0xE1, 0xE3].into();
        assert!(!p.is_return_home(&extra_gui, h));
        assert!(!p.is_return_home(&all, 0x0C));
        p.set_return_chord(None);
        assert!(!p.is_return_home(&all, h));
    }
}
