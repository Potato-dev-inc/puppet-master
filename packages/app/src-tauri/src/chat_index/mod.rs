//! Search across every chat: a small SQLite file (FTS5) in the profile's app data folder that holds the person's
//! messages, the agents' replies and the titles of tool calls, so an answer from any agent on any day can be found
//! again (Ctrl+Shift+F). Generic: it knows items, never agents.
//!
//! - `entry`: what is kept of a chat item (user and assistant text, tool title; never tool output), redacted
//!   (`redact::text`) before anything is stored, with the pane's agent, folder name (not the path) and session.
//! - `store`: the SQLite file: upsert by (pane, session, item), the cap (newest `MAX_ITEMS` kept), search.
//! - `query`: turns what the person typed into an FTS5 query, and the marked snippet into text plus ranges.
//! - `feeder`: the background writer that `session_chat::Watchers` hands items to (`ItemObserver`), so a reader
//!   thread never waits on the disk.
//!
//! Only chats that were read (a Chat tab or the phone had the pane open) are in the index.

mod entry;
mod feeder;
mod query;
mod store;

#[cfg(test)]
mod tests;

pub use entry::PaneContext;
pub use feeder::{open_store, ChatIndex, ContextLookup};
pub use store::{Hit, MAX_ITEMS};

/// File name of the index inside the profile's app data folder.
pub const FILE_NAME: &str = "chat-index.sqlite";

/// The index of this profile (`<app data>/chat-index.sqlite`), fed by what `lookup` says about each pane. `None`
/// when the file cannot be opened: search is off, the chats are not affected.
pub fn open_default(lookup: std::sync::Arc<dyn ContextLookup>) -> Option<std::sync::Arc<ChatIndex>> {
    let dir = crate::app_paths::ensure_app_data_dir().ok()?;
    match open_store(&dir.join(FILE_NAME), MAX_ITEMS) {
        Ok(store) => Some(ChatIndex::start(store, lookup)),
        Err(error) => {
            tracing::warn!(%error, "chat index: search is off");
            None
        }
    }
}
