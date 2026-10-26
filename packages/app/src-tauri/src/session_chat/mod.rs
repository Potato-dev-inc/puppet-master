//! A read-only chat view of a terminal pane, built from the session file the agent CLI writes.
//! Approvals are not in those files; they stay with the screen reader.
//! `tail` follows the file as it grows, `items` turns its lines into `ChatItem`s, `source` finds a pane's
//! file and `watch` keeps one reader per open pane and sends what it reads.

pub mod items;
pub mod model;
pub mod source;
pub mod tail;
pub mod watch;

pub use model::{cut, ChatItem, SessionFormat, TurnOutcome};
pub use source::{Located, PaneFacts, SessionSource};
pub use watch::{PaneLookup, Sink, Watchers};
