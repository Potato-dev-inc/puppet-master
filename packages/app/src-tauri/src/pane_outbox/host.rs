//! What the outbox needs from the outside: the pane it waits on, a look at that pane, typing into it, and a
//! way to tell the windows and phones. The live pane registry implements it (`live.rs`); tests use a fake, so
//! the state machine runs without a terminal.

use super::model::OutboxMessage;
use crate::agent_runs::readiness::{Readiness, Verdict};
use crate::tui::{ComposerState, Kind};

/// What kind of pane a message was sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// An agent's terminal: messages are typed into its input box.
    Agent,
    /// A plain shell.
    Shell,
    /// An agent driven over its own protocol, not typed into.
    Protocol,
}

impl PaneKind {
    pub fn of(kind: Kind) -> Self {
        match kind {
            Kind::Shell => PaneKind::Shell,
            Kind::NativeProtocol => PaneKind::Protocol,
            Kind::Cli | Kind::Unknown => PaneKind::Agent,
        }
    }
}

/// One look at a pane: what the readiness check says (`agent_runs::readiness::assess`), what the input box
/// holds, and whether the pane's own status reads idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub readiness: Readiness,
    pub composer: ComposerState,
    pub idle: bool,
}

impl Look {
    pub fn from_verdict(verdict: &Verdict, idle: bool) -> Self {
        Self { readiness: verdict.readiness().clone(), composer: verdict.composer(), idle }
    }
}

/// How one attempt to type a message ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    /// Typed and submitted.
    Sent,
    /// Nothing was typed: the input box holds someone's text. Wait.
    Draft,
    /// Nothing was typed: a dialog or question is up. Wait for the person to answer it.
    Gate,
    /// Nothing was typed: the pane was not ready. Wait.
    NotTyped,
    /// Nothing was typed and never will be: the agent is gone.
    Gone,
    /// It did not work; the sentence says what to tell the person.
    Failed(String),
}

pub trait Host: Send + Sync + 'static {
    /// The kind of pane, or `None` when there is no such pane (closed).
    fn pane_kind(&self, pane_id: &str) -> Option<PaneKind>;
    /// An agent run (`run_agent`) is working in the pane.
    fn run_controls(&self, pane_id: &str) -> bool;
    /// One look; `None` when the pane is gone.
    fn look(&self, pane_id: &str) -> Option<Look>;
    /// Types the text and presses Enter, under the typing permit, without clearing what the person typed.
    fn type_message(&self, pane_id: &str, text: &str) -> Attempt;
    /// Tells the windows and phones the pane's messages changed (the whole list, newest last).
    fn publish(&self, pane_id: &str, messages: &[OutboxMessage]);
}
