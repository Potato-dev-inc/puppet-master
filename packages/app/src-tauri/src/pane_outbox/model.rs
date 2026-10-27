//! What the outbox keeps and shows: one message a person wrote for a pane's agent, and where it stands.

use serde::Serialize;

/// Most messages waiting (queued or being sent) in one pane.
pub const MAX_WAITING: usize = 5;
/// Longest message, in characters.
pub const MAX_CHARS: usize = 8_000;

pub const TOO_MANY: &str = "Too many waiting messages.";
pub const EMPTY: &str = "Write a message first.";
pub const TOO_LONG: &str = "The message is too long. The most is 8000 characters.";
pub const NOT_AN_AGENT: &str = "Chat replies need an agent pane.";
pub const NO_SUCH_PANE: &str = "This pane is not open any more.";
pub const RUN_CONTROLS: &str = "An agent run controls this pane. Use Terminal, or wait until the run ends.";
pub const NOT_SUPPORTED: &str = "Chat replies are not available for this agent yet.";
pub const NO_SUCH_MESSAGE: &str = "That message is not waiting any more.";
pub const ALREADY_SENDING: &str = "It is already being sent.";

pub const WAIT_BUSY: &str = "The agent is working. Sends when the turn ends.";
pub const WAIT_GATE: &str = "The agent asks something. Answer it first.";
pub const WAIT_DRAFT: &str = "The terminal box has text. Send or clear it there first.";
pub const WAIT_RUN: &str = "An agent run controls this pane. Sends when the run ends.";
pub const WAIT_TURN: &str = "Waits for the messages before it.";

pub const FAIL_NOT_RUNNING: &str = "The agent is not running in this pane.";
pub const FAIL_CLOSED: &str = "The pane was closed.";
pub const FAIL_TOO_LONG: &str = "Waited too long. Not sent.";
pub const FAIL_NOT_TAKEN: &str = "The agent did not take the message. Check the terminal.";
pub const FAIL_IN_BOX: &str = "The message may be in the terminal box, not sent. Check the terminal.";
pub const FAIL_WRITE: &str = "Could not type the message. Check the terminal.";

/// Where a message stands. Only `Queued` can be cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Queued,
    Sending,
    Sent,
    Failed,
    Cancelled,
}

impl State {
    /// Waiting or being typed: still the outbox's to do.
    pub fn active(self) -> bool {
        matches!(self, State::Queued | State::Sending)
    }
}

/// One message, as the desktop and the phone see it (camelCase JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxMessage {
    pub id: String,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub created_at: u64,
    pub state: State,
    /// While queued: one plain sentence saying what it waits for.
    pub waiting_for: Option<String>,
    /// When failed: one plain sentence.
    pub error: Option<String>,
}

/// A message with the clock readings the outbox itself needs (not shown).
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub message: OutboxMessage,
    pub created: std::time::Instant,
    /// When it reached a final state.
    pub finished: Option<std::time::Instant>,
}

impl Entry {
    pub fn finish(&mut self, state: State, error: Option<&str>) {
        self.message.state = state;
        self.message.waiting_for = None;
        self.message.error = error.map(str::to_string);
        self.finished = Some(std::time::Instant::now());
    }

    pub fn wait(&mut self, why: &str) {
        self.message.state = State::Queued;
        self.message.waiting_for = Some(why.to_string());
    }
}
