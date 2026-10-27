//! `[session_chat]`: the table's load-time check, the line reader, the folder template and the file choice.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::json;

use super::path::{clean_cwd, dashes, expand, glob_matches, sha256_hex, slug, url_encoded};
use super::format::{DeclaredSessionFile, MAX_LINE_BYTES};
use super::path::Names;
use super::source::{MAX_FILE_BYTES, MAX_READ_PER_POLL};
use super::table::ChatPick;
use super::*;
use crate::session_chat::{ChatItem, Located, PaneFacts, SessionFormat, SessionSource, TurnOutcome};

const TABLE: &str = r#"
format = "jsonl"
dir = "{home}/.tool/sessions/{cwd_slug}"

[map]
role = "/role"
text = "/text"
"#;

fn table(text: &str) -> SessionChatTable {
    toml::from_str(text).unwrap_or_else(|e| panic!("{e}"))
}

fn codes(text: &str) -> Vec<(&'static str, String)> {
    validate_session_chat(&table(text)).into_iter().map(|e| (e.code, e.path)).collect()
}

fn format_of(text: &str) -> DeclaredSessionFile {
    DeclaredSessionFile::new(&table(text))
}

fn parse(format: &DeclaredSessionFile, line: &str) -> Vec<ChatItem> {
    format.parse_line(line, 40)
}

// ---- the table ----

#[test]
fn the_smallest_table_is_valid_and_has_defaults() {
    let t = table(TABLE);
    assert!(validate_session_chat(&t).is_empty());
    assert_eq!(t.file_glob, "*.jsonl");
    assert_eq!(t.pick, ChatPick::NewestSinceStart);
    assert_eq!(t.values.user, ["user"]);
    assert_eq!(t.values.assistant, ["assistant"]);
    assert_eq!(t.values.tool, ["tool"]);
}

#[test]
fn a_pointer_may_be_one_string_or_a_list() {
    let t = table(&TABLE.replace(r#"text = "/text""#, r#"text = ["/text", "/message/content"]"#));
    assert_eq!(t.map.text.0, ["/text", "/message/content"]);
    assert_eq!(table(TABLE).map.text.0, ["/text"]);
}

#[test]
fn unknown_keys_and_a_missing_required_key_are_parse_errors() {
    assert!(toml::from_str::<SessionChatTable>(&format!("{TABLE}bogus = 1\n")).is_err());
    assert!(toml::from_str::<SessionChatTable>(&TABLE.replace("format = \"jsonl\"\n", "format = \"xml\"\n")).is_err());
    assert!(toml::from_str::<SessionChatTable>(&TABLE.replace("text = \"/text\"\n", "")).is_err(), "map.text is required");
    assert!(toml::from_str::<SessionChatTable>(&TABLE.replace("role = \"/role\"\n", "")).is_err(), "map.role is required");
    assert!(toml::from_str::<SessionChatTable>(&TABLE.replace("dir = \"{home}/.tool/sessions/{cwd_slug}\"\n", "")).is_err(), "dir is required");
}

#[test]
fn a_bad_pointer_names_its_key() {
    let bad = TABLE.replace(r#"text = "/text""#, r#"text = "text""#);
    assert_eq!(codes(&bad), [("AGENT_DEF_SESSION_CHAT_BAD_POINTER", "session_chat.map.text".to_string())]);
    let in_list = TABLE.replace(r#"text = "/text""#, r#"text = ["/ok", "/a//b"]"#);
    assert_eq!(codes(&in_list), [("AGENT_DEF_SESSION_CHAT_BAD_POINTER", "session_chat.map.text.1".to_string())]);
    let escape = TABLE.replace(r#"text = "/text""#, r#"text = "/a~2b""#);
    assert_eq!(codes(&escape)[0].0, "AGENT_DEF_SESSION_CHAT_BAD_POINTER");
    let long = TABLE.replace(r#"text = "/text""#, &format!("text = \"/{}\"", "a".repeat(200)));
    assert_eq!(codes(&long)[0].0, "AGENT_DEF_SESSION_CHAT_BAD_POINTER");
    let deep = TABLE.replace(r#"text = "/text""#, r#"text = "/a/b/c/d/e/f/g/h/i""#);
    assert_eq!(codes(&deep)[0].0, "AGENT_DEF_SESSION_CHAT_BAD_POINTER");
    let empty = TABLE.replace(r#"text = "/text""#, "text = []");
    assert_eq!(codes(&empty), [("AGENT_DEF_SESSION_CHAT_BAD_POINTER", "session_chat.map.text".to_string())]);
}

#[test]
fn a_dir_must_stay_under_home_or_the_project() {
    let with = |dir: &str| TABLE.replace("{home}/.tool/sessions/{cwd_slug}", dir);
    for good in ["{home}/.tool", "{cwd}/.tool/sessions", "{env:TOOL_HOME}/sessions/{cwd_url}", "{home}/x/{cwd_sha256}", "{home}/x/{cwd_name}", "{home}/x/{cwd_dashes}"] {
        assert!(codes(&with(good)).is_empty(), "{good}");
    }
    for bad in [
        "/etc/tool",
        "C:\\\\Users\\\\you\\\\.tool",
        "relative/dir",
        "{home}/../../etc",
        "{home}\\\\..\\\\x",
        "{cwd_slug}/x",
        "{home}/{home}",
        "{home}/x/{cwd}",
        "{home}/{unknown}",
        "{home/x",
        "{home}/x}",
        "{env:}/x",
        "{env:1BAD}/x",
        "",
    ] {
        let found = codes(&with(bad));
        assert!(found.iter().any(|(code, path)| *code == "AGENT_DEF_SESSION_CHAT_BAD_DIR" && path == "session_chat.dir"), "{bad}: {found:?}");
    }
}

#[test]
fn a_file_glob_is_a_file_name() {
    let build = |glob: &str| format!("format = \"jsonl\"\ndir = \"{{home}}/x\"\nfile_glob = \"{glob}\"\n[map]\nrole = \"/r\"\ntext = \"/t\"\n");
    for good in ["*.jsonl", "session-?.jsonl", "chat_*.log", "*.JSONL"] {
        assert!(codes(&build(good)).is_empty(), "{good}");
    }
    for bad in ["../*.jsonl", "a/b.jsonl", "a\\\\b.jsonl", "*", "jsonl", "*.js;rm"] {
        let found = codes(&build(bad));
        assert!(found.iter().any(|(code, path)| *code == "AGENT_DEF_SESSION_CHAT_BAD_GLOB" && path == "session_chat.file_glob"), "{bad}: {found:?}");
    }
}

#[test]
fn fields_that_do_not_fit_together_are_refused() {
    let add = |extra: &str| TABLE.replace("text = \"/text\"\n", &format!("text = \"/text\"\n{extra}\n"));
    assert_eq!(codes(&add("tool_input = \"/i\"")), [("AGENT_DEF_INCONSISTENT", "session_chat.map.tool_input".to_string())]);
    assert_eq!(codes(&add("tool_output = \"/o\"")), [("AGENT_DEF_INCONSISTENT", "session_chat.map.tool_output".to_string())]);
    assert_eq!(codes(&add("turn_end = \"/stop\"")), [("AGENT_DEF_INCONSISTENT", "session_chat.values.turn_end".to_string())]);
    let values_only = format!("{TABLE}[values]\nturn_end = [\"stop\"]\n");
    assert_eq!(codes(&values_only), [("AGENT_DEF_INCONSISTENT", "session_chat.map.turn_end".to_string())]);
    let overlap = format!("{TABLE}[values]\nuser = [\"me\"]\nassistant = [\"ME\"]\n");
    assert_eq!(codes(&overlap), [("AGENT_DEF_INCONSISTENT", "session_chat.values.user".to_string())]);
    let empty_role = format!("{TABLE}[values]\nuser = []\n");
    assert_eq!(codes(&empty_role), [("AGENT_DEF_INCONSISTENT", "session_chat.values.user".to_string())]);
}

#[test]
fn a_source_is_built_only_from_a_valid_table() {
    assert!(DeclaredSource::new(table(TABLE)).is_ok());
    let bad = table(&TABLE.replace("/text", "text"));
    assert!(DeclaredSource::new(bad).is_err());
}

#[test]
fn the_same_table_gives_the_same_source() {
    let a = DeclaredSource::shared(&table(TABLE)).unwrap();
    let b = DeclaredSource::shared(&table(TABLE)).unwrap();
    assert!(std::ptr::eq(a, b), "a definition loaded again must not leak another source");
}

// ---- reading lines ----

const FLAT: &str = r#"
format = "jsonl"
dir = "{home}/.tool"

[map]
role = "/role"
text = ["/content", "/message/content"]
timestamp = "/ts"
model = "/model"
tool_name = "/name"
tool_input = "/input"
tool_id = "/id"
tool_result_for = "/for"
tool_output = "/out"
tool_error = "/err"
usage_input = "/in"
usage_output = "/out_tokens"
turn_end = "/stop"

[values]
user = ["user", "human"]
assistant = ["assistant"]
tool = ["tool"]
turn_end = ["end_turn", "stop"]
"#;

#[test]
fn user_and_assistant_text_become_items_with_ids_and_times() {
    let f = format_of(FLAT);
    let items = parse(&f, r#"{"role":"Human","content":"  hello  ","ts":"2026-10-28T09:00:00Z"}"#);
    assert_eq!(items, vec![ChatItem::User { id: "40:0".into(), at: Some("2026-10-28T09:00:00Z".into()), text: "hello".into() }]);
    let items = parse(&f, r#"{"role":"assistant","message":{"content":"hi"},"model":"m-1"}"#);
    assert_eq!(items, vec![ChatItem::Assistant { id: "40:0".into(), at: None, text: "hi".into(), model: Some("m-1".into()), final_answer: false }]);
}

#[test]
fn text_parts_are_joined_and_other_parts_are_left_out() {
    let f = format_of(FLAT);
    let line = json!({"role": "assistant", "content": [
        {"type": "text", "text": "one"}, {"type": "image", "text": "no"}, "two", {"text": "three"}, {"type": "output_text", "text": "four"}, 7
    ]});
    let items = parse(&f, &line.to_string());
    assert!(matches!(&items[0], ChatItem::Assistant { text, .. } if text == "one\ntwo\nthree\nfour"));
}

#[test]
fn a_time_may_be_a_number_of_seconds_or_milliseconds() {
    let f = format_of(FLAT);
    let seconds = parse(&f, r#"{"role":"user","content":"a","ts":1793178060}"#);
    let millis = parse(&f, r#"{"role":"user","content":"a","ts":1793178060000}"#);
    for items in [seconds, millis] {
        assert!(matches!(&items[0], ChatItem::User { at: Some(at), .. } if at == "2026-10-28T09:01:00.000Z"));
    }
}

#[test]
fn a_tool_call_and_its_result_share_an_id() {
    let f = format_of(FLAT);
    let call = parse(&f, r#"{"role":"tool","name":"edit_file","id":"c9","input":{"path":"src/a.rs","new_string":"x"}}"#);
    let ChatItem::ToolCall { call_id, name, summary, detail, .. } = &call[0] else { panic!("{call:?}") };
    assert_eq!((call_id.as_str(), name.as_str(), summary.as_str()), ("c9", "edit_file", "src/a.rs"));
    assert_eq!(detail.paths, ["src/a.rs"]);
    assert!(detail.writes && !detail.helper);
    assert_eq!(detail.input, Some(json!({"path": "src/a.rs", "new_string": "x"})));

    let result = parse(&f, r#"{"role":"tool","for":"c9","out":"done","err":false}"#);
    assert_eq!(result, vec![ChatItem::ToolResult { id: "40:0".into(), at: None, call_id: "c9".into(), ok: true, output: "done".into() }]);
}

#[test]
fn a_result_fails_on_true_a_message_or_a_non_zero_number() {
    let f = format_of(FLAT);
    let ok_of = |err: &str| match &parse(&f, &format!(r#"{{"role":"tool","for":"c","out":"x","err":{err}}}"#))[0] {
        ChatItem::ToolResult { ok, .. } => *ok,
        other => panic!("{other:?}"),
    };
    assert!(!ok_of("true") && !ok_of("\"boom\"") && !ok_of("2"));
    assert!(ok_of("false") && ok_of("0") && ok_of("\"\"") && ok_of("null"));
}

#[test]
fn a_tool_input_may_be_a_json_string_or_a_plain_command() {
    let f = format_of(FLAT);
    let json_text = parse(&f, r#"{"role":"tool","name":"run","id":"1","input":"{\"command\":\"ls -la\\nsecond\"}"}"#);
    assert!(matches!(&json_text[0], ChatItem::ToolCall { summary, .. } if summary == "ls -la"));
    let plain = parse(&f, r#"{"role":"tool","name":"run","id":"1","input":"cargo test"}"#);
    let ChatItem::ToolCall { summary, detail, .. } = &plain[0] else { panic!() };
    assert_eq!(summary, "cargo test");
    assert_eq!(detail.input, Some(json!({"command": "cargo test"})));
    let none = parse(&f, r#"{"role":"tool","name":"ping"}"#);
    assert!(matches!(&none[0], ChatItem::ToolCall { call_id, summary, .. } if call_id == "line-40" && summary == "ping"));
}

#[test]
fn an_assistant_line_may_carry_a_tool_call_too() {
    let f = format_of(FLAT);
    let items = parse(&f, r#"{"role":"assistant","content":"Reading.","name":"read_file","id":"r1","input":{"file_path":"a.txt"}}"#);
    assert!(matches!(&items[0], ChatItem::Assistant { .. }));
    assert!(matches!(&items[1], ChatItem::ToolCall { id, detail, .. } if id == "40:1" && !detail.writes && detail.paths == ["a.txt"]));
}

#[test]
fn write_and_helper_tools_come_from_the_table_when_it_names_them() {
    let mut named = table(FLAT);
    named.write_tools = vec!["Save".into()];
    named.helper_tools = vec!["Delegate".into()];
    assert!(validate_session_chat(&named).is_empty());
    let f = DeclaredSessionFile::new(&named);
    let call = |name: &str| match &parse(&f, &format!(r#"{{"role":"tool","name":"{name}","id":"1","input":{{"path":"a","content":"c"}}}}"#))[0] {
        ChatItem::ToolCall { detail, .. } => detail.clone(),
        other => panic!("{other:?}"),
    };
    assert!(call("save").writes, "names are compared without regard to case");
    assert!(!call("write_file").writes, "a table that names its write tools is not guessed over");
    assert!(call("Delegate").helper);
}

#[test]
fn a_write_is_guessed_from_the_name_or_the_input_when_the_table_names_none() {
    let f = format_of(FLAT);
    let detail = |name: &str, input: &str| match &parse(&f, &format!(r#"{{"role":"tool","name":"{name}","id":"1","input":{input}}}"#))[0] {
        ChatItem::ToolCall { detail, .. } => detail.clone(),
        other => panic!("{other:?}"),
    };
    assert!(detail("apply_patch", r#"{"file":"a"}"#).writes);
    assert!(detail("x", r#"{"target_file":"a","diff":"@@"}"#).writes);
    assert!(!detail("read_file", r#"{"path":"a"}"#).writes);
    assert!(!detail("apply_patch", r#"{"query":"a"}"#).writes, "no file named, no write");
}

#[test]
fn the_end_of_a_turn_marks_the_answer_and_adds_a_turn_end_and_usage() {
    let f = format_of(FLAT);
    let items = parse(&f, r#"{"role":"assistant","content":"done","stop":"END_TURN","in":10,"out_tokens":"4"}"#);
    assert!(matches!(&items[0], ChatItem::Assistant { final_answer: true, .. }));
    assert!(matches!(&items[1], ChatItem::TurnEnd { id, outcome: TurnOutcome::Completed, .. } if id == "40:1"));
    assert!(matches!(&items[2], ChatItem::Usage { input_tokens: 10, output_tokens: 4, context_window: None, .. }));
    // A line that is only the end of a turn has no role, and still ends it.
    let bare = parse(&f, r#"{"stop":"stop"}"#);
    assert!(matches!(&bare[0], ChatItem::TurnEnd { .. }));
    let other = parse(&f, r#"{"role":"assistant","content":"more","stop":"max_tokens"}"#);
    assert!(matches!(&other[0], ChatItem::Assistant { final_answer: false, .. }) && other.len() == 1);
}

#[test]
fn tool_output_loses_terminal_codes_and_is_cut() {
    let f = format_of(FLAT);
    let line = json!({"role": "tool", "for": "c", "out": format!("\u{1b}[31mred\u{1b}[0m {}", "x".repeat(5000))});
    let ChatItem::ToolResult { output, .. } = &parse(&f, &line.to_string())[0] else { panic!() };
    assert!(output.starts_with("red xxx") && output.ends_with('…'));
    assert_eq!(output.chars().count(), 4001);
}

#[test]
fn noise_never_gives_items_and_never_panics() {
    let f = format_of(FLAT);
    for line in ["", "   ", "not json", "[]", "42", "null", "\"text\"", "{}", r#"{"role":7}"#, r#"{"role":"system","content":"x"}"#, r#"{"role":"user"}"#, r#"{"role":"user","content":""}"#, r#"{"role":"tool"}"#] {
        assert!(parse(&f, line).is_empty(), "{line:?}");
    }
    let huge = format!(r#"{{"role":"user","content":"{}"}}"#, "x".repeat(MAX_LINE_BYTES));
    assert!(parse(&f, &huge).is_empty(), "a line over the cap is skipped");
}

// ---- the folder template ----

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("pm-declared-chat-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(PathBuf::from(crate::session_chat::plain_path(&std::fs::canonicalize(dir).unwrap().to_string_lossy())))
    }

    fn file(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn every_encoding_of_the_folder_is_as_documented() {
    assert_eq!(slug(r"C:\Users\you\my proj"), "C--Users-you-my-proj");
    assert_eq!(dashes(r"C:\Users\you\my proj"), "C--Users-you-my proj");
    assert_eq!(dashes("/home/you/proj"), "-home-you-proj");
    assert_eq!(url_encoded("/home/you/my proj"), "%2Fhome%2Fyou%2Fmy%20proj");
    assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(clean_cwd(Path::new(r"\\?\C:\Users\you\proj\")), r"C:\Users\you\proj");
}

#[test]
fn a_template_is_filled_in_from_home_cwd_and_the_environment() {
    let env = |name: &str| (name == "TOOL_HOME").then(|| "/data/tool".to_string());
    let names = Names { home: Some(Path::new("/home/you")), cwd: Path::new("/home/you/proj/"), env: &env };
    assert_eq!(expand("{home}/.t/{cwd_dashes}", &names).unwrap(), "/home/you/.t/-home-you-proj");
    assert_eq!(expand("{cwd}/.t", &names).unwrap(), "/home/you/proj/.t");
    assert_eq!(expand("{env:TOOL_HOME}/{cwd_name}/{cwd_slug}", &names).unwrap(), "/data/tool/proj/-home-you-proj");
    assert_eq!(expand("{env:MISSING}/x", &names), None, "an unset variable gives no folder");
    let homeless = Names { home: None, cwd: Path::new("/p"), env: &no_env };
    assert_eq!(expand("{home}/x", &homeless), None);
}

fn pane(cwd: &Path, started_at: SystemTime) -> PaneFacts {
    PaneFacts { cwd: cwd.to_path_buf(), started_at, provider_session_id: None, home: None, launch: Default::default() }
}

fn ago(seconds: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(seconds)
}

fn set_modified(path: &Path, at: SystemTime) {
    std::fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
}

fn source(dir: &str, glob: &str) -> DeclaredSource {
    DeclaredSource::new(table(&format!("format = \"jsonl\"\ndir = \"{dir}\"\nfile_glob = \"{glob}\"\n[map]\nrole = \"/r\"\ntext = \"/t\"\n"))).unwrap()
}

fn found(located: Located) -> PathBuf {
    match located {
        Located::Found(path) => path,
        other => panic!("{other:?}"),
    }
}

// ---- finding the file ----

#[test]
fn the_newest_file_written_since_the_pane_started_is_chosen() {
    let home = Scratch::new("home");
    let cwd = home.0.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let old = home.file(".t/s/old.jsonl", b"{}\n");
    let first = home.file(".t/s/first.jsonl", b"{}\n");
    let newer = home.file(".t/s/newer.jsonl", b"{}\n");
    home.file(".t/s/readme.txt", b"x");
    set_modified(&old, ago(600));
    set_modified(&first, ago(20));
    set_modified(&newer, ago(8));
    let names = Names { home: Some(&home.0), cwd: &cwd, env: &no_env };
    let src = source("{home}/.t/s", "*.jsonl");
    let located = found(src.locate_with(&pane(&cwd, ago(30)), &names));
    assert_eq!(located.file_name().unwrap(), "newer.jsonl");
    // Nothing since the pane started: not yet.
    assert_eq!(src.locate_with(&pane(&cwd, SystemTime::now()), &names), Located::NotYet);
}

#[test]
fn an_agent_session_id_in_a_file_name_wins_over_a_newer_file() {
    let home = Scratch::new("id");
    let cwd = home.0.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let mine = home.file(".t/s/run-abc123.jsonl", b"{}\n");
    let other = home.file(".t/s/run-zzz.jsonl", b"{}\n");
    set_modified(&mine, ago(10));
    set_modified(&other, ago(1));
    let names = Names { home: Some(&home.0), cwd: &cwd, env: &no_env };
    let mut facts = pane(&cwd, ago(30));
    facts.provider_session_id = Some("abc123".into());
    let located = found(source("{home}/.t/s", "*.jsonl").locate_with(&facts, &names));
    assert_eq!(located.file_name().unwrap(), "run-abc123.jsonl");
}

#[test]
fn a_missing_folder_or_a_huge_file_is_not_yet() {
    let home = Scratch::new("none");
    let cwd = home.0.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let names = Names { home: Some(&home.0), cwd: &cwd, env: &no_env };
    let src = source("{home}/.t/s", "*.jsonl");
    assert_eq!(src.locate_with(&pane(&cwd, ago(30)), &names), Located::NotYet);
    let big = home.file(".t/s/big.jsonl", b"{}\n");
    std::fs::File::options().write(true).open(&big).unwrap().set_len(MAX_FILE_BYTES + 1).unwrap();
    assert_eq!(src.locate_with(&pane(&cwd, ago(30)), &names), Located::NotYet, "a file over the size cap is not followed");
}

#[test]
fn a_folder_under_the_pane_folder_is_allowed() {
    let root = Scratch::new("cwd");
    let file = root.file(".t/s/a.jsonl", b"{}\n");
    let names = Names { home: None, cwd: &root.0, env: &no_env };
    let located = found(source("{cwd}/.t/s", "*.jsonl").locate_with(&pane(&root.0, ago(30)), &names));
    assert_eq!(located, file);
}

#[test]
fn a_folder_outside_home_and_the_pane_folder_is_refused() {
    let home = Scratch::new("home-out");
    let elsewhere = Scratch::new("elsewhere");
    elsewhere.file("s/a.jsonl", b"{}\n");
    let cwd = home.0.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let there = elsewhere.0.to_string_lossy().replace('\\', "/");
    let env = |name: &str| (name == "OUT").then(|| there.clone());
    let names = Names { home: Some(&home.0), cwd: &cwd, env: &env };
    // The template starts with an allowed placeholder, but the variable points somewhere else.
    let located = source("{env:OUT}/s", "*.jsonl").locate_with(&pane(&cwd, ago(30)), &names);
    assert_eq!(located, Located::NotYet);
}

#[test]
fn a_dot_dot_in_the_pane_folder_cannot_climb_out() {
    let home = Scratch::new("dots");
    let elsewhere = Scratch::new("dots-out");
    elsewhere.file("a.jsonl", b"{}\n");
    let climbing = home.0.join("..").join(elsewhere.0.file_name().unwrap());
    let names = Names { home: Some(&home.0), cwd: &climbing, env: &no_env };
    assert_eq!(source("{cwd}", "*.jsonl").locate_with(&pane(&climbing, ago(30)), &names), Located::NotYet);
}

#[cfg(unix)]
#[test]
fn a_symlink_that_leaves_the_roots_is_not_followed() {
    use std::os::unix::fs::symlink;
    let home = Scratch::new("link-home");
    let outside = Scratch::new("link-out");
    let secret = outside.file("secret.jsonl", b"{}\n");
    let cwd = home.0.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(home.0.join(".t/s")).unwrap();
    symlink(&secret, home.0.join(".t/s/leak.jsonl")).unwrap();
    let names = Names { home: Some(&home.0), cwd: &cwd, env: &no_env };
    assert_eq!(source("{home}/.t/s", "*.jsonl").locate_with(&pane(&cwd, ago(30)), &names), Located::NotYet);
    // A folder that is a link to the outside is refused as a whole.
    symlink(&outside.0, home.0.join(".t/linked")).unwrap();
    assert_eq!(source("{home}/.t/linked", "*.jsonl").locate_with(&pane(&cwd, ago(30)), &names), Located::NotYet);
    // A link that stays inside is fine.
    let inside = home.file(".t/real/a.jsonl", b"{}\n");
    symlink(&inside, home.0.join(".t/s/ok.jsonl")).unwrap();
    let located = found(source("{home}/.t/s", "*.jsonl").locate_with(&pane(&cwd, ago(30)), &names));
    assert_eq!(located, inside);
}

#[test]
fn a_symlink_that_leaves_the_roots_is_not_followed_where_links_are_made() {
    // On Windows a link needs a privilege the tests do not have; the canonical-path check is the same code, and
    // `checked_file` is tested directly against a path that resolves elsewhere.
    let home = Scratch::new("cf-home");
    let outside = Scratch::new("cf-out");
    let inside = home.file("a.jsonl", b"{}\n");
    let secret = outside.file("secret.jsonl", b"{}\n");
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap();
    let roots = vec![canon(&home.0)];
    assert_eq!(super::path::checked_file(&inside, &roots), Some(canon(&inside)));
    assert_eq!(super::path::checked_file(&secret, &roots), None);
    assert_eq!(super::path::checked_file(&home.0.join("missing.jsonl"), &roots), None);
    assert_eq!(super::path::checked_file(&home.0.join("..").join(outside.0.file_name().unwrap()).join("secret.jsonl"), &roots), None);
    assert_eq!(super::path::checked_dir(&home.0.to_string_lossy(), &roots), Some(canon(&home.0)));
    assert_eq!(super::path::checked_dir(&outside.0.to_string_lossy(), &roots), None);
}

#[test]
fn looking_in_names_only_a_folder_that_is_allowed() {
    let cwd = Scratch::new("look");
    cwd.file(".t/s/a.jsonl", b"{}\n");
    let src = source("{cwd}/.t/s", "*.jsonl");
    assert_eq!(src.looking_in(&pane(&cwd.0, ago(30))), Some(cwd.0.join(".t").join("s")));
    assert_eq!(source("{cwd}/.t/none", "*.jsonl").looking_in(&pane(&cwd.0, ago(30))), None);
}

#[test]
fn a_glob_matches_names_without_regard_to_case() {
    assert!(glob_matches("*.jsonl", "a.jsonl") && glob_matches("*.JSONL", "A.jsonl"));
    assert!(glob_matches("s-?.jsonl", "s-1.jsonl") && !glob_matches("s-?.jsonl", "s-12.jsonl"));
    assert!(glob_matches("*a*.log", "banana.log") && !glob_matches("*a*.log", "banana.logx"));
    assert!(!glob_matches("*.jsonl", "a.jsonl.zst") && !glob_matches("*.jsonl", "jsonl"));
    assert!(glob_matches("*", "") && glob_matches("a*", "a"));
}

#[test]
fn a_source_asks_for_a_capped_read() {
    assert_eq!(source("{home}/x", "*.jsonl").max_read_per_poll(), Some(MAX_READ_PER_POLL));
}
