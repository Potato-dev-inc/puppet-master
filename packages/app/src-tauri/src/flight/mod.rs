//! Flight recorder (K6): a small black box for every pane.
//!
//! Each pane keeps the last ~2 MB of what its PTY printed and what was typed into it, with
//! timestamps and with who wrote each input (`InputTag`). When a run ends with a failure code the
//! ring is frozen to `<project>/.puppet-master/operations/<op>/flight.cast` (asciicast v2, so any
//! asciinema player opens it), and on demand it can be exported. Everything that leaves the ring is
//! redacted first (`cast`): the live terminal is never touched.
//!
//! - `ring`: the buffer (hot path: one lock and one memcpy per chunk).
//! - `cast`: ring -> asciicast v2, redacted.
//! - `freeze`: where casts are written and the failure hook.
//! - `applog`: a tail of the app's own log for the diagnostic bundle.
//! - `bundle` / `zip`: the "Save a diagnostic file" bundle.
//!
//! ## Input tags and `WriteOrigin`
//!
//! `InputTag` is a plain enum so it can be mapped one to one from the `WriteOrigin` the typing-permit
//! branch puts on the write path:
//!
//! | `WriteOrigin`   | `InputTag`    |
//! |-----------------|---------------|
//! | `User(desktop)` | `User`        |
//! | `User(phone)`   | `Phone`       |
//! | `Task(permit)`  | `RunTask`     |
//! | `Gate(permit)`  | `GateDismiss` |
//! | `Answer(id)`    | `Answer`      |
//! | `Shell(permit)` | `Shell`       |
//!
//! `Nudge` (the supervisor's own Enter or clear-input) and `Agent` (an agent typing through the MCP
//! tools) have no `WriteOrigin` variant yet. Until that lands the tag is ambient: callers wrap their
//! write in `with_tag`, and `registry_write_input` records with `current_tag()` (default `User`).

pub mod applog;
pub mod bundle;
pub mod cast;
pub mod freeze;
pub mod ring;
pub mod zip;

#[cfg(test)]
mod tests;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub use ring::Ring;

/// Who wrote bytes into a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum InputTag {
    /// A keystroke or paste in a desktop terminal.
    User = 0,
    /// A keystroke from a paired phone.
    Phone = 1,
    /// The text of a task Puppet Master typed for a run.
    RunTask = 2,
    /// Keys that dismiss a startup screen or dialog.
    GateDismiss = 3,
    /// An answer to an agent's question or permission prompt.
    Answer = 4,
    /// Puppet Master's own Enter or clear-input to get a stuck pane moving.
    Nudge = 5,
    /// An agent typing into a pane through the MCP tools.
    Agent = 6,
    /// A command run through the shell tool.
    Shell = 7,
}

impl InputTag {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Phone => "phone",
            Self::RunTask => "run_task",
            Self::GateDismiss => "gate_dismiss",
            Self::Answer => "answer",
            Self::Nudge => "nudge",
            Self::Agent => "agent",
            Self::Shell => "shell",
        }
    }

    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Phone,
            2 => Self::RunTask,
            3 => Self::GateDismiss,
            4 => Self::Answer,
            5 => Self::Nudge,
            6 => Self::Agent,
            7 => Self::Shell,
            _ => Self::User,
        }
    }

    /// The tag for a write that arrived over the bridge: an MCP session is an agent, a paired phone
    /// (pairing token, no session) is a phone, anything else is the local app.
    pub fn for_bridge(has_mcp_session: bool, paired_phone: bool) -> Self {
        if has_mcp_session {
            Self::Agent
        } else if paired_phone {
            Self::Phone
        } else {
            Self::User
        }
    }
}

thread_local! {
    static AMBIENT: Cell<InputTag> = const { Cell::new(InputTag::User) };
}

/// The tag a write made on this thread right now carries.
pub fn current_tag() -> InputTag {
    AMBIENT.with(Cell::get)
}

/// Run `f` with every pane write made on this thread inside it tagged `tag`.
pub fn with_tag<R>(tag: InputTag, f: impl FnOnce() -> R) -> R {
    let previous = AMBIENT.with(|c| c.replace(tag));
    struct Restore(InputTag);
    impl Drop for Restore {
        fn drop(&mut self) {
            AMBIENT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(previous);
    f()
}

/// Like [`with_tag`], but an inner scope that already chose a tag keeps it (the default `User`
/// is not a choice).
pub fn with_default_tag<R>(tag: InputTag, f: impl FnOnce() -> R) -> R {
    if current_tag() == InputTag::User {
        with_tag(tag, f)
    } else {
        f()
    }
}

/// Rings of recently closed panes are kept so a failure that is reported after the pane closed can
/// still be frozen.
const KEEP_CLOSED: usize = 6;

#[derive(Default)]
struct Registry {
    live: HashMap<String, Arc<Ring>>,
    closed: VecDeque<(String, Arc<Ring>)>,
}

static REGISTRY: Lazy<Mutex<Registry>> = Lazy::new(|| Mutex::new(Registry::default()));

/// A fresh ring for a pane that is being (re)spawned. The reader thread keeps the `Arc` and writes
/// to it directly, so the hot path never touches the registry.
pub fn start(pane_id: &str, cols: u16, rows: u16) -> Arc<Ring> {
    let ring = Arc::new(Ring::new(cols, rows));
    let mut reg = REGISTRY.lock();
    reg.closed.retain(|(id, _)| id != pane_id);
    reg.live.insert(pane_id.to_string(), ring.clone());
    ring
}

/// The pane's ring, live or recently closed.
pub fn get(pane_id: &str) -> Option<Arc<Ring>> {
    let reg = REGISTRY.lock();
    reg.live
        .get(pane_id)
        .cloned()
        .or_else(|| reg.closed.iter().rev().find(|(id, _)| id == pane_id).map(|(_, ring)| ring.clone()))
}

/// The pane was closed: keep its ring for a while, then let it go.
pub fn closed(pane_id: &str) {
    let mut reg = REGISTRY.lock();
    if let Some(ring) = reg.live.remove(pane_id) {
        reg.closed.push_back((pane_id.to_string(), ring));
        while reg.closed.len() > KEEP_CLOSED {
            reg.closed.pop_front();
        }
    }
}

/// Record bytes written into a pane, tagged with the ambient writer.
pub fn input(pane_id: &str, bytes: &[u8]) {
    input_as(pane_id, current_tag(), bytes);
}

pub fn input_as(pane_id: &str, tag: InputTag, bytes: &[u8]) {
    if let Some(ring) = REGISTRY.lock().live.get(pane_id) {
        ring.input(tag, bytes);
    }
}

pub fn resized(pane_id: &str, cols: u16, rows: u16) {
    if let Some(ring) = REGISTRY.lock().live.get(pane_id) {
        ring.resize(cols, rows);
    }
}

/// A decision or fact about a pane ("status=waiting_input").
pub fn mark(pane_id: &str, text: &str) {
    if let Some(ring) = REGISTRY.lock().live.get(pane_id) {
        ring.mark(text);
    }
}
