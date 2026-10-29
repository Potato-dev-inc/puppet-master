//! Warm agent pool ("standby workers"): a worker for a project the user keeps running tasks in is
//! started ahead of time, so the first turn of the next run skips pane spawn and CLI startup.
//!
//! Opt-in (off by default), per backend and per kind:
//! - **visible** (Cursor Agent, Claude): a TUI pane spawned hidden (`pty::hidden`), waiting at an
//!   empty input box. A run claims it, the pane is revealed like any run-created pane, and the task
//!   goes through the normal verified delivery path.
//! - **headless** (Claude only): `claude -p --input-format stream-json` started with its startup
//!   paid, waiting for one user message on stdin. Cursor headless takes its prompt as an argument
//!   and cannot be pre-launched (`docs/perf/latency-baseline.md`, "Warm pool").
//!
//! Standbys belong to the bridge, not to any MCP session: nothing registers them with
//! `mcp_sessions` until a run claims one. The pool never answers a trust or approval dialog: a
//! standby that shows one is discarded and its key backs off, so an untrusted folder is simply not
//! pooled (visible runs cannot pre-trust a folder either: `trust_workspace` is headless-only).
//!
//! Layout: `pool.rs` the state machine (pure, tested with a fake host), `host.rs` the live host
//! (hidden panes, waiting processes), `config.rs` settings + history on disk, `key.rs` what a
//! standby is keyed by, `tests.rs` the pool rules. Agent-specific screen/launch knowledge lives in
//! `tui/<agent>/` behind `TuiProfile::{standby_support, standby_screen, headless_standby_*}`.

pub mod config;
mod host;
pub mod key;
pub mod pool;
mod stillness;
#[cfg(test)]
mod tests;

pub use config::StandbyConfig;
pub use host::StandbyProcess;
pub use key::PoolKey;
pub use pool::{ClaimOutcome, Handle, HealthSummary, StatusView};

use host::LiveHost;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use pool::Pool;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

use crate::pty::PaneRegistry;

struct Global {
    pool: Arc<Pool>,
    host: Arc<LiveHost>,
}

static GLOBAL: OnceCell<Global> = OnceCell::new();

/// Emitted (payload: provider ids) when the set of providers with a claimable warm worker changes;
/// the tray menu listens to mark them "(ready)".
pub const READY_EVENT: &str = "standby://ready-changed";

/// The pool does not start spawning until the app has had this long to come up.
const STARTUP_GRACE_MS: i64 = 8_000;
/// Maintenance cadence when nothing wakes the loop sooner.
const TICK: Duration = Duration::from_secs(1);

fn now_ms() -> i64 {
    crate::event_log::now_ms()
}

/// Start the pool (once). Loads the saved config; with the pool off this only keeps an idle
/// thread that does nothing.
pub fn init(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) {
    init_with_path(registry, app, config::default_path());
}

fn init_with_path(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle, path: std::path::PathBuf) {
    let _ = CONFIG_PATH.set(path.clone());
    let host = Arc::new(LiveHost::new(registry, app.clone()));
    let (config, recent) = config::load(&path);
    let pool = Arc::new(Pool::new(
        host.clone(),
        Some(path),
        config,
        recent,
        now_ms() + STARTUP_GRACE_MS,
    ));
    if GLOBAL
        .set(Global {
            pool: pool.clone(),
            host,
        })
        .is_err()
    {
        return;
    }
    let path = config_path_for_loop();
    let app_for_loop = app;
    let _ = std::thread::Builder::new()
        .name("standby-pool".into())
        .spawn(move || {
            let mut seen_mtime = mtime(&path);
            let mut seen_ready: Vec<String> = Vec::new();
            loop {
                pool.wake_handle().wait(TICK);
                if pool.is_shut_down() {
                    break;
                }
                // The settings file is the source of truth: a hand edit (or another tool) is
                // picked up within a second. The app's own writes load back as an equal config.
                let current = mtime(&path);
                if current != seen_mtime {
                    seen_mtime = current;
                    let (from_file, _) = config::load(&path);
                    if from_file != pool.config() {
                        tracing::info!("standby pool config changed on disk; applying");
                        pool.set_config(from_file);
                    }
                }
                pool.reconcile(now_ms());
                let ready = pool.ready_agents();
                if ready != seen_ready {
                    seen_ready = ready;
                    let _ = tauri::Emitter::emit(&app_for_loop, READY_EVENT, &seen_ready);
                }
            }
        });
}

fn config_path_for_loop() -> std::path::PathBuf {
    CONFIG_PATH.get().cloned().unwrap_or_else(config::default_path)
}

fn mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

static CONFIG_PATH: OnceCell<std::path::PathBuf> = OnceCell::new();

fn pool() -> Option<&'static Arc<Pool>> {
    GLOBAL.get().map(|global| &global.pool)
}

/// An agent's definition changed or was removed: drop its warm workers (T33).
pub fn discard_agent(agent_type: &str) -> usize {
    pool().map_or(0, |pool| pool.discard_agent(agent_type, "definition_changed"))
}

/// The agents in strict mode (`crate::canary`) go cold: no warm worker for them until they leave it.
pub fn set_strict_agents(agents: &[String]) {
    if let Some(pool) = pool() {
        pool.set_strict_agents(agents);
    }
}

/// Save new settings and apply them at once (standbys that are no longer allowed are shut down).
pub fn set_config(config: StandbyConfig) -> Result<StandbyConfig, String> {
    let pool = pool().ok_or("the standby pool is not running")?;
    Ok(pool.set_config(config))
}

/// Read-only pool state for the bridge `GET /standby` route and the settings tab.
pub fn status() -> StatusView {
    match pool() {
        Some(pool) => pool.status(now_ms()),
        None => Pool::new(
            Arc::new(NoHost),
            None,
            StandbyConfig::default(),
            Vec::new(),
            0,
        )
        .status(now_ms()),
    }
}

/// Counts only, for `/health`. `None` before the pool is initialised.
pub fn health_summary() -> Option<HealthSummary> {
    pool().map(|pool| pool.health_summary())
}

/// `settings.project_path` from the app's `settings.json` (`{"settings": {...}}`).
pub fn project_from_settings(file: &serde_json::Value) -> Option<String> {
    let path = file.get("settings")?.get("project_path")?.as_str()?.trim();
    // A separate copy's folder is never the project folder: it reads as the project it came from.
    (!path.is_empty()).then(|| crate::worktrees::folder::real_folder(path))
}

/// The folder Preload warms in and the tray opens panes in: the "project folder" setting when it
/// names an existing directory, else the registry's current project. The registry's own project
/// follows whatever workspace a run last selected, which would make warm workers chase it.
pub fn default_project(registry: &Mutex<PaneRegistry>) -> String {
    let from_settings = std::fs::read_to_string(crate::app_paths::app_data_dir().join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| project_from_settings(&json))
        .map(|path| crate::project_path::expand_user_path(std::path::Path::new(&path)))
        .filter(|path| path.is_dir());
    match from_settings {
        Some(path) => path.to_string_lossy().into_owned(),
        None => crate::pty::registry_get_project_path(registry),
    }
}

/// Providers with a claimable warm visible worker right now.
pub fn ready_agents() -> Vec<String> {
    pool().map(|pool| pool.ready_agents()).unwrap_or_default()
}

/// The pool key Preload warms for `agent_type`: default project, no model, default options.
#[cfg(test)]
mod project_tests {
    use super::*;
    #[test]
    fn the_default_project_is_the_project_folder_setting() {
        let file = serde_json::json!({"settings": {"project_path": " C:/work/app "}});
        assert_eq!(project_from_settings(&file).as_deref(), Some("C:/work/app"));
        assert_eq!(project_from_settings(&serde_json::json!({"settings": {"project_path": ""}})), None);
        assert_eq!(project_from_settings(&serde_json::json!({"settings": {}})), None);
        assert_eq!(project_from_settings(&serde_json::json!({})), None);
    }

    #[test]
    fn a_saved_separate_copy_folder_reads_as_its_project() {
        let file = serde_json::json!({"settings": {"project_path": "C:/work/app/.puppet-master/worktrees/pane-1a2b"}});
        assert_eq!(project_from_settings(&file).as_deref(), Some("C:/work/app"));
    }
}

pub fn preload_key(agent_type: &str, default_project: &str) -> Option<PoolKey> {
    key_for_run(agent_type, default_project, None, false, false, false, None)
}

/// A run of `key` started: remember it so the next one can find a warm worker.
pub fn note_run(key: &PoolKey, cwd: &str) {
    if let Some(pool) = pool() {
        pool.note_run(key, cwd, now_ms());
    }
}

/// Kill every standby (app exit) and stop the loop.
pub fn shutdown() {
    if let Some(pool) = pool() {
        pool.shutdown();
    }
}

/// Take a warm visible pane for a run. The pane is still hidden and unowned: the caller reserves
/// it for the operation, registers the owner, then `commit`s (which reveals it). Dropping the
/// claim without `commit` (cancelled run, failed hand-over) shuts the pane down.
pub struct VisibleClaim {
    claim: pool::Claim,
    pane_id: String,
}

impl VisibleClaim {
    pub fn pane_id(&self) -> &str {
        &self.pane_id
    }
    /// How long the standby had been warm before this run claimed it.
    pub fn age_ms(&self) -> i64 {
        self.claim.age_ms()
    }
    /// The run owns the pane: show it in pane lists. Returns the pane id.
    pub fn commit(self) -> String {
        let pane_id = self.pane_id.clone();
        self.claim.commit();
        crate::pty::hidden::reveal(&pane_id);
        pane_id
    }
}

pub fn claim_visible(key: &PoolKey) -> Option<VisibleClaim> {
    debug_assert!(!key.headless);
    let pool = pool()?;
    match pool.claim(key, now_ms()) {
        ClaimOutcome::Hit(claim) => match claim.handle().clone() {
            Handle::Pane(pane_id) => Some(VisibleClaim { claim, pane_id }),
            Handle::Process(_) => None, // dropped: shut down
        },
        ClaimOutcome::Miss(reason) => {
            tracing::debug!(?reason, key = %key.label(), "no standby for this run");
            None
        }
    }
}

/// A warm headless process taken for a run.
pub struct HeadlessClaim {
    claim: pool::Claim,
    token: u64,
}

impl HeadlessClaim {
    /// Take the process. `None` if it vanished between claim and now (the claim is dropped).
    pub fn into_process(self) -> Option<StandbyProcess> {
        let host = GLOBAL.get()?.host.clone();
        let process = host.take_process(self.token)?;
        self.claim.commit();
        Some(process)
    }
}

pub fn claim_headless(key: &PoolKey) -> Option<HeadlessClaim> {
    debug_assert!(key.headless);
    let pool = pool()?;
    match pool.claim(key, now_ms()) {
        ClaimOutcome::Hit(claim) => match claim.handle().clone() {
            Handle::Process(token) => Some(HeadlessClaim { claim, token }),
            Handle::Pane(_) => None,
        },
        ClaimOutcome::Miss(reason) => {
            tracing::debug!(?reason, key = %key.label(), "no standby for this run");
            None
        }
    }
}

/// The pool key a run would claim, or `None` when this backend has no standby of that kind, or
/// the run names a model: standbys start with the CLI's default model and cannot be re-aimed.
pub fn key_for_run(
    agent_type: &str,
    project: &str,
    model: Option<&str>,
    read_only: bool,
    allow_shell: bool,
    headless: bool,
    account: Option<&str>,
) -> Option<PoolKey> {
    if model.is_some_and(|model| !model.trim().is_empty()) {
        return None;
    }
    // An agent set to Linux has no standby, so no run can claim one (SPEC WSL-15).
    if crate::wsl::place::agent_runs_in_linux(agent_type) {
        return None;
    }
    let support = crate::tui::profile_for(agent_type).standby_support();
    let supported = if headless { support.headless } else { support.visible };
    supported.then(|| PoolKey::new(agent_type, project, model, read_only, allow_shell, headless).with_account(account))
}

/// Placeholder host for `status()` before `init`.
struct NoHost;

impl pool::Host for NoHost {
    fn spawn(&self, _key: &PoolKey, _cwd: &str) -> Result<Handle, String> {
        Err("standby pool not running".into())
    }
    fn check(&self, _key: &PoolKey, _handle: &Handle, _age_ms: i64) -> pool::Health {
        pool::Health::Dead
    }
    fn kill(&self, _handle: &Handle, _reason: &str) {}
}
