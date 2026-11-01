//! One chat turn through the bridge: start the agent's headless command with the prompt, forward what it
//! prints, and end it (done, cancelled, failed). Everything agent-specific comes from the profile.

use std::io::Read;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::acp::job::{self, Containment};
use crate::tui::{HeadlessLaunch, LaunchEnv, TuiProfile};

/// What one turn needs.
#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub cwd: PathBuf,
    pub prompt: String,
    /// The agent's own conversation id from an earlier turn (only passed when the profile can resume).
    pub resume: Option<String>,
    pub read_only: bool,
}

/// Every wait has a limit written down here.
#[derive(Debug, Clone)]
pub struct TurnLimits {
    /// The whole turn.
    pub timeout: Duration,
    /// After the output already holds the final result: how long the process may linger before it is stopped.
    pub result_grace: Duration,
    /// How long to keep reading output after the process exited (a grandchild may hold the pipe open).
    pub drain: Duration,
    /// Poll step of the wait loop.
    pub poll: Duration,
    /// How often partial output is offered to the result extractor.
    pub extract_every: Duration,
    /// Output kept for the extractors; more is dropped (the extractor caps it the same way).
    pub max_output_bytes: usize,
}

impl Default for TurnLimits {
    fn default() -> Self {
        TurnLimits {
            timeout: Duration::from_secs(20 * 60),
            result_grace: Duration::from_millis(1500),
            drain: Duration::from_secs(2),
            poll: Duration::from_millis(30),
            extract_every: Duration::from_millis(150),
            max_output_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEnd {
    /// The agent finished; its own conversation id when its output carried one.
    Done { provider_session: Option<String> },
    Cancelled,
    /// One plain sentence.
    Failed(String),
}

/// Lets another thread stop the turn.
#[derive(Default)]
pub struct TurnControl {
    cancelled: AtomicBool,
    running: Mutex<Option<Arc<Containment>>>,
}

impl TurnControl {
    pub fn new() -> Arc<TurnControl> {
        Arc::new(TurnControl::default())
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Stops the turn: the whole process tree dies (by job or by pid, never by image name).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(containment) = self.running.lock().as_ref() {
            containment.terminate();
        }
    }

    fn attach(&self, containment: Arc<Containment>) {
        *self.running.lock() = Some(containment.clone());
        // A cancel that came before the process existed.
        if self.is_cancelled() {
            containment.terminate();
        }
    }

    fn detach(&self) {
        *self.running.lock() = None;
    }
}

/// Sends text to the host only when it extends what was already sent. Redaction runs over the whole text
/// each time, and a partial last word is held back while the turn runs, so a secret is never cut in half.
#[derive(Default)]
struct Emitter {
    sent: String,
}

impl Emitter {
    fn offer(&mut self, full: &str, is_final: bool, emit: &mut dyn FnMut(&str)) {
        let redacted = crate::redact::text(full).into_inner();
        let upto = if is_final {
            redacted.len()
        } else {
            redacted.char_indices().rev().find(|(_, c)| c.is_whitespace()).map(|(i, c)| i + c.len_utf8()).unwrap_or(0)
        };
        let candidate = &redacted[..upto];
        if candidate.len() > self.sent.len() && candidate.starts_with(&self.sent) {
            emit(&candidate[self.sent.len()..]);
            self.sent = candidate.to_string();
        }
    }
}

/// Adds the valid UTF-8 of `pending` to `output`; an incomplete character at the end waits for more bytes.
fn take_text(pending: &mut Vec<u8>, output: &mut String, max: usize, flush: bool) {
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                push_capped(output, text, max);
                pending.clear();
                return;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                push_capped(output, std::str::from_utf8(&pending[..valid]).unwrap_or_default(), max);
                pending.drain(..valid);
                match error.error_len() {
                    Some(bad) => {
                        push_capped(output, "\u{fffd}", max);
                        pending.drain(..bad.min(pending.len()));
                    }
                    None if flush => {
                        push_capped(output, "\u{fffd}", max);
                        pending.clear();
                        return;
                    }
                    None => return,
                }
            }
        }
    }
}

fn push_capped(output: &mut String, text: &str, max: usize) {
    if output.len() >= max {
        return;
    }
    let room = max - output.len();
    if text.len() <= room {
        output.push_str(text);
    } else {
        let mut cut = room;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        output.push_str(&text[..cut]);
    }
}

fn spawn_stdout_reader<R: Read + Send + 'static>(mut reader: R) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
    let _ = std::thread::Builder::new().name("acp-bridge-stdout".into()).spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

const STDERR_KEEP: usize = 4096;

fn spawn_stderr_reader<R: Read + Send + 'static>(mut reader: R) -> Arc<Mutex<Vec<u8>>> {
    let tail = Arc::new(Mutex::new(Vec::new()));
    let keep = tail.clone();
    let _ = std::thread::Builder::new().name("acp-bridge-stderr".into()).spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut tail = keep.lock();
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > STDERR_KEEP {
                let drop = tail.len() - STDERR_KEEP;
                tail.drain(..drop);
            }
        }
    });
    tail
}

/// The last line the agent wrote on stderr, redacted and short; empty when there is none.
fn stderr_line(tail: &Mutex<Vec<u8>>) -> String {
    let text = String::from_utf8_lossy(&tail.lock()).into_owned();
    let line = text.lines().map(str::trim).rev().find(|l| !l.is_empty()).unwrap_or("").to_string();
    let line = crate::redact::text(&line).into_inner();
    if line.chars().count() > 200 {
        let cut: String = line.chars().take(200).collect();
        format!("{cut}...")
    } else {
        line
    }
}

fn failure_sentence(profile: &dyn TuiProfile, label: &str, output: &str, code: i32, stderr: &Mutex<Vec<u8>>) -> String {
    let base = match profile.classify_headless_failure(output, code) {
        Some((_, message)) => crate::redact::text(message.trim()).into_inner(),
        None if code == 0 => format!("{label} finished but did not give an answer this app can read."),
        None => format!("{label} stopped with an error (exit code {code})."),
    };
    let base = if base.ends_with(['.', '!', '?']) { base } else { format!("{base}.") };
    let line = stderr_line(stderr);
    if line.is_empty() || base.contains(&line) {
        base
    } else {
        format!("{base} It said: {line}")
    }
}

fn reap(child: &mut Child, containment: &Containment) {
    containment.terminate();
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs one turn. `emit` receives the text to show, in order, as it becomes available. Blocks until the turn
/// ends; another thread may stop it through `ctl`.
pub fn run_turn(
    profile: &dyn TuiProfile,
    label: &str,
    request: &TurnRequest,
    limits: &TurnLimits,
    ctl: &TurnControl,
    emit: &mut dyn FnMut(&str),
) -> TurnEnd {
    if ctl.is_cancelled() {
        return TurnEnd::Cancelled;
    }
    if !request.cwd.is_dir() {
        return TurnEnd::Failed(format!("The folder {} does not exist.", request.cwd.display()));
    }
    let project = request.cwd.to_string_lossy().into_owned();
    // A definition-backed agent is re-checked at every start (approval and pinned program), like any run.
    if let Err(gate) = profile.spawn_gate(&project) {
        return TurnEnd::Failed(gate.message);
    }
    // Only what the definition declares for headless: no shell opt-in, no trust opt-in, no model override.
    let launch = HeadlessLaunch {
        project: &project,
        task: &request.prompt,
        read_only: request.read_only,
        resume_session: request.resume.as_deref(),
        trust_workspace: false,
        allow_shell: false,
        model: None,
    };
    let spec = match crate::tui::headless_launch_with(profile, profile.id(), &launch) {
        Ok(spec) if !request.read_only || spec.read_only_enforced => spec,
        Ok(_) => return TurnEnd::Failed(format!("{label} did not enforce read-only mode, so nothing was started.")),
        Err(error) => {
            let message = error.strip_prefix("READ_ONLY_UNSUPPORTED: ").unwrap_or(&error);
            return TurnEnd::Failed(format!("{label} could not be started: {message}"));
        }
    };
    let mut command = crate::agent_runs::headless::headless_command(&spec, &LaunchEnv::default());
    job::prepare(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return TurnEnd::Failed(format!("{label} could not be started ({error}). Is it installed?")),
    };
    let containment = Arc::new(Containment::attach(&child));
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        reap(&mut child, &containment);
        return TurnEnd::Failed(format!("{label} could not be started: its output could not be read."));
    };
    ctl.attach(containment.clone());
    let rx = spawn_stdout_reader(stdout);
    let stderr = spawn_stderr_reader(stderr);

    let started = Instant::now();
    let mut output = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut emitter = Emitter::default();
    let mut last_extract = Instant::now();
    let mut new_lines = false;
    let mut stdout_done = false;
    let mut exit: Option<i32> = None;
    let mut exited_at: Option<Instant> = None;
    let mut ready_at: Option<Instant> = None;

    let end = loop {
        if ctl.is_cancelled() {
            reap(&mut child, &containment);
            break TurnEnd::Cancelled;
        }
        if started.elapsed() > limits.timeout {
            reap(&mut child, &containment);
            let minutes = (limits.timeout.as_secs() / 60).max(1);
            break TurnEnd::Failed(format!("{label} took longer than {minutes} minutes and was stopped."));
        }
        if stdout_done {
            std::thread::sleep(limits.poll);
        } else {
            match rx.recv_timeout(limits.poll) {
                Ok(bytes) => {
                    new_lines |= bytes.contains(&b'\n');
                    pending.extend_from_slice(&bytes);
                    take_text(&mut pending, &mut output, limits.max_output_bytes, false);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => stdout_done = true,
            }
        }
        if exit.is_none() {
            if let Ok(Some(status)) = child.try_wait() {
                exit = Some(status.code().unwrap_or(-1));
                exited_at = Some(Instant::now());
            }
        }
        if new_lines && last_extract.elapsed() >= limits.extract_every {
            last_extract = Instant::now();
            new_lines = false;
            if let Some(text) = profile.extract_result(&output, 0) {
                emitter.offer(&text, false, emit);
            }
            if ready_at.is_none() && profile.headless_result_ready(&output) {
                ready_at = Some(Instant::now());
            }
        }
        match (exit, exited_at, ready_at) {
            // The process ended and its output is all read (or the pipe is held by a leftover process too long).
            (Some(_), Some(at), _) if stdout_done || at.elapsed() > limits.drain => break TurnEnd::Done { provider_session: None },
            // The output already holds the final result but the process lingers: stop it, the turn is over.
            (None, _, Some(at)) if at.elapsed() > limits.result_grace => {
                reap(&mut child, &containment);
                exit = Some(0);
                while let Ok(bytes) = rx.try_recv() {
                    pending.extend_from_slice(&bytes);
                }
                take_text(&mut pending, &mut output, limits.max_output_bytes, false);
                break TurnEnd::Done { provider_session: None };
            }
            _ => {}
        }
    };
    ctl.detach();
    if !matches!(end, TurnEnd::Done { .. }) {
        return end;
    }
    take_text(&mut pending, &mut output, limits.max_output_bytes, true);
    let code = exit.unwrap_or(-1);
    if code != 0 {
        return TurnEnd::Failed(failure_sentence(profile, label, &output, code, &stderr));
    }
    match profile.extract_result(&output, 0) {
        Some(text) => {
            emitter.offer(&text, true, emit);
            TurnEnd::Done { provider_session: profile.extract_provider_session(&output).filter(|id| !id.trim().is_empty()) }
        }
        None => TurnEnd::Failed(failure_sentence(profile, label, &output, 0, &stderr)),
    }
}
