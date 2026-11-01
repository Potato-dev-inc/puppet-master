// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Modifier translation and scroll normalization (R9).
//!
//! HID usages travel on the wire. When the machines differ, Windows Alt becomes
//! Mac Command and Windows Win becomes Mac Option (and the inverse). Control
//! passes through unchanged in both directions.

use serde::{Deserialize, Serialize};

pub const HID_A: u16 = 0x04;
pub const HID_C: u16 = 0x06;
pub const HID_LCTRL: u16 = 0xE0;
pub const HID_LSHIFT: u16 = 0xE1;
pub const HID_LALT: u16 = 0xE2;
pub const HID_LGUI: u16 = 0xE3;
pub const HID_RCTRL: u16 = 0xE4;
pub const HID_RSHIFT: u16 = 0xE5;
pub const HID_RALT: u16 = 0xE6;
pub const HID_RGUI: u16 = 0xE7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    Macos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifierMap {
    /// Windows Alt <-> Mac Command. Named `ctrl_cmd` before the mapping was
    /// corrected, so old config.toml files still load through the alias.
    #[serde(alias = "ctrl_cmd")]
    pub alt_cmd: bool,
    /// Windows Win <-> Mac Option.
    pub win_option: bool,
}

impl Default for ModifierMap {
    fn default() -> Self {
        Self {
            alt_cmd: true,
            win_option: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollNorm {
    /// When true, the wheel sign is flipped (macOS "natural" scrolling).
    pub natural: bool,
    pub speed: f64,
}

impl Default for ScrollNorm {
    fn default() -> Self {
        Self {
            natural: false,
            speed: 1.0,
        }
    }
}

pub fn translate_hid(map: ModifierMap, from: HostOs, to: HostOs, hid: u16) -> u16 {
    if from == to {
        return hid;
    }
    match (from, to) {
        (HostOs::Windows, HostOs::Macos) => windows_to_mac(map, hid),
        (HostOs::Macos, HostOs::Windows) => mac_to_windows(map, hid),
        (HostOs::Windows, HostOs::Windows) | (HostOs::Macos, HostOs::Macos) => hid,
    }
}

fn windows_to_mac(map: ModifierMap, hid: u16) -> u16 {
    // On a Windows keyboard, Alt is the key people hold for Mac shortcuts, so
    // it becomes Command. Ctrl stays Ctrl, and the Windows key becomes Option.
    if map.alt_cmd && hid == HID_LALT {
        return HID_LGUI;
    }
    if map.alt_cmd && hid == HID_RALT {
        return HID_RGUI;
    }
    if map.win_option && hid == HID_LGUI {
        return HID_LALT;
    }
    if map.win_option && hid == HID_RGUI {
        return HID_RALT;
    }
    hid
}

fn mac_to_windows(map: ModifierMap, hid: u16) -> u16 {
    // The inverse: Command becomes Alt, Option becomes the Windows key, and
    // Control stays Ctrl.
    if map.alt_cmd && hid == HID_LGUI {
        return HID_LALT;
    }
    if map.alt_cmd && hid == HID_RGUI {
        return HID_RALT;
    }
    if map.win_option && hid == HID_LALT {
        return HID_LGUI;
    }
    if map.win_option && hid == HID_RALT {
        return HID_RGUI;
    }
    hid
}

/// Apply natural-scroll and the speed multiplier. High-resolution wheels are
/// just larger integer deltas; they scale the same way.
pub fn normalize_wheel(cfg: ScrollNorm, dx: i32, dy: i32) -> (i32, i32) {
    let sign = if cfg.natural { -1.0 } else { 1.0 };
    let speed = if cfg.speed.is_finite() && cfg.speed > 0.0 {
        cfg.speed
    } else {
        1.0
    };
    let scale = |v: i32| (v as f64 * sign * speed).round() as i32;
    (scale(dx), scale(dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_ctrl_passes_through_and_alt_becomes_command() {
        let map = ModifierMap {
            alt_cmd: true,
            win_option: true,
        };
        // Control is never remapped, on either side.
        assert_eq!(
            translate_hid(map, HostOs::Windows, HostOs::Macos, HID_LCTRL),
            HID_LCTRL
        );
        assert_eq!(
            translate_hid(map, HostOs::Windows, HostOs::Macos, HID_RCTRL),
            HID_RCTRL
        );
        // Windows Alt -> Mac Command, keeping left and right on their own side.
        assert_eq!(
            translate_hid(map, HostOs::Windows, HostOs::Macos, HID_LALT),
            HID_LGUI
        );
        assert_eq!(
            translate_hid(map, HostOs::Windows, HostOs::Macos, HID_RALT),
            HID_RGUI
        );
        // Windows Win -> Mac Option.
        assert_eq!(
            translate_hid(map, HostOs::Windows, HostOs::Macos, HID_LGUI),
            HID_LALT
        );
        assert_eq!(
            translate_hid(map, HostOs::Windows, HostOs::Macos, HID_RGUI),
            HID_RALT
        );
    }

    #[test]
    fn mac_command_becomes_alt_and_option_becomes_win() {
        let map = ModifierMap {
            alt_cmd: true,
            win_option: true,
        };
        // Command -> Alt.
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_LGUI),
            HID_LALT
        );
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_RGUI),
            HID_RALT
        );
        // Option -> Win.
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_LALT),
            HID_LGUI
        );
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_RALT),
            HID_RGUI
        );
        // Control passes through.
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_LCTRL),
            HID_LCTRL
        );
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_RCTRL),
            HID_RCTRL
        );
        // Non-modifier keys are untouched.
        assert_eq!(
            translate_hid(map, HostOs::Macos, HostOs::Windows, HID_C),
            HID_C
        );
    }

    #[test]
    fn each_toggle_controls_only_its_pair() {
        // Alt/Command off: Alt stays Alt, but Win still swaps with Option.
        let only_win = ModifierMap {
            alt_cmd: false,
            win_option: true,
        };
        assert_eq!(
            translate_hid(only_win, HostOs::Windows, HostOs::Macos, HID_LALT),
            HID_LALT
        );
        assert_eq!(
            translate_hid(only_win, HostOs::Windows, HostOs::Macos, HID_LGUI),
            HID_LALT
        );
        // Win/Option off: Win stays Win, but Alt still becomes Command.
        let only_alt = ModifierMap {
            alt_cmd: true,
            win_option: false,
        };
        assert_eq!(
            translate_hid(only_alt, HostOs::Windows, HostOs::Macos, HID_LALT),
            HID_LGUI
        );
        assert_eq!(
            translate_hid(only_alt, HostOs::Windows, HostOs::Macos, HID_LGUI),
            HID_LGUI
        );
    }

    #[test]
    fn disabled_maps_leave_hid_untouched() {
        let off = ModifierMap {
            alt_cmd: false,
            win_option: false,
        };
        for hid in [
            HID_LCTRL, HID_RCTRL, HID_LALT, HID_RALT, HID_LGUI, HID_RGUI, HID_LSHIFT, HID_RSHIFT,
            HID_A, HID_C,
        ] {
            assert_eq!(translate_hid(off, HostOs::Windows, HostOs::Macos, hid), hid);
            assert_eq!(translate_hid(off, HostOs::Macos, HostOs::Windows, hid), hid);
        }
    }

    #[test]
    fn same_os_leaves_usages_alone() {
        let on = ModifierMap::default();
        assert_eq!(
            translate_hid(on, HostOs::Windows, HostOs::Windows, HID_LALT),
            HID_LALT
        );
        assert_eq!(
            translate_hid(on, HostOs::Macos, HostOs::Macos, HID_LGUI),
            HID_LGUI
        );
        assert_eq!(
            translate_hid(on, HostOs::Windows, HostOs::Windows, HID_LCTRL),
            HID_LCTRL
        );
    }

    #[test]
    fn scroll_speed_and_natural_direction() {
        let cfg = ScrollNorm {
            natural: false,
            speed: 1.5,
        };
        assert_eq!(normalize_wheel(cfg, 0, 120), (0, 180));
        let natural = ScrollNorm {
            natural: true,
            speed: 1.0,
        };
        assert_eq!(normalize_wheel(natural, 8, -4), (-8, 4));
        let bad = ScrollNorm {
            natural: false,
            speed: f64::NAN,
        };
        assert_eq!(normalize_wheel(bad, 3, 4), (3, 4));
    }
}
