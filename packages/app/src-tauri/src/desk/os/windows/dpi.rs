// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Per-Monitor DPI Aware v2 for one call at a time.
//!
//! DevHop called `SetProcessDpiAwarenessContext` on start, which changes the whole process. Inside the
//! Puppet Master window process that can resize every window. This sets the awareness for the calling thread
//! only, for the length of one call, and puts the old value back.

use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

pub struct DpiScope(DPI_AWARENESS_CONTEXT);

impl DpiScope {
    pub fn new() -> Self {
        // SAFETY: plain call; returns the previous context of this thread, null on failure.
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DpiScope {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            // SAFETY: restores the value the call above returned.
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}
