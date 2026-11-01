// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Pure DevHop logic: layout geometry, edge crossing, hop modes, and control.
//! No OS calls and no networking, so the rules in the spec can be unit tested.

#![forbid(unsafe_code)]

pub mod control;
pub mod edge;
pub mod hop;
pub mod hotkey;
pub mod layout;
pub mod mapping;

pub use control::{ControlCommand, ControlEvent, ControlMachine, Phase};
pub use edge::{cross, cross_with_mode, Arrival, CrossMode, MAX_GAP_PX};
pub use hop::{
    denormalize, hop as hop_monitors, normalize, place, reading_order, HopAction, HopMode, NormPos,
    PositionMemory,
};
pub use hotkey::{chord_matches, digit_hid, parse_chord, Chord, HotAction};
pub use layout::{Device, DeviceId, Layout, Monitor, MonitorId, PhysRect, Side};
pub use mapping::{normalize_wheel, translate_hid, HostOs, ModifierMap, ScrollNorm};
