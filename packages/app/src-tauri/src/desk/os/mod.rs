//! The operating-system side of My computers. Windows has the real host; everything else gets a stub that
//! says "not supported yet". Test builds always get the stub, so no test can ever reach the real PC.

use std::sync::Arc;

use super::host::DeskHost;

pub mod stub;
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
pub mod windows;

/// The host the app uses. In a test build this is the stub even on Windows.
pub fn default_host() -> Arc<dyn DeskHost> {
    #[cfg(all(windows, not(test)))]
    {
        Arc::new(windows::WindowsHost::new())
    }
    #[cfg(not(all(windows, not(test))))]
    {
        Arc::new(stub::StubHost)
    }
}
