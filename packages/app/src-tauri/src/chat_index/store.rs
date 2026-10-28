//! The SQLite file behind the search: one row per kept chat item, an FTS5 index over its text, and the cap.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;

use super::aside;
use super::entry::Entry;
use super::query::{marks_of, match_expr, MARK_END, MARK_START};

/// Most items kept; past this the oldest go.
pub const MAX_ITEMS: usize = 50_000;
/// Most hits one search returns.
pub const MAX_HITS: usize = 100;
/// Words around a match in a snippet.
const SNIPPET_WORDS: i64 = 14;

const SCHEMA: &str = "
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

/// One search result. `marks`: `[start, end)` ranges of `snippet` to highlight, in UTF-16 code units.
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
    pub snippet: String,
    pub marks: Vec<[u32; 2]>,
}

pub struct Store {
    conn: Connection,
    cap: usize,
    /// The index file (`None` for a test database in memory); the old copies of a damaged index sit next to it.
    path: Option<PathBuf>,
}

impl Store {
    /// The index file at `path` (made when missing), keeping at most `cap` items.
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
        Ok(Store { conn, cap, path: None })
    }

    #[cfg(test)]
    pub fn count(&self) -> rusqlite::Result<usize> {
        self.conn.query_row("SELECT COUNT(*) FROM chat_items", [], |row| row.get::<_, i64>(0)).map(|n| n as usize)
    }

    /// The items kept and the bytes the file takes (its pages; the free ones, left by a clear, are not counted).
    pub fn usage(&self) -> rusqlite::Result<(usize, u64)> {
        let items: i64 = self.conn.query_row("SELECT COUNT(*) FROM chat_items", [], |row| row.get(0))?;
        let pages: i64 = self.conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
        let page_size: i64 = self.conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
        Ok((items.max(0) as usize, pages.max(0) as u64 * page_size.max(0) as u64))
    }

    /// Removes every item and its text from the file: the rows and the text index go (DELETE), the free pages are
    /// cut (VACUUM), and the write-ahead log is folded back and emptied, so no deleted text stays in either file.
    /// The index stays a valid, empty index.
    pub fn clear(&mut self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM chat_items", [])?;
        self.conn.execute("VACUUM", [])?;
        // One row back (frames in the log, frames checkpointed); there is nothing to read in it.
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
    /// its time. Then the oldest items past the cap go.
    pub fn insert(&mut self, entries: &[Entry]) -> rusqlite::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let cap = self.cap as i64;
        let tx = self.conn.transaction()?;
        {
            let mut put = tx.prepare_cached(
                "INSERT INTO chat_items (pane_id, session, item_id, kind, agent, folder, at_ms, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (pane_id, session, item_id) DO UPDATE
                 SET text = excluded.text, agent = excluded.agent, folder = excluded.folder, kind = excluded.kind
                 WHERE text IS NOT excluded.text OR agent IS NOT excluded.agent OR folder IS NOT excluded.folder",
            )?;
            for e in entries {
                let session = settle_session(&tx, e)?;
                put.execute(params![e.pane_id, session, e.item_id, e.kind, e.agent, e.folder, e.at_ms, e.text])?;
            }
        }
        tx.execute(
            "DELETE FROM chat_items WHERE id IN (
                 SELECT id FROM chat_items ORDER BY at_ms ASC, id ASC
                 LIMIT MAX(0, (SELECT COUNT(*) FROM chat_items) - ?1))",
            params![cap],
        )?;
        tx.commit()
    }

    /// The items whose text has every typed word (as a word start), newest first, at most `limit`.
    pub fn search(&self, typed: &str, limit: usize) -> rusqlite::Result<Vec<Hit>> {
        self.search_where(typed, limit, None, false)
    }

    /// Like `search`, but only inside the chats of the panes in `panes`, and only what was said (the person's and
    /// the agent's messages, not the titles of tool calls). No pane given means no hit: an empty list never means
    /// "everywhere".
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
            "SELECT i.pane_id, i.session, i.item_id, i.kind, i.agent, i.folder, i.at_ms,
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
            let marked: String = row.get(7)?;
            let (snippet, marks) = marks_of(&marked);
            Ok(Hit {
                pane_id: row.get(0)?,
                session: row.get(1)?,
                item_id: row.get(2)?,
                kind: row.get(3)?,
                agent: row.get(4)?,
                folder: row.get(5)?,
                at_ms: row.get(6)?,
                snippet,
                marks,
            })
        })?;
        rows.collect()
    }
}

/// The session an entry is stored under. An item can reach the index before the pane's session id is known (empty)
/// and again after it; both are the same item, so they must be one row:
/// - with a session, the empty-session row of the same pane and item is folded into it (it keeps its time and id);
/// - without one, the item joins the row it already has under a real session, if any.
fn settle_session(tx: &rusqlite::Transaction, e: &Entry) -> rusqlite::Result<String> {
    use rusqlite::OptionalExtension;
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
