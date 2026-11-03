//! The real macOS host: screens and cursor through Quartz, keys through a Quartz event tap. Never constructed in a
//! test build (see `os::default_host`). Like the Windows host, it changes no OS state beyond the hook it installs.
//!
//! Coordinates are points, the space macOS uses for the cursor and the screens. (Windows reports physical pixels;
//! the engine only compares positions inside one host, so each host stays in its own space.)

mod capture;
mod clipboard;
mod cursor;
mod display;
mod ffi;
mod hook;
mod pointer_watch;
mod remote;

pub use remote::MacosRemote;

use std::sync::{Arc, Mutex, MutexGuard};

use crate::desk::engine::hotkeys::{new_table, Binding, Table};
use crate::desk::host::{DeskHost, DisplayInfo, HookHealth, HostSink};

pub struct MacosHost {
    table: Table,
    hook: Mutex<Option<hook::Hook>>,
}

impl MacosHost {
    /// No side effects: no hook, no cursor change.
    pub fn new() -> Self {
        Self {
            table: new_table(),
            hook: Mutex::new(None),
        }
    }

    fn hook(&self) -> MutexGuard<'_, Option<hook::Hook>> {
        self.hook.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Default for MacosHost {
    fn default() -> Self {
        Self::new()
    }
}

impl DeskHost for MacosHost {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        display::list()
    }

    fn cursor_pos(&self) -> Result<(i32, i32), String> {
        cursor::position()
    }

    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String> {
        cursor::warp(x, y)
    }

    fn install_hooks(&self, sink: HostSink) -> Result<(), String> {
        let mut slot = self.hook();
        if slot.is_some() {
            return Ok(());
        }
        *slot = Some(hook::install(self.table.clone(), sink)?);
        Ok(())
    }

    fn remove_hooks(&self) {
        if let Some(h) = self.hook().take() {
            h.remove();
        }
    }

    fn set_hotkeys(&self, bindings: Vec<Binding>) {
        self.table.store(Arc::new(bindings));
    }

    fn hook_health(&self) -> HookHealth {
        match self.hook().as_ref() {
            Some(h) => h.health(),
            None => HookHealth::NotInstalled,
        }
    }

    fn release_all(&self) {
        // This host never clips or hides the cursor and never injects keys, so the only state to give back is a
        // swallowed key whose release will never come.
        if let Some(h) = self.hook().as_ref() {
            h.reset();
        }
    }
}
