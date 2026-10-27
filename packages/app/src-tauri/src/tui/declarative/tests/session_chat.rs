//! `[session_chat]` in a definition: the acme example, its load-time checks and the Chat tab reading a declared
//! transcript through the same watcher the built-in agents use.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;

use crate::session_chat::{ChatItem, PaneFacts, PaneLookup, SessionFormat, SessionSource, Sink, TurnOutcome, Watchers};
use crate::tui::declarative::{build_for_test, parse_definition, validate, DefError, Source, ValidationContext};
use crate::tui::TuiProfile;

const ACME: &str = include_str!("../fixtures/definitions/acme-chat.toml");
const TRANSCRIPT: &str = include_str!("../fixtures/acme/session.jsonl");
const FORMAT_DOC: &str = include_str!("../../../../../../../docs/specs/agent-definitions/DEFINITION-FORMAT.md");

fn problems(text: &str) -> Vec<DefError> {
    match parse_definition(text) {
        Err(e) => vec![e],
        Ok(def) => validate(&def, &ValidationContext { source: Source::User, code_ids: &["claude", "codex"], raw_len: text.len() }),
    }
}

fn codes(text: &str) -> Vec<(&'static str, String)> {
    problems(text).into_iter().map(|e| (e.code, e.path)).collect()
}

/// The acme text with `from` swapped for `to` (and a check that it was there).
fn patched(from: &str, to: &str) -> String {
    assert!(ACME.contains(from), "{from}");
    ACME.replace(from, to)
}

/// The fixture's lines as the watcher reads them: each with its byte offset.
fn items_of(format: &dyn SessionFormat, text: &str) -> Vec<ChatItem> {
    let mut offset = 0u64;
    let mut items = Vec::new();
    for line in text.split_inclusive('\n') {
        items.extend(format.parse_line(line.trim_end_matches(['\n', '\r']), offset));
        offset += line.len() as u64;
    }
    items
}

fn acme_profile() -> impl TuiProfile {
    build_for_test(ACME).expect("acme builds")
}

#[test]
fn the_acme_example_validates_and_declares_a_chat_source() {
    assert!(problems(ACME).is_empty(), "{:?}", problems(ACME));
    let profile = acme_profile();
    assert!(profile.session_chat().is_some());
    assert!(profile.session_api().is_none());
    // A wrong folder in a definition must not leave the Chat tab empty: the screen is read until the file has items.
    assert!(profile.session_chat().unwrap().screen_until_found());
    // A definition without the table keeps reading the screen.
    let plain = build_for_test(include_str!("../fixtures/definitions/minimal-tui.toml")).unwrap();
    assert!(plain.session_chat().is_none() && plain.screen_chat());
}

#[test]
fn the_acme_transcript_turns_into_the_expected_items() {
    let profile = acme_profile();
    let items = items_of(profile.session_chat().unwrap().format(), TRANSCRIPT);
    let json = serde_json::to_value(&items).unwrap();
    let at = |t: &str| serde_json::Value::String(t.to_string());
    let expected = serde_json::json!([
        {"kind": "user", "id": "0:0", "at": at("2026-10-28T09:00:00.000Z"), "text": "Add a greeting to main.py"},
        {"kind": "assistant", "id": "1:0", "at": null, "text": "I will read the file first.", "model": "acme-1", "finalAnswer": false},
    ]);
    // The first two lines, compared whole; the rest by shape below (their ids are byte offsets).
    assert_eq!(json[0]["kind"], "user");
    assert_eq!(json[0]["text"], expected[0]["text"]);
    assert_eq!(json[0]["at"], expected[0]["at"]);
    assert_eq!(json[1]["kind"], "assistant");
    assert_eq!(json[1]["text"], expected[1]["text"]);
    assert_eq!(json[1]["model"], "acme-1");
    assert_eq!(json[1]["finalAnswer"], false);

    let kinds: Vec<&str> = json.as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        ["user", "assistant", "toolCall", "toolResult", "toolCall", "toolResult", "toolCall", "toolResult", "assistant", "turnEnd", "usage"]
    );

    // read_file: a read of one file.
    assert_eq!(json[2]["callId"], "c1");
    assert_eq!(json[2]["name"], "read_file");
    assert_eq!(json[2]["summary"], "main.py");
    assert_eq!(json[2]["paths"], serde_json::json!(["main.py"]));
    assert!(json[2].get("writes").is_none());
    assert_eq!(json[3]["callId"], "c1");
    assert_eq!(json[3]["ok"], true);
    assert_eq!(json[3]["output"], "print('hi')\n");

    // write_file: named in `write_tools`, so it writes, and its input keeps the new text for a diff.
    assert_eq!(json[4]["name"], "write_file");
    assert_eq!(json[4]["writes"], true);
    assert_eq!(json[4]["paths"], serde_json::json!(["main.py"]));
    assert_eq!(json[4]["input"]["content"], "print('hello')\n");
    assert_eq!(json[5]["ok"], true);

    // run_shell: its input is a JSON string; a result with exit code 2 failed and lost its terminal codes.
    assert_eq!(json[6]["summary"], "python main.py --strict");
    assert!(json[6].get("paths").is_none());
    assert_eq!(json[7]["ok"], false);
    assert_eq!(json[7]["output"], "bad flag\n");

    // The last line: a numeric time, the end of the turn and the token counts.
    assert_eq!(json[8]["finalAnswer"], true);
    assert_eq!(json[8]["at"], "2026-10-28T09:01:00.000Z");
    assert!(matches!(&items[9], ChatItem::TurnEnd { outcome: TurnOutcome::Completed, .. }));
    assert!(matches!(&items[10], ChatItem::Usage { input_tokens: 1200, output_tokens: 85, context_window: None, .. }));
}

#[test]
fn ids_are_the_byte_offset_of_the_line_and_the_index_in_it() {
    let profile = acme_profile();
    let items = items_of(profile.session_chat().unwrap().format(), TRANSCRIPT);
    let second_line_at = TRANSCRIPT.find('\n').unwrap() + 1;
    assert_eq!(items[0].id(), "0:0");
    assert_eq!(items[1].id(), format!("{second_line_at}:0"));
    let last_line_at = TRANSCRIPT.trim_end().rfind('\n').unwrap() + 1;
    assert_eq!(items[8].id(), format!("{last_line_at}:0"));
    assert_eq!(items[10].id(), format!("{last_line_at}:2"));
}

#[test]
fn the_transcript_fixture_has_no_personal_data_and_is_lf() {
    assert!(!TRANSCRIPT.contains('\r'));
    for needle in ["C:\\Users", "/Users/", "/home/", "@", "token", "password"] {
        assert!(!TRANSCRIPT.contains(needle), "{needle}");
    }
}

// ---- bad mappings fail at load ----

#[test]
fn a_bad_pointer_fails_at_load_and_names_its_key() {
    let bad = patched("text = \"/content\"", "text = \"content\"");
    assert_eq!(codes(&bad), [("AGENT_DEF_SESSION_CHAT_BAD_POINTER", "session_chat.map.text".to_string())]);
    let in_list = patched("tool_error = [\"/result/is_error\", \"/result/exit_code\"]", "tool_error = [\"/result/is_error\", \"result/exit_code\"]");
    assert_eq!(codes(&in_list), [("AGENT_DEF_SESSION_CHAT_BAD_POINTER", "session_chat.map.tool_error.1".to_string())]);
    assert!(build_for_test(&bad).is_err(), "a bad mapping does not build a profile");
}

#[test]
fn a_dir_outside_home_or_the_project_fails_at_load() {
    for dir in ["/etc/acme", "C:\\\\acme", "{home}/../../acme", "{cwd_slug}/acme", "{home}/{nope}"] {
        let bad = patched("dir = \"{home}/.acme/sessions/{cwd_dashes}\"", &format!("dir = \"{dir}\""));
        assert!(codes(&bad).contains(&("AGENT_DEF_SESSION_CHAT_BAD_DIR", "session_chat.dir".to_string())), "{dir}: {:?}", codes(&bad));
    }
}

#[test]
fn a_bad_glob_a_missing_key_and_an_unknown_key_fail_at_load() {
    let glob = patched("file_glob = \"*.jsonl\"", "file_glob = \"../*.jsonl\"");
    assert_eq!(codes(&glob), [("AGENT_DEF_SESSION_CHAT_BAD_GLOB", "session_chat.file_glob".to_string())]);

    let no_text = patched("text = \"/content\"\n", "");
    let found = codes(&no_text);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "AGENT_DEF_INVALID_TOML");
    assert!(found[0].1.starts_with("session_chat"), "{found:?}");

    let typo = patched("tool_name = \"/call/name\"", "tool_name = \"/call/name\"\ntool_nme = \"/x\"");
    assert_eq!(codes(&typo), [("AGENT_DEF_UNKNOWN_FIELD", "session_chat.map.tool_nme".to_string())]);

    let format = patched("format = \"jsonl\"", "format = \"xml\"");
    assert_eq!(codes(&format)[0].0, "AGENT_DEF_INVALID_TOML");
}

#[test]
fn a_mapping_whose_parts_do_not_fit_fails_at_load() {
    let no_values = patched("turn_end = [\"end_turn\"]\n", "");
    assert_eq!(codes(&no_values), [("AGENT_DEF_INCONSISTENT", "session_chat.values.turn_end".to_string())]);
    let no_call = patched("tool_name = \"/call/name\"\n", "");
    assert_eq!(codes(&no_call), [("AGENT_DEF_INCONSISTENT", "session_chat.map.tool_input".to_string()), ("AGENT_DEF_INCONSISTENT", "session_chat.map.tool_id".to_string())]);
}

#[test]
fn the_table_is_part_of_the_approved_fingerprint() {
    let file = Path::new("acme-chat.toml");
    let fp = |text: &str| crate::tui::declarative::fingerprint::fingerprint(&parse_definition(text).unwrap(), Source::User, None, file);
    assert_eq!(fp(ACME), fp(ACME));
    assert_ne!(fp(ACME), fp(&patched("file_glob = \"*.jsonl\"", "file_glob = \"*.log\"")));
    assert_ne!(fp(ACME), fp(&patched("text = \"/content\"", "text = \"/body\"")));
}

#[test]
fn the_documented_example_is_the_tested_one() {
    // The table's own line, not the mention of it in the file's first comment.
    let table_start = ACME.find("\n[session_chat]\n").unwrap() + 1;
    let block = &ACME[table_start..];
    assert!(FORMAT_DOC.replace("\r\n", "\n").contains(block.trim_end()), "DEFINITION-FORMAT.md must show the acme [session_chat] tables exactly as the fixture has them");
}

// ---- the Chat tab over a declared transcript ----

#[derive(Default)]
struct Collect {
    sent: Mutex<Vec<ChatItem>>,
}

impl Sink for Collect {
    fn send(&self, _pane_id: &str, items: Vec<ChatItem>, _reset: bool) {
        self.sent.lock().extend(items);
    }
}

struct OnePane(PaneFacts);

impl PaneLookup for OnePane {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        Some(self.0.clone())
    }
}

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("pm-acme-chat-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(dir.join(".acme").join("sessions")).unwrap();
        Folder(dir)
    }

    fn transcript(&self) -> PathBuf {
        self.0.join(".acme").join("sessions").join("s1.jsonl")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_declarative_agent_gets_the_rich_chat_from_its_own_transcript() {
    // The pane's own folder is an allowed root, so the transcript lives there instead of under the real home.
    let definition = patched("dir = \"{home}/.acme/sessions/{cwd_dashes}\"", "dir = \"{cwd}/.acme/sessions\"");
    let profile = build_for_test(&definition).unwrap();
    let source = profile.session_chat().expect("the table declares a source");

    let folder = Folder::new();
    std::fs::write(folder.transcript(), TRANSCRIPT).unwrap();
    let facts = PaneFacts { cwd: folder.0.clone(), started_at: SystemTime::now() - Duration::from_secs(30), provider_session_id: None, home: None, launch: Default::default() };
    let sink = Arc::new(Collect::default());
    let tuning = crate::session_chat::watch::Tuning {
        poll: Duration::from_millis(10),
        screen_poll: Duration::from_millis(10),
        api_poll: Duration::from_millis(10),
        relocate: Duration::from_millis(10),
        max_items: 3000,
    };
    let watchers = Watchers::with_tuning(sink.clone(), Arc::new(OnePane(facts)), tuning);

    let items = watchers.open("pane-1", "test", source);
    assert_eq!(items.len(), 11, "{items:?}");
    assert!(matches!(&items[0], ChatItem::User { text, .. } if text == "Add a greeting to main.py"));
    assert!(matches!(&items[4], ChatItem::ToolCall { name, detail, .. } if name == "write_file" && detail.writes));
    assert_eq!(watchers.source_of("pane-1"), Some("file"));

    // A line the CLI appends later arrives through the watcher, once.
    let more = "{\"ts\":\"2026-10-28T09:02:00.000Z\",\"kind\":\"message\",\"role\":\"user\",\"content\":\"and now a farewell\"}\n";
    let mut file = std::fs::OpenOptions::new().append(true).open(folder.transcript()).unwrap();
    std::io::Write::write_all(&mut file, more.as_bytes()).unwrap();
    wait_until("the appended line", || sink.sent.lock().len() == 1);
    assert!(matches!(&sink.sent.lock()[0], ChatItem::User { text, .. } if text == "and now a farewell"));
    watchers.close_all();
}

#[test]
fn a_transcript_in_a_folder_the_pane_may_not_read_shows_nothing() {
    // The variable is not set in the test process, so the folder cannot be named at all.
    assert!(std::env::var_os("ACME_CHAT_TEST_DIR").is_none());
    let definition = patched("dir = \"{home}/.acme/sessions/{cwd_dashes}\"", "dir = \"{env:ACME_CHAT_TEST_DIR}\"");
    let profile = build_for_test(&definition).unwrap();
    let pane = Folder::new();
    std::fs::write(pane.transcript(), TRANSCRIPT).unwrap();
    let facts = PaneFacts { cwd: pane.0.clone(), started_at: SystemTime::now() - Duration::from_secs(30), provider_session_id: None, home: None, launch: Default::default() };
    assert_eq!(profile.session_chat().unwrap().locate(&facts), crate::session_chat::Located::NotYet);
}
