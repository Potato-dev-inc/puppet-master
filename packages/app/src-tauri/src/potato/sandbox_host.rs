//! The app's one sandbox: builds the real `WslSandbox` (or, on a computer that cannot have one, a stand-in that says so),
//! wraps it in the `Manager` the settings page talks to, keeps the idle-stop setting, and ticks the idle stop.
//!
//! - `apply_settings(public)` is called with the public settings whenever they change (from `live::apply_settings`):
//!   the Labs switch `potato_sandbox_enabled` (and Potato Bot itself) must both be on. On: the manager is built (on its own
//!   thread) and a sandbox the app lost track of is stopped. Off: a running sandbox is stopped.
//! - ONE sandbox, ONE lease. This file owns the app's sandbox and is the only place that hands it to the rest of the app
//!   (`sync`, run after the manager is built and after every install, start, stop, reset and remove): the screen hub gets it as
//!   its source (`ScreenHub::set_source`, the Computer panel and the screen links) and the brain's tools get it as a `Port`
//!   (`live::install_sandbox`) holding the hub's OWN lease table, so the panel, the dispatcher and this host see one lease.
//!   Labs off, or no sandbox set up yet: both are given `None`, so no tool is offered and the panel has nothing to show.
//! - `manager()` is what the commands use; it answers `POTATO_SANDBOX_OFF` while the switch is off.
//! - `stop_on_exit()`: the sandbox leaves with the app, like the brains.
//!
//! The idle-stop minutes are kept in `<app data>/potato/sandbox-settings.json` (the app's own file, not the settings file a
//! phone can patch). Nothing here names a sandbox by its Linux name: `SandboxName::main()` is the one door.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use super::sandbox::control::Action;
use super::sandbox::screen::{self, ScreenHub, ScreenSource};
use super::sandbox::install::{GuestFiles, InstallError, InstallOptions, InstallRecords, Layout, Progress, SavedInstall};
use super::sandbox::lifecycle::{Lifecycle, DEFAULT_IDLE_MINUTES};
use super::sandbox::manager::{Events, InstallView, Manager};
use super::sandbox::wsl::{SandboxName, Wsl};
use super::sandbox::{Observation, SandboxError, SbxResult, ScreenTarget, State, Status, WslSandbox};
use super::sandbox::Sandbox;
use super::sandbox_calls::Port;
use super::window_guard::Msrdc;
use super::store;

/// The Labs switch of the sandbox (it needs Potato Bot's too).
pub const LABS_KEY: &str = "potato_sandbox_enabled";
/// `{ install: InstallView }` while a set up runs and when it ends.
pub const INSTALL_EVENT: &str = "potato://sandbox-install";
/// Something about the sandbox changed: read its status again.
pub const CHANGED_EVENT: &str = "potato://sandbox-changed";

static MANAGER: OnceLock<Arc<Manager>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static BUILDING: AtomicBool = AtomicBool::new(false);

/// Called once at startup, with the app handle the events go out through.
pub fn set_app(app: AppHandle) {
    let _ = APP.set(app);
}

/// Both switches are on in a settings value.
pub fn labs_on(public: &Value) -> bool {
    let flag = |key: &str| public.get(key).and_then(Value::as_bool).unwrap_or(false);
    flag(super::live::LABS_KEY) && flag(LABS_KEY)
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Called with the public settings whenever they change (and once at startup, from the saved file).
pub fn apply_settings(public: &Value) {
    let on = labs_on(public);
    ENABLED.store(on, Ordering::SeqCst);
    if on {
        if MANAGER.get().is_none() && !BUILDING.swap(true, Ordering::SeqCst) {
            // Building asks Windows what is installed, which takes a moment: not on the thread that saved the settings.
            let _ = std::thread::Builder::new().name("potato-sandbox-init".into()).spawn(|| {
                let _ = manager_unchecked();
                BUILDING.store(false, Ordering::SeqCst);
            });
        } else if MANAGER.get().is_some() {
            // Turned back on: a sandbox that is set up is offered again (asking Windows takes a moment).
            let _ = std::thread::Builder::new().name("potato-sandbox-sync".into()).spawn(sync);
        }
    } else {
        // Nothing is offered or shown from this moment (no wait for the stop below).
        sync();
        if let Some(manager) = MANAGER.get().cloned() {
            let _ = std::thread::Builder::new().name("potato-sandbox-off".into()).spawn(move || {
                // A set up that is still running is asked to stop; the stop of a busy sandbox is refused, so it is tried after.
                let _ = manager.cancel_install();
                let _ = manager.stop();
            });
        }
    }
}

// ---- one sandbox, one lease -------------------------------------------------------------------------

/// The hub's source: the host's sandbox, seen as a screen.
struct Source(Arc<dyn Sandbox>);

impl ScreenSource for Source {
    fn name(&self) -> String {
        self.0.status().map(|s| s.name).unwrap_or_default()
    }

    fn target(&self) -> SbxResult<ScreenTarget> {
        self.0.screen_target()
    }
}

/// Gives `sandbox` (or nothing) to the screen `hub` and returns the `Port` the brain's tools use: the same sandbox and the
/// hub's own lease table. This is the only place a `Port` is made.
pub fn wire_into(hub: &ScreenHub, sandbox: Option<&Arc<dyn Sandbox>>) -> Option<Port> {
    hub.set_source(sandbox.map(|s| Arc::new(Source(s.clone())) as Arc<dyn ScreenSource>));
    sandbox.map(|s| Port { sandbox: s.clone(), lease: hub.lease().clone() })
}

static SYNC: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// The sandbox the rest of the app may use: the Labs switches are on (`on`) and one is set up.
pub fn usable_of(on: bool, manager: Option<&Arc<Manager>>) -> Option<Arc<dyn Sandbox>> {
    if !on {
        return None;
    }
    let sandbox = manager?.sandbox().clone();
    match sandbox.status() {
        // A set up that did not finish is not a sandbox the bot can use (the settings page offers Continue / Start over).
        Ok(status) if !matches!(status.state, State::NotInstalled | State::Incomplete) => Some(sandbox),
        _ => None,
    }
}

/// What `sync` does, with its inputs and its screen hub passed in: gives the hub the sandbox (or nothing) and returns the
/// port for the brain's tools (or nothing).
pub fn sync_into(on: bool, manager: Option<&Arc<Manager>>, hub: &ScreenHub) -> Option<Port> {
    wire_into(hub, usable_of(on, manager).as_ref())
}

/// Wires the app to the sandbox as it is now (see the top of this file). Safe to call from any thread, any time.
pub fn sync() {
    // One at a time, so a slow caller with an old answer cannot overwrite a newer one.
    let _one = SYNC.lock();
    let port = sync_into(enabled(), MANAGER.get(), screen::hub());
    super::live::install_sandbox(port);
}

/// The manager, or the plain reason it may not be used now.
pub fn manager() -> Result<Arc<Manager>, String> {
    if !enabled() {
        return Err("POTATO_SANDBOX_OFF: Turn on the sandbox in Settings > General > Labs first.".to_string());
    }
    Ok(manager_unchecked())
}

fn manager_unchecked() -> Arc<Manager> {
    let mut built = false;
    let manager = MANAGER
        .get_or_init(|| {
            built = true;
            build()
        })
        .clone();
    if built {
        // Just built (maybe long after the switch was turned on): wire it in if a sandbox is already set up.
        sync();
    }
    manager
}

/// "Stop everything": Fish's computer goes to sleep now if it was up. Returns whether it was up and is now asleep.
pub fn sleep() -> bool {
    match MANAGER.get() {
        Some(manager) => {
            let state = manager.status().ok().map(|view| view.state);
            sleep_if_up(state, || manager.stop().is_ok())
        }
        None => false,
    }
}

/// Stops the computer only when it was running or starting. Returns whether it was up (only then is it "asleep").
pub fn sleep_if_up(state: Option<&str>, stop: impl FnOnce() -> bool) -> bool {
    match state {
        Some("running") | Some("starting") => stop(),
        _ => false,
    }
}

/// The sandbox leaves with the app.
pub fn stop_on_exit() {
    if let Some(manager) = MANAGER.get() {
        // A set up that is still running is NOT cancelled (that would take the half-made sandbox away): it ends with the app, its
        // record stays "not finished", and the next start of the app shows "Setup didn't finish" with Continue / Start over.
        // A stop of a sandbox that is busy installing is refused, so this does not wait for it.
        let _ = manager.stop();
    }
}

fn data_root() -> PathBuf {
    super::live::root()
}

// ---- the idle-stop setting ------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
struct SandboxSettings {
    v: u32,
    idle_minutes: u32,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self { v: 1, idle_minutes: DEFAULT_IDLE_MINUTES }
    }
}

fn settings_path() -> PathBuf {
    data_root().join("sandbox-settings.json")
}

fn load_idle_minutes() -> u32 {
    match store::read_json::<SandboxSettings>(&settings_path(), "sandbox-settings.json") {
        Ok(Some(settings)) => settings.idle_minutes,
        _ => DEFAULT_IDLE_MINUTES,
    }
}

/// Saves the idle-stop minutes (already clamped by the sandbox) for the next run of the app.
pub fn save_idle_minutes(minutes: u32) {
    let _ = store::write_json(&settings_path(), &SandboxSettings { v: 1, idle_minutes: minutes }, "sandbox-settings.json");
}

// ---- the record of a set up ------------------------------------------------------------------------

/// Keeps the record of the set up (finished or not, and the answers it started with) in `<app data>/potato/sandbox-install-<name>.json`.
/// The sandbox folder writes no file on Windows, so the host does it.
struct FileRecords;

impl FileRecords {
    fn path(name: &SandboxName) -> PathBuf {
        data_root().join(format!("sandbox-install-{}.json", name.as_str()))
    }
}

impl InstallRecords for FileRecords {
    fn load(&self, name: &SandboxName) -> Option<SavedInstall> {
        let value = store::read_json::<Value>(&Self::path(name), "sandbox-install.json").ok()??;
        SavedInstall::from_json(name, &value)
    }

    fn save(&self, name: &SandboxName, saved: &SavedInstall) {
        let _ = store::write_json(&Self::path(name), &saved.to_json(), "sandbox-install.json");
    }

    fn clear(&self, name: &SandboxName) {
        let _ = std::fs::remove_file(Self::path(name));
    }
}

// ---- building it ----------------------------------------------------------------------------------

struct LiveEvents;

impl Events for LiveEvents {
    fn install_progress(&self, view: &InstallView) {
        #[cfg(not(test))]
        if let Some(app) = APP.get() {
            use tauri::Emitter;
            let _ = app.emit(INSTALL_EVENT, serde_json::json!({ "install": view }));
        }
        #[cfg(test)]
        let _ = view;
    }

    fn changed(&self) {
        // An install ended, or the sandbox was removed, reset, started or stopped: the app's wiring follows.
        sync();
        #[cfg(not(test))]
        if let Some(app) = APP.get() {
            use tauri::Emitter;
            let _ = app.emit(CHANGED_EVENT, serde_json::json!({}));
        }
    }
}

/// Where the guest files and recipes are: the installed resources, the staged copy of a dev build, or the source tree.
pub fn guest_root() -> Option<PathBuf> {
    #[cfg(not(test))]
    if let Some(dir) = APP.get().and_then(|app| super::resource_dirs::installed(app, "potato-sandbox", "recipes")) {
        return Some(dir);
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest.join("resources").join("potato-sandbox"), manifest.join("..").join("..").join("potato-sandbox")]
        .into_iter()
        .find(|dir| dir.join("recipes").is_dir())
}

fn build() -> Arc<Manager> {
    let name = SandboxName::main();
    let layout = Layout::new(&data_root());
    let root = guest_root();
    let sandbox: Arc<dyn Sandbox> = match Wsl::live() {
        _ if fake_requested() => fake(),
        Ok(wsl) => {
            // After every start the guard looks for a visible remote-desktop window; if there is one the start fails.
            let lifecycle = Arc::new(Lifecycle::new(wsl.clone(), name.clone()).with_window_check(Arc::new(Msrdc::live())));
            lifecycle.set_idle_minutes(load_idle_minutes());
            let files = GuestFiles::new(root.as_deref().unwrap_or_else(|| std::path::Path::new("")));
            Arc::new(WslSandbox::new(wsl, name.clone(), layout.clone(), files, lifecycle).with_records(Arc::new(FileRecords)))
        }
        Err(_) => Arc::new(NoLinux),
    };
    // A sandbox left running by an earlier run of the app (a crash, a forced quit) has lost its passwords: stop it.
    let _ = sandbox.stop();
    let manager = Manager::new(sandbox, name, Some(layout), root.map(|r| r.join("recipes")), Arc::new(LiveEvents));
    let ticker = manager.clone();
    let _ = std::thread::Builder::new().name("potato-sandbox-idle".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        ticker.idle_tick();
    });
    manager
}

/// An end-to-end build with a stand-in brain also gets a stand-in sandbox (no Linux needed).
#[cfg(feature = "dev-fake-tui")]
fn fake_requested() -> bool {
    super::sidecar::fake_brain_active()
}

#[cfg(not(feature = "dev-fake-tui"))]
fn fake_requested() -> bool {
    false
}

#[cfg(feature = "dev-fake-tui")]
fn fake() -> Arc<dyn Sandbox> {
    Arc::new(super::sandbox::FakeSandbox::not_installed())
}

#[cfg(not(feature = "dev-fake-tui"))]
fn fake() -> Arc<dyn Sandbox> {
    Arc::new(NoLinux)
}

/// The stand-in on a computer with no `wsl.exe` (another system, or Windows without the feature): it says what is missing
/// and does nothing else.
pub struct NoLinux;

fn needs_linux<T>() -> SbxResult<T> {
    Err(InstallError::NeedsWindowsFeature.into())
}

impl Sandbox for NoLinux {
    fn status(&self) -> SbxResult<Status> {
        Ok(Status {
            state: State::NotInstalled,
            reason: Some(InstallError::NeedsWindowsFeature.message()),
            name: SandboxName::main().to_string(),
            idle_minutes: DEFAULT_IDLE_MINUTES,
        })
    }
    fn install(&self, _options: &InstallOptions, _progress: &mut dyn FnMut(Progress)) -> SbxResult<()> {
        needs_linux()
    }
    fn start(&self) -> SbxResult<()> {
        needs_linux()
    }
    fn stop(&self) -> SbxResult<()> {
        Ok(())
    }
    fn snapshot(&self, _snapshot: &str) -> SbxResult<()> {
        needs_linux()
    }
    fn reset(&self, _snapshot: &str) -> SbxResult<()> {
        needs_linux()
    }
    fn remove(&self) -> SbxResult<()> {
        Ok(())
    }
    fn observe(&self) -> SbxResult<Observation> {
        needs_linux()
    }
    fn act(&self, _actions: &[Action]) -> SbxResult<Observation> {
        needs_linux()
    }
    fn browser(&self, _args: &[String]) -> SbxResult<String> {
        needs_linux()
    }
    fn screen_target(&self) -> SbxResult<ScreenTarget> {
        Err(SandboxError::new("SANDBOX_NOT_INSTALLED", "The sandbox is not set up yet."))
    }
    fn linux_feature(&self) -> bool {
        false
    }
}
