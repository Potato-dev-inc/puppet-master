//! The `[acp]` table of a declarative definition: it loads, it is checked, it is part of the fingerprint,
//! and the profile hands it to the chat lane with the definition's own id and label.

use super::MINIMAL;
use crate::acp::config::McpMode;
use crate::tui::declarative::{build_for_test, parse_definition, validate, Source, ValidationContext};
use crate::tui::TuiProfile;

const TABLE: &str = r##"
[acp]
command = "mycoder-acp"
args = ["--stdio"]
env_allow = ["MYCODER_HOME"]
resume_command = ["--resume", "{session}"]

[acp.quirks]
cancel_reports_end_turn = false

[acp.mcp_config]
mode = "disable_listed"
env = "MYCODER_CONFIG"
list_program = "mycoder"
list_args = ["mcp", "list", "--json"]
name_field = "name"
disable_path = ["servers", "{name}"]
disable_value = { enabled = false }

[[acp.pins]]
package = "mycoder-acp"
version = "1.0.0"
entry = "{adapters}/mycoder/index.js"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
"##;

fn with_table(table: &str) -> String {
    format!("{MINIMAL}\n{table}")
}

fn codes(text: &str, source: Source) -> Vec<(&'static str, String)> {
    let def = parse_definition(text).expect("parses");
    validate(&def, &ValidationContext { source, code_ids: &["claude", "codex"], raw_len: text.len() }).into_iter().map(|e| (e.code, e.path)).collect()
}

#[test]
fn a_definition_with_an_acp_table_loads_and_the_profile_hands_it_to_the_chat_lane() {
    let profile = build_for_test(&with_table(TABLE)).expect("a valid definition");
    let table = profile.acp_table().expect("the table");
    assert_eq!((table.id.as_str(), table.label.as_str()), ("mycoder", "My Coder"), "identity comes from [agent]");
    assert_eq!(table.command, "mycoder-acp");
    assert!(!table.quirks.cancel_reports_end_turn);
    assert_eq!(table.mcp_config.mode, McpMode::DisableListed);
    assert_eq!(table.resume_command, vec!["--resume".to_string(), "{session}".to_string()]);
}

#[test]
fn an_agent_without_the_table_has_no_chat_lane() {
    assert!(build_for_test(MINIMAL).unwrap().acp_table().is_none());
}

#[test]
fn a_bad_table_is_refused_with_its_own_code_and_path() {
    let bad = TABLE.replace("disable_path = [\"servers\", \"{name}\"]", "disable_path = [\"servers\"]");
    let found = codes(&with_table(&bad), Source::User);
    assert!(found.iter().any(|(c, p)| *c == "AGENT_DEF_ACP_INVALID" && p == "acp.mcp_config.disable_path"), "{found:?}");
    let error = build_for_test(&with_table(&bad)).err().expect("refused");
    assert!(error.contains("AGENT_DEF_ACP_INVALID"), "{error}");
}

#[test]
fn a_typo_in_the_table_is_an_error_not_ignored() {
    let typo = TABLE.replace("env_allow", "env_alow");
    assert!(parse_definition(&with_table(&typo)).is_err());
}

#[test]
fn the_table_cannot_start_an_interpreter_or_set_a_denied_variable() {
    let node = TABLE.replace("command = \"mycoder-acp\"", "command = \"node\"");
    let found = codes(&with_table(&node), Source::User);
    assert!(found.iter().any(|(c, p)| *c == "AGENT_DEF_SHELL_INTERPRETER" && p == "acp.command"), "{found:?}");
    let allowed = format!("{}\n", with_table(&node)).replacen("experimental = true", "experimental = true\nallow_interpreter = true", 1);
    assert!(!codes(&allowed, Source::User).iter().any(|(c, _)| *c == "AGENT_DEF_SHELL_INTERPRETER"));
    assert!(codes(&allowed, Source::Project).iter().any(|(c, _)| *c == "AGENT_DEF_SHELL_INTERPRETER"), "never in a project source");
    let denied = TABLE.replace("env_allow = [\"MYCODER_HOME\"]", "env_allow = [\"NODE_OPTIONS\"]");
    let found = codes(&with_table(&denied), Source::User);
    assert!(found.iter().any(|(c, _)| *c == "AGENT_DEF_DENIED_ENV"), "{found:?}");
}

#[test]
fn changing_the_table_changes_the_fingerprint_so_approval_is_asked_again() {
    let path = std::path::Path::new("mycoder.toml");
    let fp = |text: &str| crate::tui::declarative::fingerprint::fingerprint(&parse_definition(text).unwrap(), Source::User, None, path);
    let base = fp(&with_table(TABLE));
    assert_eq!(base, fp(&with_table(TABLE)));
    assert_ne!(base, fp(MINIMAL), "adding the table");
    assert_ne!(base, fp(&with_table(&TABLE.replace("--stdio", "--other"))), "changing the program's arguments");
}
