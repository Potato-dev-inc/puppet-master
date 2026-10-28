//! Tests for OpenCode's old-chat reader. Every test works on a temp copy of the made-up fixture database
//! (`fixtures/history/opencode.db`), or on a database the test writes itself, and injects its folders through
//! `HistoryEnv`. No test opens the person's OpenCode folder.
//!
//! The fixture is rewritten from the rows below by the ignored test `regenerate_fixture_db`
//! (`cargo test --lib regenerate_fixture_db -- --ignored`), so the rows and the file cannot drift apart.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};

use super::super::chat::map::map_messages;
use super::{database, HISTORY};
use crate::session_chat::history::{
    HistoryEnv, ListLimits, Listing, ReadError, ReadLimits, ReadOutcome, SessionHistory,
};
use crate::session_chat::ChatItem;

/// The committed fixture (made-up text, `C:\Users\you` paths only).
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/opencode/fixtures/history/opencode.db");

const CHAT_A: &str = "ses_fixture0000a1"; // the newest parent chat: a question, a tool call, an answer
const CHAT_B: &str = "ses_fixture0000b2"; // an older parent chat in another folder
const CHILD: &str = "ses_fixture0000c3"; // a sub-agent session under CHAT_A: never listed

/// The table layout the reader expects (the columns of OpenCode 1.19's `session`, `message` and `part`, the foreign
/// keys left out). Indexes are the ones the binary's migrations create for these tables.
const DDL: &str = "
CREATE TABLE session (
  id text PRIMARY KEY, project_id text NOT NULL, workspace_id text, parent_id text, slug text NOT NULL,
  directory text NOT NULL, path text, title text NOT NULL, version text NOT NULL, share_url text,
  summary_additions integer, summary_deletions integer, summary_files integer, summary_diffs text, metadata text,
  cost real DEFAULT 0 NOT NULL, tokens_input integer DEFAULT 0 NOT NULL, tokens_output integer DEFAULT 0 NOT NULL,
  tokens_reasoning integer DEFAULT 0 NOT NULL, tokens_cache_read integer DEFAULT 0 NOT NULL,
  tokens_cache_write integer DEFAULT 0 NOT NULL, revert text, permission text, agent text, model text,
  time_created integer NOT NULL, time_updated integer NOT NULL, time_compacting integer, time_archived integer
);
CREATE TABLE message (
  id text PRIMARY KEY, session_id text NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL,
  data text NOT NULL
);
CREATE TABLE part (
  id text PRIMARY KEY, message_id text NOT NULL, session_id text NOT NULL, time_created integer NOT NULL,
  time_updated integer NOT NULL, data text NOT NULL
);
CREATE INDEX message_session_idx ON message (session_id);
CREATE INDEX message_session_time_created_id_idx ON message (session_id, time_created, id);
CREATE INDEX part_message_idx ON part (message_id);
CREATE INDEX part_message_id_id_idx ON part (message_id, id);
CREATE INDEX part_session_idx ON part (session_id);
";

/// Sessions: (id, parent, folder, title, time created, time updated), in ms since the epoch.
const SESSIONS: [(&str, Option<&str>, &str, &str, i64, i64); 3] = [
    (CHAT_A, None, r"C:\Users\you\demo-chat", "Made-up chat A", 1_788_000_000_000, 1_788_000_600_000),
    (CHAT_B, None, r"C:\Users\you\notes", "Made-up chat B", 1_787_000_000_000, 1_787_000_100_000),
    (CHILD, Some(CHAT_A), r"C:\Users\you\demo-chat", "Made-up sub-agent", 1_788_000_000_300, 1_788_000_400_000),
];

/// Messages: (id, session, time created, data without `id` and `sessionID`).
const MESSAGES: [(&str, &str, i64, &str); 4] = [
    (
        "msg_fixture_u1",
        CHAT_A,
        1_788_000_000_100,
        r#"{"role":"user","time":{"created":1788000000100},"agent":"build"}"#,
    ),
    (
        "msg_fixture_a1",
        CHAT_A,
        1_788_000_000_200,
        r#"{"role":"assistant","parentID":"msg_fixture_u1","modelID":"demo-model","providerID":"demo","mode":"build","path":{"cwd":"C:\\Users\\you\\demo-chat","root":"C:\\Users\\you\\demo-chat"},"time":{"created":1788000000200,"completed":1788000000900},"finish":"stop","cost":0}"#,
    ),
    (
        "msg_fixture_u2",
        CHAT_B,
        1_787_000_000_100,
        r#"{"role":"user","time":{"created":1787000000100},"agent":"build"}"#,
    ),
    (
        "msg_fixture_c1",
        CHILD,
        1_788_000_000_400,
        r#"{"role":"user","time":{"created":1788000000400},"agent":"explore"}"#,
    ),
];

/// Parts: (id, message, session, time created, data without `id`, `messageID` and `sessionID`).
const PARTS: [(&str, &str, &str, i64, &str); 7] = [
    (
        "prt_fixture_u1a",
        "msg_fixture_u1",
        CHAT_A,
        1_788_000_000_110,
        r#"{"type":"text","text":"Made-up question: what is in the sample folder?"}"#,
    ),
    ("prt_fixture_a1a", "msg_fixture_a1", CHAT_A, 1_788_000_000_210, r#"{"type":"step-start"}"#),
    (
        "prt_fixture_a1b",
        "msg_fixture_a1",
        CHAT_A,
        1_788_000_000_300,
        r#"{"type":"tool","callID":"call_fixture_1","tool":"glob","state":{"status":"completed","input":{"pattern":"*.txt","path":"C:\\Users\\you\\demo-chat"},"output":"notes.txt\nlist.txt","title":"glob","metadata":{},"time":{"start":1788000000300,"end":1788000000350}}}"#,
    ),
    (
        "prt_fixture_a1c",
        "msg_fixture_a1",
        CHAT_A,
        1_788_000_000_800,
        r#"{"type":"text","text":"Made-up answer: two text files are there.","time":{"start":1788000000800,"end":1788000000850}}"#,
    ),
    (
        "prt_fixture_u2a",
        "msg_fixture_u2",
        CHAT_B,
        1_787_000_000_110,
        r#"{"type":"text","text":"Made-up note: remember the blue folder."}"#,
    ),
    (
        "prt_fixture_c1a",
        "msg_fixture_c1",
        CHILD,
        1_788_000_000_410,
        r#"{"type":"text","text":"Made-up sub-agent text."}"#,
    ),
    (
        "prt_fixture_a1d",
        "msg_fixture_a1",
        CHAT_A,
        1_788_000_000_900,
        r#"{"type":"step-finish","reason":"stop","cost":0}"#,
    ),
];

/// Writes the fixture file from the rows above. Only the ignored test calls it.
fn write_fixture(path: &Path) {
    let conn = Connection::open(path).expect("fixture file");
    conn.execute_batch(&format!("PRAGMA page_size = 1024;{DDL}")).expect("fixture layout");
    for (id, parent, folder, title, created, updated) in SESSIONS {
        conn.execute(
            "INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, time_created, time_updated)
             VALUES (?1, 'prj_fixture0000', ?2, ?1, ?3, ?4, '1.19.0', ?5, ?6)",
            params![id, parent, folder, title, created, updated],
        )
        .expect("session row");
    }
    for (id, session, created, data) in MESSAGES {
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?3, ?4)",
            params![id, session, created, data],
        )
        .expect("message row");
    }
    for (id, message, session, created, data) in PARTS {
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            params![id, message, session, created, data],
        )
        .expect("part row");
    }
}

/// Rewrites the committed fixture. Run it after a row above changes:
/// `cargo test --lib regenerate_fixture_db -- --ignored`.
#[test]
#[ignore = "rewrites the committed fixture database"]
fn regenerate_fixture_db() {
    let path = Path::new(FIXTURE);
    if path.exists() {
        fs::remove_file(path).expect("old fixture");
    }
    write_fixture(path);
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_nanos());
    std::env::temp_dir().join(format!("pm-bc16-{tag}-{}-{nanos}", std::process::id()))
}

/// A temp data folder (`<share>/opencode`) holding a copy of `db`, and the env that points at it through
/// `XDG_DATA_HOME`. The home folder is a made-up Windows one, so the folder shows as `~`.
fn env_with_db(tag: &str, db: &Path) -> (PathBuf, HistoryEnv) {
    let share = temp_dir(tag);
    let folder = share.join("opencode");
    fs::create_dir_all(&folder).expect("data folder");
    fs::copy(db, folder.join("opencode.db")).expect("copy of the database");
    let share_text = share.to_string_lossy().into_owned();
    let env = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("XDG_DATA_HOME", share_text.as_str())]);
    (folder, env)
}

fn fixture_env(tag: &str) -> (PathBuf, HistoryEnv) {
    env_with_db(tag, Path::new(FIXTURE))
}

/// Every chat item one read gives, and the read's own result.
fn read_all(env: &HistoryEnv, session: &str, limits: &ReadLimits) -> (Vec<ChatItem>, Result<ReadOutcome, ReadError>) {
    let mut got = Vec::new();
    let outcome = HISTORY.read(env, session, limits, &mut |batch| {
        got.extend(batch);
        true
    });
    (got, outcome)
}

/// The message data as the reader builds it: the stored JSON with the fields the columns hold put back.
fn with_fields(data: &str, fields: &[(&str, &str)]) -> Value {
    let mut value: Value = serde_json::from_str(data).expect("fixture json");
    for (key, field) in fields {
        value[*key] = json!(field);
    }
    value
}

/// The `{info, parts}` objects of one fixture chat, the shape `map_messages` reads.
fn expected_messages(session: &str) -> Vec<Value> {
    MESSAGES
        .iter()
        .filter(|(_, owner, _, _)| *owner == session)
        .map(|(id, owner, _, data)| {
            let info = with_fields(data, &[("id", id), ("sessionID", owner)]);
            let parts: Vec<Value> = PARTS
                .iter()
                .filter(|(_, message, _, _, _)| message == id)
                .map(|(part_id, message, part_session, _, part)| {
                    with_fields(part, &[("id", part_id), ("messageID", message), ("sessionID", part_session)])
                })
                .collect();
            json!({ "info": info, "parts": parts })
        })
        .collect()
}

/// The names of the files in a folder, sorted.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .expect("folder")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn lists_parent_chats_newest_first_and_leaves_sub_agent_sessions_out() {
    let (_dir, env) = fixture_env("list");
    let listing = HISTORY.list(&env, &ListLimits::default());
    let ids: Vec<&str> = listing.chats.iter().map(|chat| chat.session.as_str()).collect();
    assert_eq!(ids, vec![CHAT_A, CHAT_B]);
    assert_eq!((listing.too_big, listing.unreadable, listing.cut_short), (0, 0, false));
}

#[test]
fn folder_time_stamp_and_size_come_from_the_rows() {
    let (_dir, env) = fixture_env("rows");
    let listing = HISTORY.list(&env, &ListLimits::default());
    let a = listing.chats.iter().find(|chat| chat.session == CHAT_A).expect("chat a");
    assert_eq!(a.cwd.as_deref(), Some(r"~\demo-chat"), "home is shown as ~, the rest as written");
    assert_eq!(a.updated_ms, 1_788_000_600_000);
    assert_eq!(a.stamp, "1788000600000", "an equal time means the chat did not change");
    assert!(a.bytes > 0);
    let b = listing.chats.iter().find(|chat| chat.session == CHAT_B).expect("chat b");
    assert_eq!(b.cwd.as_deref(), Some(r"~\notes"));
}

#[test]
fn reads_the_same_items_as_map_messages_on_the_same_rows() {
    let (_dir, env) = fixture_env("items");
    let (got, outcome) = read_all(&env, CHAT_A, &ReadLimits::default());
    assert_eq!(outcome, Ok(ReadOutcome { items: got.len(), cut_short: false }));
    let expected = map_messages(&expected_messages(CHAT_A), &|_, _| None);
    assert!(!expected.is_empty());
    assert_eq!(got, expected);
    assert!(
        got.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text.contains("sample folder"))),
        "the question is read back"
    );
}

#[test]
fn a_read_hands_its_items_over_in_chunks_and_stops_when_the_sink_says_so() {
    let (_dir, env) = fixture_env("chunks");
    let limits = ReadLimits { chunk: 1, ..ReadLimits::default() };
    let mut calls = 0;
    let outcome = HISTORY.read(&env, CHAT_A, &limits, &mut |batch| {
        assert_eq!(batch.len(), 1);
        calls += 1;
        calls < 2
    });
    assert_eq!(calls, 2, "the sink said stop after two items");
    assert_eq!(outcome, Ok(ReadOutcome { items: 2, cut_short: false }));
}

#[test]
fn a_read_stops_at_the_item_cap_or_the_byte_cap_and_says_so() {
    let (_dir, env) = fixture_env("caps");
    let items = ReadLimits { max_items: 2, ..ReadLimits::default() };
    let (got, outcome) = read_all(&env, CHAT_A, &items);
    assert_eq!(got.len(), 2);
    assert_eq!(outcome, Ok(ReadOutcome { items: 2, cut_short: true }));
    let bytes = ReadLimits { max_decompressed: 10, ..ReadLimits::default() };
    let (_, outcome) = read_all(&env, CHAT_A, &bytes);
    assert!(outcome.is_ok_and(|read| read.cut_short), "{outcome:?}");
}

#[test]
fn a_missing_chat_or_a_missing_database_is_missing_and_lists_nothing() {
    let (_dir, env) = fixture_env("missing");
    assert_eq!(read_all(&env, "ses_nothere0000", &ReadLimits::default()).1, Err(ReadError::Missing));
    assert_eq!(
        read_all(&env, "../etc/passwd", &ReadLimits::default()).1,
        Err(ReadError::Missing),
        "an invalid id is never read"
    );

    let empty = temp_dir("empty");
    fs::create_dir_all(&empty).expect("empty folder");
    let empty_text = empty.to_string_lossy().into_owned();
    let no_db = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("XDG_DATA_HOME", empty_text.as_str())]);
    assert_eq!(HISTORY.list(&no_db, &ListLimits::default()), Listing::default());
    assert_eq!(read_all(&no_db, CHAT_A, &ReadLimits::default()).1, Err(ReadError::Missing));
}

#[test]
fn a_database_with_another_layout_is_format_for_a_read_and_unreadable_for_a_listing() {
    // Other table names: the three tables are not there.
    let other_dir = temp_dir("other-names");
    fs::create_dir_all(&other_dir).expect("folder");
    let other = other_dir.join("opencode.db");
    Connection::open(&other)
        .and_then(|conn| {
            conn.execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY); CREATE TABLE messages (id TEXT);")
        })
        .expect("other layout");
    let (_dir, env) = env_with_db("other-names-env", &other);
    let listing = HISTORY.list(&env, &ListLimits::default());
    assert!(listing.chats.is_empty());
    assert_eq!(listing.unreadable, 1);
    assert_eq!(read_all(&env, CHAT_A, &ReadLimits::default()).1, Err(ReadError::Format));

    // The right table names, another column layout: a query fails on a missing column.
    let renamed_dir = temp_dir("other-columns");
    fs::create_dir_all(&renamed_dir).expect("folder");
    let renamed = renamed_dir.join("opencode.db");
    Connection::open(&renamed)
        .and_then(|conn| {
            conn.execute_batch(
                "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT);
                 CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT);
                 CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT);
                 INSERT INTO session (id, title) VALUES ('ses_fixture0000a1', 'Made-up');",
            )
        })
        .expect("renamed layout");
    let (_dir, env) = env_with_db("other-columns-env", &renamed);
    let listing = HISTORY.list(&env, &ListLimits::default());
    assert!(listing.chats.is_empty());
    assert_eq!(listing.unreadable, 1);
    assert_eq!(read_all(&env, CHAT_A, &ReadLimits::default()).1, Err(ReadError::Format));
}

#[test]
fn a_database_another_process_holds_gives_an_empty_listing_and_an_unreadable_read() {
    let (folder, env) = fixture_env("locked");
    let writer =
        Connection::open_with_flags(folder.join("opencode.db"), OpenFlags::SQLITE_OPEN_READ_WRITE).expect("writer");
    // A write inside the lock creates the rollback journal, the sign that the file is in use.
    writer
        .execute_batch(
            "BEGIN EXCLUSIVE; UPDATE session SET title = 'Made-up chat A (locked)' WHERE id = 'ses_fixture0000a1';",
        )
        .expect("lock");
    let listing = HISTORY.list(&env, &ListLimits::default());
    assert!(listing.chats.is_empty());
    assert_eq!(listing.unreadable, 1);
    assert_eq!(read_all(&env, CHAT_A, &ReadLimits::default()).1, Err(ReadError::Unreadable));
    writer.execute_batch("ROLLBACK;").expect("unlock");
    assert_eq!(HISTORY.list(&env, &ListLimits::default()).chats.len(), 2, "readable again once unlocked");
}

#[test]
fn reading_never_changes_the_database_file_or_its_folder() {
    let (folder, env) = fixture_env("readonly");
    let db = folder.join("opencode.db");
    let before_bytes = fs::read(&db).expect("bytes");
    let before = fs::metadata(&db).expect("metadata");
    let names_before = names_in(&folder);

    let listing = HISTORY.list(&env, &ListLimits::default());
    assert_eq!(listing.chats.len(), 2);
    assert_eq!(read_all(&env, CHAT_A, &ReadLimits::default()).1.map(|read| read.items > 0), Ok(true));
    assert_eq!(read_all(&env, CHAT_B, &ReadLimits::default()).1.map(|read| read.items > 0), Ok(true));

    let after = fs::metadata(&db).expect("metadata after");
    assert_eq!(after.len(), before.len(), "size unchanged");
    assert_eq!(after.modified().expect("mtime"), before.modified().expect("mtime before"), "mtime unchanged");
    assert_eq!(fs::read(&db).expect("bytes after"), before_bytes, "same bytes");
    assert_eq!(names_in(&folder), names_before, "no journal or other file appears next to it");
}

#[test]
fn list_caps_count_too_big_chats_cut_the_list_and_filter_by_time() {
    let (_dir, env) = fixture_env("caps-list");
    let too_big = ListLimits { max_file_bytes: 10, ..ListLimits::default() };
    let listing = HISTORY.list(&env, &too_big);
    assert!(listing.chats.is_empty());
    assert_eq!(listing.too_big, 2);

    let one = ListLimits { max_chats: 1, ..ListLimits::default() };
    let listing = HISTORY.list(&env, &one);
    assert_eq!(listing.chats.len(), 1);
    assert!(listing.cut_short);

    let recent = ListLimits { since_ms: Some(1_788_000_000_000), ..ListLimits::default() };
    let listing = HISTORY.list(&env, &recent);
    let ids: Vec<&str> = listing.chats.iter().map(|chat| chat.session.as_str()).collect();
    assert_eq!(ids, vec![CHAT_A], "the older chat is outside the window");
}

#[test]
fn the_xdg_folder_is_tried_first_and_the_home_folder_is_the_fallback() {
    let home = temp_dir("home-fallback");
    let home_folder = home.join(".local").join("share").join("opencode");
    fs::create_dir_all(&home_folder).expect("home data folder");
    fs::copy(FIXTURE, home_folder.join("opencode.db")).expect("home copy");
    let xdg_empty = temp_dir("xdg-empty");
    fs::create_dir_all(&xdg_empty).expect("empty xdg");
    let xdg_empty_text = xdg_empty.to_string_lossy().into_owned();
    let env = HistoryEnv::fixed(Some(home.clone()), &[("XDG_DATA_HOME", xdg_empty_text.as_str())]);
    assert_eq!(database(&env), Some(home_folder.join("opencode.db")), "no database under XDG, so the home one");
    assert_eq!(HISTORY.list(&env, &ListLimits::default()).chats.len(), 2);

    let xdg_full = temp_dir("xdg-full");
    let xdg_full_folder = xdg_full.join("opencode");
    fs::create_dir_all(&xdg_full_folder).expect("xdg data folder");
    fs::copy(FIXTURE, xdg_full_folder.join("opencode.db")).expect("xdg copy");
    let xdg_full_text = xdg_full.to_string_lossy().into_owned();
    let env = HistoryEnv::fixed(Some(home), &[("XDG_DATA_HOME", xdg_full_text.as_str())]);
    assert_eq!(database(&env), Some(xdg_full_folder.join("opencode.db")), "XDG wins when it has the database");
}

#[test]
fn resume_is_the_session_flag_with_a_checked_id() {
    assert_eq!(HISTORY.resume_args(CHAT_A), Some(vec!["--session".to_string(), CHAT_A.to_string()]));
    assert_eq!(HISTORY.resume_args("../etc/passwd"), None);
    assert_eq!(HISTORY.resume_args("abc;rm -rf"), None);
}

#[test]
fn places_show_the_data_folders_with_home_as_tilde() {
    let home_only = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[]);
    assert_eq!(HISTORY.places(&home_only), vec!["~/.local/share/opencode".to_string()]);
    let with_xdg = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("XDG_DATA_HOME", "D:/alt")]);
    assert_eq!(HISTORY.places(&with_xdg), vec!["D:/alt/opencode".to_string(), "~/.local/share/opencode".to_string()]);
    assert!(HISTORY.places(&HistoryEnv::isolated()).is_empty(), "no home, nothing to show");
}

#[test]
fn the_committed_fixture_is_small_and_has_the_rows_it_was_written_from() {
    let len = fs::metadata(FIXTURE).expect("fixture").len();
    assert!(len <= 40 * 1024, "{len} bytes");
    let conn = Connection::open_with_flags(FIXTURE, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("fixture");
    let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM session", [], |row| row.get(0)).expect("count");
    let messages: i64 = conn.query_row("SELECT COUNT(*) FROM message", [], |row| row.get(0)).expect("count");
    let parts: i64 = conn.query_row("SELECT COUNT(*) FROM part", [], |row| row.get(0)).expect("count");
    assert_eq!((sessions, messages, parts), (SESSIONS.len() as i64, MESSAGES.len() as i64, PARTS.len() as i64));
}

#[test]
fn a_wal_database_as_opencode_runs_it_is_read_without_changing_it() {
    let (folder, env) = fixture_env("wal");
    let db = folder.join("opencode.db");
    Connection::open(&db).and_then(|conn| conn.pragma_update(None, "journal_mode", "WAL")).expect("wal mode");
    let before_bytes = fs::read(&db).expect("bytes");
    let names_before = names_in(&folder);

    assert_eq!(HISTORY.list(&env, &ListLimits::default()).chats.len(), 2);
    assert!(read_all(&env, CHAT_A, &ReadLimits::default()).1.is_ok_and(|read| read.items > 0));

    assert_eq!(fs::read(&db).expect("bytes after"), before_bytes, "the database bytes are unchanged");
    assert_eq!(names_in(&folder), names_before, "no sidecar file appears next to it");
}

#[test]
fn a_wal_database_another_process_keeps_open_is_read_alongside_its_sidecars() {
    let (folder, env) = fixture_env("wal-open");
    let db = folder.join("opencode.db");
    let writer = Connection::open(&db).expect("writer");
    writer.pragma_update(None, "journal_mode", "WAL").expect("wal mode");
    writer
        .execute_batch("UPDATE session SET title = 'Made-up chat B (open)' WHERE id = 'ses_fixture0000b2';")
        .expect("write");
    assert!(folder.join("opencode.db-wal").exists(), "the running writer keeps its log");

    let listing = HISTORY.list(&env, &ListLimits::default());
    assert_eq!(listing.chats.len(), 2);
    assert!(read_all(&env, CHAT_A, &ReadLimits::default()).1.is_ok());
    drop(writer);
}
