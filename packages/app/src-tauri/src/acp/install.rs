//! Installing the add-on a chat needs (A5): the agent's ACP adapter, a small Node package.
//!
//! "Install" for one agent does this and nothing else:
//!
//! 1. reads the agent's last `[[pins]]` entry (package, exact version, entry file, SHA-256) from its definition;
//! 2. writes `package.json` in `<app data>/acp/adapters/<folder>/` naming that one package at that exact version;
//! 3. runs `npm install --omit=dev --omit=optional --no-audit --no-fund --ignore-scripts` there, in a hidden
//!    process (never a global install, never with install scripts), npm found next to the `node` the app already
//!    uses or on PATH;
//! 4. compares the SHA-256 of the entry file with the pin. Only a match makes it "installed". A mismatch is
//!    reported and refused: nothing is deleted, and the start-time check (`pin::enforce`) keeps refusing it.
//!
//! "Remove" moves the folder to `<app data>/acp/.removed/<folder>-<time>`; nothing is hard-deleted.
//!
//! `--omit=optional` keeps out the agent's own bundled program (hundreds of MB): the chat uses the program the
//! person already has installed (`env_which` in the agent's table), the same one the terminal runs.
//!
//! What is run and where is behind [`Runner`] and [`HostEnv`], so the tests use a fake that writes files instead
//! of starting npm. Nothing here names an agent: the package, folder and size all come from the definition.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use super::config::{expand, AdapterPin, AgentConfig, HostEnv, PinPolicy};
use super::pin::{self, PinCheck};

/// The arguments of the one npm command this module runs. A test pins this list.
pub const NPM_ARGS: &[&str] = &["install", "--omit=dev", "--omit=optional", "--no-audit", "--no-fund", "--ignore-scripts", "--no-progress", "--loglevel=http"];

/// Longest an install may run before npm is stopped.
pub const INSTALL_LIMIT: Duration = Duration::from_secs(15 * 60);
/// Progress lines kept for the screen (the newest).
pub const MAX_LINES: usize = 40;
const MAX_LINE_CHARS: usize = 200;

// ---- what the person sees ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AddonState {
    NotInstalled,
    Installing,
    Installed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddonStatus {
    pub agent: String,
    pub label: String,
    pub state: AddonState,
    /// The installed version (state `installed`).
    pub version: Option<String>,
    /// The version this build installs.
    pub pinned_version: Option<String>,
    /// A plain sentence: why it failed, or what was found.
    pub detail: String,
    pub size_mb: Option<u32>,
    /// While installing: the newest progress lines from npm.
    pub lines: Vec<String>,
    /// What the sign-in card says for this agent (from its table).
    pub signin_hint: String,
}

/// Why an install or removal did not happen. `Display` is the sentence the person reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The agent's table has nothing to install (no pin, no hash, or an odd package or folder).
    NotInstallable(String),
    /// Another install of this agent is running.
    Busy,
    /// Node (and so npm) is not on this computer.
    NodeMissing,
    /// The folder could not be written.
    Io(String),
    /// npm ran and failed (exit code, last lines).
    Npm { code: Option<i32>, tail: String },
    /// npm finished but the entry file is not the one pinned. Nothing was removed.
    PinMismatch { version: String, found: String, expected: String },
    /// npm finished but the result is not usable (entry missing, wrong version).
    Unusable(String),
    /// Removal: the folder could not be moved (usually a chat is using it).
    CannotMove(String),
    /// A registry download failed (network, size, address).
    Download(String),
    /// A registry download is not the file the registry's SHA-256 names. Nothing was unpacked.
    Checksum { found: String, expected: String },
    /// A registry archive holds something that is refused (a path outside the folder, a link, too much data).
    Archive(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::NotInstallable(why) => write!(f, "This agent has no chat add-on to install: {why}"),
            InstallError::Busy => write!(f, "The add-on is already being installed."),
            InstallError::NodeMissing => write!(f, "Node.js is needed to install the chat add-on, and was not found on this computer. Install Node.js (nodejs.org), then try again."),
            InstallError::Io(why) => write!(f, "The add-on could not be written: {why}"),
            InstallError::Npm { code, tail } => {
                let code = code.map(|c| format!(" (exit code {c})")).unwrap_or_default();
                if tail.is_empty() {
                    write!(f, "npm could not install the add-on{code}.")
                } else {
                    write!(f, "npm could not install the add-on{code}: {tail}")
                }
            }
            InstallError::PinMismatch { version, found, expected } => write!(
                f,
                "The downloaded add-on {version} is not the file this app was tested with (its fingerprint is {found}, expected {expected}). It was not turned on, and nothing was removed."
            ),
            InstallError::Unusable(why) => write!(f, "The add-on did not install properly: {why}"),
            InstallError::CannotMove(why) => write!(f, "The add-on could not be removed: {why}"),
            InstallError::Download(why) => write!(f, "The download failed: {why}."),
            InstallError::Checksum { found, expected } => write!(
                f,
                "The download is not the file the registry lists (its fingerprint is {found}, expected {expected}). It was not unpacked and nothing was installed."
            ),
            InstallError::Archive(why) => write!(f, "The download was not unpacked: {why}."),
        }
    }
}

impl std::error::Error for InstallError {}

// ---- the plan: what an install of one agent means ---------------------------------------------------

/// Everything an install needs, read from the agent's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub agent_id: String,
    pub label: String,
    pub package: String,
    pub version: String,
    /// `<adapters>/<folder>`: where `package.json` and `node_modules` go.
    pub dir: PathBuf,
    pub entry: PathBuf,
    pub sha256: String,
    pub size_mb: Option<u32>,
}

pub(crate) fn valid_package_name(name: &str) -> bool {
    let part_ok = |p: &str| !p.is_empty() && !p.starts_with(['.', '_']) && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'_' | b'~'));
    match name.strip_prefix('@') {
        Some(rest) => rest.split_once('/').is_some_and(|(scope, pkg)| part_ok(scope) && part_ok(pkg)),
        None => part_ok(name),
    }
}

pub(crate) fn valid_version(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) && version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

/// The folder name in `{adapters}/<folder>/...`, if the template is shaped that way.
fn folder_of(entry_template: &str) -> Option<String> {
    let rest = entry_template.strip_prefix("{adapters}")?.trim_start_matches(['/', '\\']);
    let folder = rest.split(['/', '\\']).next()?;
    let ok = !folder.is_empty() && folder != "." && folder != ".." && folder.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    let tail_ok = !rest.split(['/', '\\']).any(|part| part == "..");
    (ok && tail_ok).then(|| folder.to_string())
}

/// The newest pin (the last one listed) is what gets installed. Refused unless it names a hash: an install
/// that cannot be verified is not offered.
pub fn plan(cfg: &AgentConfig, host: &dyn HostEnv) -> Result<Plan, InstallError> {
    if cfg.disabled {
        return Err(InstallError::NotInstallable("chat is switched off for it in this build".into()));
    }
    let pin = cfg.pins.last().ok_or_else(|| InstallError::NotInstallable("no adapter version is pinned".into()))?;
    let sha256 = pin.sha256.clone().filter(|h| h.len() == 64).ok_or_else(|| InstallError::NotInstallable("its pin has no SHA-256, so the download could not be checked".into()))?;
    if !valid_package_name(&pin.package) {
        return Err(InstallError::NotInstallable(format!("{:?} is not a package name", pin.package)));
    }
    if !valid_version(&pin.version) {
        return Err(InstallError::NotInstallable(format!("{:?} is not an exact version", pin.version)));
    }
    let folder = folder_of(&pin.entry).ok_or_else(|| InstallError::NotInstallable("its entry file is not under {adapters}/<folder>".into()))?;
    Ok(Plan {
        agent_id: cfg.id.clone(),
        label: cfg.label.clone(),
        package: pin.package.clone(),
        version: pin.version.clone(),
        dir: host.adapters_dir().join(folder),
        entry: PathBuf::from(expand(&pin.entry, host)),
        sha256: sha256.to_ascii_lowercase(),
        size_mb: cfg.addon.size_mb,
    })
}

/// The agents the screens list: those with something to install.
pub fn installable(cfg: &AgentConfig) -> bool {
    !cfg.disabled && cfg.pin_policy != PinPolicy::Off && cfg.pins.last().is_some_and(|p| p.sha256.is_some())
}

/// The `package.json` an install writes: one package, one exact version.
pub fn package_json(plan: &Plan) -> String {
    let name: String = plan.agent_id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let json = serde_json::json!({
        "name": format!("pm-acp-addon-{name}"),
        "version": "0.0.0",
        "private": true,
        "description": "Chat add-on installed by Puppet Master. Safe to remove from the app's settings.",
        "dependencies": { plan.package.clone(): plan.version.clone() },
    });
    let mut text = serde_json::to_string_pretty(&json).unwrap_or_default();
    text.push('\n');
    text
}

// ---- running npm (behind a trait so tests never start it) --------------------------------------------

pub trait Runner: Send + Sync {
    /// The npm program to run, or `None` when Node is not installed.
    fn find_npm(&self) -> Option<PathBuf>;
    /// Runs `npm args` in `cwd`, calling `on_line` for each line it prints (stdout and stderr).
    /// Returns the exit code (`None` when it was stopped by a signal or by the time limit).
    fn run(&self, npm: &Path, args: &[String], cwd: &Path, on_line: &mut dyn FnMut(&str)) -> Result<Option<i32>, String>;
}

/// The real thing: a hidden process.
pub struct RealRunner;

impl Runner for RealRunner {
    fn find_npm(&self) -> Option<PathBuf> {
        npm_next_to(crate::env_probe::find_on_path("node").as_deref()).or_else(|| crate::env_probe::find_on_path("npm"))
    }

    fn run(&self, npm: &Path, args: &[String], cwd: &Path, on_line: &mut dyn FnMut(&str)) -> Result<Option<i32>, String> {
        let mut command = crate::proc_util::hidden_command(npm);
        command.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        command.env("npm_config_update_notifier", "false").env("npm_config_fund", "false").env("npm_config_audit", "false");
        // npm.cmd starts `node` from its own folder or from PATH: make sure the folder of the node we found is on PATH.
        if let Some(dir) = npm.parent() {
            let mut paths: Vec<PathBuf> = vec![dir.to_path_buf()];
            if let Some(existing) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&existing));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                command.env("PATH", joined);
            }
        }
        let mut child = command.spawn().map_err(|e| format!("could not start npm: {e}"))?;
        let (tx, rx) = mpsc::channel::<String>();
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(spawn_reader(out, tx.clone()));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(spawn_reader(err, tx.clone()));
        }
        drop(tx);
        let deadline = Instant::now() + INSTALL_LIMIT;
        let mut code: Option<Option<i32>> = None;
        loop {
            while let Ok(line) = rx.try_recv() {
                on_line(&line);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    code = Some(status.code());
                    break;
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    // Our own child, stopped by its handle (never by image name).
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
        for reader in readers {
            let _ = reader.join();
        }
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
        }
        match code {
            Some(code) => Ok(code),
            None => Err(format!("npm took longer than {} minutes and was stopped", INSTALL_LIMIT.as_secs() / 60)),
        }
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(stream: R, tx: mpsc::Sender<String>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&line).trim().to_string();
            if !line.is_empty() && tx.send(line).is_err() {
                break;
            }
        }
    })
}

/// `npm` in the folder of `node` (a Windows install keeps `npm.cmd` beside `node.exe`).
fn npm_next_to(node: Option<&Path>) -> Option<PathBuf> {
    let dir = node?.parent()?;
    let names: &[&str] = if cfg!(windows) { &["npm.cmd", "npm.exe"] } else { &["npm"] };
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

fn clean_line(line: &str) -> String {
    let one: String = line.chars().filter(|c| !c.is_control()).collect();
    let one = one.trim();
    if one.chars().count() > MAX_LINE_CHARS {
        let cut: String = one.chars().take(MAX_LINE_CHARS).collect();
        format!("{cut}…")
    } else {
        one.to_string()
    }
}

// ---- the pieces -------------------------------------------------------------------------------------

/// The check of the installed files against the one pin that gets installed.
fn check_installed(plan: &Plan, host: &dyn HostEnv) -> PinCheck {
    let pin = AdapterPin { package: plan.package.clone(), version: plan.version.clone(), entry: plan.entry.to_string_lossy().into_owned(), sha256: Some(plan.sha256.clone()) };
    pin::check(&[pin], host)
}

/// Runs the install for `plan` and verifies it. `on_line` gets progress sentences. On success the entry file is
/// the pinned one. On a hash mismatch the files stay where they are (and stay unused).
pub fn run_install(plan: &Plan, host: &dyn HostEnv, runner: &dyn Runner, on_line: &mut dyn FnMut(&str)) -> Result<String, InstallError> {
    let npm = runner.find_npm().ok_or(InstallError::NodeMissing)?;
    std::fs::create_dir_all(&plan.dir).map_err(|e| InstallError::Io(format!("{}: {e}", plan.dir.display())))?;
    let manifest = plan.dir.join("package.json");
    crate::settings_patch::write_atomic(&manifest, package_json(plan).as_bytes()).map_err(|e| InstallError::Io(format!("{}: {e}", manifest.display())))?;
    on_line(&format!("Downloading {} {}…", plan.package, plan.version));

    let args: Vec<String> = NPM_ARGS.iter().map(|a| a.to_string()).collect();
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut seen = |line: &str| {
        let line = clean_line(line);
        if line.is_empty() {
            return;
        }
        tail.push_back(line.clone());
        if tail.len() > 8 {
            tail.pop_front();
        }
        on_line(&line);
    };
    let outcome = runner.run(&npm, &args, &plan.dir, &mut seen);
    let tail_text = tail.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" | ");
    match outcome {
        Err(why) => return Err(InstallError::Npm { code: None, tail: why }),
        Ok(Some(0)) => {}
        Ok(code) => return Err(InstallError::Npm { code, tail: tail_text }),
    }

    on_line("Checking the download…");
    match check_installed(plan, host) {
        PinCheck::Ok { version } => Ok(version),
        PinCheck::HashMismatch { version, found, expected } => Err(InstallError::PinMismatch { version, found, expected }),
        other => Err(InstallError::Unusable(other.describe())),
    }
}

/// Where removed add-ons go: next to the adapters, in `.removed`.
pub fn removed_root(host: &dyn HostEnv) -> PathBuf {
    host.adapters_dir().parent().map(|p| p.join(".removed")).unwrap_or_else(|| host.adapters_dir().join(".removed"))
}

/// Moves the add-on's folder into `.removed`. Returns where it went, or `None` when there was nothing to move.
pub fn move_away(plan: &Plan, host: &dyn HostEnv) -> Result<Option<PathBuf>, InstallError> {
    move_dir_away(&plan.dir, host)
}

/// [`move_away`] for any folder under the adapters folder.
pub fn move_dir_away(dir: &Path, host: &dyn HostEnv) -> Result<Option<PathBuf>, InstallError> {
    if !dir.exists() {
        return Ok(None);
    }
    let root = removed_root(host);
    std::fs::create_dir_all(&root).map_err(|e| InstallError::CannotMove(format!("{}: {e}", root.display())))?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "addon".into());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut to = root.join(format!("{name}-{stamp}"));
    let mut n = 1;
    while to.exists() {
        to = root.join(format!("{name}-{stamp}-{n}"));
        n += 1;
    }
    std::fs::rename(dir, &to).map_err(|e| InstallError::CannotMove(format!("{e}. If a chat with this agent is open, close it first")))?;
    Ok(Some(to))
}

// ---- the manager: who is installing, what failed -----------------------------------------------------

#[derive(Debug, Clone)]
enum Live {
    Installing { lines: Vec<String> },
    Failed { reason: String },
}

/// Remembers, per agent, an install that is running or the last one that failed. What is installed is read from
/// the disk every time (`pin::check`), so a restart of the app forgets nothing that matters.
#[derive(Default)]
pub struct Addons {
    live: Mutex<BTreeMap<String, Live>>,
}

impl Addons {
    pub fn new() -> Self {
        Self::default()
    }

    /// The status of one agent now.
    pub fn status(&self, cfg: &AgentConfig, host: &dyn HostEnv) -> AddonStatus {
        let live = self.live.lock().get(&cfg.id).cloned();
        let mut status = AddonStatus {
            agent: cfg.id.clone(),
            label: cfg.label.clone(),
            state: AddonState::NotInstalled,
            version: None,
            pinned_version: cfg.pins.last().map(|p| p.version.clone()),
            detail: String::new(),
            size_mb: cfg.addon.size_mb,
            lines: Vec::new(),
            signin_hint: cfg.addon.signin_hint.clone(),
        };
        if let Some(Live::Installing { lines }) = &live {
            status.state = AddonState::Installing;
            status.lines = lines.clone();
            return status;
        }
        match pin::check(&cfg.pins, host) {
            PinCheck::Ok { version } => {
                status.state = AddonState::Installed;
                status.version = Some(version);
            }
            PinCheck::NotPinned => status.detail = "Nothing to install.".into(),
            PinCheck::Missing { .. } => {
                if let Some(Live::Failed { reason }) = live {
                    status.state = AddonState::Failed;
                    status.detail = reason;
                }
            }
            other => {
                status.state = AddonState::Failed;
                status.detail = match live {
                    Some(Live::Failed { reason }) => reason,
                    _ => other.describe(),
                };
            }
        }
        status
    }

    pub fn is_installing(&self, agent_id: &str) -> bool {
        matches!(self.live.lock().get(agent_id), Some(Live::Installing { .. }))
    }

    /// Marks the install as started; `Busy` when one is already running.
    pub fn begin(&self, agent_id: &str) -> Result<(), InstallError> {
        let mut live = self.live.lock();
        if matches!(live.get(agent_id), Some(Live::Installing { .. })) {
            return Err(InstallError::Busy);
        }
        live.insert(agent_id.to_string(), Live::Installing { lines: Vec::new() });
        Ok(())
    }

    fn push_line(&self, agent_id: &str, line: &str) {
        let mut live = self.live.lock();
        if let Some(Live::Installing { lines }) = live.get_mut(agent_id) {
            lines.push(line.to_string());
            if lines.len() > MAX_LINES {
                lines.remove(0);
            }
        }
    }

    fn finish(&self, agent_id: &str, result: &Result<String, InstallError>) {
        let mut live = self.live.lock();
        match result {
            Ok(_) => {
                live.remove(agent_id);
            }
            Err(error) => {
                live.insert(agent_id.to_string(), Live::Failed { reason: error.to_string() });
            }
        }
    }

    /// The whole install, blocking: `begin` must have been called. `changed` is called after each progress line and
    /// at the end (the live app turns it into an event). Returns the installed version.
    pub fn install_blocking(&self, cfg: &AgentConfig, host: &dyn HostEnv, runner: &dyn Runner, changed: &dyn Fn()) -> Result<String, InstallError> {
        let result = self.install_inner(cfg, host, runner, changed);
        self.finish(&cfg.id, &result);
        changed();
        result
    }

    fn install_inner(&self, cfg: &AgentConfig, host: &dyn HostEnv, runner: &dyn Runner, changed: &dyn Fn()) -> Result<String, InstallError> {
        let plan = plan(cfg, host)?;
        // A folder that is there but is not the pinned adapter (an older version, or a changed file) is moved
        // aside first, so the new install starts clean. A folder that is fine is left for npm to confirm.
        if !matches!(check_installed(&plan, host), PinCheck::Ok { .. } | PinCheck::Missing { .. }) {
            move_away(&plan, host)?;
        }
        let id = cfg.id.clone();
        let mut on_line = |line: &str| {
            self.push_line(&id, line);
            changed();
        };
        run_install(&plan, host, runner, &mut on_line)
    }

    /// Runs `job` as the install of `agent_id` (`begin` must have been called): its progress lines are kept for the
    /// screen, `changed` is called after each line and at the end. Used by the registry install, which has no
    /// definition table to plan from.
    pub fn run_job(&self, agent_id: &str, changed: &dyn Fn(), job: &mut dyn FnMut(&mut dyn FnMut(&str)) -> Result<String, InstallError>) -> Result<String, InstallError> {
        let mut on_line = |line: &str| {
            self.push_line(agent_id, line);
            changed();
        };
        let result = job(&mut on_line);
        self.finish(agent_id, &result);
        changed();
        result
    }

    /// What `agent_id` is doing now: `Installing` with the newest lines, `Failed` with the reason, else `None`.
    pub fn live_state(&self, agent_id: &str) -> Option<(AddonState, Vec<String>, String)> {
        match self.live.lock().get(agent_id)? {
            Live::Installing { lines } => Some((AddonState::Installing, lines.clone(), String::new())),
            Live::Failed { reason } => Some((AddonState::Failed, Vec::new(), reason.clone())),
        }
    }

    /// Moves the add-on away. `Ok(None)`: nothing was installed.
    pub fn remove(&self, cfg: &AgentConfig, host: &dyn HostEnv) -> Result<Option<PathBuf>, InstallError> {
        if self.is_installing(&cfg.id) {
            return Err(InstallError::Busy);
        }
        let plan = plan(cfg, host)?;
        let moved = move_away(&plan, host)?;
        self.live.lock().remove(&cfg.id);
        Ok(moved)
    }
}

pub fn addons() -> &'static Addons {
    static ADDONS: std::sync::OnceLock<Addons> = std::sync::OnceLock::new();
    ADDONS.get_or_init(Addons::new)
}

/// Whether starting a chat for `cfg` would fail only because the add-on is not there. The start path asks this first
/// so it can say "install it" instead of "could not start".
pub fn is_missing(cfg: &AgentConfig, host: &dyn HostEnv) -> bool {
    cfg.pin_policy != PinPolicy::Off && matches!(pin::check(&cfg.pins, host), PinCheck::Missing { .. })
}

#[cfg(test)]
mod tests;
