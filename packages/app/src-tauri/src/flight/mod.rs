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
//! The tag of a write comes from the `WriteOrigin` every pane write carries
//! (`From<&WriteOrigin> for InputTag`); `pty::registry::write_input` records it with one
//! [`input_as`] call:
//!
//! | `WriteOrigin` | `InputTag`    |
//! |---------------|---------------|
//! | `User`        | `User`        |
//! | `Task(permit)`| `RunTask`     |
//! | `Gate(permit)`| `GateDismiss` |
//! | `Answer`      | `Answer`      |
//! | `Nudge`       | `Nudge`       |
//! | `Api`         | `Agent`       |
//! | `Structured`  | `RunTask` (never reaches the terminal) |
//!
//! `User` and `Api` do not say WHO the person or caller is. The bridge knows (a paired phone, an
//! agent's MCP session, the local app), so its two write routes (`bridge/ws.rs`,
//! `bridge/routes/panes.rs`) wrap the write in [`with_tag`]; that tag refines a `User` or `Api`
//! origin and never overrides any other origin ([`tag_for`]).

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

impl From<&crate::pty::WriteOrigin<'_>> for InputTag {
    fn from(origin: &crate::pty::WriteOrigin<'_>) -> Self {
        use crate::pty::WriteOrigin;
        match origin {
            WriteOrigin::User => Self::User,
            WriteOrigin::Task(_) | WriteOrigin::Structured => Self::RunTask,
            WriteOrigin::Gate(_) => Self::GateDismiss,
            WriteOrigin::Answer => Self::Answer,
            WriteOrigin::Nudge => Self::Nudge,
            WriteOrigin::Api => Self::Agent,
        }
    }
}

thread_local! {
    /// Who the bridge says a `User` / `Api` write on this thread comes from (see [`with_tag`]).
    static BRIDGE_WRITER: Cell<Option<InputTag>> = const { Cell::new(None) };
}

/// Run `f` with the bridge's knowledge of who is writing (a paired phone, an agent's MCP session,
/// the local app): a `User` or `Api` write made on this thread inside it is tagged `tag`.
pub fn with_tag<R>(tag: InputTag, f: impl FnOnce() -> R) -> R {
    let previous = BRIDGE_WRITER.with(|c| c.replace(Some(tag)));
    struct Restore(Option<InputTag>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BRIDGE_WRITER.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(previous);
    f()
}

/// The tag a write with `origin` is recorded with: the origin's own, except that the bridge's
/// [`with_tag`] says who a `User` or `Api` writer is.
pub fn tag_for(origin: &crate::pty::WriteOrigin<'_>) -> InputTag {
    use crate::pty::WriteOrigin;
    match origin {
        WriteOrigin::User | WriteOrigin::Api => BRIDGE_WRITER.with(Cell::get).unwrap_or_else(|| origin.into()),
        _ => origin.into(),
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

/// Record bytes written into a pane by `tag`.
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
