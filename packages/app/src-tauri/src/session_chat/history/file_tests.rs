//! Tests for `FileHistory`: list and read over temp homes only. The chat files come from the synthetic fixtures
//! (copied into a temp folder) or are written by the test itself. No test reads a real agent folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::file::{one_per_session, FileMeta, SessionKey};
use super::{FileHistory, FileHistorySpec, HistoryEnv, ListLimits, ReadError, ReadLimits, SessionHistory};
use crate::session_chat::{ChatItem, SessionFormat};

/// The synthetic Claude-style fixtures (BC-4), copied into a temp home by each test that needs them.
const FIXTURE_PROJECTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/claude/fixtures/history/projects");
const FIXTURE_CHAT_A: &str = "3f2a9c10-7b4e-4d21-9a6c-1e5f0b7d8a01";
const FIXTURE_CHAT_B: &str = "5c8e1d22-0a9b-4f3e-b7c6-2d4a8e9f0b02";

/// A generic test format: one item per line with a `type` of user or assistant and some text.
struct LineFormat;

impl SessionFormat for LineFormat {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { return Vec::new() };
        let text = match value.pointer("/message/content") {
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .find_map(|part| part.get("text").and_then(|text| text.as_str()).map(str::to_owned))
                .unwrap_or_default(),
            _ => String::new(),
        };
        let id = format!("{offset}:0");
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("user") if !text.is_empty() => vec![ChatItem::User { id, at: None, text }],
            Some("assistant") if !text.is_empty() => {
                vec![ChatItem::Assistant { id, at: None, text, model: None, final_answer: false }]
            }
            _ => Vec::new(),
        }
    }
}

static FORMAT: LineFormat = LineFormat;

/// A store shaped like a Claude-style folder: `<home>/.claude/projects/<project>/<chat>.jsonl`.
const PROJECTS_SPEC: FileHistorySpec = FileHistorySpec {
    roots: &["{home}/.claude/projects"],
    extra_roots_env: &[("FAKE_CONFIG_DIR", "projects")],
    glob: "*.jsonl",
    skip: &["agent-*.jsonl"],
    depth: 1,
    id: SessionKey::FileStem,
    cwd_pointer: Some("/cwd"),
    format: &FORMAT,
    resume: &["--resume", "{session}"],
};

/// The same store, with the session id taken from the first lines and no skip rule.
const POINTER_SPEC: FileHistorySpec = FileHistorySpec {
    skip: &[],
    id: SessionKey::Pointer("/session_id"),
    ..PROJECTS_SPEC
};

/// The same store, with the session id from a function that reads the file.
const META_SPEC: FileHistorySpec = FileHistorySpec { id: SessionKey::Meta(meta_of), resume: &[], ..PROJECTS_SPEC };

fn meta_of(path: &Path) -> Option<FileMeta> {
    let name = path.file_name()?.to_str()?;
    name.starts_with("meta").then(|| FileMeta { id: "meta-session-0001".to_string(), cwd: None })
}

/// A folder under the system temp folder, unique per test. It is never deleted by the tests.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_nanos());
        let dir = std::env::temp_dir().join(format!("pm-bc6-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp folder");
        Self(dir)
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy target");
    for entry in fs::read_dir(from).expect("fixture folder") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("fixture type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("fixture copy");
        }
    }
}

/// A temp home with the synthetic Claude-style fixtures in `.claude/projects`.
fn fixture_home(tag: &str) -> (TempDir, HistoryEnv) {
    let temp = TempDir::new(tag);
    copy_dir(Path::new(FIXTURE_PROJECTS), &temp.0.join(".claude").join("projects"));
    let env = HistoryEnv::fixed(Some(temp.0.clone()), &[]);
    (temp, env)
}

/// A temp home with an empty `.claude/projects` folder, for a test that writes its own files there.
fn bare_home(tag: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new(tag);
    let projects = temp.0.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&projects).expect("projects folder");
    (temp, projects)
}

fn write(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), text).expect("test chat file");
}

fn texts(items: &[ChatItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            ChatItem::User { text, .. } | ChatItem::Assistant { text, .. } => text.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

/// Reads one chat with the given limits and returns the items the sink got, plus the outcome.
fn read_all(
    history: &FileHistory,
    env: &HistoryEnv,
    session: &str,
    limits: &ReadLimits,
) -> (Result<super::ReadOutcome, ReadError>, Vec<ChatItem>) {
    let mut got = Vec::new();
    let outcome = history.read(env, session, limits, &mut |batch| {
        got.extend(batch);
        true
    });
    (outcome, got)
}

#[test]
fn lists_the_two_claude_chats_and_skips_sidechains_and_nested_files() {
    let (_temp, env) = fixture_home("list");
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &ListLimits::default());
    let mut ids: Vec<&str> = listing.chats.iter().map(|chat| chat.session.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec![FIXTURE_CHAT_A, FIXTURE_CHAT_B]);
    assert_eq!((listing.too_big, listing.unreadable, listing.cut_short), (0, 0, false));
    for chat in &listing.chats {
        assert_eq!(chat.stamp, format!("{}:{}", chat.bytes, chat.updated_ms), "stamp is length and time");
    }
}

#[test]
fn the_folder_comes_from_the_first_lines_with_home_as_tilde() {
    let (_temp, env) = fixture_home("cwd");
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &ListLimits::default());
    let with_folder = listing.chats.iter().find(|chat| chat.session == FIXTURE_CHAT_A).expect("chat a");
    let folder = with_folder.cwd.as_deref().expect("chat a has a folder");
    assert!(folder.starts_with('~'), "the account folder is never kept: {folder}");
    let without = listing.chats.iter().find(|chat| chat.session == FIXTURE_CHAT_B).expect("chat b");
    assert_eq!(without.cwd, None);
}

#[test]
fn read_gives_the_items_in_order() {
    let (_temp, env) = fixture_home("read");
    let (outcome, items) = read_all(&FileHistory::new(PROJECTS_SPEC), &env, FIXTURE_CHAT_B, &ReadLimits::default());
    assert_eq!(outcome, Ok(super::ReadOutcome { items: 4, cut_short: false }));
    assert_eq!(
        texts(&items),
        vec![
            "Summarize the release notes in three bullets.",
            "Fixed the login bug. Added the export button. Updated the docs.",
            "Thanks.",
            "You are welcome.",
        ]
    );
    assert!(items[0].id().starts_with("0:"), "ids carry the line offset");
}

#[test]
fn read_hands_items_over_in_chunks_and_stops_when_the_sink_says_so() {
    let (_temp, env) = fixture_home("sink");
    let limits = ReadLimits { chunk: 1, ..ReadLimits::default() };
    let mut calls = 0;
    let outcome = FileHistory::new(PROJECTS_SPEC).read(&env, FIXTURE_CHAT_B, &limits, &mut |batch| {
        calls += 1;
        assert_eq!(batch.len(), 1, "one item per chunk");
        false
    });
    assert_eq!(calls, 1);
    assert_eq!(outcome, Ok(super::ReadOutcome { items: 1, cut_short: false }), "a stop by the sink is not a cap");
}

#[test]
fn read_stops_at_the_item_cap_and_says_it_was_cut() {
    let (_temp, env) = fixture_home("items");
    let limits = ReadLimits { max_items: 2, ..ReadLimits::default() };
    let (outcome, items) = read_all(&FileHistory::new(PROJECTS_SPEC), &env, FIXTURE_CHAT_B, &limits);
    assert_eq!(outcome, Ok(super::ReadOutcome { items: 2, cut_short: true }));
    assert_eq!(items.len(), 2);
}

#[test]
fn read_stops_at_the_decompressed_byte_cap() {
    let (_temp, env) = fixture_home("bytes");
    let limits = ReadLimits { max_decompressed: 100, ..ReadLimits::default() };
    let (outcome, items) = read_all(&FileHistory::new(PROJECTS_SPEC), &env, FIXTURE_CHAT_B, &limits);
    assert_eq!(outcome, Ok(super::ReadOutcome { items: 0, cut_short: true }), "the first line is longer than 100 bytes");
    assert!(items.is_empty());
}

#[test]
fn a_file_over_the_size_cap_is_counted_as_too_big() {
    let (_temp, env) = fixture_home("size");
    let limits = ListLimits { max_file_bytes: 10, ..ListLimits::default() };
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &limits);
    assert!(listing.chats.is_empty());
    assert_eq!(listing.too_big, 2);
}

#[test]
fn since_filters_by_time_and_max_chats_caps_the_count() {
    let (_temp, env) = fixture_home("since");
    let history = FileHistory::new(PROJECTS_SPEC);
    let none = history.list(&env, &ListLimits { since_ms: Some(i64::MAX), ..ListLimits::default() });
    assert!(none.chats.is_empty());
    let all = history.list(&env, &ListLimits { since_ms: Some(0), ..ListLimits::default() });
    assert_eq!(all.chats.len(), 2);
    let one = history.list(&env, &ListLimits { max_chats: 1, ..ListLimits::default() });
    assert_eq!(one.chats.len(), 1);
    assert!(one.cut_short, "a cap that cut the list is said so");
}

#[test]
fn a_zero_budget_gives_a_cut_short_listing() {
    let (_temp, env) = fixture_home("budget");
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &ListLimits { budget: Duration::ZERO, ..ListLimits::default() });
    assert!(listing.cut_short);
    assert!(listing.chats.is_empty());
}

#[test]
fn a_half_written_last_line_gives_no_item() {
    let (temp, projects) = bare_home("half");
    let complete = r#"{"type":"user","message":{"role":"user","content":"First question."}}"#;
    write(&projects, "partial-chat-0001.jsonl", &format!("{complete}\n{{\"type\":\"assistant\",\"message\":{{\"content\":\"Half"));
    let env = HistoryEnv::fixed(Some(temp.0.clone()), &[]);
    let history = FileHistory::new(PROJECTS_SPEC);
    let (outcome, items) = read_all(&history, &env, "partial-chat-0001", &ReadLimits::default());
    assert_eq!(outcome, Ok(super::ReadOutcome { items: 1, cut_short: false }));
    assert_eq!(texts(&items), vec!["First question."]);
}

#[test]
fn a_missing_home_or_folder_is_an_empty_listing() {
    let history = FileHistory::new(PROJECTS_SPEC);
    let no_home = history.list(&HistoryEnv::fixed(None, &[]), &ListLimits::default());
    assert!(no_home.chats.is_empty());
    assert_eq!((no_home.too_big, no_home.unreadable, no_home.cut_short), (0, 0, false));
    let temp = TempDir::new("empty");
    let no_folder = history.list(&HistoryEnv::fixed(Some(temp.0.clone()), &[]), &ListLimits::default());
    assert!(no_folder.chats.is_empty());
}

#[test]
fn a_symlink_to_a_file_outside_the_roots_is_skipped() {
    let (temp, env) = fixture_home("link");
    let outside = temp.0.join("outside");
    fs::create_dir_all(&outside).expect("outside folder");
    write(&outside, "link-chat-000001.jsonl", "{\"type\":\"user\",\"message\":{\"content\":\"x\"}}\n");
    let link = temp.0.join(".claude").join("projects").join("C--Users-you-proj").join("link-chat-000001.jsonl");
    if !make_file_symlink(&outside.join("link-chat-000001.jsonl"), &link) {
        return; // Windows without the symlink privilege: nothing to test here.
    }
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &ListLimits::default());
    assert!(listing.chats.iter().all(|chat| chat.session != "link-chat-000001"));
    assert_eq!(listing.too_big, 0);
}

#[cfg(windows)]
fn make_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(not(windows))]
fn make_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[test]
fn an_extra_root_from_a_variable_is_read_once_even_when_it_repeats_a_root() {
    let (temp, _env) = fixture_home("extra");
    let alt = temp.0.join("alt").join("projects").join("proj");
    fs::create_dir_all(&alt).expect("alt folder");
    write(&alt, "extra-chat-0001.jsonl", "{\"type\":\"user\",\"message\":{\"content\":\"extra\"}}\n");
    let history = FileHistory::new(PROJECTS_SPEC);
    let with_alt = HistoryEnv::fixed(Some(temp.0.clone()), &[("FAKE_CONFIG_DIR", temp.0.join("alt").to_str().unwrap())]);
    let listing = history.list(&with_alt, &ListLimits::default());
    assert!(listing.chats.iter().any(|chat| chat.session == "extra-chat-0001"));
    let same_root = HistoryEnv::fixed(Some(temp.0.clone()), &[("FAKE_CONFIG_DIR", temp.0.join(".claude").to_str().unwrap())]);
    let twice = history.list(&same_root, &ListLimits::default());
    assert_eq!(twice.chats.len(), 2, "the same folder twice is one root");
}

#[test]
fn the_places_show_the_roots_with_home_as_tilde() {
    let env = HistoryEnv::fixed(Some(PathBuf::from("C:\\Users\\you")), &[]);
    assert_eq!(FileHistory::new(PROJECTS_SPEC).places(&env), vec!["~/.claude/projects".to_string()]);
    let with_var = HistoryEnv::fixed(Some(PathBuf::from("C:\\Users\\you")), &[("FAKE_CONFIG_DIR", "D:\\alt")]);
    let places = FileHistory::new(PROJECTS_SPEC).places(&with_var);
    assert_eq!(places.len(), 2);
    assert!(places[1].starts_with("D:"), "a variable root is shown as it is");
}

#[test]
fn resume_args_fill_the_placeholder_and_a_spec_without_them_has_none() {
    let history = FileHistory::new(PROJECTS_SPEC);
    assert_eq!(history.resume_args(FIXTURE_CHAT_B), Some(vec!["--resume".to_string(), FIXTURE_CHAT_B.to_string()]));
    assert_eq!(history.resume_args("../evil"), None);
    let none = FileHistory::new(FileHistorySpec { resume: &[], ..PROJECTS_SPEC });
    assert_eq!(none.resume_args(FIXTURE_CHAT_B), None);
}

#[test]
fn the_session_id_can_come_from_a_pointer_in_the_first_lines() {
    let (temp, projects) = bare_home("pointer");
    let first = r#"{"session_id":"ptr-session-0001","cwd":"C:\\Users\\you\\proj","type":"user","message":{"content":"hi"}}"#;
    let second = r#"{"type":"assistant","message":{"content":"hello"}}"#;
    write(&projects, "any-name.jsonl", &format!("{first}\n{second}\n"));
    let env = HistoryEnv::fixed(Some(temp.0.clone()), &[]);
    let history = FileHistory::new(POINTER_SPEC);
    let listing = history.list(&env, &ListLimits::default());
    let chat = listing.chats.iter().find(|chat| chat.session == "ptr-session-0001").expect("pointer chat");
    assert_eq!(chat.cwd.as_deref(), Some("~\\proj"));
    let (outcome, items) = read_all(&history, &env, "ptr-session-0001", &ReadLimits::default());
    assert_eq!(outcome, Ok(super::ReadOutcome { items: 2, cut_short: false }));
    assert_eq!(texts(&items), vec!["hi", "hello"]);
}

#[test]
fn the_session_id_can_come_from_a_function() {
    let (temp, projects) = bare_home("meta");
    write(&projects, "meta-file.jsonl", "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n");
    write(&projects, "plain-file.jsonl", "{\"type\":\"user\",\"message\":{\"content\":\"no id\"}}\n");
    let env = HistoryEnv::fixed(Some(temp.0.clone()), &[]);
    let history = FileHistory::new(META_SPEC);
    let listing = history.list(&env, &ListLimits::default());
    assert_eq!(listing.chats.len(), 1);
    assert_eq!(listing.chats[0].session, "meta-session-0001");
    assert_eq!(listing.unreadable, 1, "a file the function cannot name is unreadable");
}

#[test]
fn a_file_whose_name_is_not_a_valid_session_id_is_unreadable() {
    let (temp, projects) = bare_home("badname");
    write(&projects, "bad name.jsonl", "{\"type\":\"user\",\"message\":{\"content\":\"x\"}}\n");
    let env = HistoryEnv::fixed(Some(temp.0.clone()), &[]);
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &ListLimits::default());
    assert!(listing.chats.is_empty());
    assert_eq!(listing.unreadable, 1);
}

#[test]
fn reading_a_missing_or_invalid_id_is_missing() {
    let (_temp, env) = fixture_home("missing");
    let history = FileHistory::new(PROJECTS_SPEC);
    let (missing, _) = read_all(&history, &env, "no-such-chat-0001", &ReadLimits::default());
    assert_eq!(missing, Err(ReadError::Missing));
    let (invalid, _) = read_all(&history, &env, "../../etc/passwd", &ReadLimits::default());
    assert_eq!(invalid, Err(ReadError::Missing));
}

#[test]
fn one_per_session_keeps_the_plain_file_over_a_compressed_copy() {
    let chat = |session: &str, updated_ms: i64| super::HistoryChat {
        session: session.to_string(),
        cwd: None,
        updated_ms,
        bytes: 1,
        stamp: "1:1".to_string(),
    };
    let kept = one_per_session(vec![(true, chat("same-session-01", 5)), (false, chat("same-session-01", 3))]);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].updated_ms, 3, "the plain file was kept");
    let both = one_per_session(vec![(false, chat("first-session-01", 1)), (true, chat("second-session", 2))]);
    assert_eq!(both.len(), 2);
}

#[test]
fn a_duration_budget_of_a_second_is_enough_for_the_fixtures() {
    let (_temp, env) = fixture_home("generous");
    let listing = FileHistory::new(PROJECTS_SPEC).list(&env, &ListLimits { budget: Duration::from_secs(1), ..ListLimits::default() });
    assert_eq!(listing.chats.len(), 2);
    assert!(!listing.cut_short);
}
