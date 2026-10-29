//! The live probe (spec section 3.5): finds `wsl.exe`, runs `--list --verbose`, `--status` and the
//! per-distro probe through a `Runner`, and caches the answers. Only `check` and `facts` start a
//! distro (a person's click or a Linux spawn). `status` only lists and never starts a distro (S6).
//! Every process starts through `proc_util::hidden_command`. The live runner passes only the
//! `wsl.exe` flags in `ALLOWED_FLAGS` and kills its own child on timeout.

use crate::operations::OperationError;
use crate::proc_util::hidden_command;
use crate::tui::{descriptors, linux_program};
use crate::wsl::model::{
    DistroFacts, DistroName, WslStatus, LIST_TIMEOUT_MS, OUTPUT_CAP, PROBE_TIMEOUT_MS, WSL_DISTRO_MISSING,
    WSL_NOT_INSTALLED, WSL_PROBE_TIMEOUT, WSL_START_FAILED,
};
use crate::wsl::parse;
use crate::wsl::place::PlaceProbe;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The `wsl.exe` flags this module passes. Any other argument that starts with a dash is refused
/// before a process starts (S5). The probe and stop scripts reach Linux only after `--exec`.
const ALLOWED_FLAGS: &[&str] = &["-d", "-c", "--exec", "--list", "--verbose", "--status"];

/// The line shown for a status that timed out. Text that `wsl.exe` prints is never shown.
pub const BROKEN_LINE: &str = "WSL did not answer in time.";

/// The longest a finished child's output is still waited for (its pipe can stay open).
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_EVERY: Duration = Duration::from_millis(20);

/// Why one `wsl.exe` call failed. Nothing here is shown raw to the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The call did not finish in time. Its child was killed.
    Timeout,
    /// The process did not start (no `wsl.exe`, access denied). The OS text, for the log only.
    Start(String),
    /// The process ended with a failure status. The exit code when there is one.
    Failed(Option<i32>),
    /// An argument is not in `ALLOWED_FLAGS`. Nothing started.
    Refused,
}

/// One `wsl.exe` call: the arguments in, the captured stdout out (at most `OUTPUT_CAP` bytes).
pub trait Runner {
    fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, RunError>;
}

/// `%SystemRoot%\System32\wsl.exe` when it exists. Under feature `dev-fake-tui` only,
/// `PM_FAKE_WSL_EXE` wins (and is `None` when that file is missing).
pub fn wsl_exe() -> Option<PathBuf> {
    if let Some(fake) = dev_override() {
        return fake.is_file().then_some(fake);
    }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    exe_under(&root)
}

fn exe_under(system_root: &Path) -> Option<PathBuf> {
    let exe = system_root.join("System32").join("wsl.exe");
    exe.is_file().then_some(exe)
}

#[cfg(feature = "dev-fake-tui")]
fn dev_override() -> Option<PathBuf> {
    std::env::var_os("PM_FAKE_WSL_EXE").map(PathBuf::from)
}

#[cfg(not(feature = "dev-fake-tui"))]
fn dev_override() -> Option<PathBuf> {
    None
}

/// The real runner: the `wsl.exe` at `exe`, with `WSL_UTF8=1`, no stdin, stderr dropped and stdout
/// capped at `OUTPUT_CAP`.
pub struct LiveRunner {
    exe: PathBuf,
}

impl LiveRunner {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe }
    }
}

impl Runner for LiveRunner {
    fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, RunError> {
        if refused(args) {
            return Err(RunError::Refused);
        }
        let mut child = hidden_command(&self.exe)
            .args(args)
            .env("WSL_UTF8", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| RunError::Start(e.to_string()))?;
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::Start("no output pipe".to_string()));
        };
        // The reader keeps reading to the end (so the child never blocks on a full pipe) but keeps
        // only the first `OUTPUT_CAP` bytes.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut kept: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let room = OUTPUT_CAP - kept.len();
                        kept.extend_from_slice(&chunk[..n.min(room)]);
                    }
                }
            }
            let _ = tx.send(kept);
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_EVERY),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout);
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Start(e.to_string()));
                }
            }
        };
        let output = rx.recv_timeout(DRAIN_TIMEOUT).unwrap_or_default();
        if status.success() {
            Ok(output)
        } else {
            Err(RunError::Failed(status.code()))
        }
    }
}

/// True when `args` holds a dash argument that is not in `ALLOWED_FLAGS`.
fn refused(args: &[String]) -> bool {
    args.iter().any(|arg| arg.starts_with('-') && !ALLOWED_FLAGS.contains(&arg.as_str()))
}

fn list_args() -> Vec<String> {
    vec!["--list".to_string(), "--verbose".to_string()]
}

/// What the Settings page shows for the whole WSL install. `--list` first. `--status` runs only when
/// `--list` failed, to tell "not installed" from "installed, but no usable distro".
pub fn status(runner: &dyn Runner) -> WslStatus {
    let timeout = Duration::from_millis(LIST_TIMEOUT_MS);
    match runner.run(&list_args(), timeout) {
        Ok(bytes) => {
            let distros = parse::list(&parse::decode(&bytes));
            if distros.is_empty() {
                no_distro()
            } else {
                WslStatus::Ready { distros }
            }
        }
        Err(RunError::Timeout) => broken(),
        Err(_) => match runner.run(&["--status".to_string()], timeout) {
            Ok(_) => no_distro(),
            Err(RunError::Timeout) => broken(),
            Err(_) => WslStatus::not_installed(),
        },
    }
}

fn no_distro() -> WslStatus {
    WslStatus::no_distro()
}

fn broken() -> WslStatus {
    WslStatus::Broken { line: BROKEN_LINE.to_string() }
}

/// The facts for one distro: one probe call (it boots the distro). `now` is the time to record.
fn probe_distro(
    runner: &dyn Runner,
    distro: &str,
    programs: &[String],
    now: u64,
) -> Result<DistroFacts, OperationError> {
    let name = DistroName::parse(distro)
        .ok_or_else(|| OperationError::new(WSL_DISTRO_MISSING, "That distro name is not valid.", false))?;
    let nonce = new_nonce();
    let mut args = vec!["-d".to_string(), name.as_str().to_string()];
    args.extend(parse::probe_args(&nonce, programs).map_err(|_| start_failed())?);
    let bytes = runner.run(&args, Duration::from_millis(PROBE_TIMEOUT_MS)).map_err(run_failure)?;
    let mut facts = parse::probe(&parse::decode(&bytes), &nonce, programs).map_err(|_| start_failed())?;
    facts.distro = name.as_str().to_string();
    facts.checked_at_ms = now;
    Ok(facts)
}

fn run_failure(error: RunError) -> OperationError {
    match error {
        RunError::Timeout => {
            OperationError::new(WSL_PROBE_TIMEOUT, "The distro did not answer in time. Try again.", true)
        }
        RunError::Start(_) => OperationError::new(WSL_NOT_INSTALLED, "WSL is not available on this PC.", false),
        RunError::Failed(_) | RunError::Refused => start_failed(),
    }
}

fn start_failed() -> OperationError {
    OperationError::new(WSL_START_FAILED, "The distro did not start, or its answer was not usable.", true)
}

/// Cached answers. `now` and `max_age` are milliseconds; a time later than `now` never counts as fresh.
#[derive(Default)]
pub struct Cache {
    list: Option<(u64, WslStatus)>,
    facts: HashMap<String, DistroFacts>,
}

impl Cache {
    /// The status through `runner`, reusing a listing no older than `max_age`.
    pub fn status(&mut self, runner: &dyn Runner, now: u64, max_age: u64) -> WslStatus {
        if let Some(hit) = self.cached_status(now, max_age) {
            return hit;
        }
        let status = status(runner);
        self.store_status(now, &status);
        status
    }

    /// The facts through `runner`, reusing them when fresh and when they cover every program asked.
    pub fn facts(
        &mut self,
        runner: &dyn Runner,
        distro: &str,
        programs: &[String],
        now: u64,
        max_age: u64,
    ) -> Result<DistroFacts, OperationError> {
        if let Some(hit) = self.cached_facts(distro, programs, now, max_age) {
            return Ok(hit);
        }
        let result = probe_distro(runner, distro, programs, now);
        self.store_facts(distro, &result);
        result
    }

    /// A fresh probe, always. A failed one drops the cached facts of that distro.
    pub fn check(
        &mut self,
        runner: &dyn Runner,
        distro: &str,
        programs: &[String],
        now: u64,
    ) -> Result<DistroFacts, OperationError> {
        let result = probe_distro(runner, distro, programs, now);
        self.store_facts(distro, &result);
        result
    }

    /// Forget the facts of one distro (a pane exited at once, say).
    pub fn invalidate(&mut self, distro: &str) {
        self.facts.remove(distro);
    }

    fn cached_status(&self, now: u64, max_age: u64) -> Option<WslStatus> {
        let (at, status) = self.list.as_ref()?;
        fresh(*at, now, max_age).then(|| status.clone())
    }

    fn store_status(&mut self, now: u64, status: &WslStatus) {
        self.list = Some((now, status.clone()));
    }

    fn cached_facts(&self, distro: &str, programs: &[String], now: u64, max_age: u64) -> Option<DistroFacts> {
        let facts = self.facts.get(distro)?;
        let covered = programs.iter().all(|program| facts.programs.contains_key(program));
        (covered && fresh(facts.checked_at_ms, now, max_age)).then(|| facts.clone())
    }

    fn store_facts(&mut self, distro: &str, result: &Result<DistroFacts, OperationError>) {
        match result {
            Ok(facts) => {
                self.facts.insert(distro.to_string(), facts.clone());
            }
            Err(_) => {
                self.facts.remove(distro);
            }
        }
    }
}

fn fresh(at: u64, now: u64, max_age: u64) -> bool {
    at <= now && now - at <= max_age
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

/// The one cache of this process. The lock is held only for reads and writes, never for a call.
fn lock() -> MutexGuard<'static, Cache> {
    CACHE.get_or_init(|| Mutex::new(Cache::default())).lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The process-wide cache as a `PlaceProbe` (what `place::check_run` and the pane spawn read).
pub struct LiveCache;

static LIVE: LiveCache = LiveCache;

/// The one live cache. It answers from the cache and runs `wsl.exe` only on a miss.
pub fn cache() -> &'static LiveCache {
    &LIVE
}

impl PlaceProbe for LiveCache {
    fn status(&self, max_age_ms: u64) -> WslStatus {
        status_cached(max_age_ms)
    }

    fn facts(&self, distro: &str, max_age_ms: u64) -> Result<DistroFacts, OperationError> {
        facts(distro, max_age_ms)
    }
}

/// The distro list. Served from the cache when no older than `max_age` ms; otherwise `wsl.exe --list`
/// runs (it never starts a distro). Always `NotWindows` on other systems.
pub fn status_cached(max_age: u64) -> WslStatus {
    if !cfg!(windows) {
        return WslStatus::NotWindows;
    }
    let Some(exe) = wsl_exe() else {
        return WslStatus::not_installed();
    };
    let now = now_ms();
    if let Some(hit) = lock().cached_status(now, max_age) {
        return hit;
    }
    let status = status(&LiveRunner::new(exe));
    lock().store_status(now, &status);
    status
}

/// The facts for `distro`: the cache when fresh (`max_age` ms) and covering `asked_programs()`, else
/// one probe, which boots the distro.
pub fn facts(distro: &str, max_age: u64) -> Result<DistroFacts, OperationError> {
    let programs = asked_programs();
    let now = now_ms();
    if let Some(hit) = lock().cached_facts(distro, &programs, now, max_age) {
        return Ok(hit);
    }
    let runner = live_runner()?;
    let result = probe_distro(&runner, distro, &programs, now);
    lock().store_facts(distro, &result);
    result
}

/// A fresh probe for `distro` (the Check button, or a retry). Boots the distro. A failure drops the
/// cached facts.
pub fn check(distro: &str, programs: &[String]) -> Result<DistroFacts, OperationError> {
    let runner = live_runner()?;
    let result = probe_distro(&runner, distro, programs, now_ms());
    lock().store_facts(distro, &result);
    result
}

/// Forget the cached facts of one distro.
pub fn invalidate(distro: &str) {
    lock().invalidate(distro);
}

/// The programs to look for: `tui::linux_program` of every registered profile that has one.
pub fn asked_programs() -> Vec<String> {
    let mut programs: Vec<String> = descriptors().iter().filter_map(|d| linux_program(d.id)).collect();
    programs.sort();
    programs.dedup();
    programs
}

fn live_runner() -> Result<LiveRunner, OperationError> {
    wsl_exe()
        .map(LiveRunner::new)
        .ok_or_else(|| OperationError::new(WSL_NOT_INSTALLED, "WSL is not installed on this PC.", false))
}

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)).unwrap_or(0)
}

/// A probe nonce that is unique per call (`[a-f0-9p]`, well under 64 characters).
fn new_nonce() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("pm{:x}{:x}{:x}", std::process::id(), now_ms(), n)
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
