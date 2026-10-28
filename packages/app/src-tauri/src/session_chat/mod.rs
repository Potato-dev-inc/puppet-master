//! A read-only chat view of a terminal pane, built from the session file the agent CLI writes, or, for an agent
//! that writes none Puppet Master can read, from the pane's own text (`screen`).
//! Approvals are not in those files; they stay with the screen reader. An agent that serves its session over a
//! local HTTP API is read from there (`api`), and from its screen while the API cannot be reached.
//! `tail` follows the file as it grows, `items` turns its lines into `ChatItem`s, `source` finds a pane's
//! file, `screen` reads the chat off the screen and `watch` keeps one reader per open pane and sends what it reads.

pub mod api;
mod claims;
pub mod declared;
pub mod detail;
pub mod export;
pub mod export_save;
mod export_style;
mod export_text;
pub mod home_path;
pub mod items;
pub mod model;
pub mod opening;
pub mod phone;
pub mod screen;
mod screen_first;
pub mod source;
pub mod store;
pub mod tail;
pub mod watch;

pub use api::{ApiEndpoint, ChatFrom, SessionApi};
pub use declared::{cwd_placeholders, validate_session_chat, DeclaredSource, Names, SessionChatTable};
pub use detail::{bound_input, cut_to, paths_from_input, PlanItem, PlanStatus, ToolDetail};
pub use home_path::home_to_tilde;
pub use opening::Opening;
pub use model::{cut, ChatItem, SessionFormat, TurnOutcome};
pub use source::{plain_path, LaunchFacts, Located, PaneFacts, SessionSource};
pub use screen::{read_pane, ScreenRead};
pub use watch::{Hold, ItemObserver, PaneLookup, Sink, Watchers};
