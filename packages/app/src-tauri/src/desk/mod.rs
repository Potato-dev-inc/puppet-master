// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! My computers, phase DL-3: the one-computer engine (screen hops on this PC).
//!
//! The first capability of a PC bridge: later phases add other computers and other capabilities on top of a
//! link layer. Nothing here uses a network.
//!
//! Rules this module keeps (tests in `tests.rs` and `guard_tests.rs`):
//! - Off unless Labs `desk_enabled` is on. Every command says `FEATURE_OFF` otherwise.
//! - The keyboard hook is installed only while Labs is on AND the person turned "Screen hops" on AND hops are
//!   not paused. Pause, Labs off and quitting remove it and release everything.
//! - The hook runs on its own thread with its own message loop, never the main thread, and never waits.
//! - Only the desktop window can call it. Nothing in `bridge/`, `mcp_shim/`, `tool_registry/`, the phone routes
//!   or the MCP tools mentions this module, so no agent and no phone can drive the mouse.
//! - Everything the engine does to the PC goes through [`host::DeskHost`]. Tests use `MockHost` only; in a test
//!   build the real host is replaced by a stub, so no test can install a hook or move the real cursor.
//!
//! Layout: `host` (the OS trait), `engine` (thread, commands, snapshot, hops, hotkeys, saved settings),
//! `os` (Windows host and a stub), `commands` (the Tauri commands).

pub mod commands;
pub mod engine;
pub mod host;
pub mod os;

#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

#[cfg(not(test))]
use self::engine::config::FileStore;
use self::engine::config::ConfigStore;
#[cfg(test)]
use self::engine::config::MemStore;
use self::engine::{Cmd, EngineHandle, EngineParts, Notifier, Snapshot};
use self::host::DeskHost;

/// The Labs switch in the public settings.
pub const LABS_KEY: &str = "desk_enabled";

/// The Labs switch in a settings value: on only when it is exactly `true`.
pub fn labs_on(public: &Value) -> bool {
    public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false)
}

pub const FEATURE_OFF: &str = "FEATURE_OFF: My computers is switched off in Labs";

/// How often the watchdog looks at the hook.
const WATCHDOG_TICK: Duration = Duration::from_secs(1);

pub struct Desk {
    labs: Arc<AtomicBool>,
    host: Arc<dyn DeskHost>,
    store: Arc<dyn ConfigStore>,
    notifier: Arc<Mutex<Option<Notifier>>>,
    tick: Duration,
    engine: Mutex<Option<Arc<EngineHandle>>>,
}

impl Desk {
    pub fn new(host: Arc<dyn DeskHost>, store: Arc<dyn ConfigStore>, tick: Duration) -> Self {
        Self {
            labs: Arc::new(AtomicBool::new(false)),
            host,
            store,
            notifier: Arc::new(Mutex::new(None)),
            tick,
            engine: Mutex::new(None),
        }
    }

    pub fn labs(&self) -> bool {
        self.labs.load(Ordering::SeqCst)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn engine_running(&self) -> bool {
        self.engine.lock().unwrap_or_else(|p| p.into_inner()).is_some()
    }

    pub fn set_notifier(&self, notifier: Notifier) {
        *self.notifier.lock().unwrap_or_else(|p| p.into_inner()) = Some(notifier);
    }

    fn ensure_engine(&self) -> Result<Arc<EngineHandle>, String> {
        let mut slot = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = slot.as_ref() {
            return Ok(e.clone());
        }
        let engine = Arc::new(EngineHandle::start(EngineParts {
            host: self.host.clone(),
            store: self.store.clone(),
            labs: self.labs.clone(),
            notifier: self.notifier.clone(),
            tick: self.tick,
        })?);
        *slot = Some(engine.clone());
        Ok(engine)
    }

    /// Called when settings change (and once at start). Turning Labs off ends the engine, which removes the hook
    /// and releases everything: the switch is also the kill switch. `start` starts the engine when Labs is on, so
    /// a "Screen hops" choice saved earlier takes effect at app start.
    pub fn apply_settings(&self, public: &Value, start: bool) {
        let on = labs_on(public);
        self.labs.store(on, Ordering::SeqCst);
        if !on {
            self.shutdown();
        } else if start {
            match self.ensure_engine() {
                Ok(e) => e.reconcile(),
                Err(e) => tracing::warn!(error = %e, "desk: engine did not start"),
            }
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        self.command(Cmd::Refresh)
    }

    pub fn command(&self, cmd: Cmd) -> Result<Snapshot, String> {
        if !self.labs() {
            return Err(FEATURE_OFF.into());
        }
        self.ensure_engine()?.command(cmd)
    }

    /// Remove the hook, release everything, end the engine thread. Safe to call twice.
    pub fn shutdown(&self) {
        let engine = self.engine.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(e) = engine {
            e.shutdown();
        }
    }
}

static GLOBAL: OnceLock<Desk> = OnceLock::new();

/// The app-wide instance. In a test build it has the stub host and a memory store, so nothing can reach the PC.
pub fn global() -> &'static Desk {
    GLOBAL.get_or_init(|| {
        #[cfg(not(test))]
        let store: Arc<dyn ConfigStore> =
            Arc::new(FileStore { path: crate::app_paths::app_data_dir().join("desk").join("config.json") });
        #[cfg(test)]
        let store: Arc<dyn ConfigStore> = Arc::new(MemStore::new(None));
        Desk::new(os::default_host(), store, WATCHDOG_TICK)
    })
}

/// Called with the effective public settings whenever they change.
pub fn apply_settings(public: &Value) {
    global().apply_settings(public, !cfg!(test));
}

/// App quit.
pub fn shutdown() {
    if let Some(d) = GLOBAL.get() {
        d.shutdown();
    }
}

#[cfg(test)]
pub(crate) fn mem_store(text: Option<&str>) -> Arc<dyn ConfigStore> {
    Arc::new(MemStore::new(text))
}
