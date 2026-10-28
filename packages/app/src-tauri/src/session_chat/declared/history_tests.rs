//! `[session_chat.history]` turned into a store: a definition with the table lists and reads its own folder, one
//! without it has no old chats, and a table is built once. Every store here reads a temp folder this test writes
//! (the acme transcript fixture), through `HistoryEnv::fixed`. No test reads a real agent folder.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::DeclaredHistory;
use crate::session_chat::history::{HistoryEnv, ListLimits, ReadError, ReadLimits};
use crate::session_chat::{ChatItem, SessionHistory};
use crate::tui::declarative::{build_for_test, parse_definition};
use crate::tui::TuiProfile;

const ACME: &str = include_str!("../../tui/declarative/fixtures/definitions/acme-chat.toml");
const MINIMAL: &str = include_str!("../../tui/declarative/fixtures/definitions/minimal-tui.toml");
const TRANSCRIPT: &str = include_str!("../../tui/declarative/fixtures/acme/session.jsonl");

const CHAT: &str = "3f2a9c10-7b4e-4d21-9a6c-1e5f0b7d8a01";
const OTHER: &str = "5c8e1d22-0a9b-4f3e-b7c6-2d4a8e9f0b02";
/// The acme transcript has eleven items (the same list the Chat tab test expects).
const TRANSCRIPT_ITEMS: usize = 11;

const ROOTS: &str = "roots = [\"{home}/.acme/sessions\"]\nresume_args = [\"--resume\", \"{session}\"]\n";

/// The acme definition with a `[session_chat.history]` table added.
fn acme_with(history: &str) -> String {
    format!("{ACME}\n[session_chat.history]\n{history}")
}

/// The store a definition declares: `Ok(None)` without the table, the problems (code and path) when it is not usable.
fn store_of(text: &str) -> Result<Option<&'static dyn SessionHistory>, Vec<(&'static str, String)>> {
    let def = parse_definition(text).expect("the definition parses");
    let table = def.session_chat.as_ref().expect("a session_chat table");
    DeclaredHistory::shared(table).map_err(|errors| errors.into_iter().map(|e| (e.code, e.path)).collect())
}

/// A temp folder for one test, unique per test. Like the BC-6 tests, the folder is never deleted.
struct TempHome(PathBuf);

impl TempHome {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_nanos());
        let dir = std::env::temp_dir().join(format!("pm-bc12-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp home");
        Self(dir)
    }

    /// The environment for this folder only: it is the home and no variable is read.
    fn env(&self) -> HistoryEnv {
        HistoryEnv::fixed(Some(self.0.clone()), &[])
    }

    /// Writes `text` as `<home>/.acme/sessions/<folder>/<name>`.
    fn chat(&self, folder: &str, name: &str, text: &str) {
        let dir = self.0.join(".acme").join("sessions").join(folder);
        fs::create_dir_all(&dir).expect("sessions folder");
        fs::write(dir.join(name), text).expect("chat file");
    }
}

fn same_store(a: &dyn SessionHistory, b: &dyn SessionHistory) -> bool {
    std::ptr::eq(a as *const dyn SessionHistory as *const u8, b as *const dyn SessionHistory as *const u8)
}

#[test]
fn a_definition_without_the_table_has_no_old_chats() {
    assert!(build_for_test(ACME).expect("builds").session_history().is_none());
    assert!(build_for_test(MINIMAL).expect("builds").session_history().is_none());
    assert!(matches!(store_of(ACME), Ok(None)));
}

#[test]
fn a_definition_with_the_table_lists_and_reads_its_chats() {
    let home = TempHome::new("list");
    home.chat("proj", &format!("{CHAT}.jsonl"), TRANSCRIPT);
    let profile = build_for_test(&acme_with(ROOTS)).expect("builds");
    let store = profile.session_history().expect("the table gives old chats");
    let env = home.env();

    let listing = store.list(&env, &ListLimits::default());
    assert_eq!(listing.chats.len(), 1, "{listing:?}");
    assert_eq!(listing.chats[0].session, CHAT);
    assert_eq!(listing.chats[0].cwd, None, "no cwd_pointer, so no folder is read");

    let mut items: Vec<ChatItem> = Vec::new();
    let outcome = store.read(&env, CHAT, &ReadLimits::default(), &mut |batch| {
        items.extend(batch);
        true
    });
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(items.len(), TRANSCRIPT_ITEMS);
    assert!(matches!(items.first(), Some(ChatItem::User { text, .. }) if text == "Add a greeting to main.py"));
}

#[test]
fn resume_args_fill_the_session_and_refuse_a_bad_id() {
    let profile = build_for_test(&acme_with(ROOTS)).expect("builds");
    let store = profile.session_history().expect("old chats");
    assert_eq!(store.resume_args(CHAT), Some(vec!["--resume".to_string(), CHAT.to_string()]));
    assert_eq!(store.resume_args("../evil"), None, "an id that is not a plain id gives no command");

    let no_resume = build_for_test(&acme_with("roots = [\"{home}/.acme/sessions\"]\n")).expect("builds");
    assert_eq!(no_resume.session_history().expect("old chats").resume_args(CHAT), None, "no resume option declared");
}

#[test]
fn the_depth_of_the_table_decides_how_far_the_folders_are_walked() {
    let home = TempHome::new("depth");
    home.chat("outer/inner", &format!("{CHAT}.jsonl"), TRANSCRIPT);
    let shallow = build_for_test(&acme_with(ROOTS)).expect("builds");
    let found = shallow.session_history().expect("old chats").list(&home.env(), &ListLimits::default());
    assert!(found.chats.is_empty(), "the default depth 1 does not reach two folders down");

    let deep = build_for_test(&acme_with(&format!("{ROOTS}depth = 2\n"))).expect("builds");
    let found = deep.session_history().expect("old chats").list(&home.env(), &ListLimits::default());
    assert_eq!(found.chats.len(), 1, "{found:?}");
}

#[test]
fn a_pointer_id_and_a_folder_pointer_are_read_from_the_first_lines() {
    let home = TempHome::new("pointer");
    // The first line carries the id and the folder; the file name is not an id at all.
    let text = format!("{{\"session_id\":\"{OTHER}\",\"cwd\":\"D:/fixture/proj\"}}\n{TRANSCRIPT}");
    home.chat("proj", "not-an-id.jsonl", &text);
    let table = format!("{ROOTS}id_from = \"pointer\"\nid_pointer = \"/session_id\"\ncwd_pointer = \"/cwd\"\n");
    let profile = build_for_test(&acme_with(&table)).expect("builds");
    let store = profile.session_history().expect("old chats");
    let env = home.env();

    let listing = store.list(&env, &ListLimits::default());
    assert_eq!(listing.chats.len(), 1, "{listing:?}");
    assert_eq!(listing.chats[0].session, OTHER);
    assert_eq!(listing.chats[0].cwd.as_deref(), Some("D:/fixture/proj"));
    let mut items = 0;
    let outcome = store.read(&env, OTHER, &ReadLimits::default(), &mut |batch| {
        items += batch.len();
        true
    });
    assert!(outcome.is_ok());
    assert_eq!(items, TRANSCRIPT_ITEMS);
}

#[test]
fn a_missing_store_gives_an_empty_listing_and_a_missing_chat() {
    let home = TempHome::new("missing");
    let profile = build_for_test(&acme_with(ROOTS)).expect("builds");
    let store = profile.session_history().expect("old chats");
    assert!(store.list(&home.env(), &ListLimits::default()).chats.is_empty());
    assert_eq!(store.read(&home.env(), CHAT, &ReadLimits::default(), &mut |_| true).err(), Some(ReadError::Missing));
    assert!(store.list(&HistoryEnv::isolated(), &ListLimits::default()).chats.is_empty(), "no home, nothing to read");
}

#[test]
fn places_show_the_declared_folder_with_home_as_a_tilde() {
    let home = TempHome::new("places");
    let profile = build_for_test(&acme_with(ROOTS)).expect("builds");
    let places = profile.session_history().expect("old chats").places(&home.env());
    // The user-folder part becomes `~`; the rest keeps the separators it was written with (Windows mixes them).
    assert_eq!(places.len(), 1, "{places:?}");
    assert!(places[0].starts_with('~'), "{places:?}");
    assert!(places[0].ends_with(".acme/sessions"), "{places:?}");
    assert!(!places[0].contains(':'), "no drive letter is shown: {places:?}");
}

#[test]
fn the_same_table_shares_one_store_and_another_table_gets_its_own() {
    let first = store_of(&acme_with(ROOTS)).expect("valid").expect("old chats");
    let again = store_of(&acme_with(ROOTS)).expect("valid").expect("old chats");
    let other = store_of(&acme_with(&format!("{ROOTS}depth = 2\n"))).expect("valid").expect("old chats");
    assert!(same_store(first, again), "loading the same table again shares the store");
    assert!(!same_store(first, other), "a different table is a different store");
}

#[test]
fn a_bad_table_is_refused_with_its_code_and_path() {
    let bad = acme_with("roots = [\"/etc/acme\"]\n");
    let errors = store_of(&bad).err().expect("refused");
    assert_eq!(errors, vec![("AGENT_DEF_SESSION_CHAT_BAD_HISTORY", "session_chat.history.roots.0".to_string())]);
}
