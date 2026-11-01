//! Terminal chat: a chat on top of a real terminal agent. The agent runs in a normal pane; the Chat tab reads its
//! session file (`session_chat`) and sends by typing into the terminal (`pane_outbox`). See
//! `docs/specs/terminal-chat/HANDOFF.md`. Generic: what differs between agents lives in the profile trait.

pub mod lane;
pub mod start;

pub use lane::{available_for, TerminalChatInfo, LANE};
