//! One adapter process and its JSON-RPC pipe.
//!
//! Threads per connection (all named `acp-*`, all end when the process is gone):
//!
//! * **reader**: stdout lines -> parsed frames -> a bounded queue. When the queue is full the reader
//!   blocks, the OS pipe fills, and the adapter slows down instead of our memory growing (backpressure).
//! * **dispatcher**: takes frames in arrival order. A response resolves its pending request; a request
//!   or notification from the agent goes to the [`ConnHandler`]. Because responses travel through the same
//!   ordered queue as notifications, a `session/prompt` response is always handled after every
//!   `session/update` the agent sent before it.
//! * **writer**: owns stdin, writes whole lines from the [`WriteQueue`] (priority lane first).
//! * **stderr**: lines into a capped ring ([`StderrRing`]).
//! * **monitor**: notices the process exit (and ends whatever it left behind), and enforces the stdin
//!   write timeout: a write that has been stuck longer than the limit means the agent stopped reading,
//!   so the whole tree is killed (that also unblocks the stuck write).
//!
//! The process is started through `proc_util::hidden_command` (no window), with a cleared environment
//! plus an explicit list, and contained by `job::Containment`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::error::AcpError;
use super::job::{self, Containment, Mechanism};
use super::ring::StderrRing;
use super::wire::{self, Inbound, LineRead, RpcErrorBody};
use super::write_queue::{Lane, WriteQueue};

/// What to start. The environment is exactly `env`: nothing is inherited.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ConnLimits {
    /// Parsed frames waiting for the dispatcher.
    pub event_queue: usize,
    /// Normal-lane lines (and bytes) waiting for stdin.
    pub write_queue: usize,
    pub write_queue_bytes: usize,
    pub priority_queue: usize,
    /// A single write to stdin may block this long before the adapter is declared deaf and killed.
    pub stdin_write_timeout: Duration,
    pub max_line_bytes: usize,
    pub stderr_lines: usize,
    pub stderr_bytes: usize,
}

impl Default for ConnLimits {
    fn default() -> Self {
        Self {
            event_queue: 1024,
            write_queue: 256,
            write_queue_bytes: 32 * 1024 * 1024,
            priority_queue: 64,
            stdin_write_timeout: Duration::from_secs(10),
            max_line_bytes: wire::MAX_LINE_BYTES,
            stderr_lines: super::ring::DEFAULT_MAX_LINES,
            stderr_bytes: super::ring::DEFAULT_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCause {
    /// The process ended on its own.
    Exited,
    /// We stopped it (`kill`, or the connection was dropped).
    Killed,
    /// It stopped reading stdin.
    WriteStalled,
    /// Its stdout closed while the process lived on; we ended it.
    StdoutClosed,
}

#[derive(Debug, Clone)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub cause: ExitCause,
    pub stderr_tail: Vec<String>,
}

/// What the agent sent that is not an answer to one of our requests.
#[derive(Debug, Clone)]
pub enum ConnEvent {
    Notification { method: String, params: Value },
    /// The agent asks us something. It stays open until we call `respond` / `respond_error`.
    AgentRequest { id: Value, method: String, params: Value },
    /// Always the last event: stdout is drained, the process and its children are gone.
    Exited(ExitInfo),
}

pub trait ConnHandler: Send + Sync + 'static {
    /// Called on the dispatcher thread, in arrival order. Must not call [`AcpConnection::request`]
    /// (it would wait for the thread it runs on).
    fn on_event(&self, event: ConnEvent);
}

type Done = Box<dyn FnOnce(Result<Value, AcpError>) + Send>;

enum Pending {
    Waiter(SyncSender<Result<Value, AcpError>>),
    Callback(Done),
}

#[derive(Debug, Default)]
pub struct Stats {
    pub frames_in: AtomicU64,
    pub bad_lines: AtomicU64,
    pub oversized_lines: AtomicU64,
    pub stale_responses: AtomicU64,
    pub queued_events: AtomicUsize,
    pub max_queued_events: AtomicUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub frames_in: u64,
    pub bad_lines: u64,
    pub oversized_lines: u64,
    pub stale_responses: u64,
    pub max_queued_events: usize,
}

enum Frame {
    Msg(Inbound),
    Eof,
}

struct Inner {
    base: Instant,
    pid: u32,
    containment: Containment,
    queue: Arc<WriteQueue>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    alive: AtomicBool,
    kill_cause: Mutex<Option<ExitCause>>,
    exit_code: Mutex<Option<Option<i32>>>,
    stderr: Arc<StderrRing>,
    stderr_done: AtomicBool,
    stats: Stats,
    limits: ConnLimits,
    /// Milliseconds since `base` (+1) at which the current stdin write began; 0 = none in progress.
    write_started_ms: AtomicU64,
    last_frame_ms: AtomicU64,
}

impl Inner {
    fn ms(&self) -> u64 {
        self.base.elapsed().as_millis() as u64
    }

    fn touch(&self) {
        self.last_frame_ms.store(self.ms(), Ordering::Relaxed);
    }

    fn kill(&self, cause: ExitCause) {
        {
            let mut slot = self.kill_cause.lock();
            if slot.is_none() {
                *slot = Some(cause);
            }
        }
        self.containment.terminate();
        self.queue.close();
    }

    fn resolve(&self, id: &Value, outcome: Result<Value, RpcErrorBody>) {
        let pending = id.as_u64().and_then(|n| self.pending.lock().remove(&n));
        let result = outcome.map_err(AcpError::from);
        match pending {
            Some(Pending::Waiter(tx)) => {
                let _ = tx.send(result);
            }
            Some(Pending::Callback(done)) => run_guarded(|| done(result)),
            None => {
                // Not ours (a string id, an id we never sent, or a request that already timed out).
                self.stats.stale_responses.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn fail_all_pending(&self, error: &AcpError) {
        let drained: Vec<Pending> = self.pending.lock().drain().map(|(_, p)| p).collect();
        for pending in drained {
            match pending {
                Pending::Waiter(tx) => {
                    let _ = tx.send(Err(error.clone()));
                }
                Pending::Callback(done) => {
                    let error = error.clone();
                    run_guarded(move || done(Err(error)));
                }
            }
        }
    }
}

fn run_guarded(f: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        tracing::error!("acp: a handler panicked; the connection keeps going");
    }
}

/// Dropping the last clone kills the adapter and its tree.
struct Handle {
    inner: Arc<Inner>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.inner.kill(ExitCause::Killed);
    }
}

#[derive(Clone)]
pub struct AcpConnection {
    handle: Arc<Handle>,
}

impl std::fmt::Debug for AcpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpConnection").field("pid", &self.pid()).field("alive", &self.is_alive()).finish()
    }
}

impl AcpConnection {
    pub fn spawn(spec: SpawnSpec, limits: ConnLimits, handler: Arc<dyn ConnHandler>) -> Result<AcpConnection, AcpError> {
        let mut command = crate::proc_util::hidden_command(&spec.program);
        command
            .args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        job::prepare(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| AcpError::Spawn(format!("{}: {e}", spec.program.to_string_lossy())))?;
        let containment = Containment::attach(&child);
        let pid = child.id();
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            containment.terminate();
            return Err(AcpError::Spawn("the adapter's pipes could not be opened".into()));
        };

        let queue = Arc::new(WriteQueue::new(limits.write_queue, limits.write_queue_bytes, limits.priority_queue));
        let inner = Arc::new(Inner {
            base: Instant::now(),
            pid,
            containment,
            queue: queue.clone(),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            alive: AtomicBool::new(true),
            kill_cause: Mutex::new(None),
            exit_code: Mutex::new(None),
            stderr: Arc::new(StderrRing::new(limits.stderr_lines, limits.stderr_bytes)),
            stderr_done: AtomicBool::new(false),
            stats: Stats::default(),
            limits: limits.clone(),
            write_started_ms: AtomicU64::new(0),
            last_frame_ms: AtomicU64::new(0),
        });
        let (frames_tx, frames_rx) = mpsc::sync_channel::<Frame>(limits.event_queue.max(1));

        let started: Result<(), std::io::Error> = (|| {
            // writer
            let inner_w = inner.clone();
            thread::Builder::new().name("acp-writer".into()).spawn(move || writer_loop(&inner_w, stdin))?;
            // stderr
            let inner_e = inner.clone();
            thread::Builder::new().name("acp-stderr".into()).spawn(move || stderr_loop(&inner_e, stderr))?;
            // reader
            let inner_r = inner.clone();
            thread::Builder::new().name("acp-reader".into()).spawn(move || reader_loop(&inner_r, stdout, frames_tx))?;
            // monitor
            let inner_m = inner.clone();
            thread::Builder::new().name("acp-monitor".into()).spawn(move || monitor_loop(&inner_m, child))?;
            // dispatcher
            let inner_d = inner.clone();
            thread::Builder::new().name("acp-dispatch".into()).spawn(move || dispatch_loop(&inner_d, frames_rx, handler))?;
            Ok(())
        })();
        if let Err(e) = started {
            inner.kill(ExitCause::Killed);
            return Err(AcpError::Spawn(format!("could not start the connection threads: {e}")));
        }
        Ok(AcpConnection { handle: Arc::new(Handle { inner }) })
    }

    fn inner(&self) -> &Arc<Inner> {
        &self.handle.inner
    }

    pub fn pid(&self) -> u32 {
        self.inner().pid
    }

    pub fn is_alive(&self) -> bool {
        self.inner().alive.load(Ordering::SeqCst)
    }

    pub fn mechanism(&self) -> Mechanism {
        self.inner().containment.mechanism()
    }

    /// Processes still in the adapter's job (Windows only).
    pub fn live_processes(&self) -> Option<u32> {
        self.inner().containment.live_processes()
    }

    /// Time since the agent last sent anything (any line, even one that could not be parsed).
    pub fn quiet_for(&self) -> Duration {
        let inner = self.inner();
        Duration::from_millis(inner.ms().saturating_sub(inner.last_frame_ms.load(Ordering::Relaxed)))
    }

    pub fn stderr_tail(&self, lines: usize) -> Vec<String> {
        self.inner().stderr.tail(lines)
    }

    pub fn stderr_ring(&self) -> Arc<StderrRing> {
        self.inner().stderr.clone()
    }

    pub fn stats(&self) -> StatsSnapshot {
        let stats = &self.inner().stats;
        StatsSnapshot {
            frames_in: stats.frames_in.load(Ordering::Relaxed),
            bad_lines: stats.bad_lines.load(Ordering::Relaxed),
            oversized_lines: stats.oversized_lines.load(Ordering::Relaxed),
            stale_responses: stats.stale_responses.load(Ordering::Relaxed),
            max_queued_events: stats.max_queued_events.load(Ordering::Relaxed),
        }
    }

    pub fn write_queue(&self) -> Arc<WriteQueue> {
        self.inner().queue.clone()
    }

    /// Kills the adapter and its children. The `Exited` event follows.
    pub fn kill(&self) {
        self.inner().kill(ExitCause::Killed);
    }

    fn register(&self, pending: Pending) -> Result<u64, AcpError> {
        let inner = self.inner();
        let mut map = inner.pending.lock();
        // Checked under the lock `finish` drains with, so a request is either refused or failed later.
        if !inner.alive.load(Ordering::SeqCst) {
            return Err(AcpError::Closed);
        }
        let id = inner.next_id.fetch_add(1, Ordering::SeqCst);
        map.insert(id, pending);
        Ok(id)
    }

    fn unregister(&self, id: u64) {
        self.inner().pending.lock().remove(&id);
    }

    /// A request whose answer is waited for on this thread.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, AcpError> {
        let (tx, rx) = mpsc::sync_channel(1);
        let id = self.register(Pending::Waiter(tx))?;
        let line = match wire::request_line(id, method, params) {
            Ok(line) => line,
            Err(e) => {
                self.unregister(id);
                return Err(e);
            }
        };
        if let Err(e) = self.inner().queue.push(Lane::Normal, line) {
            self.unregister(id);
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.unregister(id);
                Err(AcpError::Timeout { method: method.to_string(), secs: timeout.as_secs() })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AcpError::Closed),
        }
    }

    /// A request whose answer is delivered to `done` on the dispatcher thread, in order with the
    /// notifications before it. If this returns an error, `done` was not and will not be called.
    pub fn request_with(&self, method: &str, params: Value, done: Done) -> Result<u64, AcpError> {
        let id = self.register(Pending::Callback(done))?;
        let line = match wire::request_line(id, method, params) {
            Ok(line) => line,
            Err(e) => {
                self.unregister(id);
                return Err(e);
            }
        };
        if let Err(e) = self.inner().queue.push(Lane::Normal, line) {
            self.unregister(id);
            return Err(e);
        }
        Ok(id)
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), AcpError> {
        self.inner().queue.push(Lane::Normal, wire::notification_line(method, params)?)
    }

    /// For `session/cancel`: skips the line in front of every queued prompt.
    pub fn notify_priority(&self, method: &str, params: Value) -> Result<(), AcpError> {
        self.inner().queue.push(Lane::Priority, wire::notification_line(method, params)?)
    }

    /// Answers a request the agent sent. Priority lane: a permission reply is never stuck behind prompts.
    pub fn respond(&self, id: &Value, result: Value) -> Result<(), AcpError> {
        self.inner().queue.push(Lane::Priority, wire::result_line(id, result)?)
    }

    pub fn respond_error(&self, id: &Value, code: i64, message: &str) -> Result<(), AcpError> {
        self.inner().queue.push(Lane::Priority, wire::error_line(id, code, message)?)
    }
}

// ---- threads -----------------------------------------------------------------------------------------

fn writer_loop(inner: &Arc<Inner>, mut stdin: std::process::ChildStdin) {
    while let Some(line) = inner.queue.pop_blocking() {
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        inner.write_started_ms.store(inner.ms() + 1, Ordering::SeqCst);
        let result = stdin.write_all(&bytes).and_then(|()| stdin.flush());
        inner.write_started_ms.store(0, Ordering::SeqCst);
        if result.is_err() {
            break;
        }
    }
    inner.queue.close();
    // Dropping stdin tells a well-behaved adapter to finish.
}

fn stderr_loop(inner: &Arc<Inner>, stderr: std::process::ChildStderr) {
    let mut reader = BufReader::new(stderr);
    while let Ok(read) = wire::read_capped_line(&mut reader, 16 * 1024) {
        match read {
            LineRead::Line(line) => inner.stderr.push_line(&line),
            LineRead::TooLong { dropped_bytes } => inner.stderr.push_line(format!("(stderr line of {dropped_bytes} bytes dropped)").as_bytes()),
            LineRead::Eof => break,
        }
    }
    inner.stderr_done.store(true, Ordering::SeqCst);
}

fn reader_loop(inner: &Arc<Inner>, stdout: std::process::ChildStdout, frames: SyncSender<Frame>) {
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let send = |frame: Frame| -> bool {
        let depth = inner.stats.queued_events.fetch_add(1, Ordering::SeqCst) + 1;
        inner.stats.max_queued_events.fetch_max(depth, Ordering::SeqCst);
        if frames.send(frame).is_err() {
            inner.stats.queued_events.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    };
    loop {
        match wire::read_capped_line(&mut reader, inner.limits.max_line_bytes) {
            Ok(LineRead::Line(bytes)) => {
                inner.touch();
                match wire::parse_line(&bytes) {
                    Ok(message) => {
                        inner.stats.frames_in.fetch_add(1, Ordering::Relaxed);
                        if !send(Frame::Msg(message)) {
                            return;
                        }
                    }
                    Err(_) => {
                        // Not JSON-RPC (a banner, a log line, a test harness header). Keep a hint for
                        // diagnostics, never act on it.
                        inner.stats.bad_lines.fetch_add(1, Ordering::Relaxed);
                        let preview: Vec<u8> = bytes.iter().copied().take(200).collect();
                        let mut note = b"(stdout, not a message) ".to_vec();
                        note.extend_from_slice(&preview);
                        inner.stderr.push_line(&note);
                    }
                }
            }
            Ok(LineRead::TooLong { dropped_bytes }) => {
                inner.touch();
                inner.stats.oversized_lines.fetch_add(1, Ordering::Relaxed);
                inner.stderr.push_line(format!("(stdout line of {dropped_bytes} bytes dropped)").as_bytes());
            }
            Ok(LineRead::Eof) | Err(_) => {
                let _ = send(Frame::Eof);
                return;
            }
        }
    }
}

fn monitor_loop(inner: &Arc<Inner>, mut child: std::process::Child) {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                *inner.exit_code.lock() = Some(status.code());
                // The adapter is gone: whatever it left behind (its CLI, that CLI's tool servers) goes too,
                // which also closes any pipe handle they inherited so the reader reaches end of stream.
                inner.containment.terminate();
                inner.queue.close();
                return;
            }
            Ok(None) => {}
            Err(_) => {
                *inner.exit_code.lock() = Some(None);
                return;
            }
        }
        let started = inner.write_started_ms.load(Ordering::SeqCst);
        if started != 0 {
            let stuck = inner.ms().saturating_sub(started - 1);
            if Duration::from_millis(stuck) > inner.limits.stdin_write_timeout {
                inner.kill(ExitCause::WriteStalled);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn dispatch_loop(inner: &Arc<Inner>, frames: mpsc::Receiver<Frame>, handler: Arc<dyn ConnHandler>) {
    for frame in frames.iter() {
        inner.stats.queued_events.fetch_sub(1, Ordering::SeqCst);
        match frame {
            Frame::Msg(Inbound::Response { id, outcome }) => inner.resolve(&id, outcome),
            Frame::Msg(Inbound::Request { id, method, params }) => {
                let handler = handler.clone();
                run_guarded(move || handler.on_event(ConnEvent::AgentRequest { id, method, params }));
            }
            Frame::Msg(Inbound::Notification { method, params }) => {
                let handler = handler.clone();
                run_guarded(move || handler.on_event(ConnEvent::Notification { method, params }));
            }
            Frame::Eof => break,
        }
    }
    finish(inner, &handler);
}

/// The stream ended: settle the process, fail what is still waiting, say goodbye.
fn finish(inner: &Arc<Inner>, handler: &Arc<dyn ConnHandler>) {
    let waited = |limit: Duration| {
        let until = Instant::now() + limit;
        while Instant::now() < until {
            if inner.exit_code.lock().is_some() {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        inner.exit_code.lock().is_some()
    };
    let mut stdout_closed_first = false;
    if !waited(Duration::from_millis(1500)) {
        // stdout is closed but the process is not: it is no use to us.
        stdout_closed_first = true;
        inner.kill(ExitCause::StdoutClosed);
        waited(Duration::from_secs(3));
    }
    inner.containment.terminate();
    // The last words of the process are on stderr: give that thread a moment to finish reading them.
    let until = Instant::now() + Duration::from_secs(1);
    while !inner.stderr_done.load(Ordering::SeqCst) && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    inner.alive.store(false, Ordering::SeqCst);
    inner.queue.close();
    let recorded = *inner.kill_cause.lock();
    let cause = recorded.unwrap_or(if stdout_closed_first { ExitCause::StdoutClosed } else { ExitCause::Exited });
    let error = match cause {
        ExitCause::WriteStalled => AcpError::WriteStalled { secs: inner.limits.stdin_write_timeout.as_secs() },
        _ => AcpError::Closed,
    };
    inner.fail_all_pending(&error);
    let info = ExitInfo { code: inner.exit_code.lock().flatten(), cause, stderr_tail: inner.stderr.tail(20) };
    run_guarded(|| handler.on_event(ConnEvent::Exited(info)));
}

// ---- helpers other modules share ---------------------------------------------------------------------

/// `{"outcome": {"outcome": "cancelled"}}`.
pub fn cancelled_outcome() -> Value {
    json!({ "outcome": { "outcome": "cancelled" } })
}

/// `{"outcome": {"outcome": "selected", "optionId": id}}`.
pub fn selected_outcome(option_id: &str) -> Value {
    json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
}

const _: fn() = || {
    fn check<T: Send + Sync>() {}
    check::<AcpConnection>();
};
