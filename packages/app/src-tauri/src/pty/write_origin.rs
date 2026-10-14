//! Who is writing into a pane, on every write (`registry::write_input`).
//!
//! The invariant (`agent_runs::readiness`): Puppet Master types a run's input only with a
//! [`TypingPermit`] and presses a startup screen's keys only with a [`GatePermit`]. Keystrokes a
//! person sends (desktop terminal, phone, cockpit buttons) are theirs and pass unchanged; so do
//! answers to a prompt that was re-detected on screen, control keys Puppet Master sends on its own
//! (interrupt, exit, a second Enter for text already verified in the box), and raw input an
//! authorised API caller asked for. The tag is also what the event log and a flight recorder can
//! name as the writer.

use crate::agent_runs::readiness::{GatePermit, TypingPermit};

#[derive(Debug, Clone, Copy)]
pub enum WriteOrigin<'a> {
    /// A person's keystrokes from the desktop or a phone (terminal view, cockpit buttons, the
    /// second-opinion "send to author" a person clicked).
    User,
    /// A run's task text, with its clear sequence and Enter. Needs a fresh permit.
    Task(&'a TypingPermit),
    /// A startup screen's own dismiss keys. Needs a gate permit for exactly those keys.
    Gate(&'a GatePermit),
    /// The answer to a prompt the caller re-detected on screen right before (approval cards, the
    /// run policy's auto-approve).
    Answer,
    /// Control keys Puppet Master sends on its own: interrupt, cancel, exit, clearing an unsent
    /// prompt, one more Enter for text already verified in the input box.
    Nudge,
    /// Raw input an authorised API caller asked for (`write_terminal_input`, `press_key`).
    Api,
    /// A prompt sent over the agent's own protocol (OpenCode's HTTP API), never typed into the
    /// screen. Refused on the terminal path.
    Structured,
}

impl WriteOrigin<'_> {
    /// Short stable name for logs.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Task(_) => "task",
            Self::Gate(_) => "gate",
            Self::Answer => "answer",
            Self::Nudge => "nudge",
            Self::Api => "api",
            Self::Structured => "structured",
        }
    }

    /// The write path's check for a terminal write. `emitted_now` is the pane's output position
    /// right now; `agent` reads the agent process (only called for a permit's first write).
    pub(crate) fn admit_terminal_write(
        &self,
        pane_id: &str,
        text: &str,
        append_newline: bool,
        emitted_now: Option<u64>,
        agent: impl FnOnce() -> crate::pty::agent_proc::AgentProcess,
    ) -> Result<(), String> {
        match self {
            Self::Task(permit) => permit.admit(pane_id, emitted_now, agent),
            Self::Gate(permit) => permit.admit(pane_id, text, append_newline, emitted_now),
            Self::Structured => Err(format!(
                "{}: a structured prompt is never typed into the terminal",
                crate::agent_runs::readiness::PERMIT_REFUSED
            )),
            Self::User | Self::Answer | Self::Nudge | Self::Api => Ok(()),
        }
    }
}
