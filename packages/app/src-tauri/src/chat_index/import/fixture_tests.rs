//! The importer end to end: the real registry catalog and the built-in stores over a temp home made from the synthetic
//! fixtures (BC-4), into an index file. The checks look at the bytes of that file. No real agent folder is read: the
//! home is a temp folder passed through `HistoryEnv::fixed`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde_json::json;

use super::fakes::{finished, NoPanes};
use super::{Catalog, Emitter, ImportRequest, Importer, Phase, Progress, RegistryCatalog};
use crate::chat_index::{open_store, ChatIndex, FILE_NAME};
use crate::session_chat::history::HistoryEnv;

const CLAUDE_PROJECTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/claude/fixtures/history/projects");
const CODEX_SESSIONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/codex/fixtures/history/sessions");
/// The two Codex chats that exist as plain files (the third has only a compressed one).
const CODEX_FILES: [&str; 2] = [
    "2026/08/14/rollout-2026-08-14T10-00-00-0198b000-0000-7000-8000-0000000000a1.jsonl",
    "2026/08/15/rollout-2026-08-15T09-30-00-0198b100-0000-7000-8000-0000000000b2.jsonl",
];
const PLANTED_SESSION: &str = "7d1f3a90-2b6c-4e58-8a14-9c0e5b7f2d03";

/// Made-up secrets and personal data. None is real.
const KEY: &str = "sk-ant-api03-PLANTEDPLANTEDPLANTED0000000000000000000000";
const GITHUB: &str = "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8";
const AWS: &str = "AKIAIOSFODNN7EXAMPLE";
const HEX: &str = "0123456789abcdef0123456789abcdef01234567";
const EMAIL: &str = "jane.doe@example.com";
const HOME_PATH: &str = "C:\\Users\\you\\proj\\.env";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-bc17-{tag}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A temp home: the two Claude fixtures, one more Claude chat with planted secrets, and two Codex rollouts.
fn fixture_home() -> PathBuf {
    let home = temp_dir("home");
    let projects = home.join(".claude").join("projects");
    copy_dir(Path::new(CLAUDE_PROJECTS), &projects);
    let planted = json!({
        "type": "user",
        "message": { "role": "user", "content": format!(
            "plantedmarker keys {KEY} and {GITHUB} and {AWS} and {HEX}, write to {EMAIL}, read {HOME_PATH}") },
        "uuid": "u-c01", "parentUuid": null, "timestamp": "2026-08-21T09:00:00Z",
        "sessionId": PLANTED_SESSION, "cwd": "C:\\Users\\you\\proj", "isSidechain": false
    });
    let reply = json!({
        "type": "assistant",
        "message": { "model": "m", "id": "msg-c02", "type": "message", "role": "assistant", "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": format!("plantedmarker reply with {EMAIL} and token {GITHUB}") }] },
        "uuid": "a-c02", "parentUuid": "u-c01", "timestamp": "2026-08-21T09:00:01Z", "sessionId": PLANTED_SESSION
    });
    let file = projects.join("C--Users-you-proj").join(format!("{PLANTED_SESSION}.jsonl"));
    fs::write(file, format!("{planted}\n{reply}\n")).unwrap();
    for relative in CODEX_FILES {
        let target = home.join(".codex").join("sessions").join(relative);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(Path::new(CODEX_SESSIONS).join(relative), target).unwrap();
    }
    home
}

fn env_of(home: &Path) -> HistoryEnv {
    HistoryEnv::fixed(Some(home.to_path_buf()), &[])
}

/// An importer over the real registry catalog and an index file in `dir`.
fn file_importer(dir: &Path) -> (Arc<Importer>, Arc<ChatIndex>) {
    let store = open_store(&dir.join(FILE_NAME), 1000).unwrap();
    let index = ChatIndex::start_switched(store, Arc::new(NoPanes), Arc::new(AtomicBool::new(true)));
    let emit: Emitter = Arc::new(|_: &Progress| {});
    (Arc::new(Importer::new(Arc::new(RegistryCatalog), index.clone(), emit)), index)
}

fn all_agents() -> ImportRequest {
    ImportRequest { agents: RegistryCatalog.sources().iter().map(|source| source.id.clone()).collect(), since_days: None }
}

/// Whether `needle` is in the index file or its write-ahead log.
fn in_file(dir: &Path, needle: &str) -> bool {
    [FILE_NAME.to_string(), format!("{FILE_NAME}-wal")].iter().any(|name| {
        let bytes = fs::read(dir.join(name)).unwrap_or_default();
        bytes.windows(needle.len()).any(|window| window == needle.as_bytes())
    })
}

#[test]
fn three_claude_and_two_codex_chats_become_five_indexed_chats() {
    let (home, dir) = (fixture_home(), temp_dir("index"));
    let (importer, index) = file_importer(&dir);
    let found = importer.scan(&env_of(&home), None);
    assert_eq!(found.iter().map(|f| f.chats).sum::<usize>(), 5, "{found:?}");
    assert!(found.iter().all(|f| !f.from_definition && f.can_continue && !f.places.is_empty()));
    importer.start(all_agents(), env_of(&home)).unwrap();
    let end = finished(&importer);
    assert_eq!((end.phase, end.chats_total, end.chats_done, end.unreadable, end.too_big), (Phase::Done, 5, 5, 0, 0));
    assert_eq!(index.status().unwrap().imported_chats, 5);
    let codex = index.search_hits("stripe webhook", 20).unwrap();
    assert!(!codex.is_empty() && codex.iter().all(|hit| hit.imported));
    assert!(index.search_hits("plantedmarker", 20).unwrap().len() >= 2);
    // A second run finds everything unchanged and reads nothing.
    importer.start(all_agents(), env_of(&home)).unwrap();
    let second = finished(&importer);
    assert_eq!((second.phase, second.unchanged, second.items), (Phase::Done, 5, 0));
}

#[test]
fn planted_secrets_are_not_in_the_index_file_and_old_text_goes_with_remove() {
    let (home, dir) = (fixture_home(), temp_dir("privacy"));
    let (importer, index) = file_importer(&dir);
    importer.start(all_agents(), env_of(&home)).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    assert!(in_file(&dir, "plantedmarker"), "the ordinary text of the chat is saved (so the check below means something)");
    for secret in [KEY, "sk-ant-api03", "FAKEFAKEFAKE", GITHUB, AWS, HEX, EMAIL, "jane.doe", "Users\\you", "Users/you"] {
        assert!(!in_file(&dir, secret), "{secret} is in the index file");
    }
    let hits = index.search_hits("plantedmarker", 20).unwrap();
    assert!(hits.iter().all(|hit| !hit.snippet.contains("@") && !hit.snippet.contains("ghp_")), "{hits:?}");
    // Remove imported chats: the text is gone from the file and from its log.
    let status = index.clear_imported().unwrap();
    assert_eq!((status.imported_chats, status.imported_items), (0, 0));
    assert!(!in_file(&dir, "plantedmarker"));
    assert!(!in_file(&dir, "stripe webhook"));
    assert!(index.search_hits("plantedmarker", 20).unwrap().is_empty());
}

#[test]
fn titles_and_folders_are_clean_and_the_resume_folder_keeps_the_home_as_a_tilde() {
    let (home, dir) = (fixture_home(), temp_dir("titles"));
    let (importer, index) = file_importer(&dir);
    importer.start(all_agents(), env_of(&home)).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    let mut rows = Vec::new();
    for source in RegistryCatalog.sources() {
        let listing = source.history.list(&env_of(&home), &Default::default());
        for chat in listing.chats {
            rows.push(index.imported_chat(&source.id, &chat.session).unwrap().expect("a record per chat"));
        }
    }
    assert_eq!(rows.len(), 5);
    let planted = rows.iter().find(|row| row.session == PLANTED_SESSION).expect("the planted chat");
    assert!(planted.title.starts_with("plantedmarker keys "), "{}", planted.title);
    assert!(planted.title.chars().count() <= 120);
    for secret in [KEY, GITHUB, AWS, HEX, EMAIL] {
        assert!(!planted.title.contains(secret), "{}", planted.title);
    }
    assert_eq!((planted.folder.as_str(), planted.cwd.as_str()), ("proj", "~\\proj"));
    assert!(rows.iter().all(|row| !row.folder.contains("Users") && !row.cwd.contains("Users")));
}
