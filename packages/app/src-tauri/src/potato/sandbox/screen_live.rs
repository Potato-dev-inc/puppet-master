//! The app's one screen hub, its keeper thread and its Tauri event. Everything else in this folder is testable
//! without an app handle.
//!
//! - `start(app)` once at startup (`lib.rs`): tells the hub to emit `potato://screen` whenever the screen's state
//!   changes (a lease or a card ended by the clock, a button pressed, a Labs switch turned) and starts the 1 s
//!   keeper that lets the clock end leases and cards. It starts no sandbox and opens no window.
//! - `set_enabled` follows the two Labs switches: the Computer works only while `potato_bot_enabled` AND
//!   `potato_sandbox_enabled` are on.
//! - `set_source` is how the real sandbox (or, in a `dev-fake-tui` build, the fake) is given to the hub.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::screen::{self, ScreenSource};

/// The page reads `screen_state` again whenever this fires. No payload.
pub const SCREEN_EVENT: &str = "potato://screen";
pub const SANDBOX_LABS_KEY: &str = "potato_sandbox_enabled";

/// The Computer is on only when both Labs switches are exactly `true`.
pub fn labs_on(public: &Value) -> bool {
    let on = |key: &str| public.get(key).and_then(Value::as_bool).unwrap_or(false);
    on(crate::potato::live::LABS_KEY) && on(SANDBOX_LABS_KEY)
}

/// Called with the public settings whenever they change (`potato::live::apply_settings` passes them on).
pub fn apply_settings(public: &Value) {
    screen::hub().set_enabled(labs_on(public));
}

static VIEWER_DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();

/// The folder with the viewer page and `novnc/` (`embed.html` is in it): the app's resources, else the source tree of
/// a dev build. `None` when neither is there (the route then says so).
pub fn viewer_dir() -> Option<std::path::PathBuf> {
    VIEWER_DIR.get().cloned().flatten().or_else(dev_viewer_dir)
}

fn dev_viewer_dir() -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox").join("viewer");
    dir.join("embed.html").is_file().then_some(dir)
}

/// Gives the hub its sandbox.
pub fn set_source(source: Option<Arc<dyn ScreenSource>>) {
    screen::hub().set_source(source);
}

/// Starts the keeper and the event. Safe to call once; a second call only adds a second keeper, so `lib.rs` calls it once.
pub fn start(app: Option<tauri::AppHandle>) {
    let hub = screen::hub().clone();
    #[cfg(not(test))]
    if let Some(app) = app {
        use tauri::{path::BaseDirectory, Emitter, Manager};
        let shipped = app
            .path()
            .resolve("potato-sandbox/viewer", BaseDirectory::Resource)
            .ok()
            .filter(|dir| dir.join("embed.html").is_file());
        let _ = VIEWER_DIR.set(shipped);
        hub.set_listener(Some(Arc::new(move |_| {
            let _ = app.emit(SCREEN_EVENT, ());
        })));
    }
    #[cfg(test)]
    let _ = app;
    // A build with the fake terminal agents also has the fake sandbox (a picture, no distro).
    #[cfg(feature = "dev-fake-tui")]
    set_source(Some(Arc::new(super::FakeSandbox::new())));
    let _ = std::thread::Builder::new().name("potato-screen".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        hub.tick();
    });
}
