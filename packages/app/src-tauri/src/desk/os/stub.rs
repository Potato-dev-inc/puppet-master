//! Any OS without a real host (Linux, and every test build). Every call says so; nothing is faked.

use super::super::engine::hotkeys::Binding;
use super::super::host::{DeskHost, DisplayInfo, HookHealth, HostSink};

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
pub struct StubHost;

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
const NO: &str = "My computers does not work on this system yet";

impl DeskHost for StubHost {
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
    fn install_hooks(&self, _sink: HostSink) -> Result<(), String> {
        Err(NO.into())
    }
    fn remove_hooks(&self) {}
    fn set_hotkeys(&self, _bindings: Vec<Binding>) {}
    fn hook_health(&self) -> HookHealth {
        HookHealth::NotInstalled
    }
    fn release_all(&self) {}
}
