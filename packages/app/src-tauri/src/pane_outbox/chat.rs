//! The outbox over a chat session (ACP): the same queue and the same worker, with the chat session standing in
//! for the terminal. A message waits while a turn runs ("The agent is working. Sends when the turn ends.") and goes
//! in as a prompt when the session is ready; when the agent takes text while it works the message is steered into
//! the running turn instead (`steer`). Nothing here types into anything.

use super::host::{Attempt, Look};
use crate::acp::chat::{AcpChat, Readiness as ChatReadiness, SendError};
use crate::agent_runs::readiness::Readiness;
use crate::tui::ComposerState;

/// One look at a chat session: ready (idle), or busy. `None`: the chat was closed.
pub fn look(chat: &AcpChat) -> Option<Look> {
    let (readiness, idle) = match chat.readiness() {
        ChatReadiness::Ready => (Readiness::Ready, true),
        ChatReadiness::Busy => (Readiness::Busy, false),
        ChatReadiness::Gone => return None,
    };
    Some(Look { readiness, composer: ComposerState::Empty, idle, waiting: false })
}

/// Sends the message as a prompt.
pub fn type_message(chat: &AcpChat, text: &str) -> Attempt {
    match chat.send(text) {
        Ok(_) => Attempt::Sent,
        Err(SendError::Busy) => Attempt::NotTyped,
        Err(SendError::Gone) => Attempt::Gone,
        Err(SendError::Refused(why)) => Attempt::Failed(why),
    }
}

#[cfg(test)]
mod tests {
    // The behaviour with a real session (a fake agent) is tested in `acp/chat/tests.rs`.
    use super::*;

    #[test]
    fn a_closed_chat_has_no_look() {
        let _ = look as fn(&AcpChat) -> Option<Look>;
        let _ = type_message as fn(&AcpChat, &str) -> Attempt;
    }
}
