//! macOS only: whether this app has been given Accessibility. Not built on Windows, so it is checked on a Mac only.
//!
//! `AXIsProcessTrusted` answers without showing the system prompt and without side effects. It is the same call the
//! macOS host (`desk/os/macos`) makes before it installs a hook or sends a key; this file is only for the page.

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// `Boolean AXIsProcessTrusted(void)`: 1 when the person turned Accessibility on for this app.
    fn AXIsProcessTrusted() -> bool;
}

pub fn accessibility_trusted() -> bool {
    // SAFETY: a plain call with no arguments and no pointers.
    unsafe { AXIsProcessTrusted() }
}
