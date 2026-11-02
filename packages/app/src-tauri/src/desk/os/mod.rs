//! The operating-system side of My computers. Windows and macOS have real hosts; everything else gets a stub that
//! says "not supported yet". Test builds always get the stub, so no test can ever reach the real PC.

use std::sync::Arc;

use super::host::DeskHost;

pub mod stub;
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
pub mod windows;
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
pub mod macos;

/// The host the app uses. In a test build this is the stub on every OS.
pub fn default_host() -> Arc<dyn DeskHost> {
    #[cfg(all(windows, not(test)))]
    {
        Arc::new(windows::WindowsHost::new())
    }
    #[cfg(all(target_os = "macos", not(test)))]
    {
        Arc::new(macos::MacosHost::new())
    }
    #[cfg(not(any(all(windows, not(test)), all(target_os = "macos", not(test)))))]
    {
        Arc::new(stub::StubHost)
    }
}
