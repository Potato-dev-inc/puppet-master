//! A chat view for an agent whose definition declares where its JSONL transcript is (`[session_chat]`).
//! `table` is the TOML table and its load-time check, `format` reads a line with its JSON pointers, `path` turns
//! the folder template into a folder under the home or pane folder, and `source` finds the pane's file.
//! Generic: no agent is named here.

mod format;
mod history_table;
mod path;
mod source;
mod table;

pub use history_table::{HistoryIdFrom, HistoryTable};
pub use path::{cwd_placeholders, Names};
pub use source::DeclaredSource;
pub use table::{validate_session_chat, SessionChatTable};

#[cfg(test)]
mod history_table_tests;
#[cfg(test)]
mod tests;
