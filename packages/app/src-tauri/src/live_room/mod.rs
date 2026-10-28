//! Live Room: share the chat of chosen panes, read-only, through the relay (SPEC-n32, section 3).
//! The app only makes outbound HTTPS calls. Text leaves the PC only through `view` (strict redaction, path cut,
//! caps); tool inputs and outputs, terminal bytes and card content never do.
//! `seal` encrypts each frame with AES-256-GCM. The key lives in the link's #fragment, so the relay sees ciphertext.
//! Nothing here is reachable from the bridge, a phone or an agent: start and preview are Tauri commands only.
//! `config` keeps the relay address and defaults, `client` talks to the relay with the room key,
//! `publisher` ticks, sends frames and stops the room, `service` owns the one running room.
//! This file holds the module list and the limits.

use std::time::Duration;

pub mod active;
pub mod client;
pub mod config;
pub mod publisher;
pub mod seal;
pub mod service;
pub mod source;
pub mod view;

#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod view_tests;

/// Most panes one room shares.
pub const MAX_PANES: usize = 6;
/// How often the publisher reads the panes.
pub const TICK: Duration = Duration::from_secs(1);
/// A full snapshot is sent at least this often.
pub const SNAPSHOT_EVERY: Duration = Duration::from_secs(60);
/// The room stops when nothing changed in any shared pane for this long.
pub const IDLE_STOP: Duration = Duration::from_secs(20 * 60);
/// A gap between two ticks this long means the PC slept: the room stops.
pub const CLOCK_JUMP: Duration = Duration::from_secs(5 * 60);
/// Chat items read off the screen are held back this long after a card leaves the pane.
pub const SCREEN_HOLD_BACK: Duration = Duration::from_secs(2);
/// Room lengths the person can pick, in minutes (the relay's hard maximum is the last one).
pub const MINUTES: [u32; 5] = [30, 60, 120, 240, 480];
/// Viewer limits the person can pick.
pub const VIEWERS: [u32; 4] = [5, 10, 25, 50];
/// The holder name a Live Room takes on `Watchers` (`Hold::lease`).
pub const HOLDER: &str = "live-room";
