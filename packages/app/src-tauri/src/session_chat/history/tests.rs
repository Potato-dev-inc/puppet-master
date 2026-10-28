//! Tests for the history core: ids, the environment (fixed, process, override) and the caps.
//! No test reads the real process environment or a real agent folder: a fake process lookup stands in for it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::{resume_args_from, valid_session_id, HistoryEnv, ListLimits, ReadLimits};

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A fake "real process": these variables are set, and the home folder is `process_home`.
fn fake_process(vars: &'static [(&'static str, &'static str)]) -> Arc<dyn Fn(&str) -> Option<String> + Send + Sync> {
    Arc::new(move |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string()))
}

#[test]
fn valid_session_id_accepts_uuid_and_plain_ids() {
    assert!(valid_session_id("123e4567-e89b-12d3-a456-426614174000"));
    assert!(valid_session_id("rollout_2026-08-14-abc"));
    assert!(valid_session_id("abcdefgh"), "8 characters is the minimum");
    assert!(valid_session_id(&"a".repeat(80)), "80 characters is the maximum");
}

#[test]
fn valid_session_id_rejects_paths_metacharacters_and_bad_lengths() {
    assert!(!valid_session_id("../x"));
    assert!(!valid_session_id("../../../etc/passwd"));
    assert!(!valid_session_id("abc def ghi"), "spaces");
    assert!(!valid_session_id("abcd\"efgh"), "quotes");
    assert!(!valid_session_id("abcd&&efgh"), "ampersands");
    assert!(!valid_session_id("abcd;rm -rf x"), "semicolons");
    assert!(!valid_session_id("abcd%0aefgh"), "percent escapes");
    assert!(!valid_session_id("abcdefg"), "7 characters is too short");
    assert!(!valid_session_id(""), "empty");
    assert!(!valid_session_id(&"a".repeat(81)), "81 characters is too long");
}

#[test]
fn resume_args_fill_the_session_placeholder() {
    let id = "123e4567-e89b-12d3-a456-426614174000";
    let template = args(&["--resume", "{session}"]);
    assert_eq!(resume_args_from(&template, id), Some(args(&["--resume", id])));
}

#[test]
fn resume_args_refuse_unknown_placeholders() {
    let id = "123e4567-e89b-12d3-a456-426614174000";
    assert_eq!(resume_args_from(&args(&["--resume", "{session}", "{cwd}"]), id), None);
    assert_eq!(resume_args_from(&args(&["--resume", "{session}{x}"]), id), None, "a brace left after filling");
}

#[test]
fn resume_args_need_the_placeholder_and_a_valid_id() {
    assert_eq!(resume_args_from(&args(&["--resume", "latest"]), "123e4567-e89b-12d3"), None, "template without the placeholder");
    assert_eq!(resume_args_from(&args(&["--resume", "{session}"]), "../evil"), None, "invalid id");
    assert_eq!(resume_args_from(&args(&["--resume", "{session}"]), ""), None, "empty id");
    assert_eq!(resume_args_from(&[], "123e4567-e89b-12d3"), None, "empty template");
}

#[test]
fn resume_args_never_carry_a_shell_metacharacter_from_the_id() {
    let hostile = "abcd;rm -rf";
    assert_eq!(resume_args_from(&args(&["--resume", "{session}"]), hostile), None);
}

#[test]
fn from_process_under_test_has_no_home_and_no_variables() {
    let env = HistoryEnv::from_process();
    assert!(env.home().is_none());
    assert!(env.var("CLAUDE_CONFIG_DIR").is_none());
    assert!(env.var("CODEX_HOME").is_none());
}

#[test]
fn fixed_env_gives_its_home_and_only_its_variables() {
    let home = PathBuf::from("C:\\Users\\you\\fixture-home");
    let env = HistoryEnv::fixed(Some(home.clone()), &[("CODEX_HOME", "D:\\fixture\\codex"), ("EMPTY", "")]);
    assert_eq!(env.home(), Some(home.as_path()));
    assert_eq!(env.var("CODEX_HOME").as_deref(), Some("D:\\fixture\\codex"));
    assert!(env.var("EMPTY").is_none(), "an empty value counts as not set");
    assert!(env.var("CLAUDE_CONFIG_DIR").is_none());
}

#[test]
fn fixed_env_without_home_finds_nothing() {
    let env = HistoryEnv::fixed(None, &[]);
    assert!(env.home().is_none());
    assert!(HistoryEnv::isolated().home().is_none());
}

#[test]
fn override_home_is_the_only_home_and_blocks_other_variables() {
    static VARS: &[(&str, &str)] = &[
        ("PUPPET_MASTER_HISTORY_HOME", "D:\\pm-night\\bc-fixture-home"),
        ("CLAUDE_CONFIG_DIR", "C:\\Users\\you\\.claude"),
        ("CODEX_HOME", "C:\\Users\\you\\.codex"),
        ("XDG_DATA_HOME", "C:\\Users\\you\\.local\\share"),
    ];
    let env = HistoryEnv::from_lookup(fake_process(VARS), Some(PathBuf::from("C:\\Users\\you")));
    assert_eq!(env.home(), Some(Path::new("D:\\pm-night\\bc-fixture-home")));
    assert!(env.var("CLAUDE_CONFIG_DIR").is_none(), "the real config folder is ignored");
    assert!(env.var("CODEX_HOME").is_none());
    assert!(env.var("XDG_DATA_HOME").is_none());
}

#[test]
fn process_env_without_override_reads_variables_and_process_home() {
    static VARS: &[(&str, &str)] = &[("CLAUDE_CONFIG_DIR", "D:\\custom\\claude")];
    let env = HistoryEnv::from_lookup(fake_process(VARS), Some(PathBuf::from("C:\\Users\\you")));
    assert_eq!(env.home(), Some(Path::new("C:\\Users\\you")));
    assert_eq!(env.var("CLAUDE_CONFIG_DIR").as_deref(), Some("D:\\custom\\claude"));
}

#[test]
fn blank_override_is_ignored() {
    static VARS: &[(&str, &str)] = &[("PUPPET_MASTER_HISTORY_HOME", "   ")];
    let env = HistoryEnv::from_lookup(fake_process(VARS), Some(PathBuf::from("C:\\Users\\you")));
    assert_eq!(env.home(), Some(Path::new("C:\\Users\\you")));
}

#[test]
fn debug_output_never_prints_the_home_folder() {
    let env = HistoryEnv::fixed(Some(PathBuf::from("C:\\Users\\you")), &[]);
    assert!(!format!("{env:?}").contains("Users"));
}

#[test]
fn default_caps_match_the_spec() {
    let list = ListLimits::default();
    assert_eq!(list.since_ms, None);
    assert_eq!(list.max_chats, 2000);
    assert_eq!(list.max_file_bytes, 64 * 1024 * 1024);
    assert_eq!(list.max_entries, 20_000);
    assert_eq!(list.budget, Duration::from_secs(8));

    let read = ReadLimits::default();
    assert_eq!(read.max_items, 3000);
    assert_eq!(read.max_decompressed, 256 * 1024 * 1024);
    assert_eq!(read.chunk, 200);
}
