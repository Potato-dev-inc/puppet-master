//! Run one child process with limits: a wall-clock timeout, a cap on the output kept, stdin closed
//! (or one fixed string), a given working folder, no window on Windows, and a cancel flag. Used by
//! probes, the version check and the live test. Never goes through a shell.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use regex::Regex;

/// How often the wait loop looks at the child, the clock, the cancel flag and the early-stop rule.
const POLL: Duration = Duration::from_millis(25);
/// After the child exited, how long the readers get to deliver what is left in the pipes.
const READER_GRACE: Duration = Duration::from_millis(750);

pub struct RunSpec<'a> {
    pub program: &'a Path,
    pub args: &'a [String],
    pub cwd: &'a Path,
    /// `None`: stdin is closed. `Some(text)`: written once, then closed.
    pub stdin: Option<&'a str>,
    pub env: &'a [(String, String)],
    pub env_remove: &'a [String],
    pub timeout: Duration,
    /// Bytes kept per stream; the rest is read and thrown away so the child never blocks on a full pipe.
    pub max_bytes: usize,
    pub cancel: Option<&'a AtomicBool>,
    /// Looked at with the stdout so far; when it says "done" the child gets `grace` and is then killed.
    pub early_stop: Option<&'a dyn Fn(&str) -> bool>,
    pub grace: Duration,
}

impl<'a> RunSpec<'a> {
    pub fn new(program: &'a Path, args: &'a [String], cwd: &'a Path) -> Self {
        Self {
            program,
            args,
            cwd,
            stdin: None,
            env: &[],
            env_remove: &[],
            timeout: Duration::from_secs(5),
            max_bytes: 64 * 1024,
            cancel: None,
            early_stop: None,
            grace: Duration::from_millis(1500),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    /// `None` when the child was killed (timeout, cancel, early stop) or never started.
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stopped_early: bool,
    /// Either stream was longer than `max_bytes`.
    pub truncated: bool,
    pub duration: Duration,
    /// Set when the program could not be started.
    pub spawn_error: Option<String>,
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    total: usize,
}

fn reader<R: Read + Send + 'static>(mut source: R, cap: usize, into: Arc<Mutex<Capture>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = source.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut c = into.lock().unwrap_or_else(|e| e.into_inner());
            c.total += n;
            let room = cap.saturating_sub(c.bytes.len());
            c.bytes.extend_from_slice(&buf[..n.min(room)]);
        }
    })
}

fn snapshot(c: &Arc<Mutex<Capture>>) -> (String, bool, usize) {
    let c = c.lock().unwrap_or_else(|e| e.into_inner());
    (String::from_utf8_lossy(&c.bytes).into_owned(), c.total > c.bytes.len(), c.total)
}

fn command(spec: &RunSpec<'_>) -> Command {
    let mut cmd = Command::new(spec.program);
    cmd.args(spec.args)
        .current_dir(spec.cwd)
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in spec.env {
        cmd.env(k, v);
    }
    for k in spec.env_remove {
        cmd.env_remove(k);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Kill the child and, on Windows, everything it started (an npm wrapper leaves `node` behind).
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
    let _ = child.kill();
}

pub fn run(spec: &RunSpec<'_>) -> RunOutput {
    let started = Instant::now();
    let mut out = RunOutput::default();
    let mut child = match command(spec).spawn() {
        Ok(child) => child,
        Err(e) => {
            out.spawn_error = Some(e.to_string());
            out.duration = started.elapsed();
            return out;
        }
    };
    if let (Some(text), Some(mut pipe)) = (spec.stdin, child.stdin.take()) {
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
        });
    }
    let stdout_cap = Arc::new(Mutex::new(Capture::default()));
    let stderr_cap = Arc::new(Mutex::new(Capture::default()));
    let mut readers = Vec::new();
    if let Some(s) = child.stdout.take() {
        readers.push(reader(s, spec.max_bytes, stdout_cap.clone()));
    }
    if let Some(s) = child.stderr.take() {
        readers.push(reader(s, spec.max_bytes, stderr_cap.clone()));
    }

    let mut last_seen = 0usize;
    let mut done_at: Option<Instant> = None;
    let mut killed = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                out.exit = if killed { None } else { status.code() };
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if killed {
            std::thread::sleep(POLL);
            continue;
        }
        if spec.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            out.cancelled = true;
            kill_tree(&mut child);
            killed = true;
            continue;
        }
        if started.elapsed() >= spec.timeout {
            out.timed_out = true;
            kill_tree(&mut child);
            killed = true;
            continue;
        }
        if let Some(rule) = spec.early_stop {
            match done_at {
                None => {
                    let total = stdout_cap.lock().unwrap_or_else(|e| e.into_inner()).total;
                    if total != last_seen {
                        last_seen = total;
                        if rule(&snapshot(&stdout_cap).0) {
                            done_at = Some(Instant::now());
                        }
                    }
                }
                Some(at) if at.elapsed() >= spec.grace => {
                    out.stopped_early = true;
                    kill_tree(&mut child);
                    killed = true;
                    continue;
                }
                Some(_) => {}
            }
        }
        std::thread::sleep(POLL);
    }
    let deadline = Instant::now() + READER_GRACE;
    while Instant::now() < deadline && readers.iter().any(|r| !r.is_finished()) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let (stdout, out_trunc, _) = snapshot(&stdout_cap);
    let (stderr, err_trunc, _) = snapshot(&stderr_cap);
    out.stdout = stdout;
    out.stderr = stderr;
    out.truncated = out_trunc || err_trunc;
    out.duration = started.elapsed();
    out
}

static ANSI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]").expect("ansi regex")
});

/// Terminal escape codes removed, `\r\n` folded to `\n`, nothing else touched (indentation stays: the
/// subcommand parser and the drafter both read it).
pub fn plain(text: &str) -> String {
    ANSI.replace_all(text, "").replace("\r\n", "\n").replace('\r', "\n")
}

static USER_DIR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)([A-Za-z]:[\\/]+users[\\/]+|/home/|/users/)[^\\/\s"']+"#).expect("user dir"));

/// `plain`, then secrets masked (`redact::text`), the user's folder name replaced by `user` and every email
/// address replaced by `<email>` (`redact::mask_emails`; the account row a TUI shows since startup is the
/// person's): what may be stored, shown, sent on or kept as a fixture.
pub fn clean(text: &str) -> String {
    let redacted = crate::redact::text(&plain(text)).into_inner();
    mask_private(&USER_DIR.replace_all(&redacted, "${1}user"))
}

/// The wizard's last net for text it saves or sends that did not go through `clean` (a record, a fixture file,
/// a message): email addresses become `<email>`. Idempotent.
pub fn mask_private(text: &str) -> String {
    crate::redact::mask_emails(text)
}

/// At most `max` characters, cut on a character boundary.
pub fn cut_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// At most `max` bytes, cut on a character boundary.
pub fn cut_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}
