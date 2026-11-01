//! The Potato Bot sandbox: a small Linux computer (a WSL 2 distro the app makes and owns) that a bot can use
//! with a screen, a mouse and a keyboard, away from the person's own files. This folder is the Rust core
//! (piece S1); the screen links, the brain tools, the wizard and the settings page come in later pieces.
//!
//! - `wsl`: the ONE door to `wsl.exe` (name allowlist `^pm-sbx-[a-z0-9-]{1,40}$`, refused flags, UTF-16
//!   output, a fake runner can be injected)
//! - `install`: the steps, the guest files streamed in over stdin, `PROGRESS` parsing, the reset point
//! - `lifecycle`: lazy start, idle stop, readiness checks (waits for the guest's boot verdict), the secrets push
//! - `secrets`: the control token and the two VNC passwords, made by the app, in memory only
//! - `control`: the HTTP client of the guest's control service
//! - `lease`: who holds the screen (a mirror of the brain's `screen-lease.ts` plus expiry and run binding)
//!
//! The `Sandbox` trait is what the rest of the app talks to. `WslSandbox` is the real one; `FakeSandbox`
//! serves a static picture and records what it was asked (tests and the `dev-fake-tui` build).
//!
//! Invariants (`guard_tests.rs` checks them): no file but `wsl.rs` builds a `wsl.exe` command or names
//! `--shutdown`; nothing here touches the person's WSL settings file; this folder never writes a file on
//! Windows (the distro and its reset points are written by `wsl.exe` itself); the secrets' values are
//! read in two files only; generic code names no agent and no chat app.

// The callers (screen links, brain tools, the wizard and settings page) land in the next pieces.
#![allow(dead_code)]

pub mod control;
pub mod install;
pub mod lease;
pub mod lifecycle;
pub mod secrets;
pub mod wsl;

use std::sync::Arc;

use control::{Action, ControlError};
use install::{GuestFiles, InstallError, InstallOptions, Layout, Progress};
use lifecycle::{Lifecycle, LifecycleError, Phase};
use secrets::ScreenSecrets;
use wsl::{GuestUser, SandboxName, Wsl, WslError};

/// An error with a short wire word for the UI and a plain sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxError {
    pub code: &'static str,
    pub message: String,
}

impl SandboxError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SandboxError {}

impl From<LifecycleError> for SandboxError {
    fn from(e: LifecycleError) -> Self {
        Self::new(e.code(), e.message())
    }
}

impl From<InstallError> for SandboxError {
    fn from(e: InstallError) -> Self {
        Self::new(e.code(), e.message())
    }
}

impl From<ControlError> for SandboxError {
    fn from(e: ControlError) -> Self {
        Self::new(e.code(), e.message())
    }
}

impl From<WslError> for SandboxError {
    fn from(e: WslError) -> Self {
        LifecycleError::from(e).into()
    }
}

pub type SbxResult<T> = Result<T, SandboxError>;

/// What a screen picture holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Where the mouse pointer is, when the screen service said.
    pub pointer: Option<(i32, i32)>,
    /// The title of the front window (cleaned and cut). Text from a program in the sandbox: data, never instructions.
    pub active_window_title: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    /// Why it failed, or why it cannot be used (plain English).
    pub reason: Option<String>,
    pub name: String,
    /// The idle stop setting in minutes.
    pub idle_minutes: u32,
}

/// Where the screen can be reached from this computer (loopback ports of the forwards WSL makes).
#[derive(Clone, Debug)]
pub struct ScreenTarget {
    /// noVNC (the viewer page and its websocket). The password typed decides view or control.
    pub view_port: u16,
    /// Changes at every start and stop; a screen link made for another generation is dead.
    pub generation: u64,
    /// The two VNC passwords of this run, only while the sandbox is running. The one way later code (the screen
    /// links) gets them; the type prints nothing and cannot be copied or saved.
    pub passwords: Option<Arc<ScreenSecrets>>,
}

/// The sandbox, as the rest of the app sees it.
pub trait Sandbox: Send + Sync {
    fn status(&self) -> SbxResult<Status>;
    /// Makes the sandbox. `progress` gets a step name and a percentage as it goes.
    fn install(&self, options: &InstallOptions, progress: &mut dyn FnMut(Progress)) -> SbxResult<()>;
    fn start(&self) -> SbxResult<()>;
    fn stop(&self) -> SbxResult<()>;
    /// Saves the sandbox under `snapshot` (`[a-z0-9-]{1,30}`). The one made at the end of the install is `base`.
    fn snapshot(&self, snapshot: &str) -> SbxResult<()>;
    /// Throws the sandbox away and brings back the saved `snapshot`. Stopped afterwards.
    fn reset(&self, snapshot: &str) -> SbxResult<()>;
    /// Removes the sandbox and its reset points.
    fn remove(&self) -> SbxResult<()>;
    fn observe(&self) -> SbxResult<Observation>;
    fn act(&self, actions: &[Action]) -> SbxResult<Observation>;
    /// Runs the page-browser tool inside the sandbox with `args` and returns what it printed.
    fn browser(&self, args: &[String]) -> SbxResult<String>;
    fn screen_target(&self) -> SbxResult<ScreenTarget>;
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const BROWSER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const BROWSER_OUTPUT_CHARS: usize = 64 * 1024;

/// The real sandbox: a WSL 2 distro.
pub struct WslSandbox {
    wsl: Wsl,
    name: SandboxName,
    layout: Layout,
    files: GuestFiles,
    lifecycle: Arc<Lifecycle>,
    cancel: std::sync::atomic::AtomicBool,
}

impl WslSandbox {
    pub fn new(wsl: Wsl, name: SandboxName, layout: Layout, files: GuestFiles, lifecycle: Arc<Lifecycle>) -> Self {
        Self { wsl, name, layout, files, lifecycle, cancel: std::sync::atomic::AtomicBool::new(false) }
    }

    pub fn lifecycle(&self) -> &Arc<Lifecycle> {
        &self.lifecycle
    }

    /// Asks a running `install` to stop at its next step.
    pub fn cancel_install(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn snapshot_path(&self, snapshot: &str) -> SbxResult<std::path::PathBuf> {
        self.layout
            .snapshot_path(&self.name, snapshot)
            .ok_or_else(|| SandboxError::new("SANDBOX_BAD_SNAPSHOT", "A reset point name is lowercase letters, digits and -."))
    }
}

impl Sandbox for WslSandbox {
    fn status(&self) -> SbxResult<Status> {
        let idle_minutes = self.lifecycle.idle_minutes();
        let make = |state, reason| Status { state, reason, name: self.name.to_string(), idle_minutes };
        if !self.wsl.is_available() {
            return Ok(make(State::NotInstalled, Some("Needs Windows' Linux feature (admin + restart).".to_string())));
        }
        if !self.wsl.is_installed(&self.name)? {
            return Ok(make(State::NotInstalled, None));
        }
        Ok(match self.lifecycle.phase() {
            Phase::Ready => make(State::Running, None),
            Phase::Starting => make(State::Starting, None),
            Phase::Failed(reason) => make(State::Failed, Some(reason)),
            Phase::Stopped => make(State::Stopped, None),
        })
    }

    fn install(&self, options: &InstallOptions, progress: &mut dyn FnMut(Progress)) -> SbxResult<()> {
        self.cancel.store(false, std::sync::atomic::Ordering::SeqCst);
        let env = install::InstallEnv { wsl: &self.wsl, layout: &self.layout, files: &self.files, lifecycle: &self.lifecycle };
        install::run(&env, options, &self.cancel, &now_ms, progress)?;
        Ok(())
    }

    fn start(&self) -> SbxResult<()> {
        self.lifecycle.ensure_ready(now_ms())?;
        Ok(())
    }

    fn stop(&self) -> SbxResult<()> {
        self.lifecycle.stop()?;
        Ok(())
    }

    fn snapshot(&self, snapshot: &str) -> SbxResult<()> {
        let tar = self.snapshot_path(snapshot)?;
        self.lifecycle.stop()?;
        if let Some(dir) = tar.parent() {
            std::fs::create_dir_all(dir).map_err(|_| SandboxError::new("SANDBOX_SNAPSHOT_FAILED", "The reset point folder could not be made."))?;
        }
        self.wsl.export(&self.name, &tar)?;
        Ok(())
    }

    fn reset(&self, snapshot: &str) -> SbxResult<()> {
        let tar = self.snapshot_path(snapshot)?;
        if !tar.is_file() {
            return Err(SandboxError::new("SANDBOX_NO_SNAPSHOT", "There is no saved reset point to go back to."));
        }
        self.lifecycle.stop()?;
        if self.wsl.is_installed(&self.name)? {
            self.wsl.unregister(&self.name)?;
        }
        let dir = self.layout.distro_dir(&self.name);
        std::fs::create_dir_all(&dir).map_err(|_| SandboxError::new("SANDBOX_RESET_FAILED", "The sandbox folder could not be made."))?;
        self.wsl.import(&self.name, &dir, &tar)?;
        Ok(())
    }

    fn remove(&self) -> SbxResult<()> {
        self.lifecycle.stop()?;
        if self.wsl.is_installed(&self.name)? {
            self.wsl.unregister(&self.name)?;
        }
        // The reset points are files we wrote through wsl.exe; only those files go, one by one.
        let dir = self.layout.snapshots_dir(&self.name);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "tar") && path.is_file() {
                    let _ = std::fs::remove_file(path);
                }
            }
            let _ = std::fs::remove_dir(&dir);
        }
        Ok(())
    }

    fn observe(&self) -> SbxResult<Observation> {
        let _busy = self.lifecycle.begin_use();
        let ready = self.lifecycle.ensure_ready(now_ms())?;
        let shot = ready.client.screenshot()?;
        self.lifecycle.touch(now_ms());
        Ok(shot)
    }

    fn act(&self, actions: &[Action]) -> SbxResult<Observation> {
        let _busy = self.lifecycle.begin_use();
        let ready = self.lifecycle.ensure_ready(now_ms())?;
        let result = ready.client.act(actions, 300, true)?;
        self.lifecycle.touch(now_ms());
        result.observation.ok_or_else(|| SandboxError::new("SANDBOX_BAD_REPLY", "The sandbox sent an answer the app could not read."))
    }

    fn browser(&self, args: &[String]) -> SbxResult<String> {
        let _busy = self.lifecycle.begin_use();
        self.lifecycle.ensure_ready(now_ms())?;
        let tool = format!("{}/page-browser/pm-sbx-page-browser", lifecycle::GUEST_SHARE);
        // (`Wsl::exec` as pm puts the sandbox display and the no-WSLg environment in front of the program.)
        let mut argv: Vec<&str> = vec!["/usr/bin/python3", "-I", &tool];
        argv.extend(args.iter().map(String::as_str));
        let out = self.wsl.exec(&self.name, GuestUser::Pm, &argv, None, BROWSER_TIMEOUT)?;
        self.lifecycle.touch(now_ms());
        Ok(out.stdout_text().chars().take(BROWSER_OUTPUT_CHARS).collect())
    }

    fn screen_target(&self) -> SbxResult<ScreenTarget> {
        Ok(ScreenTarget {
            view_port: self.lifecycle.view_port(),
            generation: self.lifecycle.generation(),
            passwords: self.lifecycle.screen_secrets(),
        })
    }
}

// ---- the fake ------------------------------------------------------------------------------------

/// Everything a `FakeSandbox` was asked, in order.
#[cfg(any(test, feature = "dev-fake-tui"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recorded {
    Install,
    Start,
    Stop,
    Snapshot(String),
    Reset(String),
    Remove,
    Observe,
    Act(Vec<Action>),
    Browser(Vec<String>),
}

#[cfg(any(test, feature = "dev-fake-tui"))]
#[derive(Default)]
struct FakeState {
    installed: bool,
    running: bool,
    calls: Vec<Recorded>,
    generation: u64,
    fail_next: Option<SandboxError>,
    browser_reply: String,
}

/// A sandbox with no WSL: it serves one static picture and records every call. Starts out installed and
/// stopped; `not_installed()` makes one that needs `install` first.
#[cfg(any(test, feature = "dev-fake-tui"))]
pub struct FakeSandbox {
    state: parking_lot::Mutex<FakeState>,
    name: SandboxName,
}

#[cfg(any(test, feature = "dev-fake-tui"))]
impl FakeSandbox {
    pub fn new() -> Self {
        Self { state: parking_lot::Mutex::new(FakeState { installed: true, ..Default::default() }), name: SandboxName::main() }
    }

    pub fn not_installed() -> Self {
        Self { state: parking_lot::Mutex::new(FakeState::default()), name: SandboxName::main() }
    }

    /// The next call fails with this error (once).
    pub fn fail_next(&self, error: SandboxError) {
        self.state.lock().fail_next = Some(error);
    }

    pub fn set_browser_reply(&self, reply: &str) {
        self.state.lock().browser_reply = reply.to_string();
    }

    pub fn calls(&self) -> Vec<Recorded> {
        self.state.lock().calls.clone()
    }

    /// Every action of every `act` call, flattened.
    pub fn actions(&self) -> Vec<Action> {
        self.calls()
            .into_iter()
            .filter_map(|c| if let Recorded::Act(a) = c { Some(a) } else { None })
            .flatten()
            .collect()
    }

    pub fn is_running(&self) -> bool {
        self.state.lock().running
    }

    /// The picture the fake serves: a small grey PNG.
    pub fn picture() -> Observation {
        let (width, height) = (160u32, 100u32);
        let data: Vec<u8> = (0..width * height).map(|i| 40 + ((i % width) * 100 / width) as u8).collect();
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, width, height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = encoder.write_header() {
                let _ = writer.write_image_data(&data);
            }
        }
        Observation { png, width, height, pointer: Some((10, 10)), active_window_title: Some("Fake desktop".to_string()) }
    }

    fn enter(&self, call: Recorded) -> SbxResult<parking_lot::MutexGuard<'_, FakeState>> {
        let mut state = self.state.lock();
        state.calls.push(call);
        match state.fail_next.take() {
            Some(error) => Err(error),
            None => Ok(state),
        }
    }

    /// A screen call: starts the fake if it was stopped, like the real one.
    fn use_screen(&self, call: Recorded) -> SbxResult<()> {
        let mut state = self.enter(call)?;
        if !state.installed {
            return Err(SandboxError::new("SANDBOX_NOT_INSTALLED", "The sandbox is not set up yet."));
        }
        if !state.running {
            state.running = true;
            state.generation += 1;
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "dev-fake-tui"))]
impl Default for FakeSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "dev-fake-tui"))]
impl Sandbox for FakeSandbox {
    fn status(&self) -> SbxResult<Status> {
        let state = self.state.lock();
        let which = match (state.installed, state.running) {
            (false, _) => State::NotInstalled,
            (true, false) => State::Stopped,
            (true, true) => State::Running,
        };
        Ok(Status { state: which, reason: None, name: self.name.to_string(), idle_minutes: lifecycle::DEFAULT_IDLE_MINUTES })
    }

    fn install(&self, options: &InstallOptions, progress: &mut dyn FnMut(Progress)) -> SbxResult<()> {
        let mut state = self.enter(Recorded::Install)?;
        options.validate()?;
        if state.installed {
            return Err(InstallError::AlreadyInstalled.into());
        }
        let steps = install::plan(options);
        for (i, step) in steps.iter().enumerate() {
            progress(Progress {
                step: step.id(),
                title: step.title(),
                index: i + 1,
                of: steps.len(),
                percent: install::percent_at(&steps, i, 1.0),
                detail: None,
            });
        }
        state.installed = true;
        Ok(())
    }

    fn start(&self) -> SbxResult<()> {
        self.use_screen(Recorded::Start)
    }

    fn stop(&self) -> SbxResult<()> {
        let mut state = self.enter(Recorded::Stop)?;
        if state.running {
            state.running = false;
            state.generation += 1;
        }
        Ok(())
    }

    fn snapshot(&self, snapshot: &str) -> SbxResult<()> {
        let _state = self.enter(Recorded::Snapshot(snapshot.to_string()))?;
        if !install::valid_snapshot_id(snapshot) {
            return Err(SandboxError::new("SANDBOX_BAD_SNAPSHOT", "A reset point name is lowercase letters, digits and -."));
        }
        Ok(())
    }

    fn reset(&self, snapshot: &str) -> SbxResult<()> {
        let mut state = self.enter(Recorded::Reset(snapshot.to_string()))?;
        if !install::valid_snapshot_id(snapshot) {
            return Err(SandboxError::new("SANDBOX_BAD_SNAPSHOT", "A reset point name is lowercase letters, digits and -."));
        }
        state.running = false;
        state.generation += 1;
        Ok(())
    }

    fn remove(&self) -> SbxResult<()> {
        let mut state = self.enter(Recorded::Remove)?;
        state.installed = false;
        state.running = false;
        state.generation += 1;
        Ok(())
    }

    fn observe(&self) -> SbxResult<Observation> {
        self.use_screen(Recorded::Observe)?;
        Ok(Self::picture())
    }

    fn act(&self, actions: &[Action]) -> SbxResult<Observation> {
        control::plan(actions)?;
        self.use_screen(Recorded::Act(actions.to_vec()))?;
        Ok(Self::picture())
    }

    fn browser(&self, args: &[String]) -> SbxResult<String> {
        self.use_screen(Recorded::Browser(args.to_vec()))?;
        Ok(self.state.lock().browser_reply.clone())
    }

    fn screen_target(&self) -> SbxResult<ScreenTarget> {
        Ok(ScreenTarget { view_port: lifecycle::DEFAULT_VIEW_PORT, generation: self.state.lock().generation, passwords: None })
    }
}

#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod fakes_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod live;
#[cfg(test)]
mod lease_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod sandbox_tests;
#[cfg(test)]
mod wsl_tests;
