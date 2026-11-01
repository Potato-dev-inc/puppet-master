// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Hotkey chords. The hook thread matches these without taking a lock.

use serde::{Deserialize, Serialize};

/// USB HID usage for the non-modifier key. `0` means the chord is incomplete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub gui: bool,
    pub hid: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HotAction {
    NextMonitor,
    PrevMonitor,
    /// 1-based monitor index.
    Monitor(u8),
    Center,
    FindCursor,
    LockMonitor,
    /// 1-based peer slot.
    JumpDevice(u8),
    ReturnHome,
    Panic,
}

/// Parse `ctrl+alt+right` or `scrolllock`. Modifier names are `ctrl`, `alt`,
/// `shift`, `win`, `cmd`, and `super`.
pub fn parse_chord(text: &str) -> Option<Chord> {
    let mut chord = Chord {
        ctrl: false,
        alt: false,
        shift: false,
        gui: false,
        hid: 0,
    };
    let mut saw_key = false;
    for raw in text.split('+') {
        let token = raw.trim().to_ascii_lowercase();
        if token.is_empty() {
            return None;
        }
        match token.as_str() {
            "ctrl" | "control" => chord.ctrl = true,
            "alt" | "opt" | "option" => chord.alt = true,
            "shift" => chord.shift = true,
            "win" | "cmd" | "command" | "super" | "meta" => chord.gui = true,
            other => {
                if saw_key {
                    return None;
                }
                chord.hid = key_hid(other)?;
                saw_key = true;
            }
        }
    }
    if saw_key {
        Some(chord)
    } else {
        None
    }
}

pub fn chord_matches(
    chord: &Chord,
    ctrl: bool,
    alt: bool,
    shift: bool,
    gui: bool,
    hid: u16,
) -> bool {
    chord.hid == hid
        && chord.ctrl == ctrl
        && chord.alt == alt
        && chord.shift == shift
        && chord.gui == gui
}

fn key_hid(name: &str) -> Option<u16> {
    match name {
        "a" => Some(0x04),
        "b" => Some(0x05),
        "c" => Some(0x06),
        "d" => Some(0x07),
        "e" => Some(0x08),
        "f" => Some(0x09),
        "g" => Some(0x0A),
        "h" => Some(0x0B),
        "i" => Some(0x0C),
        "j" => Some(0x0D),
        "k" => Some(0x0E),
        "l" => Some(0x0F),
        "m" => Some(0x10),
        "n" => Some(0x11),
        "o" => Some(0x12),
        "p" => Some(0x13),
        "q" => Some(0x14),
        "r" => Some(0x15),
        "s" => Some(0x16),
        "t" => Some(0x17),
        "u" => Some(0x18),
        "v" => Some(0x19),
        "w" => Some(0x1A),
        "x" => Some(0x1B),
        "y" => Some(0x1C),
        "z" => Some(0x1D),
        "1" => Some(0x1E),
        "2" => Some(0x1F),
        "3" => Some(0x20),
        "4" => Some(0x21),
        "5" => Some(0x22),
        "6" => Some(0x23),
        "7" => Some(0x24),
        "8" => Some(0x25),
        "9" => Some(0x26),
        "0" => Some(0x27),
        "esc" | "escape" => Some(0x29),
        "space" => Some(0x2C),
        "f1" => Some(0x3A),
        "f2" => Some(0x3B),
        "f3" => Some(0x3C),
        "f4" => Some(0x3D),
        "f5" => Some(0x3E),
        "f6" => Some(0x3F),
        "f7" => Some(0x40),
        "f8" => Some(0x41),
        "f9" => Some(0x42),
        "f10" => Some(0x43),
        "f11" => Some(0x44),
        "f12" => Some(0x45),
        "scrolllock" | "scroll" => Some(0x47),
        "right" => Some(0x4F),
        "left" => Some(0x50),
        "down" => Some(0x51),
        "up" => Some(0x52),
        _ => None,
    }
}

/// Digit HID for `1`..=`9`.
pub fn digit_hid(n: u8) -> Option<u16> {
    if (1..=9).contains(&n) {
        Some(0x1E + u16::from(n - 1))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_chords() {
        let next = parse_chord("ctrl+alt+right").unwrap();
        assert!(next.ctrl && next.alt && !next.shift && !next.gui);
        assert_eq!(next.hid, 0x4F);
        let panic = parse_chord("Ctrl+Alt+Esc").unwrap();
        assert_eq!(panic.hid, 0x29);
        let lock = parse_chord("scrolllock").unwrap();
        assert_eq!(lock.hid, 0x47);
        assert!(!lock.ctrl);
        assert!(parse_chord("ctrl+alt").is_none());
        assert!(parse_chord("ctrl+a+b").is_none());
    }

    #[test]
    fn match_requires_the_same_modifiers() {
        let chord = parse_chord("ctrl+alt+shift+1").unwrap();
        assert!(chord_matches(
            &chord,
            true,
            true,
            true,
            false,
            digit_hid(1).unwrap()
        ));
        assert!(!chord_matches(
            &chord,
            true,
            true,
            false,
            false,
            digit_hid(1).unwrap()
        ));
    }
}
