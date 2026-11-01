//! Making the sandbox: the steps, the guest files, and the progress the person sees.
//!
//! The steps (`plan`):
//!
//! 1. `CheckWsl`: Windows' Linux feature answers. If not, the install stops with `NeedsWindowsFeature`
//!    ("Needs Windows' Linux feature (admin + restart)"). This code never turns the feature on: that is
//!    `Wsl::enable_windows_feature`, which needs a `Consent` made on the person's button press.
//! 2. `CheckFree`: our name is not already taken.
//! 3. `CreateDistro`: `wsl --install Ubuntu-24.04 --name <n> --location <dir> --no-launch`, with `<dir>`
//!    under `<app data>/potato/sandbox/<n>/` (`Layout`), never the repo.
//! 4. `CopyGuestFiles`: the guest files go in over stdin as a tar stream (`GuestFiles::tar_stream`) and are
//!    unpacked by `tar` inside the distro. Nothing is copied through a Windows folder. Then the short install-time
//!    `/etc/wsl.conf` (systemd off) is put in place and the distro is restarted once, so the installer runs without
//!    systemd (a fresh Ubuntu starts snapd, unattended-upgrades and PackageKit, which compete with apt).
//! 5. `ApplyRecipes`: `apply-recipe.py` runs as root; its `PROGRESS n/total text` lines become progress
//!    events (`parse_progress_line`; `total` is 1000 and `n` the share of the whole job done, so the bar moves while
//!    packages download), its `ERROR text` line becomes the failure reason.
//! 6. `Restart`: `wsl --terminate <n>` so `/etc/wsl.conf` takes effect.
//! 7. `BootCheck`: a real start with every readiness check (`Lifecycle::ensure_ready`), then a stop.
//! 8. `Snapshot` (optional, on by default): `wsl --export` to the reset point.
//!
//! A failure after the distro exists unregisters that distro again (it is ours, made a minute ago), so a
//! retry starts clean. A cancel between steps does the same. No secret is made or handled here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::lifecycle::{Lifecycle, GUEST_SHARE};
use super::wsl::{GuestUser, SandboxName, Wsl, WslError};

const APPLY_TIMEOUT: Duration = Duration::from_secs(90 * 60);
const COPY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PROGRESS_TEXT_CHARS: usize = 200;
/// The short `/etc/wsl.conf` for the install (a file of the guest archive).
const INSTALL_CONF: &str = "/usr/local/share/pm-sbx/desktop/wsl-install.conf";
/// What the person is told the Ubuntu download weighs (`wsl --install` prints no percentage).
const UBUNTU_DOWNLOAD_TEXT: &str = "about 340 MB";
const MAX_OPTION_VALUE: usize = 200;
const ADDONS: [&str; 2] = ["coding", "daily"];
/// The default reset point.
pub const BASE_SNAPSHOT: &str = "base";

// ---- where things live ---------------------------------------------------------------------------

/// `<app data>/potato/sandbox/`: every distro folder and every reset point is under it.
#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    /// `potato_root` is the `<app data>/potato` folder.
    pub fn new(potato_root: &Path) -> Self {
        Self { root: potato_root.join("sandbox") }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the distro's disk lives: `<root>/<name>/`.
    pub fn distro_dir(&self, name: &SandboxName) -> PathBuf {
        self.root.join(name.as_str())
    }

    pub fn snapshots_dir(&self, name: &SandboxName) -> PathBuf {
        self.root.join("snapshots").join(name.as_str())
    }

    /// `snapshot` is `[a-z0-9-]{1,30}`; anything else is `None`.
    pub fn snapshot_path(&self, name: &SandboxName, snapshot: &str) -> Option<PathBuf> {
        valid_snapshot_id(snapshot).then(|| self.snapshots_dir(name).join(format!("{snapshot}.tar")))
    }
}

pub fn valid_snapshot_id(id: &str) -> bool {
    (1..=30).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ---- the guest files -----------------------------------------------------------------------------

/// The folder with the guest side: `guest/{desktop,control,egress,page-browser}/` and `recipes/` (the
/// shape of `packages/potato-sandbox/`). The app finds it in its resources.
#[derive(Clone, Debug)]
pub struct GuestFiles {
    root: PathBuf,
}

const GUEST_FOLDERS: [&str; 4] = ["desktop", "control", "egress", "page-browser"];

/// Paths under the guest folder that an install and a start run (the installer, the walls and their probe, the
/// desktop starter, the control service, the base recipe).
const REQUIRED_FILES: [&str; 6] = [
    "guest/desktop/apply-recipe.py",
    "guest/desktop/pm-sbx-start-desktop.sh",
    "guest/egress/pm-sbx-egress.sh",
    "guest/egress/pm-sbx-egress-probe.sh",
    "guest/control/pm-sbx-control",
    "recipes/base.json",
];

fn shipped(file_name: &str) -> bool {
    !(file_name.ends_with(".md") || file_name.starts_with("test_") || file_name.ends_with(".pyc"))
}

impl GuestFiles {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    /// Every file to ship: (path inside the tar, path on disk), sorted.
    pub fn list(&self) -> std::io::Result<Vec<(String, PathBuf)>> {
        let mut files = Vec::new();
        for folder in GUEST_FOLDERS {
            collect(&self.root.join("guest").join(folder), folder, &mut files)?;
        }
        collect(&self.root.join("recipes"), "recipes", &mut files)?;
        files.sort();
        Ok(files)
    }

    /// The files without which an install or a start cannot work must all be there.
    pub fn check(&self) -> Result<(), InstallError> {
        for rel in REQUIRED_FILES {
            if !self.root.join(rel).is_file() {
                return Err(InstallError::GuestFilesMissing);
            }
        }
        Ok(())
    }

    /// A ustar archive of `list()`, with Windows line endings taken out of every file.
    pub fn tar_stream(&self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        for (name, path) in self.list()? {
            let bytes = std::fs::read(&path)?;
            write_tar_entry(&mut out, &name, &strip_cr(&bytes))?;
        }
        out.extend_from_slice(&[0u8; 1024]);
        Ok(out)
    }
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if path.is_dir() {
            collect(&path, &format!("{prefix}/{name}"), out)?;
        } else if shipped(&name) {
            out.push((format!("{prefix}/{name}"), path));
        }
    }
    Ok(())
}

/// `\r\n` becomes `\n`: a checkout with Windows line endings would break `#!/bin/bash` scripts.
pub fn strip_cr(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            continue;
        }
        out.push(*b);
    }
    out
}

fn octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    let text = format!("{value:0width$o}");
    field[..width].copy_from_slice(text.as_bytes());
    field[width] = 0;
}

/// One regular file as a ustar header and padded data.
pub fn write_tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8]) -> std::io::Result<()> {
    if name.is_empty() || name.len() > 100 || !name.is_ascii() || name.starts_with('/') || name.contains("..") {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad name in the guest archive"));
    }
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    let executable = !(name.ends_with(".json") || name.ends_with(".conf") || name.ends_with(".desktop"));
    octal(&mut header[100..108], if executable { 0o755 } else { 0o644 });
    octal(&mut header[108..116], 0);
    octal(&mut header[116..124], 0);
    octal(&mut header[124..136], data.len() as u64);
    octal(&mut header[136..148], 0);
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    let check = format!("{sum:06o}");
    header[148..154].copy_from_slice(check.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    let pad = (512 - data.len() % 512) % 512;
    out.extend(std::iter::repeat(0u8).take(pad));
    Ok(())
}

// ---- the options ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    /// The light desktop (fluxbox).
    Light,
    /// XFCE, the plain Ubuntu look.
    Xfce,
    /// XFCE with the Zorin look (themes, icons, wallpaper and font from the Zorin PPA). Not Zorin's own desktop.
    Zorin,
}

impl Desktop {
    /// The desktop recipe (the one with a session).
    pub fn recipe_id(self) -> &'static str {
        match self {
            Desktop::Light => "light",
            Desktop::Xfce | Desktop::Zorin => "xfce",
        }
    }

    /// Every recipe this choice installs, desktop first (the look needs XFCE already there).
    pub fn recipe_ids(self) -> Vec<&'static str> {
        match self {
            Desktop::Zorin => vec!["xfce", "zorin-look"],
            other => vec![other.recipe_id()],
        }
    }

    /// The wizard's word for it: `light`, `ubuntu` or `zorin`.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "light" => Some(Desktop::Light),
            "ubuntu" => Some(Desktop::Xfce),
            "zorin" => Some(Desktop::Zorin),
            _ => None,
        }
    }
}

/// The wizard's answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOptions {
    pub name: SandboxName,
    pub desktop: Desktop,
    /// `coding` and/or `daily`.
    pub addons: Vec<String>,
    /// `(recipe.option, value)`, for example `("coding.pkg", "pnpm")`.
    pub options: Vec<(String, String)>,
    /// Save a reset point at the end.
    pub snapshot: bool,
}

impl InstallOptions {
    pub fn new(name: SandboxName, desktop: Desktop) -> Self {
        Self { name, desktop, addons: Vec::new(), options: Vec::new(), snapshot: true }
    }

    fn chosen(&self) -> Vec<&str> {
        let mut ids = self.desktop.recipe_ids();
        ids.extend(self.addons.iter().map(String::as_str));
        ids
    }

    pub fn validate(&self) -> Result<(), InstallError> {
        let bad = |why: &'static str| Err(InstallError::BadOptions(why));
        for (i, addon) in self.addons.iter().enumerate() {
            if !ADDONS.contains(&addon.as_str()) {
                return bad("an add-on is coding or daily");
            }
            if self.addons[..i].contains(addon) {
                return bad("an add-on is listed twice");
            }
        }
        let chosen = self.chosen();
        for (key, value) in &self.options {
            let Some((recipe, option)) = key.split_once('.') else { return bad("an option is recipe.option") };
            let word = |w: &str| {
                let mut chars = w.chars();
                (1..=48).contains(&w.len())
                    && chars.next().is_some_and(|c| c.is_ascii_lowercase())
                    && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            };
            if !word(recipe) || !word(option) {
                return bad("an option name is lowercase words");
            }
            if !chosen.contains(&recipe) {
                return bad("an option belongs to a recipe that is not chosen");
            }
            if value.len() > MAX_OPTION_VALUE
                || !value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b',' | b'.' | b'_' | b'+' | b'-'))
            {
                return bad("an option value is lowercase letters, digits and , . _ + -");
            }
        }
        Ok(())
    }

    /// The command line for `apply-recipe.py` (`base` is always installed by the script itself).
    pub fn recipe_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for id in self.chosen() {
            args.push("--recipe".to_string());
            args.push(id.to_string());
        }
        for (key, value) in &self.options {
            args.push("--opt".to_string());
            args.push(format!("{key}={value}"));
        }
        args
    }
}

// ---- steps and progress --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    CheckWsl,
    CheckFree,
    CreateDistro,
    CopyGuestFiles,
    ApplyRecipes,
    Restart,
    BootCheck,
    Snapshot,
}

impl Step {
    pub fn id(self) -> &'static str {
        match self {
            Step::CheckWsl => "check_linux",
            Step::CheckFree => "check_free",
            Step::CreateDistro => "create",
            Step::CopyGuestFiles => "copy",
            Step::ApplyRecipes => "apply",
            Step::Restart => "restart",
            Step::BootCheck => "check_start",
            Step::Snapshot => "snapshot",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::CheckWsl => "Checking Windows' Linux feature",
            Step::CheckFree => "Checking the name is free",
            Step::CreateDistro => "Downloading Ubuntu",
            Step::CopyGuestFiles => "Copying the sandbox files",
            Step::ApplyRecipes => "Installing the desktop and tools",
            Step::Restart => "Restarting the sandbox",
            Step::BootCheck => "Checking it starts safely",
            Step::Snapshot => "Saving a reset point",
        }
    }

    /// Share of the whole install, for the percentage bar.
    pub fn weight(self) -> u32 {
        match self {
            Step::CheckWsl | Step::CheckFree | Step::Restart => 1,
            Step::CopyGuestFiles => 3,
            Step::BootCheck => 5,
            Step::Snapshot => 10,
            Step::CreateDistro => 25,
            Step::ApplyRecipes => 45,
        }
    }
}

pub fn plan(options: &InstallOptions) -> Vec<Step> {
    let mut steps = vec![
        Step::CheckWsl,
        Step::CheckFree,
        Step::CreateDistro,
        Step::CopyGuestFiles,
        Step::ApplyRecipes,
        Step::Restart,
        Step::BootCheck,
    ];
    if options.snapshot {
        steps.push(Step::Snapshot);
    }
    steps
}

/// What the person sees: a step name, a bar, and (while recipes run) the line the installer printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub step: &'static str,
    pub title: &'static str,
    /// 1-based position of the step.
    pub index: usize,
    pub of: usize,
    /// 0 to 100 over the whole install.
    pub percent: u8,
    pub detail: Option<String>,
}

pub fn percent_at(steps: &[Step], index: usize, fraction: f64) -> u8 {
    let total: u32 = steps.iter().map(|s| s.weight()).sum();
    let before: u32 = steps.iter().take(index).map(|s| s.weight()).sum();
    let within = steps.get(index).map(|s| f64::from(s.weight()) * fraction.clamp(0.0, 1.0)).unwrap_or(0.0);
    (((f64::from(before) + within) / f64::from(total.max(1))) * 100.0).round().clamp(0.0, 100.0) as u8
}

/// One `PROGRESS n/total text` line of `apply-recipe.py`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyLine {
    pub done: u32,
    pub total: u32,
    pub text: String,
}

pub fn parse_progress_line(line: &str) -> Option<ApplyLine> {
    let rest = line.trim().strip_prefix("PROGRESS ")?;
    let (counts, text) = rest.split_once(' ').unwrap_or((rest, ""));
    let (done, total) = counts.split_once('/')?;
    let all_digits = |s: &str| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(done) || !all_digits(total) {
        return None;
    }
    let (done, total) = (done.parse::<u32>().ok()?, total.parse::<u32>().ok()?);
    if total == 0 || done > total {
        return None;
    }
    let text = crate::redact::text(text).into_inner().chars().filter(|c| !c.is_control()).take(PROGRESS_TEXT_CHARS).collect();
    Some(ApplyLine { done, total, text })
}

/// An `ERROR text` line of `apply-recipe.py`.
pub fn parse_error_line(line: &str) -> Option<String> {
    let text = line.trim().strip_prefix("ERROR ")?;
    Some(crate::redact::text(text).into_inner().chars().filter(|c| !c.is_control()).take(PROGRESS_TEXT_CHARS).collect())
}

// ---- errors --------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// Windows' Linux feature is missing. Enabling it needs admin and a restart, and the person's button.
    NeedsWindowsFeature,
    /// A distro with this name is already there.
    AlreadyInstalled,
    BadOptions(&'static str),
    GuestFilesMissing,
    Cancelled,
    Step { step: &'static str, reason: String },
}

impl InstallError {
    pub fn code(&self) -> &'static str {
        match self {
            InstallError::NeedsWindowsFeature => "SANDBOX_NEEDS_LINUX_FEATURE",
            InstallError::AlreadyInstalled => "SANDBOX_ALREADY_INSTALLED",
            InstallError::BadOptions(_) => "SANDBOX_BAD_OPTIONS",
            InstallError::GuestFilesMissing => "SANDBOX_GUEST_FILES_MISSING",
            InstallError::Cancelled => "SANDBOX_INSTALL_CANCELLED",
            InstallError::Step { .. } => "SANDBOX_INSTALL_FAILED",
        }
    }

    pub fn message(&self) -> String {
        match self {
            InstallError::NeedsWindowsFeature => "Needs Windows' Linux feature (admin + restart).".to_string(),
            InstallError::AlreadyInstalled => "A sandbox with this name is already set up.".to_string(),
            InstallError::BadOptions(why) => format!("These choices cannot be used: {why}."),
            InstallError::GuestFilesMissing => "The sandbox files that ship with the app are missing.".to_string(),
            InstallError::Cancelled => "Set up was stopped.".to_string(),
            InstallError::Step { step, reason } => format!("Set up failed at \"{step}\": {reason}"),
        }
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for InstallError {}

fn step_error(step: Step, error: impl std::fmt::Display) -> InstallError {
    let reason: String = crate::redact::text(&error.to_string()).into_inner().chars().take(200).collect();
    InstallError::Step { step: step.title(), reason }
}

/// What `apply-recipe.py` means by an exit code.
pub fn apply_exit_reason(code: Option<i32>) -> &'static str {
    match code {
        Some(1) => "a step failed",
        Some(2) => "the choices are not valid",
        Some(3) => "it did not run as root",
        Some(4) => "the desktop user has more rights than it may",
        _ => "it stopped unexpectedly",
    }
}

// ---- the run -------------------------------------------------------------------------------------

pub struct InstallEnv<'a> {
    pub wsl: &'a Wsl,
    pub layout: &'a Layout,
    pub files: &'a GuestFiles,
    pub lifecycle: &'a Lifecycle,
}

/// Runs the plan. `now_ms` is read once per start check. A failure or a cancel after the distro was made
/// unregisters it again.
pub fn run(
    env: &InstallEnv<'_>,
    options: &InstallOptions,
    cancel: &AtomicBool,
    now_ms: &dyn Fn() -> i64,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), InstallError> {
    options.validate()?;
    env.files.check()?;
    let steps = plan(options);
    let mut created = false;
    let outcome = run_steps(env, options, &steps, cancel, now_ms, progress, &mut created);
    if outcome.is_err() && created {
        // Ours, made a moment ago: take it away so a retry starts clean. A failure to do so is not shown over the first.
        let _ = env.lifecycle.stop();
        let _ = env.wsl.unregister(&options.name);
    }
    outcome
}

fn emit(progress: &mut dyn FnMut(Progress), steps: &[Step], index: usize, fraction: f64, detail: Option<String>) {
    let step = steps[index];
    progress(Progress {
        step: step.id(),
        title: step.title(),
        index: index + 1,
        of: steps.len(),
        percent: percent_at(steps, index, fraction),
        detail,
    });
}

fn run_steps(
    env: &InstallEnv<'_>,
    options: &InstallOptions,
    steps: &[Step],
    cancel: &AtomicBool,
    now_ms: &dyn Fn() -> i64,
    progress: &mut dyn FnMut(Progress),
    created: &mut bool,
) -> Result<(), InstallError> {
    let name = &options.name;
    for (index, step) in steps.iter().copied().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err(InstallError::Cancelled);
        }
        emit(progress, steps, index, 0.0, None);
        match step {
            Step::CheckWsl => {
                if !env.wsl.is_available() {
                    return Err(InstallError::NeedsWindowsFeature);
                }
            }
            Step::CheckFree => {
                if env.wsl.is_installed(name).map_err(|e| step_error(step, e))? {
                    return Err(InstallError::AlreadyInstalled);
                }
            }
            Step::CreateDistro => {
                let dir = env.layout.distro_dir(name);
                std::fs::create_dir_all(&dir).map_err(|e| step_error(step, e))?;
                *created = true;
                create_distro(env, name, &dir, steps, index, progress).map_err(|e| step_error(step, e))?;
            }
            Step::CopyGuestFiles => {
                let tar = env.files.tar_stream().map_err(|e| step_error(step, e))?;
                env.wsl.exec(name, GuestUser::Root, &["/bin/mkdir", "-p", GUEST_SHARE], None, COPY_TIMEOUT).and_then(|o| o.ok()).map_err(|e| step_error(step, e))?;
                env.wsl
                    .exec(name, GuestUser::Root, &["/bin/tar", "-x", "-m", "-C", GUEST_SHARE, "-f", "-"], Some(tar), COPY_TIMEOUT)
                    .and_then(|o| o.ok())
                    .map_err(|e| step_error(step, e))?;
                restart_without_systemd(env, name);
            }
            Step::ApplyRecipes => apply_recipes(env, options, steps, index, progress)?,
            Step::Restart => env.wsl.terminate(name).map_err(|e| step_error(step, e))?,
            Step::BootCheck => {
                env.lifecycle.ensure_ready(now_ms()).map_err(|e| step_error(step, e.message()))?;
                env.lifecycle.stop().map_err(|e| step_error(step, e.message()))?;
            }
            Step::Snapshot => {
                let tar = env.layout.snapshot_path(name, BASE_SNAPSHOT).ok_or(InstallError::BadOptions("bad snapshot name"))?;
                if let Some(dir) = tar.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| step_error(step, e))?;
                }
                env.wsl.export(name, &tar).map_err(|e| step_error(step, e))?;
            }
        }
    }
    emit_done(progress, steps);
    Ok(())
}

/// Puts the short install-time wsl.conf in place and restarts the distro, so the installer runs without systemd. This only
/// makes the install faster, so a failure here is not an install failure: the installer still works with systemd on.
fn restart_without_systemd(env: &InstallEnv<'_>, name: &SandboxName) {
    let copied = env.wsl.exec(name, GuestUser::Root, &["/bin/cp", INSTALL_CONF, "/etc/wsl.conf"], None, COPY_TIMEOUT).and_then(|o| o.ok());
    if copied.is_ok() {
        let _ = env.wsl.terminate(name);
    }
}

enum CreateEvent {
    /// `wsl --install` printed its second line (the first is "Downloading", the second "Installing", in the person's language).
    Unpacking,
    Done(Result<(), WslError>),
}

fn mm_ss(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The share of the step done and the line to show, for how long `wsl --install` has been running. It prints no
/// percentage, so the bar eases toward the end of each phase and the line carries the elapsed time.
pub fn create_progress(elapsed: Duration, unpacking_for: Option<Duration>) -> (f64, String) {
    match unpacking_for {
        None => {
            let t = elapsed.as_secs_f64();
            (0.7 * t / (t + 60.0), format!("Downloading Ubuntu ({UBUNTU_DOWNLOAD_TEXT}) \u{b7} {}", mm_ss(elapsed)))
        }
        Some(since) => {
            let u = since.as_secs_f64();
            (0.7 + 0.28 * u / (u + 15.0), format!("Unpacking Ubuntu \u{b7} {}", mm_ss(elapsed)))
        }
    }
}

/// `wsl --install` on a thread, while this one keeps the bar and the elapsed time moving once a second.
fn create_distro(
    env: &InstallEnv<'_>,
    name: &SandboxName,
    dir: &Path,
    steps: &[Step],
    index: usize,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), WslError> {
    let (tx, rx) = mpsc::channel::<CreateEvent>();
    let wsl = env.wsl;
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut lines = 0usize;
            let mut last = String::new();
            let result = wsl.install_distro_streaming(name, dir, &mut |raw| {
                // wsl.exe prints UTF-16: after a split at the newline byte, NULs are left in the text.
                let line: String = raw.chars().filter(|c| *c != '\0' && !c.is_control()).collect();
                let line = line.trim().to_string();
                if line.is_empty() || line == last {
                    return;
                }
                last = line;
                lines += 1;
                if lines == 2 {
                    let _ = tx.send(CreateEvent::Unpacking);
                }
            });
            let _ = tx.send(CreateEvent::Done(result));
        });
        let started = Instant::now();
        let mut unpacking: Option<Instant> = None;
        loop {
            let (fraction, detail) = create_progress(started.elapsed(), unpacking.map(|t| t.elapsed()));
            emit(progress, steps, index, fraction, Some(detail));
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(CreateEvent::Done(result)) => return result,
                Ok(CreateEvent::Unpacking) => {
                    unpacking.get_or_insert_with(Instant::now);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(WslError::Start("the download stopped unexpectedly".to_string())),
            }
        }
    })
}

fn emit_done(progress: &mut dyn FnMut(Progress), steps: &[Step]) {
    if let Some(last) = steps.last() {
        progress(Progress {
            step: last.id(),
            title: last.title(),
            index: steps.len(),
            of: steps.len(),
            percent: 100,
            detail: Some("Done".to_string()),
        });
    }
}

fn apply_recipes(
    env: &InstallEnv<'_>,
    options: &InstallOptions,
    steps: &[Step],
    index: usize,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), InstallError> {
    let step = steps[index];
    let script = format!("{GUEST_SHARE}/desktop/apply-recipe.py");
    let args = options.recipe_args();
    let mut argv: Vec<&str> = vec!["/usr/bin/python3", "-I", &script];
    argv.extend(args.iter().map(String::as_str));
    let mut last_error: Option<String> = None;
    let output = env
        .wsl
        .exec_streaming(&options.name, GuestUser::Root, &argv, None, APPLY_TIMEOUT, &mut |line| {
            if let Some(p) = parse_progress_line(line) {
                let fraction = f64::from(p.done) / f64::from(p.total);
                emit(progress, steps, index, fraction, Some(p.text));
            } else if let Some(error) = parse_error_line(line) {
                last_error = Some(error);
            }
        })
        .map_err(|e: WslError| step_error(step, e))?;
    if output.success() {
        return Ok(());
    }
    let reason = last_error.unwrap_or_else(|| apply_exit_reason(output.code).to_string());
    Err(InstallError::Step { step: step.title(), reason })
}
