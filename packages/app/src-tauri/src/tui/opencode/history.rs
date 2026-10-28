//! OpenCode's old chats, read from its SQLite database. Nothing here writes to the database: it is opened read-only
//! with `query_only` on, and a short busy timeout so a running OpenCode is never waited for long.
//!
//! Where the database is: `<data home>/opencode/opencode.db`. The data home is `XDG_DATA_HOME` when that variable is
//! set, then `~/.local/share` (OpenCode 1.19 uses the same folder on Windows; it does not use `%LOCALAPPDATA%` or
//! `%APPDATA%`). The first folder that has the file wins. `opencode.db` is the name for the release channels
//! (`latest`, `beta`, `prod`). `OPENCODE_DB` and the per-channel file names of dev builds are not read.
//!
//! Where the names come from (checked against the installed package, not against a live file): the `opencode` 1.19.0
//! binary of the `opencode-ai` npm package (`%APPDATA%\npm\node_modules\opencode-ai\node_modules\opencode-windows-x64\bin\opencode.exe`)
//! carries its own migration SQL and drizzle schema. From it:
//! - `CREATE TABLE session`: `id`, `project_id`, `parent_id` (NULL for a top-level chat), `directory`, `title`,
//!   `time_created`, `time_updated` (ms since the epoch).
//! - `CREATE TABLE message`: `id`, `session_id`, `time_created`, `time_updated`, `data` (JSON). Index
//!   `message_session_time_created_id_idx` on `(session_id, time_created, id)`.
//! - `CREATE TABLE part`: `id`, `message_id`, `session_id`, `time_created`, `time_updated`, `data` (JSON). Indexes
//!   `part_session_idx` and `part_message_id_id_idx`.
//! - The writer (the event projector) stores a message's `data` without its `id` and `sessionID`, and a part's `data`
//!   without `id`, `messageID` and `sessionID`. The columns hold them, so the reader puts them back. The result has
//!   the shape the HTTP API gives (`{info, parts}`), which `chat::map::map_messages` reads.
//! - Resume: the TUI option `--session <id>` (alias `-s`, "session id to continue") in the same binary.
//!
//! NOT verified on a live file: no OpenCode session was run to create a database (a session needs a prompt). The
//! layout above is what the binary's migrations create. The reader checks the three tables first, and a query that
//! fails on a missing table or column is `ReadError::Format`, so a layout change shows as "can't read", not as wrong items.
//!
//! The reader keeps the parts of a message together, reads a whole chat in time order and only then maps it, so
//! `map_messages` sees the same data it sees over HTTP (turn lengths depend on the order of messages).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, ErrorCode, OpenFlags};
use serde_json::{json, Value};

use super::chat::map::map_messages;
use crate::session_chat::history::{
    resume_args_from, valid_session_id, HistoryChat, HistoryEnv, ListLimits, Listing, ReadError, ReadLimits,
    ReadOutcome, SessionHistory,
};
use crate::session_chat::{home_to_tilde, plain_path, ChatItem};

/// The database file of the release channels.
const DB_FILE: &str = "opencode.db";
/// The folder under the data home.
const DATA_FOLDER: &str = "opencode";
/// How long a read waits for a lock another process holds before it gives up.
const BUSY: Duration = Duration::from_millis(500);
/// The tables a chat is read from. A database without all three is another layout.
const TABLES: [&str; 3] = ["session", "message", "part"];
/// The resume template: OpenCode's `--session <id>`.
const RESUME: [&str; 2] = ["--session", "{session}"];

/// Parent chats (sub-agent sessions have a parent and are left out), newest first. `bytes` is the size of the
/// chat's message and part data, which the importer counts against `max_file_bytes`.
const LIST_SQL: &str = "SELECT s.id, s.directory, s.time_updated,
       (SELECT COALESCE(SUM(length(CAST(m.data AS BLOB))), 0) FROM message m WHERE m.session_id = s.id)
     + (SELECT COALESCE(SUM(length(CAST(p.data AS BLOB))), 0) FROM part p WHERE p.session_id = s.id) AS bytes
FROM session s
WHERE s.parent_id IS NULL AND s.time_updated >= ?1
ORDER BY s.time_updated DESC, s.id";

/// The messages of one chat in time order (ties by id), and their data as stored.
const MESSAGES_SQL: &str = "SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created, id";
/// The parts of one chat in time order (ties by id).
const PARTS_SQL: &str = "SELECT id, message_id, data FROM part WHERE session_id = ?1 ORDER BY time_created, id";

/// OpenCode's old-chat store: its database, read-only.
pub struct OpenCodeHistory;

/// The store `OpenCode::session_history()` returns.
pub static HISTORY: OpenCodeHistory = OpenCodeHistory;

impl SessionHistory for OpenCodeHistory {
    fn list(&self, env: &HistoryEnv, limits: &ListLimits) -> Listing {
        let Some(path) = database(env) else {
            return Listing::default();
        };
        list_from(&path, env, limits).unwrap_or(Listing { unreadable: 1, ..Listing::default() })
    }

    fn read(
        &self,
        env: &HistoryEnv,
        session: &str,
        limits: &ReadLimits,
        sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
    ) -> Result<ReadOutcome, ReadError> {
        if !valid_session_id(session) {
            return Err(ReadError::Missing);
        }
        let path = database(env).ok_or(ReadError::Missing)?;
        let conn = open(&path).map_err(|err| read_error(&err))?;
        if !has_layout(&conn).map_err(|err| read_error(&err))? {
            return Err(ReadError::Format);
        }
        let known: i64 = conn
            .query_row("SELECT COUNT(*) FROM session WHERE id = ?1", [session], |row| row.get(0))
            .map_err(|err| read_error(&err))?;
        if known == 0 {
            return Err(ReadError::Missing);
        }
        let (messages, mut cut_short) = load_messages(&conn, session, limits.max_decompressed)?;
        let mut items = map_messages(&messages, &no_window);
        if items.len() > limits.max_items {
            items.truncate(limits.max_items);
            cut_short = true;
        }
        let chunk = limits.chunk.max(1);
        let mut outcome = ReadOutcome { items: 0, cut_short };
        let mut rest = items.into_iter();
        loop {
            let batch: Vec<ChatItem> = rest.by_ref().take(chunk).collect();
            if batch.is_empty() {
                break;
            }
            outcome.items += batch.len();
            if !sink(batch) {
                break;
            }
        }
        Ok(outcome)
    }

    fn resume_args(&self, session: &str) -> Option<Vec<String>> {
        let template: Vec<String> = RESUME.iter().map(|arg| arg.to_string()).collect();
        resume_args_from(&template, session)
    }

    fn places(&self, env: &HistoryEnv) -> Vec<String> {
        let home = env.home().map(|home| home.to_string_lossy().into_owned());
        data_folders(env).iter().map(|folder| shown_place(&folder.to_string_lossy(), home.as_deref())).collect()
    }
}

/// The folders that may hold the database, in the order they are tried: `XDG_DATA_HOME/opencode` when it is set,
/// then `~/.local/share/opencode`. The home folder comes from `HistoryEnv`, so a fixture home is never the real one.
fn data_folders(env: &HistoryEnv) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(data_home) = env.var("XDG_DATA_HOME") {
        folders.push(PathBuf::from(data_home).join(DATA_FOLDER));
    }
    if let Some(home) = env.home() {
        folders.push(home.join(".local").join("share").join(DATA_FOLDER));
    }
    folders
}

/// The database to read: the first data folder that has one. `None` when no folder has one.
fn database(env: &HistoryEnv) -> Option<PathBuf> {
    data_folders(env).into_iter().map(|folder| folder.join(DB_FILE)).find(|path| path.is_file())
}

/// Opens the file read-only: no writes, and a query-only connection on top of that.
///
/// A running OpenCode keeps a `-wal` (or a `-journal`) next to the file, so such a database is opened the normal
/// read-only way, with the busy timeout for its locks. When neither file exists, the database is not in use: it is
/// opened as `immutable`, so SQLite creates no `-shm` or `-wal` next to it (a normal read-only open of a WAL file does).
fn open(path: &Path) -> rusqlite::Result<Connection> {
    let in_use = ["-wal", "-journal"].iter().any(|suffix| sidecar(path, suffix).exists());
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
    let conn = if in_use {
        Connection::open_with_flags(file_uri(path, "mode=ro"), flags)?
    } else {
        Connection::open_with_flags(file_uri(path, "immutable=1"), flags)?
    };
    conn.busy_timeout(BUSY)?;
    conn.execute_batch("PRAGMA query_only = ON;")?;
    Ok(conn)
}

/// The file SQLite keeps next to `path` with this suffix (`opencode.db-wal`).
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The path as a SQLite `file:` URI with the query `query`. The characters a URI reserves are escaped, and the
/// separators are forward slashes.
fn file_uri(path: &Path, query: &str) -> String {
    let mut uri = String::from("file:");
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3f"),
            '#' => uri.push_str("%23"),
            '\\' => uri.push('/'),
            other => uri.push(other),
        }
    }
    uri.push('?');
    uri.push_str(query);
    uri
}

/// True when the database has the three tables this reader needs.
fn has_layout(conn: &Connection) -> rusqlite::Result<bool> {
    let found: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN (?1, ?2, ?3)",
        TABLES,
        |row| row.get(0),
    )?;
    Ok(found == TABLES.len() as i64)
}

/// A query that fails on a missing table or column (SQLite's generic error) is another layout: `Format`. A busy,
/// locked or damaged file is `Unreadable`.
fn read_error(err: &rusqlite::Error) -> ReadError {
    match err {
        // A statement that does not compile (rusqlite reports SQLite's generic error with the SQL and an offset).
        rusqlite::Error::SqlInputError { .. } => ReadError::Format,
        rusqlite::Error::SqliteFailure(failure, _) if failure.code == ErrorCode::Unknown => ReadError::Format,
        _ => ReadError::Unreadable,
    }
}

/// The listing of one database file. A layout that is not the expected one is an empty listing with one unreadable chat.
fn list_from(path: &Path, env: &HistoryEnv, limits: &ListLimits) -> rusqlite::Result<Listing> {
    let conn = open(path)?;
    if !has_layout(&conn)? {
        return Ok(Listing { unreadable: 1, ..Listing::default() });
    }
    let home = env.home().map(|home| home.to_string_lossy().into_owned());
    let started = Instant::now();
    let since = limits.since_ms.unwrap_or(i64::MIN);
    let mut out = Listing::default();
    let mut stmt = conn.prepare(LIST_SQL)?;
    let rows = stmt.query_map(params![since], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
    })?;
    for row in rows {
        if started.elapsed() >= limits.budget {
            out.cut_short = true;
            break;
        }
        let (session, directory, updated_ms, bytes) = row?;
        let bytes = bytes.max(0) as u64;
        if bytes > limits.max_file_bytes {
            out.too_big += 1;
            continue;
        }
        if !valid_session_id(&session) {
            out.unreadable += 1;
            continue;
        }
        if out.chats.len() >= limits.max_chats {
            out.cut_short = true;
            break;
        }
        let cwd = directory.filter(|dir| !dir.is_empty()).map(|dir| home_to_tilde(&dir, home.as_deref()));
        out.chats.push(HistoryChat { session, cwd, updated_ms, bytes, stamp: updated_ms.to_string() });
    }
    Ok(out)
}

/// The messages of one chat as `{info, parts}` objects, the shape the HTTP API gives. `info` is the message data
/// with its `id` and `sessionID`; each part is the part data with its `id`, `messageID` and `sessionID`. Rows whose
/// JSON is not an object are skipped. Stops (and says so) once the rows read add up to `max_bytes`.
fn load_messages(conn: &Connection, session: &str, max_bytes: u64) -> Result<(Vec<Value>, bool), ReadError> {
    let mut used: u64 = 0;
    let mut cut_short = false;
    let mut infos: Vec<(String, Value)> = Vec::new();
    let mut stmt = conn.prepare(MESSAGES_SQL).map_err(|err| read_error(&err))?;
    let rows = stmt
        .query_map([session], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|err| read_error(&err))?;
    for row in rows {
        let (id, data) = row.map_err(|err| read_error(&err))?;
        used += data.len() as u64;
        if used > max_bytes {
            cut_short = true;
            break;
        }
        if let Some(info) = object(&data) {
            infos.push((id.clone(), with_fields(info, &[("id", id.as_str()), ("sessionID", session)])));
        }
    }
    drop(stmt);

    let mut parts: HashMap<String, Vec<Value>> = HashMap::new();
    if !cut_short {
        let mut stmt = conn.prepare(PARTS_SQL).map_err(|err| read_error(&err))?;
        let rows = stmt
            .query_map([session], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|err| read_error(&err))?;
        for row in rows {
            let (id, message_id, data) = row.map_err(|err| read_error(&err))?;
            used += data.len() as u64;
            if used > max_bytes {
                cut_short = true;
                break;
            }
            if let Some(part) = object(&data) {
                let part = with_fields(
                    part,
                    &[("id", id.as_str()), ("messageID", message_id.as_str()), ("sessionID", session)],
                );
                parts.entry(message_id).or_default().push(part);
            }
        }
    }

    let messages = infos
        .into_iter()
        .map(|(id, info)| json!({ "info": info, "parts": parts.remove(&id).unwrap_or_default() }))
        .collect();
    Ok((messages, cut_short))
}

/// A JSON object from one stored value, or `None` when it is not one.
fn object(data: &str) -> Option<Value> {
    serde_json::from_str::<Value>(data).ok().filter(Value::is_object)
}

/// Adds string fields to a JSON object.
fn with_fields(mut value: Value, fields: &[(&str, &str)]) -> Value {
    if let Value::Object(map) = &mut value {
        for (key, field) in fields {
            map.insert(key.to_string(), Value::String(field.to_string()));
        }
    }
    value
}

/// No model context windows are needed for old chats: the turn length comes from the messages alone.
fn no_window(_provider: &str, _model: &str) -> Option<u64> {
    None
}

/// A folder as the screen shows it: the home folder as `~`, and one kind of separator (`/`).
fn shown_place(path: &str, home: Option<&str>) -> String {
    home_to_tilde(&plain_path(path), home).replace('\\', "/")
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
