// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The real Windows host. Never constructed in a test build (see `os::default_host`).

mod capture;
mod clipboard;
mod cursor;
mod cursor_lock;
mod display;
mod dpi;
mod hook;
mod remote;

pub use remote::WindowsRemote;

use std::sync::Mutex;

use crate::desk::engine::hotkeys::{new_table, Binding, Table};
use crate::desk::host::{DeskHost, DisplayInfo, HookHealth, HostSink};

pub struct WindowsHost {
    table: Table,
    hook: Mutex<Option<hook::Hook>>,
}

impl WindowsHost {
    /// No side effects: no hook, no process-wide DPI change.
    pub fn new() -> Self {
        Self { table: new_table(), hook: Mutex::new(None) }
    }

    fn hook(&self) -> std::sync::MutexGuard<'_, Option<hook::Hook>> {
        self.hook.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Default for WindowsHost {
    fn default() -> Self {
        Self::new()
    }
}

impl DeskHost for WindowsHost {
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
        self.table.store(std::sync::Arc::new(bindings));
    }

    fn hook_health(&self) -> HookHealth {
        match self.hook().as_ref() {
            Some(h) => h.health(),
            None => HookHealth::NotInstalled,
        }
    }

    fn release_all(&self) {
        // This phase never clips or hides the cursor and never injects keys, so there is no OS state
        // to give back. What can be left over is a swallowed key with a missing release.
        if let Some(h) = self.hook().as_ref() {
            h.reset();
        }
    }
}
