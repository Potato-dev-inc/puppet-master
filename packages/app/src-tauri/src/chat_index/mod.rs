//! Search across every chat: a small SQLite file (FTS5) in the profile's app data folder that holds the person's
//! messages, the agents' replies and the titles of tool calls, so an answer from any agent on any day can be found
//! again (Ctrl+Shift+F). Generic: it knows items, never agents.
//!
//! - `entry`: what is kept of a chat item (user and assistant text, tool title; never tool output), redacted
//!   (`redact::text`) before anything is stored, with the pane's agent, folder name (not the path) and session.
//! - `store`: the SQLite file: upsert by (pane, session, item), the cap (newest `MAX_ITEMS` kept), search.
//! - `filter`: the chips' filters (agent, folder, start time, who spoke) as bound SQL parameters.
//! - `query`: turns what the person typed into an FTS5 query, and the marked snippet into text plus ranges.
//! - `feeder`: the background writer that `session_chat::Watchers` hands items to (`ItemObserver`), so a reader
//!   thread never waits on the disk.
//!
//! Only chats that were read (a Chat tab or the phone had the pane open) are in the index.

mod aside;
mod entry;
mod feeder;
mod imported;
mod filter;
mod query;
mod store;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod import_api_tests;

pub use entry::{entry_of_with, PaneContext, Strength};
pub use feeder::{open_store, Answer, ChatIndex, ContextLookup, Status};
pub use imported::ImportedChatRow;
pub use filter::{Facets, Filters, Role};
pub use store::{Hit, MAX_ITEMS};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use once_cell::sync::Lazy;
use serde_json::Value;

/// File name of the index inside the profile's app data folder.
pub const FILE_NAME: &str = "chat-index.sqlite";
/// The setting in `settings.json` that turns the index on or off ("Keep a search index of my chats"). On by default.
pub const SETTING_KEY: &str = "chat_index_enabled";

/// The on/off switch of this process, shared by every index it opens. Starts on, as the setting does.
static SWITCH: Lazy<Arc<AtomicBool>> = Lazy::new(|| Arc::new(AtomicBool::new(true)));

/// The switch the index reads (`ChatIndex::enabled`).
pub fn switch() -> Arc<AtomicBool> {
    SWITCH.clone()
}

/// Whether the index is on for this process.
pub fn enabled() -> bool {
    SWITCH.load(Ordering::SeqCst)
}

/// Turns the index on or off for this process. The saved setting is written by `settings_patch`.
pub fn set_enabled(on: bool) {
    SWITCH.store(on, Ordering::SeqCst);
}

/// Takes the switch from a settings object (`settings.json`'s `settings`, or a patch) when it holds `chat_index_enabled`.
pub fn apply_setting(settings: &Value) {
    if let Some(on) = settings.get(SETTING_KEY).and_then(Value::as_bool) {
        set_enabled(on);
    }
}

/// The index of this profile (`<app data>/chat-index.sqlite`), fed by what `lookup` says about each pane. `None`
/// when the file cannot be opened: search is off, the chats are not affected.
pub fn open_default(lookup: Arc<dyn ContextLookup>) -> Option<Arc<ChatIndex>> {
    let dir = crate::app_paths::ensure_app_data_dir().ok()?;
    match open_store(&dir.join(FILE_NAME), MAX_ITEMS) {
        Ok(store) => Some(ChatIndex::start_switched(store, lookup, switch())),
        Err(error) => {
            tracing::warn!(%error, "chat index: search is off");
            None
        }
    }
}
