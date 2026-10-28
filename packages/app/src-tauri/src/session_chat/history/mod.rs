//! Old chats: an agent's whole store (every chat it kept on this PC), not only the panes that are open.
//! Each profile may answer `session_history()` with a `SessionHistory`; the importer (`chat_index/import/`) asks
//! every registered profile and does what the answer says. Nothing here names an agent.
//! Every read goes through `HistoryEnv`, so a test or a live check never reaches the person's real agent folders.
//! `env` is the only place that reads the process, `ids` checks the ids that end up on a command line, and
//! `limits` holds the caps (chats, bytes, items, time). Errors carry no path and no chat text.

mod env;
mod file;
mod ids;
mod limits;
mod open;
#[cfg(test)]
mod guard;
#[cfg(test)]
mod tests;

use crate::session_chat::ChatItem;

pub use env::HistoryEnv;
pub use ids::{resume_args_from, valid_session_id};
pub use limits::{ListLimits, ReadLimits};

/// One agent's store, read without writing anything.
pub trait SessionHistory: Send + Sync {
    /// Chats in this agent's own store, newest first, cheap (names, sizes, times, first lines for the folder).
    /// No full read.
    fn list(&self, env: &HistoryEnv, limits: &ListLimits) -> Listing;
    /// One chat's items in order, in chunks. `sink` returns false to stop. Never writes anything.
    fn read(
        &self,
        env: &HistoryEnv,
        session: &str,
        limits: &ReadLimits,
        sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
    ) -> Result<ReadOutcome, ReadError>;
    /// Arguments that start the agent on this old session; `None` when the agent has no resume option.
    fn resume_args(&self, session: &str) -> Option<Vec<String>>;
    /// Where it looks, for the screen: `~/.claude/projects`. Home already replaced by `~`.
    fn places(&self, env: &HistoryEnv) -> Vec<String>;
}

/// One chat found in an agent's store. `stamp` is `<len>:<mtime_ms>` (or the database time); an equal stamp
/// means the chat did not change since the last import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryChat {
    pub session: String,
    pub cwd: Option<String>,
    pub updated_ms: i64,
    pub bytes: u64,
    pub stamp: String,
}

/// What `list` found. `too_big` and `unreadable` count chats that were skipped; `cut_short` means a cap or the
/// time budget stopped the walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub chats: Vec<HistoryChat>,
    pub too_big: usize,
    pub unreadable: usize,
    pub cut_short: bool,
}

/// What `read` gave back. `cut_short` means a cap stopped the read before the end of the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOutcome {
    pub items: usize,
    pub cut_short: bool,
}

/// Why one chat could not be read. No path and no text inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    Missing,
    TooBig,
    Unreadable,
    Format,
}
