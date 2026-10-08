//! Parity (P3-3): a declarative definition of Claude's headless output contract returns the same
//! facts as the code profile `tui::claude::Claude` on the strings its own tests use.
//!
//! Documented gaps (where parity is impossible; asserted below so they cannot drift silently):
//! - `extract_model`: Claude takes the key of `modelUsage` with the most `outputTokens`; a selector
//!   reads values at a path, never object keys, so the declarative profile has no model.
//! - `headless_result_ready`: Claude's profile never reports ready early (it waits for the process
//!   exit); a definition without `[headless.output.done]` behaves the same, so this is parity.
//! - Launch flags (`--permission-mode plan|acceptEdits`, standby stream-json input) are not
//!   expressible: `claude` is a reserved id and read-only enforcement is a code-profile feature.

use crate::tui::declarative::build_for_test;
use crate::tui::TuiProfile;

const DEFINITION: &str = include_str!("../fixtures/definitions/claude-parity.toml");

const STREAM: &str = concat!(
    "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s1\"}\n",
    "{\"type\":\"assistant\",\"message\":{}}\n",
    "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"PONG\",\"session_id\":\"s1\",\"modelUsage\":{\"claude-sonnet-5-5\":{\"outputTokens\":5}}}\n",
);

/// (output, exit code) pairs from the Claude headless tests.
const CORPUS: &[(&str, i32)] = &[
    (r#"{"result":"done"}"#, 0),
    ("I am done", 0),
    (r#"{"result":"done"}"#, 1),
    (r#"{"result":"x","session_id":"abc"}"#, 0),
    (r#"{"session_id":""}"#, 0),
    (r#"{"result":"x","modelUsage":{"claude-sonnet-5-5":{"outputTokens":14}}}"#, 0),
    (r#"{"modelUsage":{"claude-haiku":{"outputTokens":3},"claude-opus":{"outputTokens":90}}}"#, 0),
    (r#"{"result":"x"}"#, 0),
    ("not json", 0),
    (STREAM, 0),
    (STREAM, 1),
    ("{\"type\":\"system\"}\n{\"type\":\"assistant\"}\n", 0),
    ("", 0),
];

#[test]
fn matches_code_profile_on_fixtures() {
    let declared = build_for_test(DEFINITION).expect("claude-parity.toml builds");
    let claude = crate::tui::profile_for("claude");
    assert_eq!(claude.id(), "claude");
    for (output, exit) in CORPUS {
        assert_eq!(
            declared.extract_result(output, *exit),
            claude.extract_result(output, *exit),
            "extract_result({output:?}, {exit})"
        );
        assert_eq!(
            declared.extract_provider_session(output),
            claude.extract_provider_session(output),
            "extract_provider_session({output:?})"
        );
        assert_eq!(
            declared.headless_result_ready(output),
            claude.headless_result_ready(output),
            "headless_result_ready({output:?})"
        );
    }
}

#[test]
fn model_is_the_documented_gap() {
    let declared = build_for_test(DEFINITION).unwrap();
    let claude = crate::tui::profile_for("claude");
    let two = r#"{"modelUsage":{"claude-haiku":{"outputTokens":3},"claude-opus":{"outputTokens":90}}}"#;
    assert_eq!(claude.extract_model(two).as_deref(), Some("claude-opus"));
    assert_eq!(declared.extract_model(two), None, "selectors cannot read object keys");
}
