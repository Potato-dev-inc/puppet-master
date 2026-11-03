// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-platform `Host`: inject, capture, clipboard.)
//! What the desk link needs from an operating system, apart from the screens and hotkeys `DeskHost` already gives.
//! Everything the link does to a real PC goes through [`RemoteHost`]: tests use `MockRemote` and nothing else, so no
//! test can move the real cursor, press a real key, install a hook or touch the real clipboard.

use std::sync::Arc;

use super::super::host::DisplayInfo;

/// One thing to do to this computer on behalf of the computer that holds the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inject {
    Key { hid: u16, down: bool },
    Button { button: u8, down: bool },
    /// Relative pointer move in pixels.
    Move { dx: i32, dy: i32 },
    Wheel { dx: i32, dy: i32 },
}

/// One thing the person did while the pointer is on another computer (captured, not delivered here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    Motion { dx: i32, dy: i32 },
    Button { button: u8, down: bool },
    Wheel { dx: i32, dy: i32 },
    Key { hid: u16, down: bool },
}

/// Called on the capture thread, so it must never block.
pub type CaptureSink = Arc<dyn Fn(Capture) + Send + Sync>;

/// One local pointer move on this computer, as a watch reports it: where the pointer is now, and how far it moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerMove {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
}

/// Called for every local pointer move while watched. Called on the watch's own thread, so it must never block.
pub type PointerSink = Arc<dyn Fn(PointerMove) + Send + Sync>;

/// What travels with a copy. Text and one image; files come later (DL-10).
#[derive(Clone, PartialEq, Eq)]
pub enum ClipItem {
    Text(String),
    /// PNG bytes.
    Png(Vec<u8>),
}

impl ClipItem {
    pub fn len(&self) -> usize {
        match self {
            ClipItem::Text(t) => t.len(),
            ClipItem::Png(b) => b.len(),
        }
    }
}

/// A clipboard item never prints its contents: only its kind and size can reach a log, a panic message or a test
/// failure. (`redact` tests in `tests.rs`.)
impl std::fmt::Debug for ClipItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipItem::Text(t) => write!(f, "ClipItem::Text({} bytes)", t.len()),
            ClipItem::Png(b) => write!(f, "ClipItem::Png({} bytes)", b.len()),
        }
    }
}

pub trait RemoteHost: Send + Sync {
    /// False on a system without a real implementation (nothing is faked).
    fn supported(&self) -> bool {
        true
    }
    fn displays(&self) -> Result<Vec<DisplayInfo>, String>;
    fn cursor_pos(&self) -> Result<(i32, i32), String>;
    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String>;
    /// True while a mouse button is held down. A drag is not carried over an edge in this version.
    fn buttons_held(&self) -> bool {
        false
    }

    /// Whether another computer could drive this one right now. Asked before the pointer is let in, so a computer
    /// that cannot send input (macOS without Accessibility) says no in plain words at the edge instead of taking the
    /// mouse and then ignoring every move.
    fn can_be_controlled(&self) -> Result<(), String> {
        Ok(())
    }

    /// Do one thing to this computer. Remembers what it pressed so `release_injected` can undo it.
    fn inject(&self, event: Inject) -> Result<(), String>;
    /// Key-up and button-up for everything `inject` pressed and has not released.
    fn release_injected(&self);

    /// The pointer leaves for another computer: hide it, hold it still, and report everything the person does through
    /// `sink` instead of delivering it here. `exit` is where the pointer left, in this computer's own pixel space; a
    /// system that parks the pointer near that point uses it (see `park.rs`). Safe to call when already capturing.
    fn start_capture(&self, sink: CaptureSink, exit: (i32, i32)) -> Result<(), String>;
    /// Give the pointer and keyboard back. Does not move the pointer. Safe to call when not capturing.
    fn stop_capture(&self);

    /// Report every local pointer move to `sink` until `unwatch_pointer`: the hub decides crossings from those moves.
    /// A system that cannot watch says so with an error, and the hub looks at the pointer on a 10 ms poll instead.
    fn watch_pointer(&self, _sink: PointerSink) -> Result<(), String> {
        Err("unsupported".into())
    }
    /// Stop reporting local moves. Safe to call when not watching.
    fn unwatch_pointer(&self) {}

    fn clipboard_read(&self) -> Result<Vec<ClipItem>, String>;
    fn clipboard_write(&self, items: &[ClipItem]) -> Result<(), String>;
}

/// Any system without a real implementation yet. Every call says so.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
pub struct NoRemote;

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
const NO: &str = "My computers does not work on this system yet";

impl RemoteHost for NoRemote {
    fn supported(&self) -> bool {
        false
    }
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Err(NO.into())
    }
    fn cursor_pos(&self) -> Result<(i32, i32), String> {
        Err(NO.into())
    }
    fn move_cursor(&self, _x: i32, _y: i32) -> Result<(), String> {
        Err(NO.into())
    }
    fn can_be_controlled(&self) -> Result<(), String> {
        Err(NO.into())
    }
    fn inject(&self, _event: Inject) -> Result<(), String> {
        Err(NO.into())
    }
    fn release_injected(&self) {}
    fn start_capture(&self, _sink: CaptureSink, _exit: (i32, i32)) -> Result<(), String> {
        Err(NO.into())
    }
    fn stop_capture(&self) {}
    fn clipboard_read(&self) -> Result<Vec<ClipItem>, String> {
        Err(NO.into())
    }
    fn clipboard_write(&self, _items: &[ClipItem]) -> Result<(), String> {
        Err(NO.into())
    }
}

/// The host the app uses. In a test build this is `NoRemote` even on Windows.
pub fn default_remote() -> Arc<dyn RemoteHost> {
    #[cfg(all(windows, not(test)))]
    {
        Arc::new(super::super::os::windows::WindowsRemote::new())
    }
    #[cfg(all(target_os = "macos", not(test)))]
    {
        Arc::new(super::super::os::macos::MacosRemote::new())
    }
    #[cfg(not(any(all(windows, not(test)), all(target_os = "macos", not(test)))))]
    {
        Arc::new(NoRemote)
    }
}
