//! The running Potato Bot: the one `Service` of the app, the `Host` that connects it to Puppet Master, and
//! the supervisor thread. Everything else in `potato/` is testable without this file.
//!
//! - `start(app)` once at startup (`lib.rs`): installs the service and starts the `potato-keeper` thread.
//!   No brain starts here: a bot's brain starts lazily on its first message.
//! - `apply_settings(public)` is called whenever the public settings change: the Labs switch
//!   `potato_bot_enabled` is the kill switch (off kills every brain, the brain routes answer 404, helper
//!   cards are withdrawn).
//! - `set_bridge_port(port)` is told by the bridge when it is listening; the brain gets that address.
//! - `stop_all(grace)` is for the panic button and for the app's exit.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::Value;
use tauri::AppHandle;

use super::auth;
use super::env::HostEnv;
use super::service::{BotProfile, Host, Service};
use super::sidecar::RealLauncher;

pub const LABS_KEY: &str = "potato_bot_enabled";
pub const CHANGED_EVENT: &str = "potato://changed";

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
static BRIDGE_PORT: AtomicU16 = AtomicU16::new(0);

pub fn service() -> Option<Arc<Service>> {
    SERVICE.get().cloned()
}

pub fn set_bridge_port(port: u16) {
    BRIDGE_PORT.store(port, Ordering::SeqCst);
}

/// The Labs switch in a settings value: on only when it is exactly `true`.
pub fn labs_on(public: &Value) -> bool {
    public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false)
}

/// Called with the public settings whenever they change (and once at startup, from the saved file).
pub fn apply_settings(public: &Value) {
    let on = labs_on(public);
    match service() {
        Some(service) => service.set_enabled(on),
        None => auth::brains().set_enabled(on),
    }
}

/// Stops every brain (the panic button, the app's exit).
pub fn stop_all(grace: Duration) {
    if let Some(service) = service() {
        service.stop_all(grace);
    }
}

pub fn root() -> PathBuf {
    crate::app_paths::app_data_dir().join("potato")
}

struct LiveHost {
    app: Option<AppHandle>,
}

impl Host for LiveHost {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }

    fn local_day(&self, ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|utc| utc.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "1970-01-01".to_string())
    }

    fn bridge_url(&self) -> Option<String> {
        match BRIDGE_PORT.load(Ordering::SeqCst) {
            0 => None,
            port => Some(format!("http://127.0.0.1:{port}")),
        }
    }

    fn profile(&self, bot: &str) -> Option<BotProfile> {
        let companions = crate::companion::store::load(&crate::app_paths::app_data_dir()).ok()?;
        companions.into_iter().find(|c| c.id == bot).map(|c| BotProfile { name: c.name, folder: Some(c.folder) })
    }

    fn host_env(&self) -> HostEnv {
        HostEnv::from_process(crate::shell_env::path_for_spawn())
    }

    fn secret(&self, name: &str) -> Option<String> {
        crate::secrets::read_secret(name)
    }

    fn resources(&self) -> Option<PathBuf> {
        #[cfg(not(test))]
        {
            use tauri::{path::BaseDirectory, Manager};
            if let Some(dir) = self
                .app
                .as_ref()
                .and_then(|app| app.path().resolve("potato-brain", BaseDirectory::Resource).ok())
                .filter(|dir| dir.is_dir())
            {
                return Some(dir);
            }
        }
        // A dev build has no installed resource folder: the generated one in the source tree.
        let staged = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("potato-brain");
        staged.is_dir().then_some(staged)
    }

    fn changed(&self, bot: &str) {
        // Not in unit tests: emitting through the app handle links the windowing runtime into the lib test exe.
        #[cfg(not(test))]
        if let Some(app) = &self.app {
            use tauri::Emitter;
            let _ = app.emit(CHANGED_EVENT, serde_json::json!({ "botId": bot }));
        }
        #[cfg(test)]
        let _ = bot;
    }
}

/// Installs the service (once) and starts its supervisor thread.
pub fn start(app: Option<AppHandle>) -> Option<Arc<Service>> {
    let service = Service::new(root(), Arc::new(LiveHost { app }), Arc::new(RealLauncher::new()), auth::brains().clone());
    if SERVICE.set(service.clone()).is_err() {
        return None;
    }
    let ticker = service.clone();
    let _ = std::thread::Builder::new().name("potato-keeper".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        ticker.tick();
    });
    Some(service)
}
