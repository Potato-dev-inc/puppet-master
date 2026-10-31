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

use parking_lot::Mutex;
use serde_json::Value;
use tauri::AppHandle;

use super::auth;
use super::env::HostEnv;
use super::helpers::{CompanionAgent, Folder, Hands, RunState, Started};
use super::protocol::HelperStatus;
use super::service::{BotProfile, Host, Service};
use super::sidecar::RealLauncher;
use crate::agent_runs::AgentRunRequest;
use crate::companion::broad::Places;

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
        // A stand-in brain (e2e builds only) needs no key: the page must not ask for one, and the person's real
        // key in the credential store is neither read nor sent to it.
        if super::sidecar::fake_brain_active() {
            return (name == super::service::SECRET_DEEPSEEK).then(|| "e2e-stand-in-key".to_string());
        }
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

/// The real hands of the helpers: the companion's agent, the person's recent projects, `run_agent`, the
/// operation store and the model maker's address.
struct LiveHands {
    app: AppHandle,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
}

/// Recent projects the engine looks at, most recent first.
const RECENT_LIMIT: usize = 40;

impl Hands for LiveHands {
    fn agent_of(&self, bot: &str) -> Option<CompanionAgent> {
        let companions = crate::companion::store::load(&crate::app_paths::app_data_dir()).ok()?;
        let c = companions.into_iter().find(|c| c.id == bot)?;
        Some(CompanionAgent {
            label: crate::crews::live::label_of(&c.agent_type),
            agent_type: c.agent_type,
            pane_id: crate::companion::service::pane_id_of(&c.id),
            home: Some(c.folder),
        })
    }

    fn recent_folders(&self) -> Vec<Folder> {
        let mut others = crate::operations::indexed_project_paths();
        others.push(crate::standby::default_project(&self.registry));
        others.truncate(RECENT_LIMIT);
        crate::worktrees::folder::recent_light(&crate::app_paths::app_data_dir(), &others, RECENT_LIMIT)
            .into_iter()
            .filter(|p| p.state != "missing")
            .map(|p| Folder { name: p.name, path: p.path })
            .collect()
    }

    fn places(&self) -> Places {
        Places::from_env()
    }

    fn start(&self, request: AgentRunRequest) -> Result<Started, String> {
        let registry = self.registry.clone();
        let app = self.app.clone();
        let view = crate::agent_runs::run_agent(request, registry, app, None).map_err(|e| e.message)?;
        Ok(Started { operation_id: view.operation_id, pane_id: view.pane_id })
    }

    fn poll(&self, project: &str, operation_id: &str) -> RunState {
        use crate::operations::OperationStatus;
        let Ok(op) = crate::operations::get_operation(project, operation_id) else { return RunState::Gone };
        let pane_id = op.pane_id.clone();
        match op.status {
            OperationStatus::Completed => RunState::Done { status: HelperStatus::Finished, summary: op.result.unwrap_or_default(), pane_id },
            OperationStatus::Failed => RunState::Done { status: HelperStatus::Failed, summary: op.error.map(|e| e.message).unwrap_or_default(), pane_id },
            OperationStatus::Cancelled => RunState::Done { status: HelperStatus::Stopped, summary: String::new(), pane_id },
            _ => RunState::Running,
        }
    }

    fn check_key(&self, key: &str) -> Result<(), String> {
        // A stand-in brain (e2e builds only) has no model behind it, so there is nothing to ask.
        if super::sidecar::fake_brain_active() {
            return Ok(());
        }
        ask_model(key)
    }
}

/// One tiny turn: "hi", one word back at most. The key is sent to the model maker's address and nowhere else;
/// no redirect is followed, and the answer's body is never shown or logged.
fn ask_model(key: &str) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .redirects(0)
        .build();
    let body = serde_json::json!({ "model": "deepseek-chat", "messages": [{ "role": "user", "content": "hi" }], "max_tokens": 1 });
    match agent.post("https://api.deepseek.com/chat/completions").set("Authorization", &format!("Bearer {key}")).send_json(body) {
        Ok(resp) if resp.status() == 200 => Ok(()),
        Ok(_) => Err("DeepSeek gave an answer that was not expected. Try again.".to_string()),
        Err(ureq::Error::Status(401, _)) => Err("DeepSeek did not accept that key.".to_string()),
        Err(ureq::Error::Status(402, _)) => Err("The key works, but the DeepSeek account has no balance.".to_string()),
        Err(ureq::Error::Status(429, _)) => Err("DeepSeek is busy right now. Try again in a minute.".to_string()),
        Err(ureq::Error::Status(_, _)) => Err("DeepSeek could not answer right now. Try again in a minute.".to_string()),
        Err(ureq::Error::Transport(_)) => Err("Could not reach DeepSeek. Check your internet.".to_string()),
    }
}

/// Installs the service (once) and starts its supervisor thread.
pub fn start(app: Option<AppHandle>, registry: Option<Arc<Mutex<crate::pty::PaneRegistry>>>) -> Option<Arc<Service>> {
    let hands = app.clone().zip(registry).map(|(app, registry)| Arc::new(LiveHands { app, registry }));
    let service = Service::new(root(), Arc::new(LiveHost { app }), Arc::new(RealLauncher::new()), auth::brains().clone());
    if SERVICE.set(service.clone()).is_err() {
        return None;
    }
    if let Some(hands) = hands {
        service.set_hands(hands);
    }
    let ticker = service.clone();
    let _ = std::thread::Builder::new().name("potato-keeper".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        ticker.tick();
    });
    Some(service)
}
