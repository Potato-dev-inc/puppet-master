//! A read-only chat view of a terminal pane, built from the session file the agent CLI writes, or, for an agent
//! that writes none Puppet Master can read, from the pane's own text (`screen`).
//! Approvals are not in those files; they stay with the screen reader.
//! `tail` follows the file as it grows, `items` turns its lines into `ChatItem`s, `source` finds a pane's
//! file, `screen` reads the chat off the screen and `watch` keeps one reader per open pane and sends what it reads.

pub mod items;
pub mod model;
pub mod screen;
pub mod source;
pub mod tail;
pub mod watch;

pub use model::{cut, ChatItem, SessionFormat, TurnOutcome};
pub use source::{plain_path, Located, PaneFacts, SessionSource};
pub use screen::{read_pane, ScreenRead};
pub use watch::{PaneLookup, Sink, Watchers};
