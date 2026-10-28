//! A declared agent's old chats: `[session_chat.history]` turned into the generic `FileHistory` store. The table's
//! roots, glob, depth and id rule become a `FileHistorySpec`; the chat lines are read with the same
//! `[session_chat.map]` as the pane's own transcript. Generic: no agent is named here.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use super::format::DeclaredSessionFile;
use super::history_table::{validate_history, HistoryIdFrom, HistoryTable};
use super::table::SessionChatTable;
use crate::session_chat::history::{FileHistory, FileHistorySpec, SessionHistory, SessionKey};
use crate::session_chat::SessionFormat;
use crate::tui::declarative::schema::DefError;

static STORES: Lazy<Mutex<HashMap<String, &'static dyn SessionHistory>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Builds the old-chat store of a declared agent. Namespace only: the store itself is a `FileHistory`.
pub struct DeclaredHistory;

impl DeclaredHistory {
    /// The store for `table`'s `[session_chat.history]`: `Ok(None)` when the table has none, `Err` with the problems
    /// when it is not usable. A spec is `'static`, so the store is leaked once per distinct table and loading the
    /// same definition again shares it.
    pub fn shared(table: &SessionChatTable) -> Result<Option<&'static dyn SessionHistory>, Vec<DefError>> {
        let Some(history) = &table.history else {
            return Ok(None);
        };
        let errors = validate_history(history, &table.file_glob);
        if !errors.is_empty() {
            return Err(errors);
        }
        let key = serde_json::to_string(table).unwrap_or_default();
        let mut stores = STORES.lock();
        if let Some(store) = stores.get(&key) {
            return Ok(Some(*store));
        }
        let store: &'static dyn SessionHistory = Box::leak(Box::new(FileHistory::new(spec_of(table, history))));
        stores.insert(key, store);
        Ok(Some(store))
    }
}

/// The spec for a table that passed `validate_history`. The strings and the format are leaked with the store.
fn spec_of(table: &SessionChatTable, history: &HistoryTable) -> FileHistorySpec {
    let glob = history.file_glob.clone().unwrap_or_else(|| table.file_glob.clone());
    // `validate_history` makes `id_pointer` required with `id_from = "pointer"`, so the pointer is always there then.
    let id = match (history.id_from, &history.id_pointer) {
        (HistoryIdFrom::Pointer, Some(pointer)) => SessionKey::Pointer(leak(pointer.clone())),
        _ => SessionKey::FileStem,
    };
    let format: &'static dyn SessionFormat = Box::leak(Box::new(DeclaredSessionFile::new(table)));
    FileHistorySpec {
        roots: leak_all(history.roots.clone()),
        extra_roots_env: &[],
        glob: leak(glob),
        skip: &[],
        depth: history.depth as usize,
        id,
        cwd_pointer: history.cwd_pointer.clone().map(leak),
        format,
        resume: leak_all(history.resume_args.clone()),
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn leak_all(items: Vec<String>) -> &'static [&'static str] {
    let refs: Vec<&'static str> = items.into_iter().map(leak).collect();
    Box::leak(refs.into_boxed_slice())
}
