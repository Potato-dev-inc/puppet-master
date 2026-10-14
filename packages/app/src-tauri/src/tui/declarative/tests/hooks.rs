//! `[hooks]` schema and validator (Hook Lane §2): the shipped Claude/Codex tables are valid, every
//! `AGENT_DEF_HOOKS_*` code has a bad-definition fixture that fails with exactly that code, and a
//! definition that adds hooks gets a new fingerprint (so it needs approval again).

use std::path::Path;

use super::*;
use crate::tui::TuiProfile;
use crate::tui::declarative::{parse_definition, validate, Source, ValidationContext};

const CODE_IDS: &[&str] = &["claude", "codex", "cursor_agent", "opencode", "shell"];

fn errors_of(text: &str) -> Vec<DefError> {
    let def = parse_definition(text).unwrap_or_else(|e| panic!("parse: {e}"));
    validate(&def, &ValidationContext { source: Source::User, code_ids: CODE_IDS, raw_len: text.len() })
}

#[test]
fn the_shipped_claude_and_codex_tables_are_valid() {
    for (name, text) in [
        ("claude", include_str!("../../claude/hooks.toml")),
        ("codex", include_str!("../../codex/hooks.toml")),
    ] {
        let parsed: HooksFile = toml::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(validate_hooks(&parsed.hooks), vec![], "{name}");
        assert!(parse_hooks_toml(text).is_some(), "{name}");
    }
    let claude = crate::tui::profile_for("claude").hook_scheme().expect("claude declares hooks");
    assert_eq!(claude.install, "launch_flag");
    let codex = crate::tui::profile_for("codex").hook_scheme().expect("codex declares hooks");
    assert_eq!(codex.install, "project_file");
    assert!(crate::tui::profile_for("cursor_agent").hook_scheme().is_none(), "no Cursor hooks in v0");
}

#[test]
fn every_bad_hooks_fixture_fails_with_its_code() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/declarative/fixtures/definitions-bad-hooks");
    let mut seen = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let expected = text
            .lines()
            .find_map(|l| l.strip_prefix("# expect-code: "))
            .unwrap_or_else(|| panic!("{} has no expect-code", path.display()))
            .trim()
            .to_string();
        let errors = errors_of(&text);
        assert!(
            errors.iter().any(|e| e.code == expected),
            "{}: expected {expected}, got {errors:?}",
            path.display()
        );
        seen.insert(expected);
    }
    for code in [
        "AGENT_DEF_HOOKS_GLOBAL_CONFIG",
        "AGENT_DEF_HOOKS_SHARED_FILE",
        "AGENT_DEF_HOOKS_FAIL_CLOSED",
        "AGENT_DEF_HOOKS_EVENT",
        "AGENT_DEF_HOOKS_SYNC",
    ] {
        assert!(seen.contains(code), "no fixture for {code}");
    }
}

#[test]
fn bad_events_name_each_problem() {
    let text = include_str!("../fixtures/definitions-bad-hooks/bad-events.toml");
    let paths: Vec<String> = errors_of(text).into_iter().filter(|e| e.code == "AGENT_DEF_HOOKS_EVENT").map(|e| e.path).collect();
    assert!(paths.contains(&"hooks.events.0.pm".to_string()), "{paths:?}");
    assert!(paths.contains(&"hooks.events.1.fields.final_text".to_string()), "prompt events never carry reply text: {paths:?}");
    assert!(paths.contains(&"hooks.events.2.fields".to_string()), "permission_request needs `tool`: {paths:?}");
}

const GOOD_HOOKS: &str = r#"
[hooks]
schema = 1
install = "project_file"
[hooks.project_file]
path = ".mycoder/hooks.json"
layout = "event_groups"
root_key = "hooks"
[hooks.entry]
type = "command"
command = "{exe_quoted} {args_joined}"
async = true
timeout = 3
[[hooks.events]]
vendor = "Stop"
pm = "turn_end"
fields = { session = "session_id", final_text = "last_assistant_message" }
"#;

#[test]
fn a_declarative_definition_may_declare_hooks_and_that_changes_its_fingerprint() {
    let base = include_str!("../fixtures/definitions/minimal-tui.toml");
    let with = format!("{base}\n{GOOD_HOOKS}");
    assert_eq!(errors_of(&with), vec![]);
    let profile = crate::tui::declarative::build_for_test(&with).expect("builds");
    assert_eq!(profile.hook_scheme().map(|h| h.events.len()), Some(1));
    let file = Path::new("C:/defs/minitui.toml");
    let a = crate::tui::declarative::fingerprint::fingerprint(&parse_definition(base).unwrap(), Source::User, None, file);
    let b = crate::tui::declarative::fingerprint::fingerprint(&parse_definition(&with).unwrap(), Source::User, None, file);
    assert_ne!(a, b, "adding hooks must re-require approval");
}

#[test]
fn unknown_keys_in_the_hooks_table_are_parse_errors() {
    let base = include_str!("../fixtures/definitions/minimal-tui.toml");
    let text = format!("{base}\n{}", GOOD_HOOKS.replace("timeout = 3", "timeout = 3\n[hooks.extra]\nx = 1"));
    let err = parse_definition(&text).expect_err("unknown table");
    assert_eq!(err.code, "AGENT_DEF_UNKNOWN_FIELD");
}

#[test]
fn a_broken_embedded_scheme_means_no_hooks_not_half_hooks() {
    assert!(parse_hooks_toml("[hooks]\nschema = 2\n").is_none());
    let bad = include_str!("../../codex/hooks.toml").replace("install = \"project_file\"", "install = \"global\"");
    assert!(parse_hooks_toml(&bad).is_none());
}
