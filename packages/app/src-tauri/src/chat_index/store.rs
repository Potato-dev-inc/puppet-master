//! The SQLite file behind the search: one row per kept chat item, an FTS5 index over its text, and the caps.
//!
//! Two kinds of rows share the file: live items (`imported = 0`, what the panes said) and imported items (`imported
//! = 1`, old chats read from the agents' own stores, see `imported.rs`). Each kind has its own cap, so an import never
//! pushes a live chat out, and a live write of a chat replaces the imported copy of that chat.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::aside;
use super::entry::Entry;
use super::imported::{import_pane_id, ImportedChatRow};
use super::query::{marks_of, match_expr, MARK_END, MARK_START};
use crate::redact;
use crate::session_chat::cut_to;

/// Most live items kept; past this the oldest live items go.
pub const MAX_ITEMS: usize = 50_000;
/// Most imported items kept; past this the oldest imported items go (live items are never touched by it).
pub const MAX_IMPORTED_ITEMS: usize = 150_000;
/// Longest title of an imported chat, in characters.
const MAX_IMPORTED_TITLE_CHARS: usize = 120;
/// Most hits one search returns.
pub const MAX_HITS: usize = 100;
/// Words around a match in a snippet.
const SNIPPET_WORDS: i64 = 14;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS chat_items (
    id INTEGER PRIMARY KEY,
    pane_id TEXT NOT NULL,
    session TEXT NOT NULL,
    item_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    agent TEXT NOT NULL,
    folder TEXT NOT NULL,
    at_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    UNIQUE (pane_id, session, item_id)
);
CREATE INDEX IF NOT EXISTS chat_items_at ON chat_items (at_ms);
CREATE VIRTUAL TABLE IF NOT EXISTS chat_fts USING fts5(text, content='chat_items', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER IF NOT EXISTS chat_items_ai AFTER INSERT ON chat_items BEGIN
    INSERT INTO chat_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS chat_items_ad AFTER DELETE ON chat_items BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER IF NOT EXISTS chat_items_au AFTER UPDATE ON chat_items BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO chat_fts (rowid, text) VALUES (new.id, new.text);
END;
";

/// The part of the schema that came with imports: the `imported` column (added to files made before it), its index,
/// the index on (agent, session) that the per-chat checks use, and one row per imported chat.
const IMPORT_SCHEMA: &str = "
CREATE INDEX IF NOT EXISTS chat_items_imported ON chat_items (imported, at_ms);
CREATE INDEX IF NOT EXISTS chat_items_session ON chat_items (agent, session);
CREATE TABLE IF NOT EXISTS import_chats (
    agent TEXT NOT NULL,
    session TEXT NOT NULL,
    folder TEXT NOT NULL,
    cwd TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    updated_ms INTEGER NOT NULL,
    stamp TEXT NOT NULL,
    items INTEGER NOT NULL,
    cut_short INTEGER NOT NULL DEFAULT 0,
    imported_ms INTEGER NOT NULL,
    PRIMARY KEY (agent, session)
);
";

/// One search result. `marks`: `[start, end)` ranges of `snippet` to highlight, in UTF-16 code units.
/// `imported`: the item is from an old chat that was read in (never inside a search of the panes).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub pane_id: String,
    pub session: String,
    pub item_id: String,
    pub kind: String,
    pub agent: String,
    pub folder: String,
    pub at_ms: i64,
    pub imported: bool,
    pub snippet: String,
    pub marks: Vec<[u32; 2]>,
}

pub struct Store {
    conn: Connection,
    /// Most live items kept.
    cap: usize,
    /// Most imported items kept.
    imported_cap: usize,
    /// The index file (`None` for a test database in memory); the old copies of a damaged index sit next to it.
    path: Option<PathBuf>,
}

impl Store {
    /// The index file at `path` (made when missing), keeping at most `cap` live items.
    pub fn open(path: &Path, cap: usize) -> rusqlite::Result<Store> {
        let mut store = Store::init(Connection::open(path)?, cap)?;
        store.path = Some(path.to_path_buf());
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_memory(cap: usize) -> rusqlite::Result<Store> {
        Store::init(Connection::open_in_memory()?, cap)
    }

    fn init(conn: Connection, cap: usize) -> rusqlite::Result<Store> {
        // Wait briefly instead of failing when another connection (a second process on the same profile) writes.
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Store { conn, cap, imported_cap: MAX_IMPORTED_ITEMS, path: None })
    }

    #[cfg(test)]
    pub fn count(&self) -> rusqlite::Result<usize> {
        self.conn.query_row("SELECT COUNT(*) FROM chat_items", [], |row| row.get::<_, i64>(0)).map(|n| n as usize)
    }

    /// Lowers the imported cap (tests only: the real one is `MAX_IMPORTED_ITEMS`).
    #[cfg(test)]
    pub fn set_imported_cap(&mut self, cap: usize) {
        self.imported_cap = cap;
    }

    /// The live items kept and the bytes the file takes (its pages; the free ones, left by a clear, are not counted).
    /// Imported items are not counted here (see `imported_usage`).
    pub fn usage(&self) -> rusqlite::Result<(usize, u64)> {
        let items: i64 = self.conn.query_row("SELECT COUNT(*) FROM chat_items WHERE imported = 0", [], |row| row.get(0))?;
        let pages: i64 = self.conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
        let page_size: i64 = self.conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
        Ok((items.max(0) as usize, pages.max(0) as u64 * page_size.max(0) as u64))
    }

    /// Removes every item (live and imported) and every imported-chat record from the file: the rows and the text
    /// index go (DELETE), the free pages are cut (VACUUM), and the write-ahead log is folded back and emptied, so no
    /// deleted text stays in either file. The index stays a valid, empty index.
    pub fn clear(&mut self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM chat_items", [])?;
        self.conn.execute("DELETE FROM import_chats", [])?;
        self.conn.execute("VACUUM", [])?;
        // One row back (frames in the log, frames checkpointed); there is nothing to read in it.
        let _ = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        Ok(())
    }

    /// Removes only the imported chats (their items and their records), with the same erasing as `clear`. Live items
    /// stay.
    pub fn clear_imported(&mut self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM chat_items WHERE imported = 1", [])?;
        self.conn.execute("DELETE FROM import_chats", [])?;
        self.conn.execute("VACUUM", [])?;
        let _ = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        Ok(())
    }

    /// How many old copies of a damaged index are next to the file (see `aside`).
    pub fn old_copies(&self) -> usize {
        self.path.as_deref().map_or(0, aside::count)
    }

    /// Removes the old copies of a damaged index next to the file; how many files went.
    pub fn remove_old_copies(&self) -> usize {
        self.path.as_deref().map_or(0, aside::remove_all)
    }

    /// `PRAGMA integrity_check` of the open file: "ok" when it is sound.
    #[cfg(test)]
    pub fn integrity(&self) -> rusqlite::Result<String> {
        self.conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))
    }

    /// Adds the entries; one already there (same pane, session and item) gets its text and labels replaced and keeps
    /// its time. A live entry of a chat that was imported first removes the imported copy of that chat (same agent
    /// and session). Then the oldest live items past the cap go; imported items are not counted.
    pub fn insert(&mut self, entries: &[Entry]) -> rusqlite::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let cap = self.cap as i64;
        let tx = self.conn.transaction()?;
        {
            let mut put = tx.prepare_cached(
                "INSERT INTO chat_items (pane_id, session, item_id, kind, agent, folder, at_ms, text, imported)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (pane_id, session, item_id) DO UPDATE
                 SET text = excluded.text, agent = excluded.agent, folder = excluded.folder, kind = excluded.kind
                 WHERE text IS NOT excluded.text OR agent IS NOT excluded.agent OR folder IS NOT excluded.folder",
            )?;
            for e in entries {
                let session = settle_session(&tx, e)?;
                if !e.imported && !session.is_empty() {
                    drop_imported_copy(&tx, &e.agent, &session)?;
                }
                put.execute(params![e.pane_id, session, e.item_id, e.kind, e.agent, e.folder, e.at_ms, e.text, e.imported as i64])?;
            }
        }
        tx.execute(
            "DELETE FROM chat_items WHERE imported = 0 AND id IN (
                 SELECT id FROM chat_items WHERE imported = 0 ORDER BY at_ms ASC, id ASC
                 LIMIT MAX(0, (SELECT COUNT(*) FROM chat_items WHERE imported = 0) - ?1))",
            params![cap],
        )?;
        tx.commit()
    }

    /// Writes one imported chat in one transaction: its old imported items go, its new items (in the chat's import
    /// pane) and its record go in, the imported cap is applied. A killed app or a Stop leaves whole chats only, and an
    /// error leaves the chat as it was. Live rows are never touched. The pane, session and agent come from `chat`; the
    /// title is strict-redacted and cut to 120 characters here too.
    pub fn replace_chat(&mut self, chat: &ImportedChatRow, entries: &[Entry]) -> rusqlite::Result<()> {
        let cap = self.imported_cap as i64;
        let pane = import_pane_id(&chat.agent);
        // `cut_to` adds its "…" on top of the limit, so the title stays within 120 characters with one less.
        let title = cut_to(redact::rules::mask_emails(redact::strict(&chat.title).as_str()).trim(), MAX_IMPORTED_TITLE_CHARS - 1);
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM chat_items WHERE imported = 1 AND agent = ?1 AND session = ?2", params![chat.agent, chat.session])?;
        {
            let mut put = tx.prepare_cached(
                "INSERT INTO chat_items (pane_id, session, item_id, kind, agent, folder, at_ms, text, imported)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            )?;
            for e in entries {
                put.execute(params![pane, chat.session, e.item_id, e.kind, e.agent, e.folder, e.at_ms, e.text])?;
            }
        }
        tx.execute(
            "INSERT INTO import_chats (agent, session, folder, cwd, title, updated_ms, stamp, items, cut_short, imported_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (agent, session) DO UPDATE
             SET folder = excluded.folder, cwd = excluded.cwd, title = excluded.title, updated_ms = excluded.updated_ms,
                 stamp = excluded.stamp, items = excluded.items, cut_short = excluded.cut_short, imported_ms = excluded.imported_ms",
            params![
                chat.agent,
                chat.session,
                chat.folder,
                chat.cwd,
                title,
                chat.updated_ms,
                chat.stamp,
                chat.items as i64,
                chat.cut_short as i64,
                chat.imported_ms
            ],
        )?;
        let pruned = tx.execute(
            "DELETE FROM chat_items WHERE imported = 1 AND id IN (
                 SELECT id FROM chat_items WHERE imported = 1 ORDER BY at_ms ASC, id ASC
                 LIMIT MAX(0, (SELECT COUNT(*) FROM chat_items WHERE imported = 1) - ?1))",
            params![cap],
        )?;
        if pruned > 0 {
            // A record whose items all went with the cap has nothing left to show.
            tx.execute(
                "DELETE FROM import_chats WHERE NOT EXISTS (
                     SELECT 1 FROM chat_items i WHERE i.imported = 1 AND i.agent = import_chats.agent AND i.session = import_chats.session)",
                [],
            )?;
        }
        tx.commit()
    }

    /// The stamp an imported chat was read with (`None` when it was never imported).
    pub fn imported_stamp(&self, agent: &str, session: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT stamp FROM import_chats WHERE agent = ?1 AND session = ?2", params![agent, session], |row| row.get(0))
            .optional()
    }

    /// Whether a live chat (the panes said it) is in the index under this agent and session.
    pub fn has_live(&self, agent: &str, session: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM chat_items WHERE imported = 0 AND agent = ?1 AND session = ?2)",
            params![agent, session],
            |row| row.get(0),
        )
    }

    /// The record of one imported chat, if any.
    pub fn imported_chat(&self, agent: &str, session: &str) -> rusqlite::Result<Option<ImportedChatRow>> {
        self.conn
            .query_row(
                "SELECT agent, session, folder, cwd, title, updated_ms, stamp, items, cut_short, imported_ms
                 FROM import_chats WHERE agent = ?1 AND session = ?2",
                params![agent, session],
                |row| {
                    Ok(ImportedChatRow {
                        agent: row.get(0)?,
                        session: row.get(1)?,
                        folder: row.get(2)?,
                        cwd: row.get(3)?,
                        title: row.get(4)?,
                        updated_ms: row.get(5)?,
                        stamp: row.get(6)?,
                        items: row.get::<_, i64>(7)?.max(0) as usize,
                        cut_short: row.get::<_, i64>(8)? != 0,
                        imported_ms: row.get(9)?,
                    })
                },
            )
            .optional()
    }

    /// How many imported chats and how many imported items the index holds.
    pub fn imported_usage(&self) -> rusqlite::Result<(usize, usize)> {
        let chats: i64 = self.conn.query_row("SELECT COUNT(*) FROM import_chats", [], |row| row.get(0))?;
        let items: i64 = self.conn.query_row("SELECT COUNT(*) FROM chat_items WHERE imported = 1", [], |row| row.get(0))?;
        Ok((chats.max(0) as usize, items.max(0) as usize))
    }

    /// The items whose text has every typed word (as a word start), newest first, at most `limit`.
    pub fn search(&self, typed: &str, limit: usize) -> rusqlite::Result<Vec<Hit>> {
        self.search_where(typed, limit, None, false)
    }

    /// Like `search`, but only inside the chats of the panes in `panes`, and only what was said (the person's and
    /// the agent's messages, not the titles of tool calls). No pane given means no hit: an empty list never means
    /// "everywhere". Imported items have their own pane ids, so they are never found here.
    pub fn search_in(&self, typed: &str, limit: usize, panes: &[String]) -> rusqlite::Result<Vec<Hit>> {
        if panes.is_empty() {
            return Ok(Vec::new());
        }
        self.search_where(typed, limit, Some(panes), true)
    }

    fn search_where(&self, typed: &str, limit: usize, panes: Option<&[String]>, said_only: bool) -> rusqlite::Result<Vec<Hit>> {
        let Some(expr) = match_expr(typed) else { return Ok(Vec::new()) };
        // ?1 query, ?2 limit, ?3 snippet words, then one placeholder per pane.
        let mut filter = String::new();
        if let Some(panes) = panes {
            let marks: Vec<String> = (0..panes.len()).map(|n| format!("?{}", n + 4)).collect();
            filter.push_str(&format!(" AND i.pane_id IN ({})", marks.join(", ")));
        }
        if said_only {
            filter.push_str(" AND i.kind IN ('user', 'assistant')");
        }
        let sql = format!(
            "SELECT i.pane_id, i.session, i.item_id, i.kind, i.agent, i.folder, i.at_ms, i.imported,
                    snippet(chat_fts, 0, char(1), char(2), '…', ?3)
             FROM chat_fts JOIN chat_items i ON i.id = chat_fts.rowid
             WHERE chat_fts MATCH ?1{filter}
             ORDER BY i.at_ms DESC, i.id DESC
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut values: Vec<rusqlite::types::Value> =
            vec![expr.into(), (limit.clamp(1, MAX_HITS) as i64).into(), SNIPPET_WORDS.into()];
        values.extend(panes.unwrap_or(&[]).iter().map(|pane| rusqlite::types::Value::from(pane.clone())));
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |row| {
            let marked: String = row.get(8)?;
            let (snippet, marks) = marks_of(&marked);
            Ok(Hit {
                pane_id: row.get(0)?,
                session: row.get(1)?,
                item_id: row.get(2)?,
                kind: row.get(3)?,
                agent: row.get(4)?,
                folder: row.get(5)?,
                at_ms: row.get(6)?,
                imported: row.get::<_, i64>(7)? != 0,
                snippet,
                marks,
            })
        })?;
        rows.collect()
    }
}

/// Adds the parts of the schema that came with imports to a file made before them. Guarded: the column is added only
/// when it is missing, and the existing rows keep their place (they count as live).
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let columns: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(chat_items)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()?
    };
    if !columns.iter().any(|name| name == "imported") {
        conn.execute_batch("ALTER TABLE chat_items ADD COLUMN imported INTEGER NOT NULL DEFAULT 0;")?;
    }
    conn.execute_batch(IMPORT_SCHEMA)
}

/// A live write of a chat removes the imported copy of the same chat (same agent and session): the live one wins.
fn drop_imported_copy(tx: &rusqlite::Transaction, agent: &str, session: &str) -> rusqlite::Result<()> {
    let there: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM import_chats WHERE agent = ?1 AND session = ?2)
             OR EXISTS (SELECT 1 FROM chat_items WHERE imported = 1 AND agent = ?1 AND session = ?2)",
        params![agent, session],
        |row| row.get(0),
    )?;
    if there {
        tx.execute("DELETE FROM chat_items WHERE imported = 1 AND agent = ?1 AND session = ?2", params![agent, session])?;
        tx.execute("DELETE FROM import_chats WHERE agent = ?1 AND session = ?2", params![agent, session])?;
    }
    Ok(())
}

/// The session an entry is stored under. An item can reach the index before the pane's session id is known (empty)
/// and again after it; both are the same item, so they must be one row:
/// - with a session, the empty-session row of the same pane and item is folded into it (it keeps its time and id);
/// - without one, the item joins the row it already has under a real session, if any.
fn settle_session(tx: &rusqlite::Transaction, e: &Entry) -> rusqlite::Result<String> {
    if e.session.is_empty() {
        let known: Option<String> = tx
            .query_row(
                "SELECT session FROM chat_items WHERE pane_id = ?1 AND item_id = ?2 AND session <> '' ORDER BY id DESC LIMIT 1",
                params![e.pane_id, e.item_id],
                |row| row.get(0),
            )
            .optional()?;
        return Ok(known.unwrap_or_default());
    }
    let taken: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM chat_items WHERE pane_id = ?1 AND item_id = ?2 AND session = ?3)",
        params![e.pane_id, e.item_id, e.session],
        |row| row.get(0),
    )?;
    if taken {
        tx.execute("DELETE FROM chat_items WHERE pane_id = ?1 AND item_id = ?2 AND session = ''", params![e.pane_id, e.item_id])?;
    } else {
        tx.execute(
            "UPDATE chat_items SET session = ?3 WHERE pane_id = ?1 AND item_id = ?2 AND session = ''",
            params![e.pane_id, e.item_id, e.session],
        )?;
    }
    Ok(e.session.clone())
}

// The marks the SQL passes to snippet() are `char(1)` and `char(2)`.
const _: () = assert!(MARK_START as u32 == 1 && MARK_END as u32 == 2);
