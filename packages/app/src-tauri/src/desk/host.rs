// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! What the engine needs from an operating system. Everything the engine does to the real PC goes through
//! [`DeskHost`], so tests run on [`mock::MockHost`] and never touch the real cursor, hooks or keys.

use std::sync::Arc;

use serde::Serialize;

use super::engine::hotkeys::{Binding, DeskAction};

/// One screen in physical pixels (Per-Monitor DPI Aware v2 space). `x`/`y` may be negative.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// dpi / 96.
    pub scale: f64,
    pub primary: bool,
}

/// What the hook thread tells the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    Hotkey(DeskAction),
}

/// Called on the hook thread, so it must never block. The engine's sink only does `try_send`.
pub type HostSink = Arc<dyn Fn(HostEvent) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookHealth {
    NotInstalled,
    Healthy,
    /// Windows removes a low-level hook without telling anyone when its callback is too slow, and a
    /// hook thread can die. Both read as `Dropped`; the engine's watchdog installs it again.
    Dropped,
}

pub trait DeskHost: Send + Sync {
    /// False on an OS this phase does not support (the stub host).
    fn supported(&self) -> bool {
        true
    }
    fn displays(&self) -> Result<Vec<DisplayInfo>, String>;
    fn cursor_pos(&self) -> Result<(i32, i32), String>;
    /// Reports failure (the original ignored it).
    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String>;
    /// Start the hook thread (its own thread and message loop, never the caller's). `sink` is the new
    /// receiver of every event from this install on (the original kept the first receiver forever).
    fn install_hooks(&self, sink: HostSink) -> Result<(), String>;
    /// Stop the hook thread and wait for it. Safe to call when nothing is installed.
    fn remove_hooks(&self);
    /// The chords the hook matches. Applies to a running hook at once.
    fn set_hotkeys(&self, bindings: Vec<Binding>);
    fn hook_health(&self) -> HookHealth;
    /// Give everything back: no cursor clip, no swallowed key left half-pressed.
    fn release_all(&self);
}

/// The dropped-hook test, as a pure rule. A low-level keyboard hook sees every key press. If a normal
/// key has been held for a while and the hook has not been called for longer than a key-repeat gap,
/// the hook is gone. Modifiers are ignored (a held Ctrl makes no repeats). Mouse input says nothing
/// about a keyboard hook, so it is not used (`GetLastInputInfo` would flap on every mouse move).
pub fn hook_looks_dropped(ms_since_last_callback: u64, normal_key_held: bool) -> bool {
    normal_key_held && ms_since_last_callback > DROPPED_AFTER_MS
}

pub const DROPPED_AFTER_MS: u64 = 2_000;

#[cfg(test)]
pub mod mock;
