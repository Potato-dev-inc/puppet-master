//! The ONE door to `wsl.exe` for the Potato Bot sandbox.
//!
//! Every call goes through `gate` (reached by `run`, `run_streaming` and `spawn_holder`). The gate
//!
//! - looks at the arguments and accepts only the few call shapes the sandbox needs (list, status, install,
//!   import, export, terminate, unregister, run-in-the-distro with `--exec`);
//! - accepts a distro name only when it is `^pm-sbx-[a-z0-9-]{1,40}$`, so the person's own distros can
//!   never be touched;
//! - refuses the flags in `REFUSED_FLAGS` (the one that stops every distro, the ones that change the
//!   default distro, update WSL or manage a distro), whatever else is in the call;
//! - lets only the one call that enables the Windows Linux feature run elevated, and only when the
//!   caller holds a `Consent` (made on the person's button press, never by install code);
//! - sends scripts over stdin, never through a shell string built from data.
//!
//! The real runner starts `wsl.exe` through `proc_util::hidden_command` (no window). A fake runner can be
//! injected, so everything above it is tested without WSL. This file never reads or writes the WSL
//! settings file of the person.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const NAME_PREFIX: &str = "pm-sbx-";
pub const MAX_NAME_SUFFIX: usize = 40;
/// The one distro image the sandbox installs.
pub const DISTRO_IMAGE: &str = "Ubuntu-24.04";
/// `wsl.exe` output is kept up to this many bytes (the rest is read and dropped).
pub const OUTPUT_CAP: usize = 8 * 1024 * 1024;
/// The biggest script or archive sent over stdin.
pub const STDIN_CAP: usize = 64 * 1024 * 1024;
/// Exit code of the feature install that means "worked, restart Windows".
pub const EXIT_RESTART_NEEDED: i32 = 3010;

pub const SHORT: Duration = Duration::from_secs(30);
pub const LONG: Duration = Duration::from_secs(30 * 60);

/// Words that are refused anywhere in a call, even in the argv of a program run inside the distro. The
/// first one stops every distro of the person.
pub const REFUSED_ANYWHERE: &[&str] = &["--shutdown", "--set-default", "--update", "--manage"];

/// More flags that wsl.exe itself must never be given (the part of the call before `--exec`).
pub const REFUSED_FLAGS: &[&str] = &[
    "-s",
    "--set-default-version",
    "--set-version",
    "--mount",
    "--unmount",
    "--import-in-place",
    "--debug-shell",
    "--system",
    "--uninstall",
];

/// The accounts a call may run as inside the distro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestUser {
    Root,
    Pm,
}

impl GuestUser {
    pub fn as_str(self) -> &'static str {
        match self {
            GuestUser::Root => "root",
            GuestUser::Pm => "pm",
        }
    }
}

/// What every program run as `pm` starts with: `wsl -u pm` injects the WSLg variables (`WAYLAND_DISPLAY`, `PULSE_SERVER`,
/// ...), so the sandbox's own display and runtime folder are set and the WSLg ones are removed. `Wsl::exec` puts this in front
/// of the program and the gate refuses a `pm` call that lacks it, so nothing run as `pm` can reach WSLg by forgetting it.
pub const PM_ENV: &[&str] = &[
    "/usr/bin/env",
    "-u",
    "WAYLAND_DISPLAY",
    "-u",
    "PULSE_SERVER",
    "-u",
    "XDG_SESSION_TYPE",
    "DISPLAY=:99",
    "XDG_RUNTIME_DIR=/tmp/pm-runtime",
];

/// `^pm-sbx-[a-z0-9-]{1,40}$`
pub fn valid_name(name: &str) -> bool {
    match name.strip_prefix(NAME_PREFIX) {
        Some(rest) => {
            (1..=MAX_NAME_SUFFIX).contains(&rest.len())
                && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        }
        None => false,
    }
}

/// A distro name that passed `valid_name`. The only kind of name the rest of the sandbox handles.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SandboxName(String);

impl SandboxName {
    pub fn new(name: &str) -> Result<Self, WslError> {
        if valid_name(name) {
            Ok(Self(name.to_string()))
        } else {
            Err(WslError::Refused("a sandbox name is pm-sbx- followed by 1 to 40 of a-z, 0-9 and -"))
        }
    }

    /// The name the app uses for the one sandbox a person has.
    pub fn main() -> Self {
        Self("pm-sbx-main".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SandboxName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a call did not give an answer. Text that `wsl.exe` printed is kept for the log, never shown raw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WslError {
    /// The gate refused the call. Nothing started.
    Refused(&'static str),
    /// There is no `wsl.exe` on this computer.
    NotAvailable,
    /// The call took too long. Its process was killed.
    Timeout,
    /// The process did not start (the OS text, for the log).
    Start(String),
    /// The caller's cancel flag was set: the process was killed (with its children) and nothing more ran.
    Cancelled,
    /// The process ended with a failure code. A short cleaned piece of what it printed.
    Failed { code: Option<i32>, detail: String },
}

impl std::fmt::Display for WslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WslError::Refused(why) => write!(f, "refused: {why}"),
            WslError::NotAvailable => f.write_str("Windows' Linux feature is not available"),
            WslError::Timeout => f.write_str("the Linux call took too long"),
            WslError::Cancelled => f.write_str("the Linux call was cancelled"),
            WslError::Start(e) => write!(f, "could not start wsl: {e}"),
            WslError::Failed { code, detail } => write!(f, "wsl failed (code {code:?}): {detail}"),
        }
    }
}

impl std::error::Error for WslError {}

/// What a finished call printed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// `wsl.exe`'s own messages are UTF-16; a program inside the distro prints UTF-8. Both read here.
    pub fn stdout_text(&self) -> String {
        decode_text(&self.stdout)
    }

    pub fn stderr_text(&self) -> String {
        decode_text(&self.stderr)
    }

    /// `Err(Failed)` unless the call exited with 0.
    pub fn ok(self) -> Result<Output, WslError> {
        if self.success() {
            Ok(self)
        } else {
            Err(WslError::Failed { code: self.code, detail: short_detail(&self) })
        }
    }
}

fn short_detail(out: &Output) -> String {
    let text = if out.stderr.is_empty() { out.stdout_text() } else { out.stderr_text() };
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    crate::redact::text(line).into_inner().chars().take(200).collect()
}

/// UTF-16LE (with or without a byte order mark) or UTF-8, whichever the bytes are. Carriage returns go.
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = bytes.starts_with(&[0xFF, 0xFE])
        || (bytes.len() >= 2 && {
            let pairs = bytes.len() / 2;
            bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count() * 2 > pairs
        });
    let text = if utf16 {
        let body = bytes.strip_prefix(&[0xFF, 0xFE][..]).unwrap_or(bytes);
        let units: Vec<u16> = body.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.replace(['\r', '\u{FEFF}'], "")
}

// ---- the call and the gate -----------------------------------------------------------------------

/// One request to `wsl.exe`: the arguments (argv, no shell), optional stdin, and a time limit.
#[derive(Clone, Debug)]
pub struct Call {
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    /// When set to true while the call runs, the runner kills the call's process tree and answers `Cancelled`.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Set only by `Wsl::enable_windows_feature`, which needs a `Consent`.
    consented: bool,
}

impl Call {
    pub fn new(args: &[&str]) -> Self {
        Self { args: args.iter().map(|a| a.to_string()).collect(), stdin: None, timeout: SHORT, cancel: None, consented: false }
    }

    pub fn from_args(args: Vec<String>) -> Self {
        Self { args, stdin: None, timeout: SHORT, cancel: None, consented: false }
    }

    pub fn stdin(mut self, bytes: Vec<u8>) -> Self {
        self.stdin = Some(bytes);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn cancel(mut self, flag: Option<Arc<AtomicBool>>) -> Self {
        self.cancel = flag;
        self
    }
}

/// What the gate understood a call to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verb {
    List,
    Status,
    /// The feature install: runs elevated, once, when the person pressed the button.
    EnableFeature,
    Install(SandboxName),
    Import(SandboxName),
    Export(SandboxName),
    Unregister(SandboxName),
    Terminate(SandboxName),
    Guest { name: SandboxName, user: GuestUser },
}

/// A call that passed the gate. Only the gate makes one; a runner just carries it out.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub cancel: Option<Arc<AtomicBool>>,
    pub verb: Verb,
}

impl Prepared {
    pub fn elevated(&self) -> bool {
        self.verb == Verb::EnableFeature
    }
}

/// Proof that the person pressed the button that enables Windows' Linux feature. It is made in one
/// place, by the command bound to that button; `guard_tests` fails when install or start code makes one.
pub struct Consent(());

impl Consent {
    pub fn the_person_pressed_the_button() -> Self {
        Consent(())
    }
}

fn plain(arg: &str) -> bool {
    !arg.is_empty() && !arg.contains(['\0', '\n', '\r'])
}

/// A path or other value in a flag position: not empty, no control characters, not a flag itself.
fn value(arg: &str) -> bool {
    plain(arg) && !arg.starts_with('-')
}

fn name_at(args: &[String], at: usize) -> Result<SandboxName, WslError> {
    SandboxName::new(args.get(at).map(String::as_str).unwrap_or(""))
}

/// Looks at the arguments and says what the call is, or why it is refused.
pub fn check_args(args: &[String]) -> Result<Verb, WslError> {
    let refuse = WslError::Refused;
    let first = args.first().map(String::as_str).ok_or(refuse("no arguments"))?;
    if args.iter().any(|a| !plain(a)) {
        return Err(refuse("an argument is empty or holds a control character"));
    }
    if args.iter().any(|a| REFUSED_ANYWHERE.contains(&a.as_str())) {
        return Err(refuse("this wsl flag is never used by the sandbox"));
    }
    // The other flags are checked only in the part wsl.exe itself reads: after `--exec` everything is the
    // program's own argv (`sh -s` is the program's `-s`, not wsl's).
    let own_end = args.iter().position(|a| a == "--exec").unwrap_or(args.len());
    if args[..own_end].iter().any(|a| REFUSED_FLAGS.contains(&a.as_str())) {
        return Err(refuse("this wsl flag is never used by the sandbox"));
    }
    let rest = &args[1..];
    match first {
        "--list" | "-l" => {
            let ok = rest.iter().all(|a| matches!(a.as_str(), "--verbose" | "-v" | "--quiet" | "-q" | "--running"));
            if ok {
                Ok(Verb::List)
            } else {
                Err(refuse("a list call takes only --verbose, --quiet and --running"))
            }
        }
        "--status" if rest.is_empty() => Ok(Verb::Status),
        "--install" => match rest {
            [flag] if flag == "--no-distribution" => Ok(Verb::EnableFeature),
            [image, n_flag, n, l_flag, dir, launch]
                if image == DISTRO_IMAGE
                    && n_flag == "--name"
                    && l_flag == "--location"
                    && launch == "--no-launch"
                    && value(dir) =>
            {
                Ok(Verb::Install(SandboxName::new(n)?))
            }
            _ => Err(refuse("only the Ubuntu 24.04 sandbox install and the feature install are allowed")),
        },
        "--import" => match rest {
            [_, dir, tar] if value(dir) && value(tar) => Ok(Verb::Import(name_at(rest, 0)?)),
            _ => Err(refuse("an import takes a sandbox name, a folder and an archive")),
        },
        "--export" => match rest {
            [_, tar] if value(tar) => Ok(Verb::Export(name_at(rest, 0)?)),
            _ => Err(refuse("an export takes a sandbox name and an archive")),
        },
        "--unregister" if rest.len() == 1 => Ok(Verb::Unregister(name_at(rest, 0)?)),
        "--terminate" | "-t" if rest.len() == 1 => Ok(Verb::Terminate(name_at(rest, 0)?)),
        "-d" | "--distribution" => {
            let name = name_at(rest, 0)?;
            let (user, tail) = match rest.get(1..).unwrap_or(&[]) {
                [u_flag, user, tail @ ..] if u_flag == "-u" || u_flag == "--user" => {
                    let user = match user.as_str() {
                        "root" => GuestUser::Root,
                        "pm" => GuestUser::Pm,
                        _ => return Err(refuse("a call runs as root or pm only")),
                    };
                    (user, tail)
                }
                tail => (GuestUser::Root, tail),
            };
            match tail {
                [exec, program, ..] if exec == "--exec" && program.starts_with('/') => {
                    let program_argv = &tail[1..];
                    let has_env = program_argv.len() > PM_ENV.len() && program_argv.iter().zip(PM_ENV).all(|(a, b)| a == b);
                    if user == GuestUser::Pm && !has_env {
                        return Err(refuse("a call as pm starts with the sandbox display and without the WSLg variables"));
                    }
                    Ok(Verb::Guest { name, user })
                }
                _ => Err(refuse("a call into the distro is --exec with an absolute program path")),
            }
        }
        _ => Err(refuse("this wsl call is not one the sandbox makes")),
    }
}

/// Something running that keeps a distro up (its stop is a kill of this one process).
pub trait Holder: Send {
    fn alive(&mut self) -> bool;
    fn stop(&mut self);
}

/// Carries out calls that passed the gate.
pub trait Runner: Send + Sync {
    /// Runs the call to its end (or its time limit). `on_line` gets each finished stdout line as it comes.
    fn run(&self, call: &Prepared, on_line: &mut dyn FnMut(&str)) -> Result<Output, WslError>;
    /// Starts the call and leaves it running.
    fn spawn(&self, call: &Prepared) -> Result<Box<dyn Holder>, WslError>;
}

fn gate(call: Call) -> Result<Prepared, WslError> {
    let verb = check_args(&call.args)?;
    if verb == Verb::EnableFeature && !call.consented {
        return Err(WslError::Refused("the feature install needs the person's button press"));
    }
    if call.stdin.is_some() && !matches!(verb, Verb::Guest { .. }) {
        return Err(WslError::Refused("only a call into the distro takes stdin"));
    }
    if call.stdin.as_ref().is_some_and(|s| s.len() > STDIN_CAP) {
        return Err(WslError::Refused("the stdin stream is too big"));
    }
    Ok(Prepared { args: call.args, stdin: call.stdin, timeout: call.timeout, cancel: call.cancel, verb })
}

/// The one function every `wsl.exe` call passes through (`run_streaming` and `spawn_holder` are the same
/// gate with a line callback, or with the process left running).
pub fn run(runner: &dyn Runner, call: Call) -> Result<Output, WslError> {
    run_streaming(runner, call, &mut |_| {})
}

pub fn run_streaming(runner: &dyn Runner, call: Call, on_line: &mut dyn FnMut(&str)) -> Result<Output, WslError> {
    runner.run(&gate(call)?, on_line)
}

pub fn spawn_holder(runner: &dyn Runner, call: Call) -> Result<Box<dyn Holder>, WslError> {
    let prepared = gate(call)?;
    if !matches!(prepared.verb, Verb::Guest { .. }) {
        return Err(WslError::Refused("only a call into the distro can be left running"));
    }
    runner.spawn(&prepared)
}

// ---- the typed helpers ---------------------------------------------------------------------------

/// One row of `wsl -l -v`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distro {
    pub name: String,
    /// The word `wsl.exe` printed (it is localized); use `Wsl::is_running` to know.
    pub state: String,
    pub version: u8,
    pub is_default: bool,
}

/// Rows of `wsl -l -v` (the header and any message line have no version number and are skipped).
pub fn parse_list(text: &str) -> Vec<Distro> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_default = trimmed.starts_with('*');
        let tokens: Vec<&str> = trimmed.trim_start_matches('*').split_whitespace().collect();
        if tokens.len() < 3 {
            continue;
        }
        let Ok(version) = tokens[tokens.len() - 1].parse::<u8>() else { continue };
        if !(1..=2).contains(&version) {
            continue;
        }
        rows.push(Distro { name: tokens[0].to_string(), state: tokens[1].to_string(), version, is_default });
    }
    rows
}

/// What `enable_windows_feature` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureResult {
    pub restart_needed: bool,
}

/// The typed face of the gate: what the rest of the sandbox calls.
#[derive(Clone)]
pub struct Wsl {
    runner: Arc<dyn Runner>,
    /// When set, every call made through this handle is killed (with its process tree) once the flag turns true.
    cancel: Option<Arc<AtomicBool>>,
}

impl Wsl {
    pub fn new(runner: Arc<dyn Runner>) -> Self {
        Self { runner, cancel: None }
    }

    /// The same Wsl, whose calls stop when `flag` turns true. The original handle is not changed, so a clean-up
    /// made through it still runs.
    pub fn with_cancel(&self, flag: Arc<AtomicBool>) -> Self {
        Self { runner: self.runner.clone(), cancel: Some(flag) }
    }

    fn run_call(&self, call: Call) -> Result<Output, WslError> {
        run(self.runner(), call.cancel(self.cancel.clone()))
    }

    fn run_call_streaming(&self, call: Call, on_line: &mut dyn FnMut(&str)) -> Result<Output, WslError> {
        run_streaming(self.runner(), call.cancel(self.cancel.clone()), on_line)
    }

    /// The real thing, when this computer has `wsl.exe`.
    pub fn live() -> Result<Self, WslError> {
        let exe = wsl_exe().ok_or(WslError::NotAvailable)?;
        Ok(Self::new(Arc::new(LiveRunner::new(exe))))
    }

    pub fn runner(&self) -> &dyn Runner {
        self.runner.as_ref()
    }

    /// True when `wsl --status` answers with success. A PC without the feature says no.
    pub fn is_available(&self) -> bool {
        self.run_call(Call::new(&["--status"])).map(|o| o.success()).unwrap_or(false)
    }

    pub fn list(&self) -> Result<Vec<Distro>, WslError> {
        let out = self.run_call(Call::new(&["--list", "--verbose"]))?;
        Ok(parse_list(&out.stdout_text()))
    }

    /// Only the distros that are ours; everything else on the PC is not looked at again.
    pub fn list_ours(&self) -> Result<Vec<Distro>, WslError> {
        Ok(self.list()?.into_iter().filter(|d| valid_name(&d.name)).collect())
    }

    pub fn is_installed(&self, name: &SandboxName) -> Result<bool, WslError> {
        Ok(self.list_ours()?.iter().any(|d| d.name == name.as_str()))
    }

    /// Asks for the running distros by name only (the words around them are localized, the names are not).
    pub fn is_running(&self, name: &SandboxName) -> Result<bool, WslError> {
        let out = self.run_call(Call::new(&["--list", "--running", "--quiet"]))?;
        Ok(out.stdout_text().lines().any(|l| l.trim().trim_start_matches('*').trim() == name.as_str()))
    }

    pub fn install_distro(&self, name: &SandboxName, dir: &Path) -> Result<(), WslError> {
        self.install_distro_streaming(name, dir, &mut |_| {})
    }

    /// Like `install_distro`, handing each line `wsl.exe` prints to `on_line` (UTF-16 as it comes: the caller strips the NULs).
    pub fn install_distro_streaming(&self, name: &SandboxName, dir: &Path, on_line: &mut dyn FnMut(&str)) -> Result<(), WslError> {
        let dir = dir.to_string_lossy();
        let call = Call::new(&["--install", DISTRO_IMAGE, "--name", name.as_str(), "--location", &dir, "--no-launch"]);
        self.run_call_streaming(call.timeout(LONG), on_line)?.ok().map(|_| ())
    }

    pub fn terminate(&self, name: &SandboxName) -> Result<(), WslError> {
        self.run_call(Call::new(&["--terminate", name.as_str()]))?.ok().map(|_| ())
    }

    pub fn unregister(&self, name: &SandboxName) -> Result<(), WslError> {
        self.run_call(Call::new(&["--unregister", name.as_str()]).timeout(LONG))?.ok().map(|_| ())
    }

    pub fn export(&self, name: &SandboxName, tar: &Path) -> Result<(), WslError> {
        let tar = tar.to_string_lossy();
        self.run_call(Call::new(&["--export", name.as_str(), &tar]).timeout(LONG))?.ok().map(|_| ())
    }

    pub fn import(&self, name: &SandboxName, dir: &Path, tar: &Path) -> Result<(), WslError> {
        let (dir, tar) = (dir.to_string_lossy(), tar.to_string_lossy());
        self.run_call(Call::new(&["--import", name.as_str(), &dir, &tar]).timeout(LONG))?.ok().map(|_| ())
    }

    fn guest_call(name: &SandboxName, user: GuestUser, argv: &[&str]) -> Call {
        let mut args = vec!["-d", name.as_str(), "-u", user.as_str(), "--exec"];
        if user == GuestUser::Pm {
            args.extend_from_slice(PM_ENV);
        }
        args.extend_from_slice(argv);
        Call::new(&args)
    }

    /// Runs a program inside the distro (starting the distro if it is stopped). `argv[0]` is an absolute path.
    pub fn exec(
        &self,
        name: &SandboxName,
        user: GuestUser,
        argv: &[&str],
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Output, WslError> {
        let mut call = Self::guest_call(name, user, argv).timeout(timeout);
        call.stdin = stdin;
        self.run_call(call)
    }

    /// Like `exec`, handing each finished stdout line to `on_line` while the program runs.
    pub fn exec_streaming(
        &self,
        name: &SandboxName,
        user: GuestUser,
        argv: &[&str],
        stdin: Option<Vec<u8>>,
        timeout: Duration,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Output, WslError> {
        let mut call = Self::guest_call(name, user, argv).timeout(timeout);
        call.stdin = stdin;
        self.run_call_streaming(call, on_line)
    }

    /// Starts a process inside the distro that never ends by itself (it keeps the distro up).
    pub fn spawn_holder(&self, name: &SandboxName) -> Result<Box<dyn Holder>, WslError> {
        spawn_holder(self.runner(), Self::guest_call(name, GuestUser::Root, &["/bin/sleep", "infinity"]))
    }

    /// The feature install, elevated. Only the button press makes a `Consent`.
    pub fn enable_windows_feature(&self, _consent: Consent) -> Result<FeatureResult, WslError> {
        let mut call = Call::new(&["--install", "--no-distribution"]).timeout(LONG);
        call.consented = true;
        let out = self.run_call(call)?;
        match out.code {
            Some(0) => Ok(FeatureResult { restart_needed: false }),
            Some(EXIT_RESTART_NEEDED) => Ok(FeatureResult { restart_needed: true }),
            code => Err(WslError::Failed { code, detail: short_detail(&out) }),
        }
    }
}

// ---- the real runner -----------------------------------------------------------------------------

/// `%SystemRoot%\System32\wsl.exe` when it exists.
pub fn wsl_exe() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    exe_under(&root)
}

pub fn exe_under(system_root: &Path) -> Option<PathBuf> {
    let exe = system_root.join("System32").join("wsl.exe");
    exe.is_file().then_some(exe)
}

pub struct LiveRunner {
    exe: PathBuf,
}

impl LiveRunner {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe }
    }

    fn command(&self, call: &Prepared) -> Result<std::process::Command, WslError> {
        if !call.elevated() {
            let mut command = crate::proc_util::hidden_command(&self.exe);
            command.args(&call.args);
            return Ok(command);
        }
        // Elevated: the one fixed script that asks Windows for the admin prompt. The only data in it is
        // the path of wsl.exe; no text of the person or of an agent reaches it.
        let exe = self.exe.to_string_lossy().to_string();
        if exe.contains(['\'', '"', '`', '$']) {
            return Err(WslError::Refused("the wsl.exe path has a character that cannot be quoted"));
        }
        let script = format!(
            "$p = Start-Process -FilePath '{exe}' -ArgumentList '--install','--no-distribution' -Verb RunAs -Wait -PassThru; exit $p.ExitCode"
        );
        let mut command = crate::proc_util::hidden_command("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        Ok(command)
    }
}

enum Chunk {
    Data(Vec<u8>),
    End,
}

impl Runner for LiveRunner {
    fn run(&self, call: &Prepared, on_line: &mut dyn FnMut(&str)) -> Result<Output, WslError> {
        let mut command = self.command(call)?;
        command
            .stdin(if call.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| WslError::Start(e.to_string()))?;
        if let (Some(bytes), Some(mut stdin)) = (call.stdin.clone(), child.stdin.take()) {
            // A thread, so a big archive cannot fill the pipe while nobody reads the output.
            std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes);
            });
        }
        let (tx, rx) = mpsc::channel::<Chunk>();
        if let Some(mut stdout) = child.stdout.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(Chunk::Data(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = tx.send(Chunk::End);
            });
        }
        drop(tx);
        let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();
        if let Some(mut stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let cap = OUTPUT_CAP / 8;
                let mut kept = Vec::new();
                let mut buf = [0u8; 4096];
                while let Ok(n) = stderr.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let room = cap.saturating_sub(kept.len());
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
                let _ = err_tx.send(kept);
            });
        }
        let deadline = Instant::now() + call.timeout;
        let cancelled = || call.cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst));
        let mut stdout_bytes: Vec<u8> = Vec::new();
        let mut pending: Vec<u8> = Vec::new();
        loop {
            let wait = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(200));
            match rx.recv_timeout(wait) {
                Ok(Chunk::Data(data)) => {
                    let room = OUTPUT_CAP.saturating_sub(stdout_bytes.len());
                    stdout_bytes.extend_from_slice(&data[..data.len().min(room)]);
                    pending.extend_from_slice(&data);
                    while let Some(at) = pending.iter().position(|b| *b == b'\n') {
                        let line: Vec<u8> = pending.drain(..=at).collect();
                        on_line(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']));
                    }
                    if pending.len() > 64 * 1024 {
                        pending.clear();
                    }
                }
                Ok(Chunk::End) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if cancelled() {
                        crate::pty::proc_tree::kill_process_tree(child.id());
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(WslError::Cancelled);
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(WslError::Timeout);
                    }
                }
            }
        }
        if !pending.is_empty() {
            on_line(String::from_utf8_lossy(&pending).trim_end_matches(['\r', '\n']));
        }
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if cancelled() => {
                    crate::pty::proc_tree::kill_process_tree(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(WslError::Cancelled);
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(WslError::Timeout);
                }
                Err(e) => return Err(WslError::Start(e.to_string())),
            }
        };
        let stderr = err_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
        Ok(Output { code: status.code(), stdout: stdout_bytes, stderr })
    }

    fn spawn(&self, call: &Prepared) -> Result<Box<dyn Holder>, WslError> {
        let mut command = self.command(call)?;
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = command.spawn().map_err(|e| WslError::Start(e.to_string()))?;
        Ok(Box::new(LiveHolder { child: Some(child) }))
    }
}

struct LiveHolder {
    child: Option<std::process::Child>,
}

impl Holder for LiveHolder {
    fn alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for LiveHolder {
    fn drop(&mut self) {
        self.stop();
    }
}
