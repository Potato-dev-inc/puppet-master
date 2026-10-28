//! `[session_chat.history]`: the load-time check of the table, the fields that default, and the fingerprint rule
//! (the table counts in the approval only when it is there).

use std::path::Path;

use super::*;
use crate::tui::declarative::fingerprint::canonical_value;
use crate::tui::declarative::{parse_definition, validate, Source, ValidationContext};

const BASE: &str = r#"
format = "jsonl"
dir = "{home}/.tool/sessions/{cwd_slug}"

[map]
role = "/role"
text = "/text"
"#;

const ACME: &str = include_str!("../../tui/declarative/fixtures/definitions/acme-chat.toml");

const BAD: &str = "AGENT_DEF_SESSION_CHAT_BAD_HISTORY";
const INCONSISTENT: &str = "AGENT_DEF_INCONSISTENT";

/// The base table with a `[history]` section added.
fn with_history(history: &str) -> String {
    format!("{BASE}\n[history]\n{history}")
}

fn table_of(text: &str) -> SessionChatTable {
    toml::from_str(text).unwrap_or_else(|e| panic!("{e}"))
}

/// The (code, path) of every problem with a base table that has this `[history]` section.
fn codes(history: &str) -> Vec<(&'static str, String)> {
    validate_session_chat(&table_of(&with_history(history))).into_iter().map(|e| (e.code, e.path)).collect()
}

fn only(code: &'static str, path: &str) -> Vec<(&'static str, String)> {
    vec![(code, path.to_string())]
}

const ROOTS_OK: &str = "roots = [\"{home}/.tool/sessions\"]\n";

#[test]
fn a_minimal_history_table_is_valid_and_its_defaults_are_set() {
    let t = table_of(&with_history(ROOTS_OK));
    assert!(validate_session_chat(&t).is_empty(), "{:?}", validate_session_chat(&t));
    let h = t.history.as_ref().expect("history is there");
    assert_eq!(h.depth, 1);
    assert_eq!(h.id_from, HistoryIdFrom::FileStem);
    assert!(h.file_glob.is_none());
    assert!(h.id_pointer.is_none() && h.cwd_pointer.is_none());
    assert!(h.resume_args.is_empty(), "no resume option by default");
}

#[test]
fn a_full_history_table_is_valid() {
    let full = "roots = [\"{home}/.acme/sessions\", \"{env:ACME_HOME}/chats\"]\n\
                file_glob = \"*.jsonl.zst\"\ndepth = 2\nid_from = \"pointer\"\nid_pointer = \"/session_id\"\n\
                cwd_pointer = \"/cwd\"\nresume_args = [\"--resume\", \"{session}\"]\n";
    assert!(codes(full).is_empty(), "{:?}", codes(full));
    let h = table_of(&with_history(full)).history.unwrap();
    assert_eq!(h.id_from, HistoryIdFrom::Pointer);
    assert_eq!(h.resume_args, ["--resume", "{session}"]);
}

#[test]
fn a_table_that_is_absent_stays_absent() {
    let t = table_of(BASE);
    assert!(t.history.is_none());
    assert!(validate_session_chat(&t).is_empty());
}

#[test]
fn unknown_keys_and_missing_roots_are_parse_errors() {
    assert!(toml::from_str::<SessionChatTable>(&with_history(&format!("{ROOTS_OK}bogus = 1\n"))).is_err());
    assert!(toml::from_str::<SessionChatTable>(&with_history("depth = 1\n")).is_err(), "roots is required");
    assert!(toml::from_str::<SessionChatTable>(&with_history("id_from = \"uuid\"\n")).is_err(), "unknown id_from");
}

#[test]
fn a_bad_root_names_its_index() {
    for bad in [
        "C:/Users/you/sessions",
        "/abs/sessions",
        "{cwd}/sessions",
        "{home}/../sessions",
        "{home}/{cwd_slug}",
        "{homes}/sessions",
        "{home}sessions",
        "{env:1BAD}/sessions",
        "{home/sessions",
    ] {
        assert_eq!(codes(&format!("roots = [\"{bad}\"]\n")), only(BAD, "session_chat.history.roots.0"), "{bad}");
    }
}

#[test]
fn the_roots_count_is_one_to_four() {
    assert_eq!(codes("roots = []\n"), only(BAD, "session_chat.history.roots"));
    let five = "roots = [\"{home}/a\", \"{home}/b\", \"{home}/c\", \"{home}/d\", \"{home}/e\"]\n";
    assert_eq!(codes(five), only(BAD, "session_chat.history.roots"));
    let four = "roots = [\"{home}/a\", \"{home}/b\", \"{home}/c\", \"{home}/d\"]\n";
    assert!(codes(four).is_empty());
}

#[test]
fn the_glob_must_end_in_jsonl_or_jsonl_zst() {
    assert_eq!(codes("roots = [\"{home}/a\"]\nfile_glob = \"*.txt\"\n"), only(BAD, "session_chat.history.file_glob"));
    assert_eq!(codes("roots = [\"{home}/a\"]\nfile_glob = \"../*.jsonl\"\n"), only(BAD, "session_chat.history.file_glob"));
    assert!(codes("roots = [\"{home}/a\"]\nfile_glob = \"*.jsonl.zst\"\n").is_empty());
    // With no table glob, the session's own `file_glob` is the one checked.
    let inherited = with_history(ROOTS_OK).replace("format = \"jsonl\"", "format = \"jsonl\"\nfile_glob = \"*.txt\"");
    let found: Vec<(&str, String)> = validate_session_chat(&table_of(&inherited)).into_iter().map(|e| (e.code, e.path)).collect();
    assert_eq!(found, only(BAD, "session_chat.history.file_glob"));
}

#[test]
fn depth_is_zero_to_four() {
    assert_eq!(codes("roots = [\"{home}/a\"]\ndepth = 5\n"), only(BAD, "session_chat.history.depth"));
    assert!(codes("roots = [\"{home}/a\"]\ndepth = 4\n").is_empty());
    assert!(codes("roots = [\"{home}/a\"]\ndepth = 0\n").is_empty());
}

#[test]
fn the_id_source_and_its_pointer_must_agree() {
    assert_eq!(codes("roots = [\"{home}/a\"]\nid_from = \"pointer\"\n"), only(INCONSISTENT, "session_chat.history.id_pointer"));
    assert_eq!(codes("roots = [\"{home}/a\"]\nid_pointer = \"/id\"\n"), only(INCONSISTENT, "session_chat.history.id_pointer"));
    assert_eq!(
        codes("roots = [\"{home}/a\"]\nid_from = \"pointer\"\nid_pointer = \"id\"\n"),
        only(BAD, "session_chat.history.id_pointer")
    );
    assert_eq!(codes("roots = [\"{home}/a\"]\ncwd_pointer = \"cwd\"\n"), only(BAD, "session_chat.history.cwd_pointer"));
}

#[test]
fn resume_args_may_only_use_the_session_placeholder() {
    assert!(codes("roots = [\"{home}/a\"]\nresume_args = []\n").is_empty());
    assert!(codes("roots = [\"{home}/a\"]\nresume_args = [\"--resume\", \"{session}\"]\n").is_empty());
    assert_eq!(
        codes("roots = [\"{home}/a\"]\nresume_args = [\"--resume\"]\n"),
        only(BAD, "session_chat.history.resume_args"),
        "no {{session}} at all"
    );
    assert_eq!(
        codes("roots = [\"{home}/a\"]\nresume_args = [\"--resume\", \"{session}\", \"{id}\"]\n"),
        only(BAD, "session_chat.history.resume_args.2"),
        "an unknown placeholder"
    );
    let long = format!("{{session}}{}", "a".repeat(60));
    assert_eq!(
        codes(&format!("roots = [\"{{home}}/a\"]\nresume_args = [\"{long}\"]\n")),
        only(BAD, "session_chat.history.resume_args.0"),
        "longer than 64 characters"
    );
    assert_eq!(
        codes("roots = [\"{home}/a\"]\nresume_args = [\"\", \"{session}\"]\n"),
        only(BAD, "session_chat.history.resume_args.0")
    );
    let many: Vec<String> = (0..9).map(|_| "\"{session}\"".to_string()).collect();
    assert_eq!(
        codes(&format!("roots = [\"{{home}}/a\"]\nresume_args = [{}]\n", many.join(", "))),
        only(BAD, "session_chat.history.resume_args"),
        "more than 8 elements"
    );
}

#[test]
fn the_full_definition_with_a_history_table_validates_and_round_trips() {
    let text = format!("{ACME}\n[session_chat.history]\nroots = [\"{{home}}/.acme/sessions\"]\nresume_args = [\"--resume\", \"{{session}}\"]\n");
    let def = parse_definition(&text).expect("parses");
    let problems = validate(&def, &ValidationContext { source: Source::User, code_ids: &["claude", "codex"], raw_len: text.len() });
    assert!(problems.is_empty(), "{:?}", problems.iter().map(|e| (e.code, e.path.clone())).collect::<Vec<_>>());
    let table = def.session_chat.expect("session_chat");
    let json = serde_json::to_value(&table).unwrap();
    assert_eq!(json["history"]["depth"], 1);
    let back: SessionChatTable = serde_json::from_value(json).unwrap();
    assert_eq!(back, table);
}

#[test]
fn the_history_table_is_in_the_fingerprint_only_when_declared() {
    let file = Path::new("C:/defs/acme.toml");
    let plain = parse_definition(ACME).unwrap();
    let plain_value = canonical_value(&plain, Source::User, None, file);
    assert!(plain_value["session_chat"].get("history").is_none(), "no history key: the old approval still matches");

    let one = format!("{ACME}\n[session_chat.history]\nroots = [\"{{home}}/.acme/one\"]\n");
    let two = format!("{ACME}\n[session_chat.history]\nroots = [\"{{home}}/.acme/two\"]\n");
    let with_one = parse_definition(&one).unwrap();
    let with_two = parse_definition(&two).unwrap();
    assert!(canonical_value(&with_one, Source::User, None, file)["session_chat"].get("history").is_some());
    assert_ne!(
        canonical_value(&with_one, Source::User, None, file).to_string(),
        canonical_value(&with_two, Source::User, None, file).to_string(),
        "changing the history table re-requires approval"
    );
    assert_ne!(
        canonical_value(&plain, Source::User, None, file).to_string(),
        canonical_value(&with_one, Source::User, None, file).to_string(),
        "adding it re-requires approval"
    );
}
