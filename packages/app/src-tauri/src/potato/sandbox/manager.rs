//! What the settings page and the setup wizard talk to: one `Manager` over a `Sandbox`.
//!
//! It adds, on top of the trait, only what a page needs and the trait does not have:
//!
//! - `status()`: one plain view (installed? running? disk? is Windows' Linux feature there? how far is the install?);
//! - `start_install(answers)`: checks the wizard's answers against the recipes, then runs the install on its own thread and
//!   tells an `Events` listener about each step (the page shows a bar and the step name);
//! - `cancel_install()`, `start()`, `stop()`, `reset()`, `remove(typed)` (Remove needs the word typed), `set_idle_minutes()`;
//! - `ask_linux_feature()` / `take_linux_feature_ask()`: the page asks, shows a confirm dialog, and only a press on its button
//!   hands the token back. The command that turns the feature on needs that token (single use, ten minutes). This file never
//!   turns the feature on itself and never makes the `Consent`.
//!
//! One thing runs at a time: an install, a start, a stop, a reset and a remove each claim the manager and the others answer
//! "busy". It writes no file and starts no process; it reads file sizes under the sandbox folder it is given.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::install::{Desktop, InstallError, InstallOptions, Layout, Progress, BASE_SNAPSHOT};
use super::wsl::SandboxName;
use super::{Sandbox, SandboxError, SbxResult, State};

/// The word the person types to remove the sandbox.
pub const REMOVE_WORD: &str = "remove";
/// How long a "turn on Windows' Linux feature" confirm dialog stays valid.
const FEATURE_ASK_MS: i64 = 10 * 60 * 1000;

// ---- what the page sees ---------------------------------------------------------------------------

/// Where an install is. `idle` before any install in this run of the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallView {
    /// `idle`, `running`, `done`, `failed` or `cancelled`.
    pub state: &'static str,
    /// 0 to 100 over the whole install.
    pub percent: u8,
    /// The step's short id (`create`, `apply`, ...).
    pub step: Option<String>,
    /// The step's plain name ("Downloading Ubuntu").
    pub title: Option<String>,
    pub index: usize,
    pub of: usize,
    /// While the recipes run: the line the installer printed ("Installing XFCE desktop").
    pub detail: Option<String>,
    /// A cancel was asked and the install is stopping at its next step.
    pub cancelling: bool,
    pub error: Option<ErrorView>,
    /// The install stopped because Windows' Linux feature is missing: the page shows the button for it.
    pub needs_linux_feature: bool,
}

impl InstallView {
    pub fn idle() -> Self {
        Self { state: "idle", percent: 0, step: None, title: None, index: 0, of: 0, detail: None, cancelling: false, error: None, needs_linux_feature: false }
    }

    fn running() -> Self {
        Self { state: "running", ..Self::idle() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErrorView {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxView {
    /// `not_installed`, `installing`, `stopped`, `starting`, `running` or `failed`.
    pub state: &'static str,
    /// Why it failed or cannot be used, in plain English.
    pub reason: Option<String>,
    pub name: String,
    pub idle_minutes: u32,
    /// Windows' Linux feature answers. False: setup shows the "turn on" button before anything else.
    pub linux_feature: bool,
    /// Size of the sandbox's disk file on this computer (it grows as the sandbox is used).
    pub disk_bytes: Option<u64>,
    /// There is a saved reset point (`Reset` goes back to it).
    pub has_reset_point: bool,
    pub reset_point_bytes: Option<u64>,
    pub install: InstallView,
}

fn state_word(state: State) -> &'static str {
    match state {
        State::NotInstalled => "not_installed",
        State::Stopped => "stopped",
        State::Starting => "starting",
        State::Running => "running",
        State::Failed => "failed",
    }
}

/// What the install tells a listener (the page).
pub trait Events: Send + Sync {
    /// The install moved (a step, a line, the end).
    fn install_progress(&self, view: &InstallView);
    /// Something about the sandbox changed (it started, stopped, was made or removed): read the status again.
    fn changed(&self);
}

pub struct NoEvents;

impl Events for NoEvents {
    fn install_progress(&self, _view: &InstallView) {}
    fn changed(&self) {}
}

// ---- the wizard's answers -------------------------------------------------------------------------

/// What the setup wizard collected.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WizardAnswers {
    /// `ubuntu`, `zorin` (Ubuntu with the Zorin look) or `light`.
    pub look: String,
    /// `coding` and/or `daily`.
    pub uses: Vec<String>,
    /// `recipe.option` -> value; a list is the ids joined by commas (`coding.languages` -> `python,go`).
    pub options: BTreeMap<String, String>,
    /// Save a reset point at the end (default yes).
    pub reset_point: bool,
}

impl Default for WizardAnswers {
    fn default() -> Self {
        Self { look: String::new(), uses: Vec::new(), options: BTreeMap::new(), reset_point: true }
    }
}

// ---- the recipe menu ------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceView {
    pub id: String,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionView {
    pub id: String,
    pub title: String,
    pub description: String,
    /// `toggle`, `single` or `multi`.
    pub kind: String,
    /// A bool, a choice id, or a list of choice ids.
    pub default: Value,
    pub choices: Vec<ChoiceView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeView {
    pub id: String,
    pub title: String,
    pub description: String,
    pub exclusive: Option<String>,
    pub requires: Option<String>,
    pub options: Vec<OptionView>,
}

fn text_of(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn option_view(opt: &Value) -> Option<OptionView> {
    let kind = text_of(opt, "kind");
    let choices: Vec<ChoiceView> = opt
        .get("choices")
        .and_then(Value::as_array)
        .map(|list| list.iter().map(|c| ChoiceView { id: text_of(c, "id"), title: text_of(c, "title"), description: text_of(c, "description") }).collect())
        .unwrap_or_default();
    let default = match kind.as_str() {
        "toggle" => Value::Bool(opt.get("default").and_then(Value::as_bool).unwrap_or(false)),
        "multi" => opt.get("default").filter(|d| d.is_array()).cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "single" => match opt.get("default").and_then(Value::as_str) {
            Some(d) => Value::String(d.to_string()),
            None => Value::String(choices.first().map(|c| c.id.clone()).unwrap_or_default()),
        },
        _ => return None,
    };
    Some(OptionView { id: text_of(opt, "id"), title: text_of(opt, "title"), description: text_of(opt, "description"), kind, default, choices })
}

/// The recipes the wizard can offer: every `*.json` in the recipes folder except `base` (always installed), by file name.
pub fn menu(recipes_dir: &Path) -> SbxResult<Vec<RecipeView>> {
    let bad = || SandboxError::new("SANDBOX_GUEST_FILES_MISSING", "The sandbox files that ship with the app are missing.");
    let mut files: Vec<PathBuf> = std::fs::read_dir(recipes_dir)
        .map_err(|_| bad())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|_| bad())?).map_err(|_| bad())?;
        let id = text_of(&value, "id");
        if id.is_empty() || id == "base" {
            continue;
        }
        out.push(RecipeView {
            id,
            title: text_of(&value, "title"),
            description: text_of(&value, "description"),
            exclusive: value.get("exclusive").and_then(Value::as_str).map(str::to_string),
            requires: value.get("requires").and_then(Value::as_str).map(str::to_string),
            options: value.get("options").and_then(Value::as_array).map(|list| list.iter().filter_map(option_view).collect()).unwrap_or_default(),
        });
    }
    Ok(out)
}

fn bad_answers(message: &str) -> SandboxError {
    SandboxError::new("SANDBOX_BAD_OPTIONS", message)
}

/// Turns the wizard's answers into the install's options. With a `menu`, every option and choice is checked against the
/// recipes; without one only the install's own checks apply (`InstallOptions::validate`).
pub fn to_options(name: &SandboxName, answers: &WizardAnswers, menu: Option<&[RecipeView]>) -> SbxResult<InstallOptions> {
    let desktop = Desktop::parse(&answers.look).ok_or_else(|| bad_answers("Pick how the desktop should look."))?;
    let mut options = InstallOptions::new(name.clone(), desktop);
    options.snapshot = answers.reset_point;
    for use_ in &answers.uses {
        if !matches!(use_.as_str(), "coding" | "daily") {
            return Err(bad_answers("Pick coding, daily use, or both."));
        }
        if !options.addons.contains(use_) {
            options.addons.push(use_.clone());
        }
    }
    if options.addons.is_empty() {
        return Err(bad_answers("Pick coding, daily use, or both."));
    }
    for (key, value) in &answers.options {
        // An empty list is "nothing extra", which is also what leaving the option out means.
        if value.is_empty() {
            continue;
        }
        if let Some(menu) = menu {
            check_against_menu(menu, key, value)?;
        }
        options.options.push((key.clone(), value.clone()));
    }
    options.validate()?;
    Ok(options)
}

fn check_against_menu(menu: &[RecipeView], key: &str, value: &str) -> SbxResult<()> {
    let unknown = || bad_answers("One of the choices is not one this version of the app offers.");
    let (recipe, option) = key.split_once('.').ok_or_else(unknown)?;
    let option = menu.iter().find(|r| r.id == recipe).and_then(|r| r.options.iter().find(|o| o.id == option)).ok_or_else(unknown)?;
    match option.kind.as_str() {
        "toggle" => (value == "true" || value == "false").then_some(()).ok_or_else(unknown),
        "single" => option.choices.iter().any(|c| c.id == value).then_some(()).ok_or_else(unknown),
        _ => value.split(',').all(|part| option.choices.iter().any(|c| c.id == part)).then_some(()).ok_or_else(unknown),
    }
}

// ---- the manager ----------------------------------------------------------------------------------

struct Inner {
    install: InstallView,
    /// (token, asked at ms)
    feature_ask: Option<(String, i64)>,
}

pub struct Manager {
    sandbox: Arc<dyn Sandbox>,
    name: SandboxName,
    layout: Option<Layout>,
    recipes_dir: Option<PathBuf>,
    events: Arc<dyn Events>,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    busy: AtomicBool,
    inner: Mutex<Inner>,
}

/// Holds the manager's one lane; gives it back when dropped (also when a thread panics).
struct Lane {
    manager: Arc<Manager>,
}

impl Drop for Lane {
    fn drop(&mut self) {
        self.manager.busy.store(false, Ordering::SeqCst);
    }
}

/// What `ask_linux_feature` gives the page: the sentence for the confirm dialog and the token its button hands back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureAsk {
    pub token: String,
    pub title: String,
    pub body: String,
}

impl Manager {
    pub fn new(sandbox: Arc<dyn Sandbox>, name: SandboxName, layout: Option<Layout>, recipes_dir: Option<PathBuf>, events: Arc<dyn Events>) -> Arc<Self> {
        Self::with_clock(sandbox, name, layout, recipes_dir, events, Box::new(super::now_ms))
    }

    /// Like `new`, with a clock the caller controls (tests).
    pub fn with_clock(
        sandbox: Arc<dyn Sandbox>,
        name: SandboxName,
        layout: Option<Layout>,
        recipes_dir: Option<PathBuf>,
        events: Arc<dyn Events>,
        clock: Box<dyn Fn() -> i64 + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sandbox,
            name,
            layout,
            recipes_dir,
            events,
            clock,
            busy: AtomicBool::new(false),
            inner: Mutex::new(Inner { install: InstallView::idle(), feature_ask: None }),
        })
    }

    pub fn sandbox(&self) -> &Arc<dyn Sandbox> {
        &self.sandbox
    }

    fn claim(self: &Arc<Self>) -> SbxResult<Lane> {
        if self.busy.swap(true, Ordering::SeqCst) {
            let running = self.inner.lock().install.state == "running";
            return Err(SandboxError::new(
                "SANDBOX_BUSY",
                if running { "Set up is still running. Wait for it to finish or stop it first." } else { "The sandbox is busy with something else. Try again in a moment." },
            ));
        }
        Ok(Lane { manager: self.clone() })
    }

    // ---- status ----------------------------------------------------------------------------

    fn folder_bytes(dir: &Path) -> Option<u64> {
        let mut total = 0u64;
        let mut any = false;
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                    any = true;
                }
            }
        }
        any.then_some(total)
    }

    pub fn status(&self) -> SbxResult<SandboxView> {
        let status = self.sandbox.status()?;
        let install = self.inner.lock().install.clone();
        let (disk_bytes, has_reset_point, reset_point_bytes) = match &self.layout {
            Some(layout) => {
                let tar = layout.snapshot_path(&self.name, BASE_SNAPSHOT);
                let size = tar.as_ref().and_then(|p| std::fs::metadata(p).ok()).filter(|m| m.is_file()).map(|m| m.len());
                (Self::folder_bytes(&layout.distro_dir(&self.name)), size.is_some(), size)
            }
            None => (None, false, None),
        };
        let state = if install.state == "running" { "installing" } else { state_word(status.state) };
        Ok(SandboxView {
            state,
            reason: status.reason,
            name: status.name,
            idle_minutes: status.idle_minutes,
            linux_feature: self.sandbox.linux_feature(),
            disk_bytes,
            has_reset_point,
            reset_point_bytes,
            install,
        })
    }

    /// The recipes the wizard offers (from the guest files that ship with the app).
    pub fn menu(&self) -> SbxResult<Vec<RecipeView>> {
        let dir = self.recipes_dir.as_ref().ok_or_else(|| SandboxError::new("SANDBOX_GUEST_FILES_MISSING", "The sandbox files that ship with the app are missing."))?;
        menu(dir)
    }

    // ---- install ---------------------------------------------------------------------------

    /// Checks the answers, then installs on a thread of its own and returns. The page follows `Events::install_progress`
    /// (and can read `status()` any time).
    pub fn start_install(self: &Arc<Self>, answers: &WizardAnswers) -> SbxResult<()> {
        let options = self.prepare_install(answers)?;
        let lane = self.claim()?;
        self.inner.lock().install = InstallView::running();
        self.events.install_progress(&InstallView::running());
        let manager = self.clone();
        let spawned = std::thread::Builder::new().name("potato-sandbox-install".into()).spawn(move || {
            manager.run_install(&options);
            drop(lane);
        });
        if spawned.is_err() {
            self.inner.lock().install = InstallView::idle();
            // The lane was moved into the closure that never ran; it is dropped with it, which frees the manager.
            return Err(SandboxError::new("SANDBOX_INSTALL_FAILED", "Set up could not start. Try again."));
        }
        Ok(())
    }

    /// The checks that happen before anything runs, so the page gets its answer at once.
    pub fn prepare_install(&self, answers: &WizardAnswers) -> SbxResult<InstallOptions> {
        let menu = self.menu().ok();
        let options = to_options(&self.name, answers, menu.as_deref())?;
        if !self.sandbox.linux_feature() {
            return Err(InstallError::NeedsWindowsFeature.into());
        }
        if self.sandbox.status()?.state != State::NotInstalled {
            return Err(InstallError::AlreadyInstalled.into());
        }
        Ok(options)
    }

    /// Runs the install on this thread and records how it went. (`start_install` calls this on a thread.)
    pub fn run_install(&self, options: &InstallOptions) {
        let result = self.sandbox.install(options, &mut |p: Progress| {
            let view = {
                let mut inner = self.inner.lock();
                let cancelling = inner.install.cancelling;
                inner.install = InstallView {
                    state: "running",
                    percent: p.percent,
                    step: Some(p.step.to_string()),
                    title: Some(p.title.to_string()),
                    index: p.index,
                    of: p.of,
                    detail: p.detail.clone(),
                    cancelling,
                    error: None,
                    needs_linux_feature: false,
                };
                inner.install.clone()
            };
            self.events.install_progress(&view);
        });
        let view = {
            let mut inner = self.inner.lock();
            let mut view = inner.install.clone();
            match result {
                Ok(()) => {
                    view.state = "done";
                    view.percent = 100;
                    view.cancelling = false;
                    view.error = None;
                }
                Err(error) => {
                    view.state = if error.code == "SANDBOX_INSTALL_CANCELLED" { "cancelled" } else { "failed" };
                    view.cancelling = false;
                    view.needs_linux_feature = error.code == "SANDBOX_NEEDS_LINUX_FEATURE";
                    view.error = Some(ErrorView { code: error.code.to_string(), message: error.message });
                }
            }
            inner.install = view.clone();
            view
        };
        self.events.install_progress(&view);
        self.events.changed();
    }

    /// Asks a running install to stop at its next step (the distro made so far is taken away again).
    pub fn cancel_install(&self) -> SbxResult<()> {
        {
            let mut inner = self.inner.lock();
            if inner.install.state != "running" {
                return Err(SandboxError::new("SANDBOX_NOT_INSTALLING", "Set up is not running."));
            }
            inner.install.cancelling = true;
        }
        self.sandbox.cancel_install();
        let view = self.inner.lock().install.clone();
        self.events.install_progress(&view);
        Ok(())
    }

    /// The page read the end of an install (done, failed, cancelled): go back to `idle`.
    pub fn dismiss_install(&self) {
        let mut inner = self.inner.lock();
        if inner.install.state != "running" {
            inner.install = InstallView::idle();
        }
    }

    // ---- start, stop, reset, remove --------------------------------------------------------

    fn after(&self) {
        self.events.changed();
    }

    pub fn start(self: &Arc<Self>) -> SbxResult<()> {
        let _lane = self.claim()?;
        let result = self.sandbox.start();
        self.after();
        result
    }

    pub fn stop(self: &Arc<Self>) -> SbxResult<()> {
        let _lane = self.claim()?;
        let result = self.sandbox.stop();
        self.after();
        result
    }

    /// Goes back to the reset point saved at the end of set up. Everything done in the sandbox since is gone.
    pub fn reset(self: &Arc<Self>) -> SbxResult<()> {
        let _lane = self.claim()?;
        let result = self.sandbox.reset(BASE_SNAPSHOT);
        self.after();
        result
    }

    /// Removes the sandbox and its reset points. `typed` must be the word `remove` (any case, spaces around are fine).
    pub fn remove(self: &Arc<Self>, typed: &str) -> SbxResult<()> {
        if !typed.trim().eq_ignore_ascii_case(REMOVE_WORD) {
            return Err(SandboxError::new("SANDBOX_CONFIRM_WORD", format!("Type the word {REMOVE_WORD} to remove the sandbox.")));
        }
        let _lane = self.claim()?;
        let result = self.sandbox.remove();
        {
            let mut inner = self.inner.lock();
            if inner.install.state != "running" {
                inner.install = InstallView::idle();
            }
        }
        self.after();
        result
    }

    /// Returns the number that is used (the sandbox keeps it between 1 and 240).
    pub fn set_idle_minutes(&self, minutes: u32) -> u32 {
        let used = self.sandbox.set_idle_minutes(minutes);
        self.after();
        used
    }

    /// The app calls this every half minute.
    pub fn idle_tick(&self) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        self.sandbox.idle_tick();
    }

    // ---- Windows' Linux feature ------------------------------------------------------------

    /// Step one of "turn on Windows' Linux feature": what the confirm dialog says, and a token for its button.
    pub fn ask_linux_feature(&self) -> FeatureAsk {
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.inner.lock().feature_ask = Some((token.clone(), (self.clock)()));
        FeatureAsk {
            token,
            title: "Turn on Windows' Linux feature?".to_string(),
            body: "The sandbox is a small Linux computer that runs inside Windows. Windows needs its Linux feature (WSL) for that. \
                   Windows will ask for an administrator's OK, and you will most likely have to restart the computer afterwards. \
                   Nothing else on your computer changes."
                .to_string(),
        }
    }

    /// Step two: the person pressed the dialog's button. Good once, within ten minutes of the ask, for the token it gave.
    pub fn take_linux_feature_ask(&self, token: &str) -> SbxResult<()> {
        let mut inner = self.inner.lock();
        let valid = matches!(&inner.feature_ask, Some((t, at)) if t == token && (self.clock)() - *at <= FEATURE_ASK_MS);
        inner.feature_ask = None;
        if valid {
            Ok(())
        } else {
            Err(SandboxError::new("SANDBOX_NOT_CONFIRMED", "Windows' Linux feature is only turned on after you confirm. Press the button again."))
        }
    }
}
