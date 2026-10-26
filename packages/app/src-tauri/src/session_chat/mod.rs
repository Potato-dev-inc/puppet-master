//! A read-only chat view of a terminal pane, built from the session file the agent CLI writes.
//! Approvals are not in those files; they stay with the screen reader.
//! `tail` follows the file as it grows, `items` turns its lines into `ChatItem`s.

pub mod items;
pub mod model;
pub mod tail;

pub use items::read_lines;
pub use model::{cut, ChatItem, SessionFormat, TurnOutcome, MAX_OUTPUT_CHARS};
pub use tail::{Polled, Tail};
