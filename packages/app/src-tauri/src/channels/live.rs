//! Text Your Agents: the one real `Runtime` of the app, and the hooks the rest of the app calls
//! (`push` hands over every notification candidate and companion reply; `lib.rs` starts it).
//!
//! Starting never turns Telegram on by itself: the runtime starts a poller only if the saved
//! settings already say the person turned it on.

use super::host_live::LiveHost;
use super::runtime::{Deps, Runtime};
use super::telegram::api::HttpApi;
use super::telegram::poller::Timing;
use crate::push::hub::Candidate;
use crate::pty::PaneRegistry;
use crate::secrets::{self, SecretBackend};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

static RUNTIME: OnceCell<Arc<Runtime>> = OnceCell::new();

/// How often the runtime looks at held notifications and ended turns.
const TICK: Duration = Duration::from_secs(5);

/// The process's credential store, behind a value the runtime can own.
struct LiveSecrets;

impl SecretBackend for LiveSecrets {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        secrets::backend().get(name)
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        secrets::backend().set(name, value)
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        secrets::backend().delete(name)
    }
    fn label(&self) -> &'static str {
        secrets::backend().label()
    }
    fn available(&self) -> bool {
        secrets::backend().available()
    }
}

/// Builds the runtime and the 5 s thread. Call once at start-up, after `push::init`. A second call
/// does nothing.
pub fn start(app: AppHandle, registry: Arc<Mutex<PaneRegistry>>) {
    let deps = Deps {
        host: Arc::new(LiveHost::new(registry, app)),
        secrets: Arc::new(LiveSecrets),
        profile: crate::app_paths::profile(),
        api_for: Arc::new(|token: &str| Arc::new(HttpApi::new(token))),
        timing: Timing::LIVE,
        audit: Arc::new(|kind, who, summary, detail| crate::audit::record(kind, who, summary, detail)),
        turns: Arc::new(|since_ms| crate::turn_summary::tracker().since(since_ms)),
    };
    let runtime = Arc::new(Runtime::new(deps));
    if RUNTIME.set(runtime.clone()).is_err() {
        return;
    }
    // Only if the person turned it on before: a saved "on" with a paired chat.
    runtime.apply();
    let ticker = runtime;
    let spawned = std::thread::Builder::new().name("telegram-tick".into()).spawn(move || loop {
        std::thread::sleep(TICK);
        ticker.tick();
    });
    if let Err(err) = spawned {
        tracing::warn!(%err, "Telegram tick thread not started");
    }
}

/// The app's runtime, once `start` ran.
pub fn current() -> Option<&'static Arc<Runtime>> {
    RUNTIME.get()
}

/// True while Telegram is on and connected: `push` then does not skip a notification just because
/// no phone is subscribed.
pub fn listening() -> bool {
    current().is_some_and(|rt| rt.listening())
}

/// A notification candidate (called by `push` before its own subscriber check).
pub fn offer(candidate: &Candidate) {
    if let Some(rt) = current() {
        rt.offer(candidate);
    }
}

/// A run end held by Focus, offered to the chat only as an answer in a conversation window.
pub fn offer_answer(candidate: &Candidate) {
    if let Some(rt) = current() {
        rt.offer_answer(candidate);
    }
}

/// The end of a Focus or its reminder, counts only (called by `attention::focus`).
pub fn focus_note(line: &str) {
    if let Some(rt) = current() {
        rt.focus_note(line);
    }
}

/// A pane's status changed (called by `push::notify_pane_status`, whether or not anyone listens).
pub fn pane_status(pane_id: &str, status: &str) {
    if let Some(rt) = current() {
        rt.pane_status(pane_id, status);
    }
}

/// A companion's reply (called by `push::notify_companion_reply`).
pub fn companion_reply(companion_id: &str, name: &str, text: &str) {
    if let Some(rt) = current() {
        rt.companion_reply(companion_id, name, text);
    }
}

/// Installs a runtime built by a test, in place of `start` (which needs a running app).
#[cfg(test)]
pub fn install_for_test(runtime: Arc<Runtime>) -> bool {
    RUNTIME.set(runtime).is_ok()
}
