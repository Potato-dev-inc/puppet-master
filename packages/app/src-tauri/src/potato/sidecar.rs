//! The IO of the brain process (spec sections 1.4 and 1.5): finding Node, starting the brain with an
//! empty environment and no window, reading its output, and stopping its whole tree. The decisions about
//! when to do that are in `keeper.rs` and `service.rs`; this file only does it.
//!
//! - Node is looked up with `ToolEnv { path: shell_env::path_for_spawn() }.resolve("node")`, then
//!   `node --version` (20 or newer) through `hidden_command` with a 3 second limit. The answer is cached
//!   for the launch while it is good, and re-asked at most every 30 seconds while it is not (so the person
//!   who installs Node does not have to restart the app).
//! - The process starts only through `proc_util::hidden_command`, so no window can appear.
//! - The environment is `BrainEnv` applied after `env_clear()`; stdin is null; stdout and stderr are read
//!   line by line into a sink (the log ring, which cleans every line).
//! - `PM_POTATO_BRAIN` replaces the script path, but only in a `dev-fake-tui` build (the pattern of
//!   `PM_E2E_PATH_PREPEND` in `shell_env.rs`).
//!
//! Tests start the same code with a fake `Launcher` or the fake brain script.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::env::BrainEnv;
use crate::worktrees::proc::ToolEnv;

pub const MIN_NODE_MAJOR: u32 = 20;
pub const VERSION_LIMIT: Duration = Duration::from_secs(3);
/// How long a bad answer about Node is trusted.
pub const RETRY_NODE_AFTER: Duration = Duration::from_secs(30);

/// What the PC has to run the brain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    Ready { path: PathBuf, major: u32 },
    /// No `node` on the path.
    Missing,
    /// There is one, but it is older than 20 (or it did not answer `--version`).
    TooOld { found: String },
}

impl NodeState {
    pub fn is_ready(&self) -> bool {
        matches!(self, NodeState::Ready { .. })
    }
}

/// The major number of `v22.20.0`, or `None` for anything else.
pub fn parse_major(version: &str) -> Option<u32> {
    let v = version.trim().trim_start_matches('v');
    v.split('.').next()?.parse().ok()
}

/// The state for a version answer.
pub fn judge(path: PathBuf, version: &str) -> NodeState {
    match parse_major(version) {
        Some(major) if major >= MIN_NODE_MAJOR => NodeState::Ready { path, major },
        _ => NodeState::TooOld { found: version.trim().chars().take(40).collect() },
    }
}

/// What a brain needs to start.
pub struct SpawnSpec {
    pub node: PathBuf,
    pub script: PathBuf,
    pub cwd: PathBuf,
    pub env: BrainEnv,
}

/// Where the brain's output lines go. Called from the reader threads.
pub type LineSink = Arc<dyn Fn(&str) + Send + Sync>;

/// A started brain process.
pub trait Child: Send {
    fn pid(&self) -> u32;
    /// The exit code once it has ended (`-1` when the system gave none), `None` while it runs.
    fn try_wait(&mut self) -> Option<i32>;
    /// Stops the process and everything it started, and waits for it.
    fn kill_tree(&mut self);
}

pub trait Launcher: Send + Sync {
    fn node(&self) -> NodeState;
    fn spawn(&self, spec: &SpawnSpec, sink: LineSink) -> Result<Box<dyn Child>, String>;
}

struct RealChild {
    child: std::process::Child,
}

impl Child for RealChild {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
            Ok(None) => None,
            Err(_) => Some(-1),
        }
    }

    fn kill_tree(&mut self) {
        crate::pty::proc_tree::kill_process_tree(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_lines<R: Read + Send + 'static>(stream: R, sink: LineSink) {
    let _ = std::thread::Builder::new().name("potato-brain-log".into()).spawn(move || {
        for line in BufReader::new(stream).lines() {
            match line {
                Ok(line) => sink(&line),
                Err(_) => break,
            }
        }
    });
}

/// The brain script to run: `PM_POTATO_BRAIN` in a dev build, else `brain.mjs` in the resource folder.
pub fn script_path(resources: Option<&Path>) -> Option<PathBuf> {
    #[cfg(feature = "dev-fake-tui")]
    if let Some(path) = std::env::var_os("PM_POTATO_BRAIN").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let script = resources?.join("brain.mjs");
    script.is_file().then_some(script)
}

/// True when `PM_POTATO_BRAIN` replaces the brain, which only a `dev-fake-tui` build allows. A stand-in brain
/// has no model to pay for, so the app asks it for no key (and never hands it the person's real one).
pub fn fake_brain_active() -> bool {
    #[cfg(feature = "dev-fake-tui")]
    {
        std::env::var_os("PM_POTATO_BRAIN").is_some_and(|p| !p.is_empty())
    }
    #[cfg(not(feature = "dev-fake-tui"))]
    {
        false
    }
}

/// Looks for Node and asks its version, with a time limit.
pub fn probe_node() -> NodeState {
    let tools = ToolEnv { path: Some(crate::shell_env::path_for_spawn().into()), extra: Vec::new() };
    let Some(path) = tools.resolve("node") else { return NodeState::Missing };
    let mut command = crate::proc_util::hidden_command(&path);
    command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else { return NodeState::TooOld { found: String::new() } };
    let mut out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = out.as_mut() {
            let _ = out.take(256).read_to_string(&mut text);
        }
        text
    });
    let deadline = Instant::now() + VERSION_LIMIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                crate::pty::proc_tree::kill_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let version = reader.join().unwrap_or_default();
    judge(path, &version)
}

/// The real launcher: Node from the PATH, the brain as a hidden child process.
#[derive(Default)]
pub struct RealLauncher {
    cache: Mutex<Option<(Instant, NodeState)>>,
}

impl RealLauncher {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Launcher for RealLauncher {
    fn node(&self) -> NodeState {
        let mut cache = self.cache.lock();
        if let Some((at, state)) = cache.as_ref() {
            if state.is_ready() || at.elapsed() < RETRY_NODE_AFTER {
                return state.clone();
            }
        }
        let state = probe_node();
        *cache = Some((Instant::now(), state.clone()));
        state
    }

    fn spawn(&self, spec: &SpawnSpec, sink: LineSink) -> Result<Box<dyn Child>, String> {
        let mut command = crate::proc_util::hidden_command(&spec.node);
        command.arg(&spec.script).current_dir(&spec.cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        spec.env.apply(&mut command);
        let mut child = command.spawn().map_err(|e| format!("could not start the brain: {e}"))?;
        if let Some(out) = child.stdout.take() {
            read_lines(out, sink.clone());
        }
        if let Some(err) = child.stderr.take() {
            read_lines(err, sink);
        }
        Ok(Box::new(RealChild { child }))
    }
}
