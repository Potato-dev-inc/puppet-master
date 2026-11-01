//! Start and stop of the sandbox: lazy start on first use, idle stop, and the readiness checks.
//!
//! - Start is lazy: nothing runs until a screen action asks (`ensure_ready`). A start is, in order: a hidden
//!   holder process keeps the distro up; the guest's boot hook (`pm-sbx-boot.sh`: WSLg cut, network walls,
//!   proof of the walls) finishes and writes `/run/pm-sbx/boot.done` (or `boot.failed` with its reason; the
//!   app waits up to about 90 s and does not run the walls itself); the app's secrets for this run (the control
//!   token and the two VNC passwords, `secrets.rs`) are made here and pushed in over stdin as root (they live
//!   in the distro's memory file system and in this process, nowhere on Windows); one script starts the
//!   desktop and the control service as the `pm` user; the checks pass. If any check does not, the start
//!   fails with the reason and the distro is stopped again.
//! - Idle stop: after N minutes (default 15, the setting) without use, `idle_check` stops only our own
//!   distro with `wsl --terminate <our name>`. Nothing is stopped while a `UseGuard` is alive.
//! - A start always begins from a stopped distro: a distro that is running without our holder (the app
//!   restarted) has a control service with a token this process never saw, so it is terminated first.
//!
//! The clock is passed in (`now_ms`), as in the brain keeper. Waiting is by counted polls, so tests run
//! with `Timing::instant()`.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::control::{ControlClient, DEFAULT_PORT};
use super::secrets::{RunSecrets, ScreenSecrets};
use super::wsl::{GuestUser, Holder, SandboxName, Wsl, WslError};

/// Reads of the guest in a row that may fail before the boot wait gives up (each also uses one poll).
pub const MAX_READ_ERRORS: u32 = 5;
pub const DEFAULT_IDLE_MINUTES: u32 = 15;
pub const MIN_IDLE_MINUTES: u32 = 1;
pub const MAX_IDLE_MINUTES: u32 = 240;
/// Where the guest files live inside the distro (see `install.rs`).
pub const GUEST_SHARE: &str = "/usr/local/share/pm-sbx";
/// The noVNC port inside the distro (view only).
pub const DEFAULT_VIEW_PORT: u16 = 6080;
/// The one script that starts (and stops) the desktop, the VNC servers and the control service. Runs as `pm`.
pub const START_DESKTOP: &str = "/usr/local/bin/pm-sbx-start-desktop";

pub fn clamp_idle_minutes(minutes: u32) -> u32 {
    minutes.clamp(MIN_IDLE_MINUTES, MAX_IDLE_MINUTES)
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub poll: Duration,
    /// How many polls to wait for the boot hook (the proof of the walls alone takes about 14 s).
    pub boot_polls: u32,
    /// How many polls to wait for the desktop and the control service.
    pub ready_polls: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Self { poll: Duration::from_millis(500), boot_polls: 190, ready_polls: 60 }
    }
}

impl Timing {
    /// For tests: no waiting, a few polls.
    pub fn instant() -> Self {
        Self { poll: Duration::ZERO, boot_polls: 3, ready_polls: 3 }
    }
}

// ---- readiness -----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Egress {
    Passed,
    Failed(String),
    /// No status file: the probe has not run.
    Unknown,
}

/// What the guest says about itself (see `readiness_script`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readiness {
    pub boot_done: bool,
    pub boot_failed: bool,
    /// The `reason=` line of `/run/pm-sbx/boot.failed`.
    pub boot_reason: String,
    pub egress: Egress,
    pub desktop_up: bool,
    pub control_up: bool,
}

/// Why the sandbox is not ready, in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotReady(pub String);

impl Readiness {
    /// A reason to stop waiting: the guest says it will not get there.
    pub fn fatal(&self) -> Option<NotReady> {
        if let Egress::Failed(reason) = &self.egress {
            let why = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
            return Some(NotReady(format!("The sandbox could not prove its network walls hold{why}, so it will not start.")));
        }
        if self.boot_failed {
            let why = if self.boot_reason.is_empty() { String::new() } else { format!(" ({})", self.boot_reason) };
            return Some(NotReady(format!("The sandbox's start-up safety check failed{why}, so it will not start.")));
        }
        None
    }

    /// Ready to use: boot done, walls proved, screen and control service listening.
    pub fn ready(&self) -> bool {
        self.boot_done && self.egress == Egress::Passed && self.desktop_up && self.control_up && !self.boot_failed
    }

    /// The first thing still missing, for the error card.
    pub fn missing(&self) -> NotReady {
        if let Some(fatal) = self.fatal() {
            return fatal;
        }
        let what = if !self.boot_done {
            "The sandbox did not finish starting."
        } else if self.egress != Egress::Passed {
            "The sandbox has not proved its network walls yet."
        } else if !self.desktop_up {
            "The sandbox screen did not come up."
        } else {
            "The sandbox control service did not come up."
        };
        NotReady(what.to_string())
    }
}

/// The script (POSIX sh, as root, over stdin) that prints `key=value` lines about the guest. The boot verdict is one
/// pair of files, `/run/pm-sbx/boot.done` and `boot.failed`, written by the guest's boot hook; nothing else is read for it.
pub fn readiness_script(view_port: u16, control_port: u16) -> String {
    format!(
        r#"listening() {{ ss -ltn 2>/dev/null | awk '{{print $4}}' | grep -Fqx "127.0.0.1:$1" && echo 1 || echo 0; }}
echo "boot_done=$( [ -e /run/pm-sbx/boot.done ] && echo 1 || echo 0 )"
echo "boot_failed=$( [ -e /run/pm-sbx/boot.failed ] && echo 1 || echo 0 )"
echo "boot_reason=$(sed -n 's/^reason=//p' /run/pm-sbx/boot.failed 2>/dev/null | head -1)"
echo "egress_state=$(sed -n 's/^state=//p' /run/pm-sbx/egress-probe.status 2>/dev/null | head -1)"
echo "egress_reason=$(sed -n 's/^reason=//p' /run/pm-sbx/egress-probe.status 2>/dev/null | head -1)"
echo "desktop_up=$(listening {view_port})"
echo "control_up=$(listening {control_port})"
"#
    )
}

pub fn parse_readiness(text: &str) -> Readiness {
    let get = |key: &str| -> String {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let egress = match get("egress_state").as_str() {
        "passed" => Egress::Passed,
        "failed" => Egress::Failed(crate::redact::text(&get("egress_reason")).into_inner().chars().take(120).collect()),
        _ => Egress::Unknown,
    };
    Readiness {
        boot_done: get("boot_done") == "1",
        boot_failed: get("boot_failed") == "1",
        boot_reason: crate::redact::text(&get("boot_reason")).into_inner().chars().filter(|c| !c.is_control()).take(120).collect(),
        egress,
        desktop_up: get("desktop_up") == "1",
        control_up: get("control_up") == "1",
    }
}

// ---- the idle rule (pure) ------------------------------------------------------------------------

/// True when a ready sandbox has been idle for `minutes` and nothing is using it.
pub fn should_idle_stop(ready: bool, last_active_ms: i64, now_ms: i64, minutes: u32, in_use: u32) -> bool {
    ready && in_use == 0 && now_ms.saturating_sub(last_active_ms) >= i64::from(clamp_idle_minutes(minutes)) * 60_000
}

// ---- errors and status ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    NotAvailable,
    NotInstalled,
    /// The start did not get to ready; the reason is plain English.
    NotReady(String),
    /// A visible remote-desktop window appeared during the start.
    VisibleWindow,
    /// A Stop ended the start before it finished.
    Cancelled,
    Wsl(WslError),
}

impl LifecycleError {
    pub fn code(&self) -> &'static str {
        match self {
            LifecycleError::NotAvailable => "SANDBOX_NO_WSL",
            LifecycleError::NotInstalled => "SANDBOX_NOT_INSTALLED",
            LifecycleError::NotReady(_) => "SANDBOX_NOT_READY",
            LifecycleError::VisibleWindow => "SANDBOX_VISIBLE_WINDOW",
            LifecycleError::Cancelled => "SANDBOX_START_CANCELLED",
            LifecycleError::Wsl(_) => "SANDBOX_LINUX_CALL_FAILED",
        }
    }

    pub fn message(&self) -> String {
        match self {
            LifecycleError::NotAvailable => "Needs Windows' Linux feature (admin + restart).".to_string(),
            LifecycleError::NotInstalled => "The sandbox is not set up yet.".to_string(),
            LifecycleError::NotReady(why) => why.clone(),
            LifecycleError::VisibleWindow => "A window appeared on screen while the sandbox started, so it was stopped.".to_string(),
            LifecycleError::Cancelled => "The sandbox was stopped before it finished starting.".to_string(),
            LifecycleError::Wsl(WslError::NotAvailable) => "Needs Windows' Linux feature (admin + restart).".to_string(),
            LifecycleError::Wsl(WslError::Timeout) => "The sandbox took too long to answer.".to_string(),
            LifecycleError::Wsl(_) => "A Linux call for the sandbox failed.".to_string(),
        }
    }
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for LifecycleError {}

impl From<WslError> for LifecycleError {
    fn from(e: WslError) -> Self {
        match e {
            WslError::NotAvailable => LifecycleError::NotAvailable,
            WslError::Cancelled => LifecycleError::Cancelled,
            other => LifecycleError::Wsl(other),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Stopped,
    Starting,
    Ready,
    Failed(String),
}

/// Looks for a visible remote-desktop window (the live check is `EnumWindows` for `msrdc`; it lands with
/// the live tests). A start that sees one is stopped.
pub trait WindowCheck: Send + Sync {
    fn visible_remote_window(&self) -> bool;
}

pub struct NoWindowCheck;

impl WindowCheck for NoWindowCheck {
    fn visible_remote_window(&self) -> bool {
        false
    }
}

/// What a caller gets once the sandbox is ready.
#[derive(Clone, Debug)]
pub struct Ready {
    pub client: ControlClient,
    /// Bumped at every start and stop; screen links carry it and die when it changes.
    pub generation: u64,
}

struct Inner {
    phase: Phase,
    holder: Option<Box<dyn Holder>>,
    secrets: Option<Arc<RunSecrets>>,
    last_active_ms: i64,
    /// Until when the person holds the screen (a live link or lease). Counts as use for the idle rule.
    person_until_ms: i64,
    idle_minutes: u32,
}

pub struct Lifecycle {
    wsl: Wsl,
    /// The same Wsl, whose calls die when `cancel_start` turns true. Only a start uses it; a stop and the clean-up
    /// after a failed or cancelled start use `wsl`, which nothing cancels.
    start_wsl: Wsl,
    /// True while a Stop is waiting for (or running behind) a start: the start's waits and calls end at once.
    cancel_start: Arc<AtomicBool>,
    stops_pending: AtomicU32,
    name: SandboxName,
    timing: Timing,
    control_port: u16,
    view_port: u16,
    window_check: Arc<dyn WindowCheck>,
    inner: Mutex<Inner>,
    start_lock: Mutex<()>,
    in_use: Arc<AtomicU32>,
    generation: AtomicU64,
}

/// While one exists, the idle stop does not run.
pub struct UseGuard {
    in_use: Arc<AtomicU32>,
}

impl Drop for UseGuard {
    fn drop(&mut self) {
        self.in_use.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Lifecycle {
    pub fn new(wsl: Wsl, name: SandboxName) -> Self {
        let cancel_start = Arc::new(AtomicBool::new(false));
        Self {
            start_wsl: wsl.with_cancel(cancel_start.clone()),
            cancel_start,
            stops_pending: AtomicU32::new(0),
            wsl,
            name,
            timing: Timing::default(),
            control_port: DEFAULT_PORT,
            view_port: DEFAULT_VIEW_PORT,
            window_check: Arc::new(NoWindowCheck),
            inner: Mutex::new(Inner {
                phase: Phase::Stopped,
                holder: None,
                secrets: None,
                last_active_ms: 0,
                person_until_ms: 0,
                idle_minutes: DEFAULT_IDLE_MINUTES,
            }),
            start_lock: Mutex::new(()),
            in_use: Arc::new(AtomicU32::new(0)),
            generation: AtomicU64::new(0),
        }
    }

    pub fn with_timing(mut self, timing: Timing) -> Self {
        self.timing = timing;
        self
    }

    pub fn with_ports(mut self, view_port: u16, control_port: u16) -> Self {
        self.view_port = view_port;
        self.control_port = control_port;
        self
    }

    pub fn with_window_check(mut self, check: Arc<dyn WindowCheck>) -> Self {
        self.window_check = check;
        self
    }

    pub fn name(&self) -> &SandboxName {
        &self.name
    }

    pub fn view_port(&self) -> u16 {
        self.view_port
    }

    /// The two VNC passwords of the run in progress (`None` when the sandbox is not ready). Handed out only
    /// through `Sandbox::screen_target`, in a type that prints nothing.
    pub fn screen_secrets(&self) -> Option<Arc<ScreenSecrets>> {
        let inner = self.inner.lock();
        if inner.phase != Phase::Ready {
            return None;
        }
        inner.secrets.as_ref().map(|s| s.screen.clone())
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn phase(&self) -> Phase {
        self.inner.lock().phase.clone()
    }

    pub fn set_idle_minutes(&self, minutes: u32) {
        self.inner.lock().idle_minutes = clamp_idle_minutes(minutes);
    }

    pub fn idle_minutes(&self) -> u32 {
        self.inner.lock().idle_minutes
    }

    pub fn touch(&self, now_ms: i64) {
        self.inner.lock().last_active_ms = now_ms;
    }

    /// The person holds the screen (a live link or lease) until `until_ms`. While that runs the sandbox counts as in
    /// use; the idle minutes run from the later of the last bot use and this hold.
    pub fn hold_for_person(&self, until_ms: i64) {
        let mut inner = self.inner.lock();
        inner.person_until_ms = inner.person_until_ms.max(until_ms);
    }

    /// Test helper: holds the start lock (what a start or a stop takes) until the guard is dropped.
    #[cfg(test)]
    pub fn hold_starts_for_test(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.start_lock.lock()
    }

    pub fn begin_use(&self) -> UseGuard {
        self.in_use.fetch_add(1, Ordering::SeqCst);
        UseGuard { in_use: self.in_use.clone() }
    }

    fn read_readiness(&self) -> Result<Readiness, WslError> {
        let script = readiness_script(self.view_port, self.control_port);
        let out = self.start_wsl.exec(&self.name, GuestUser::Root, &["/bin/sh", "-s"], Some(script.into_bytes()), super::wsl::SHORT)?;
        Ok(parse_readiness(&out.stdout_text()))
    }

    /// The sandbox, started if it was not. Takes the start lock, so two callers start it once.
    pub fn ensure_ready(&self, now_ms: i64) -> Result<Ready, LifecycleError> {
        let _start = self.start_lock.lock();
        if self.start_cancelled() {
            // A Stop is queued behind us: it wins, nothing is started.
            return Err(LifecycleError::Cancelled);
        }
        if let Some(ready) = self.quick_ready(now_ms) {
            return Ok(ready);
        }
        self.inner.lock().phase = Phase::Starting;
        match self.start_inner(now_ms) {
            Ok(ready) => Ok(ready),
            Err(error) => {
                let _ = self.stop_inner();
                self.inner.lock().phase = if error == LifecycleError::Cancelled { Phase::Stopped } else { Phase::Failed(error.message()) };
                Err(error)
            }
        }
    }

    /// Already running and answering? Then no start.
    fn quick_ready(&self, now_ms: i64) -> Option<Ready> {
        let token = {
            let mut inner = self.inner.lock();
            if inner.phase != Phase::Ready {
                return None;
            }
            let alive = inner.holder.as_mut().is_some_and(|h| h.alive());
            if !alive {
                return None;
            }
            inner.secrets.as_ref()?.token.clone()
        };
        match self.read_readiness() {
            Ok(r) if r.ready() => {
                self.touch(now_ms);
                Some(Ready { client: ControlClient::new(self.control_port, token), generation: self.generation() })
            }
            _ => None,
        }
    }

    fn start_cancelled(&self) -> bool {
        self.cancel_start.load(Ordering::SeqCst)
    }

    fn poll(&self, polls: u32, mut done: impl FnMut(&Readiness) -> bool) -> Result<Readiness, LifecycleError> {
        let mut last = None;
        let mut errors = 0u32;
        for attempt in 0..polls.max(1) {
            if self.start_cancelled() {
                return Err(LifecycleError::Cancelled);
            }
            let r = match self.read_readiness() {
                Ok(r) => {
                    errors = 0;
                    r
                }
                // One slow or failed read (a first boot can be slow) does not end the wait: it counts as a poll.
                // Only a cancel, a missing wsl, or too many reads in a row that failed end it.
                Err(error @ (WslError::Cancelled | WslError::NotAvailable | WslError::Refused(_))) => return Err(error.into()),
                Err(error) => {
                    errors += 1;
                    if errors >= MAX_READ_ERRORS {
                        return Err(error.into());
                    }
                    if attempt + 1 < polls {
                        std::thread::sleep(self.timing.poll);
                    }
                    continue;
                }
            };
            if let Some(fatal) = r.fatal() {
                return Err(LifecycleError::NotReady(fatal.0));
            }
            if done(&r) {
                return Ok(r);
            }
            last = Some(r);
            if attempt + 1 < polls {
                std::thread::sleep(self.timing.poll);
            }
        }
        Err(LifecycleError::NotReady(last.map(|r| r.missing().0).unwrap_or_else(|| "The sandbox did not answer.".to_string())))
    }

    fn start_inner(&self, now_ms: i64) -> Result<Ready, LifecycleError> {
        if !self.start_wsl.is_available() {
            return Err(LifecycleError::NotAvailable);
        }
        if !self.start_wsl.is_installed(&self.name)? {
            return Err(LifecycleError::NotInstalled);
        }
        // A clean start: a distro left running by an earlier app run has a service whose token we never saw.
        if self.start_wsl.is_running(&self.name)? {
            self.start_wsl.terminate(&self.name)?;
        }
        let holder = self.start_wsl.spawn_holder(&self.name)?;
        self.inner.lock().holder = Some(holder);

        // 1. the boot hook: it cuts WSLg, sets the network walls and proves them, then writes boot.done (or
        //    boot.failed with its reason, which `poll` turns into the error). The app waits; it runs no wall itself.
        self.poll(self.timing.boot_polls, |r| r.boot_done)?;

        // 2. this run's secrets, pushed in as root over stdin (never an argument, never a Windows file). Nothing
        //    is handed to a sandbox whose boot did not finish clean.
        let secrets = Arc::new(RunSecrets::generate());
        let (push, stdin) = secrets.push();
        self.start_wsl.exec(&self.name, GuestUser::Root, &["/bin/sh", "-c", &push], Some(stdin), super::wsl::SHORT)?.ok()?;

        // 3. the desktop, the VNC servers and the control service: ONE script, as the pm user. It uses the pushed files.
        let desktop = self.start_wsl.exec(&self.name, GuestUser::Pm, &[START_DESKTOP, "start"], None, Duration::from_secs(120))?;
        if !desktop.success() {
            return Err(LifecycleError::NotReady("The sandbox screen did not start.".to_string()));
        }

        // 4. everything answers
        self.poll(self.timing.ready_polls, |r| r.ready())?;

        if self.window_check.visible_remote_window() {
            return Err(LifecycleError::VisibleWindow);
        }

        self.generation.fetch_add(1, Ordering::SeqCst);
        let client = ControlClient::new(self.control_port, secrets.token.clone());
        let mut inner = self.inner.lock();
        inner.secrets = Some(secrets);
        inner.phase = Phase::Ready;
        inner.last_active_ms = now_ms;
        Ok(Ready { client, generation: self.generation() })
    }

    /// Stops our distro (and only ours). Safe when it is not running.
    pub fn stop(&self) -> Result<(), LifecycleError> {
        // Say so first, without the lock: a start holding it ends within a poll, then this stop runs.
        self.stops_pending.fetch_add(1, Ordering::SeqCst);
        self.cancel_start.store(true, Ordering::SeqCst);
        let result = {
            let _start = self.start_lock.lock();
            let result = self.stop_inner();
            self.inner.lock().phase = Phase::Stopped;
            result
        };
        if self.stops_pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.cancel_start.store(false, Ordering::SeqCst);
        }
        result.map_err(LifecycleError::from)
    }

    /// True while a Stop is waiting for a start to end, or running ("Stopping...").
    pub fn stopping(&self) -> bool {
        self.stops_pending.load(Ordering::SeqCst) > 0
    }

    fn stop_inner(&self) -> Result<(), WslError> {
        let holder = self.inner.lock().holder.take();
        self.generation.fetch_add(1, Ordering::SeqCst);
        if self.wsl.is_installed(&self.name)? && self.wsl.is_running(&self.name)? {
            // The desktop first (the guest script stops Chrome, the services and the display), then the distro. The
            // terminate below ends everything anyway, so a script that fails or times out changes nothing.
            let _ = self.wsl.exec(&self.name, GuestUser::Pm, &[START_DESKTOP, "stop"], None, super::wsl::SHORT);
        }
        {
            let mut inner = self.inner.lock();
            if let Some(mut holder) = holder {
                holder.stop();
            }
            inner.secrets = None;
        }
        if self.wsl.is_installed(&self.name)? && self.wsl.is_running(&self.name)? {
            self.wsl.terminate(&self.name)?;
        }
        Ok(())
    }

    /// Stops the sandbox when it has been idle long enough. Returns true when it did.
    pub fn idle_check(&self, now_ms: i64) -> Result<bool, LifecycleError> {
        // The decision and the stop share the start lock. A start (and a bot action's `ensure_ready`) takes the same
        // lock, so a use that begins after an earlier decision is seen by the re-check here, and nothing is stopped
        // under a use that has begun.
        let _start = self.start_lock.lock();
        let due = {
            let inner = self.inner.lock();
            should_idle_stop(
                inner.phase == Phase::Ready,
                inner.last_active_ms.max(inner.person_until_ms),
                now_ms,
                inner.idle_minutes,
                self.in_use.load(Ordering::SeqCst),
            )
        };
        if !due {
            return Ok(false);
        }
        let result = self.stop_inner();
        self.inner.lock().phase = Phase::Stopped;
        result.map_err(LifecycleError::from)?;
        Ok(true)
    }
}
