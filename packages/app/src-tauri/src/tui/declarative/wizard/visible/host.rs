//! The panes the wizard owns, behind a trait (SPEC 11.3): the real pane registry in the app
//! (`LiveHost`), a scripted pane in unit tests. The capture engine and the visible test only talk to
//! `PaneHost`, so their state machines run without a process.
//!
//! There is deliberately no "type these keys" method. The wizard's only way to put text into a pane is
//! `deliver`, which is the app's own delivery path (`agent_runs::delivery::deliver_prompt_traced`): it
//! looks at the screen first and refuses a dialog, a shell prompt or a dead agent. Nothing here can
//! answer a trust or permission question.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;

use crate::tui::types::{Features, LaunchSpec, SpawnRequest};
use crate::tui::TuiProfile;

/// One look at a pane.
#[derive(Debug, Clone, Default)]
pub struct Look {
    /// The screen as readiness reads it (every cell, dimmed placeholder text included).
    pub screen: String,
    /// The screen as completion and approval detection read it.
    pub watch: String,
    /// The pane status string (`running`, `idle`, `waiting_input`, `error`).
    pub status: String,
    /// The agent process is still running.
    pub alive: bool,
    /// Bytes the pane has printed so far (a readiness look must still be the latest when typing).
    pub output_pos: Option<u64>,
    /// `hello.txt` exists in the pane's project folder (filled in by the runner, not the host).
    pub hello_written: bool,
    /// The grid the PTY has right now (columns, rows), when the host knows it.
    pub grid: Option<(u16, u16)>,
}

impl Look {
    pub fn running(&self) -> bool {
        self.status == "running"
    }
}

/// What a delivery did, in words the UI can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivered {
    /// Typed, verified and submitted (a turn was seen starting, or at least Enter went in).
    Submitted,
    /// Nothing was typed or submitted; the reason is plain text.
    Refused(String),
}

/// Everything needed to start one pane.
#[derive(Clone)]
pub struct PaneSpec {
    pub profile: Arc<dyn TuiProfile>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    /// Open the pane as a pop-out window (the real app; never in tests).
    pub window: bool,
}

pub trait PaneHost: Send + Sync + std::fmt::Debug {
    /// Start a pane. The profile of `spec` must already be registered (`Registry::add_transient`): the
    /// pane registry finds it by id.
    fn spawn(&self, spec: &PaneSpec) -> Result<String, String>;
    /// `None` when the pane is gone.
    fn look(&self, pane: &str) -> Option<Look>;
    /// Type `text` as a task through the app's delivery path. Blocks until delivery ends.
    fn deliver(&self, pane: &str, text: &str, trace: &mut Vec<String>) -> Delivered;
    /// The approval card the pane's screen makes (what the desktop shows), if any.
    fn card(&self, pane: &str) -> Option<Value>;
    /// The grid the PTY really has.
    fn grid(&self, pane: &str) -> Option<(u16, u16)>;
    /// Open the pane's window (a no-op without one).
    fn show(&self, pane: &str);
    fn kill(&self, pane: &str);
}

// ---- the capture profile -----------------------------------------------------------------------

static LEAKED: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// `TuiProfile::id` is `&'static str`; each distinct wizard id is leaked once (a session makes two).
pub fn intern(text: &str) -> &'static str {
    let mut set = LEAKED.lock();
    if let Some(hit) = set.get(text) {
        return hit;
    }
    let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// The profile of a capture pane: the program the person typed, the arguments they confirmed, and the
/// generic screen rules (the trait's defaults). It is found by the registry only while the wizard
/// owns the pane (`Registry::add_transient`).
pub struct CaptureProfile {
    id: &'static str,
    program: PathBuf,
    args: Vec<String>,
}

impl CaptureProfile {
    pub fn new(id: &str, program: PathBuf, args: Vec<String>) -> Self {
        Self { id: intern(id), program, args }
    }

    /// Program and arguments as the pane starts them: an npm `.cmd` wrapper runs under `cmd.exe /C`.
    pub fn command_line(program: &std::path::Path, args: &[String]) -> (String, Vec<String>) {
        let ext = program.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "cmd" || ext == "bat" {
            let mut all = vec!["/C".to_string(), program.to_string_lossy().into_owned()];
            all.extend(args.iter().cloned());
            ("cmd.exe".to_string(), all)
        } else {
            (program.to_string_lossy().into_owned(), args.to_vec())
        }
    }
}

impl TuiProfile for CaptureProfile {
    fn id(&self) -> &'static str {
        self.id
    }

    fn features(&self) -> Features {
        Features::CLI
    }

    fn experimental(&self) -> bool {
        true
    }

    fn spawn_command(&self, request: &SpawnRequest) -> Option<LaunchSpec> {
        let (program, args) = Self::command_line(&self.program, &self.args);
        Some(LaunchSpec::new(program, args, PathBuf::from(request.cwd), false))
    }
}

// ---- the real host -----------------------------------------------------------------------------

static INSTALLED: std::sync::OnceLock<Arc<dyn PaneHost>> = std::sync::OnceLock::new();

/// Install the host that talks to the app's pane registry (`RegistryHost`). The app does it at startup
/// and the end-to-end harness does it for its windowless app; nothing in the unit-test executables
/// does, which keeps the Tauri window code out of their link graph.
pub fn install(host: Arc<dyn PaneHost>) {
    let _ = INSTALLED.set(host);
}

/// What `WizardEnv::app()` uses: it hands every call to the installed host, and says so plainly when
/// the app is not up (a unit test, or a request before startup finished).
#[derive(Debug)]
pub struct LiveHost;

impl LiveHost {
    fn inner() -> Result<&'static Arc<dyn PaneHost>, String> {
        INSTALLED.get().ok_or_else(|| "the app is not ready yet (no pane registry)".to_string())
    }
}

impl PaneHost for LiveHost {
    fn spawn(&self, spec: &PaneSpec) -> Result<String, String> {
        Self::inner()?.spawn(spec)
    }

    fn look(&self, pane: &str) -> Option<Look> {
        Self::inner().ok()?.look(pane)
    }

    fn deliver(&self, pane: &str, text: &str, trace: &mut Vec<String>) -> Delivered {
        match Self::inner() {
            Ok(host) => host.deliver(pane, text, trace),
            Err(e) => Delivered::Refused(e),
        }
    }

    fn card(&self, pane: &str) -> Option<Value> {
        Self::inner().ok()?.card(pane)
    }

    fn grid(&self, pane: &str) -> Option<(u16, u16)> {
        Self::inner().ok()?.grid(pane)
    }

    fn show(&self, pane: &str) {
        if let Ok(host) = Self::inner() {
            host.show(pane);
        }
    }

    fn kill(&self, pane: &str) {
        if let Ok(host) = Self::inner() {
            host.kill(pane);
        }
    }
}
