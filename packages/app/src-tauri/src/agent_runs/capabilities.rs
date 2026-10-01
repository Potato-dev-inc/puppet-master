//! Honest per-backend capability contract. Do not derive flags from pane heuristics
//! except `visible_terminal` (true only when a pane exists).

use crate::operations::OperationSnapshot;
use serde::{Deserialize, Serialize};

pub const NOTE_INTERRUPT_ACK: &str =
    "interrupt acknowledgement is best-effort (native abort, TUI Ctrl+C, or no process); there is no provider-ack wait loop";
pub const NOTE_SELECTED_HISTORY: &str =
    "selected_history is a stored policy, not provider API replay";
pub const NOTE_ATTACH_TOKENS: &str =
    "attach tokens are in-memory only and do not survive process restart";
pub const NOTE_HEADLESS_INTERRUPT: &str =
    "headless interrupt stops the child; a follow-up starts a new child; pane kill is close_agent only";
pub const NOTE_LIVE_NATIVE_ONLY: &str =
    "live_messages is opencode_native only; other backends queue or report unsupported and never inject text into a busy TUI";
pub const NOTE_CURSOR_BOUNDARIES: &str =
    "cursor TUI response boundaries are version-specific screen heuristics, not protocol events";
pub const NOTE_CURSOR_QUESTIONS: &str =
    "cursor questions (trust / allow-once) are version-specific screen matches, not a stable question API";
pub const NOTE_CURSOR_INTERRUPT: &str =
    "cursor interrupt is Ctrl+C on a visible TUI or child stop when headless; not a graceful provider abort";
pub const NOTE_CURSOR_EXTRACT: &str =
    "cursor output extraction is headless JSON (--print) or TUI screen delta; exact parity across versions is not guaranteed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendInterface {
    NativeProtocol,
    StructuredHeadless,
    TuiObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub session_resume: bool,
    pub live_messages: bool,
    pub queued_followups: bool,
    pub structured_results: bool,
    pub model_selection: bool,
    pub reasoning_selection: bool,
    pub graceful_interrupt: bool,
    pub visible_terminal: bool,
    pub interface: BackendInterface,
    pub notes: Vec<String>,
}

impl BackendCapabilities {
    pub fn value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

pub fn read_only_supported(agent_type: &str, has_pane: bool) -> bool {
    !has_pane && READ_ONLY_BACKENDS.contains(&agent_type)
}

#[cfg(test)]
mod read_only_tests {
    use super::*;

    #[test]
    fn flag_matches_supported_backends_and_native_is_unsupported() {
        for backend in READ_ONLY_BACKENDS {
            assert!(read_only_supported(backend, false));
            assert!(!read_only_supported(backend, true));
        }
        assert!(!read_only_supported("opencode_native", false));
        assert!(!read_only_supported("opencode_native", true));
    }

    #[test]
    fn message_names_backend_and_supported_list() {
        let msg = read_only_unsupported_message("opencode_native", false);
        assert!(msg.contains("opencode_native"));
        assert!(msg.contains("claude, codex, cursor_agent"));
    }
}

/// Backends whose headless launch enforces read-only (see `launch_spec`).
pub const READ_ONLY_BACKENDS: &[&str] = &["claude", "codex", "cursor_agent"];

/// Clear error for `read_only=true` on a backend/mode that cannot enforce it.
pub fn read_only_unsupported_message(agent_type: &str, has_pane: bool) -> String {
    let mode = if has_pane { " in a shared terminal pane" } else { "" };
    format!(
        "read_only=true cannot be enforced for backend '{agent_type}'{mode}; read_only is supported only for headless runs on: {}. Retry with read_only=false or a supported backend",
        READ_ONLY_BACKENDS.join(", ")
    )
}

pub fn for_snapshot(snapshot: &OperationSnapshot) -> BackendCapabilities {
    for_backend(&snapshot.agent_type, snapshot.pane_id.is_some())
}

pub fn for_backend(agent_type: &str, has_pane: bool) -> BackendCapabilities {
    match agent_type {
        "opencode_native" => native_protocol(has_pane),
        "cursor_agent" | "cursor" => cursor_contract(has_pane),
        "codex" => cli_contract(has_pane, !has_pane),
        "claude" | "opencode" => cli_contract(has_pane, false),
        _ => shell_contract(has_pane),
    }
}

fn shared_ceilings() -> Vec<String> {
    vec![
        NOTE_INTERRUPT_ACK.into(),
        NOTE_SELECTED_HISTORY.into(),
        NOTE_ATTACH_TOKENS.into(),
        NOTE_LIVE_NATIVE_ONLY.into(),
    ]
}

fn native_protocol(has_pane: bool) -> BackendCapabilities {
    let mut notes = shared_ceilings();
    notes.push(NOTE_HEADLESS_INTERRUPT.into());
    BackendCapabilities {
        session_resume: true,
        live_messages: true,
        queued_followups: true,
        structured_results: true,
        model_selection: true,
        reasoning_selection: false,
        graceful_interrupt: true,
        visible_terminal: has_pane,
        interface: BackendInterface::NativeProtocol,
        notes,
    }
}

fn cli_contract(has_pane: bool, session_resume: bool) -> BackendCapabilities {
    let mut notes = shared_ceilings();
    notes.push(NOTE_HEADLESS_INTERRUPT.into());
    BackendCapabilities {
        session_resume,
        live_messages: false,
        queued_followups: true,
        structured_results: !has_pane,
        model_selection: false,
        reasoning_selection: false,
        graceful_interrupt: false,
        visible_terminal: has_pane,
        interface: if has_pane {
            BackendInterface::TuiObservation
        } else {
            BackendInterface::StructuredHeadless
        },
        notes,
    }
}

fn cursor_contract(has_pane: bool) -> BackendCapabilities {
    let mut caps = cli_contract(has_pane, false);
    caps.notes.extend([
        NOTE_CURSOR_BOUNDARIES.into(),
        NOTE_CURSOR_QUESTIONS.into(),
        NOTE_CURSOR_INTERRUPT.into(),
        NOTE_CURSOR_EXTRACT.into(),
    ]);
    caps
}

fn shell_contract(has_pane: bool) -> BackendCapabilities {
    let mut notes = shared_ceilings();
    notes.push(NOTE_HEADLESS_INTERRUPT.into());
    BackendCapabilities {
        session_resume: false,
        live_messages: false,
        queued_followups: true,
        structured_results: false,
        model_selection: false,
        reasoning_selection: false,
        graceful_interrupt: false,
        visible_terminal: has_pane,
        interface: BackendInterface::TuiObservation,
        notes,
    }
}
