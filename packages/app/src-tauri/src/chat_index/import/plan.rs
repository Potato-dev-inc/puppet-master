//! What the importer plans: the agents it can read (`RegistryCatalog`), what each has (`scan`), and how one old chat
//! becomes rows of the index (`ChatDraft`). Items go through `entry_of_with` at `Strict` strength, the one function
//! that redacts, so a text can never reach a row without it.

use std::collections::HashSet;

use super::{Catalog, Found, Source};
use crate::chat_index::entry::{entry_of_with, scrub_text, Entry, PaneContext, Strength, MAX_IMPORTED_TEXT_CHARS};
use crate::chat_index::imported::{import_pane_id, ImportedChatRow};
use crate::chat_index::ChatIndex;
use crate::session_chat::history::{HistoryChat, HistoryEnv, ListLimits};
use crate::session_chat::{cut_to, home_to_tilde, ChatItem};

/// Longest title kept, in characters.
const TITLE_CHARS: usize = 120;
/// Longest folder path kept (it stays on this PC and is only used to resume).
const CWD_CHARS: usize = 1024;
/// The title of a chat with no message of the person.
const UNTITLED: &str = "Untitled chat";
/// An id that passes `valid_session_id`, to ask a store whether it has a resume option.
const SAMPLE_SESSION: &str = "sample-session-id";
const DAY_MS: i64 = 86_400_000;

/// Every registered profile that answers `session_history()`, built-in and from a TOML definition, in registry order.
/// An agent counts as "from a definition" when it is not one of the compiled-in profiles.
pub struct RegistryCatalog;

impl Catalog for RegistryCatalog {
    fn sources(&self) -> Vec<Source> {
        let registry = crate::tui::registry::registry();
        let code = registry.code_ids();
        registry
            .ordered()
            .iter()
            .filter_map(|profile| {
                let history = profile.session_history()?;
                let id = profile.id();
                let from_definition = !code.iter().any(|known| known.eq_ignore_ascii_case(id));
                // The screen turns the id into the agent's name (it knows them); the importer only carries the id.
                Some(Source { id: id.to_string(), label: id.to_string(), from_definition, history })
            })
            .collect()
    }
}

/// The oldest time a chat may have been updated at, for "the last N days" (`None` = all).
pub(super) fn since_ms(since_days: Option<u32>, now_ms: i64) -> Option<i64> {
    since_days.map(|days| now_ms.saturating_sub(i64::from(days).saturating_mul(DAY_MS)))
}

/// The catalog's sources whose id is in `agents`, once each, in catalog order. Unknown ids are ignored.
pub(super) fn chosen(sources: Vec<Source>, agents: &[String]) -> Vec<Source> {
    sources.into_iter().filter(|source| agents.iter().any(|id| *id == source.id)).collect()
}

/// `list` of every source in the catalog, summed up. Reads no chat.
pub(super) fn scan(catalog: &dyn Catalog, index: &ChatIndex, env: &HistoryEnv, since_days: Option<u32>, now_ms: i64) -> Vec<Found> {
    let limits = ListLimits { since_ms: since_ms(since_days, now_ms), ..ListLimits::default() };
    catalog
        .sources()
        .into_iter()
        .map(|source| {
            let listing = source.history.list(env, &limits);
            let times = || listing.chats.iter().map(|chat| chat.updated_ms);
            let already = listing
                .chats
                .iter()
                .filter(|chat| index.imported_stamp(&source.id, &chat.session).ok().flatten().as_deref() == Some(chat.stamp.as_str()))
                .count();
            Found {
                id: source.id.clone(),
                label: source.label.clone(),
                from_definition: source.from_definition,
                chats: listing.chats.len(),
                bytes: listing.chats.iter().map(|chat| chat.bytes).sum(),
                oldest_ms: times().min(),
                newest_ms: times().max(),
                already,
                too_big: listing.too_big,
                places: source.history.places(env),
                can_continue: source.history.resume_args(SAMPLE_SESSION).is_some(),
            }
        })
        .collect()
}

/// One old chat while it is read: collects the rows of the items that are kept (user text, assistant text, tool
/// title), strict-redacted, and builds the record at the end. Nothing is written until the whole chat was read.
pub(super) struct ChatDraft {
    agent: String,
    chat: HistoryChat,
    ctx: PaneContext,
    pane: String,
    entries: Vec<Entry>,
    seen: HashSet<String>,
}

impl ChatDraft {
    pub(super) fn new(agent: &str, chat: &HistoryChat) -> Self {
        let cwd = chat.cwd.as_deref().unwrap_or_default();
        ChatDraft {
            agent: agent.to_string(),
            chat: chat.clone(),
            ctx: PaneContext::of(agent, cwd, Some(&chat.session)),
            pane: import_pane_id(agent),
            entries: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Adds the kept items of a chunk. An item without a time gets the chat's last-update time; an item id seen twice
    /// is kept once.
    pub(super) fn push(&mut self, items: &[ChatItem]) {
        for item in items {
            let Some(entry) =
                entry_of_with(&self.pane, &self.ctx, item, self.chat.updated_ms, Strength::Strict, true, MAX_IMPORTED_TEXT_CHARS)
            else {
                continue;
            };
            if self.seen.insert(entry.item_id.clone()) {
                self.entries.push(entry);
            }
        }
    }

    /// The record and the rows of the chat. The title is the first user text on one line; the folder is the folder
    /// name, strict-redacted; the folder path keeps the home as `~` and stays on this PC.
    pub(super) fn finish(self, cut_short: bool, imported_ms: i64) -> (ImportedChatRow, Vec<Entry>) {
        let title = self.entries.iter().find(|entry| entry.kind == "user").map(|entry| one_line(&entry.text, TITLE_CHARS));
        let home = crate::redact::known::context().home.clone();
        let cwd = cut_to(&home_to_tilde(self.chat.cwd.as_deref().unwrap_or_default(), home.as_deref()), CWD_CHARS);
        let row = ImportedChatRow {
            agent: self.agent,
            session: self.chat.session.clone(),
            folder: scrub_text(&self.ctx.folder, Strength::Strict),
            cwd,
            title: title.filter(|title| !title.is_empty()).unwrap_or_else(|| UNTITLED.to_string()),
            updated_ms: self.chat.updated_ms,
            stamp: self.chat.stamp.clone(),
            items: self.entries.len(),
            cut_short,
            imported_ms,
        };
        (row, self.entries)
    }
}

/// `text` on one line (runs of white space become one space), cut to `limit` characters.
fn one_line(text: &str, limit: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    cut_to(&joined, limit.saturating_sub(1))
}
