//! Reading the output of a headless `cursor-agent --print` run.

use serde_json::{json, Value};

/// Text the rejected-call warning always contains; `run_warnings` looks for it in the result.
pub(super) const REJECTED_CALL_MARKER: &str = "Cursor reported a rejected tool or shell call";

/// Cursor prints a refusal when a tool call is rejected (headless runs cannot approve shell
/// commands) yet exits 0. Returns a warning to attach to the result so `completed` is not read as
/// "the commands ran".
pub(super) fn rejected_call_warning(output: &str) -> Option<&'static str> {
    let lower = output.to_ascii_lowercase();
    let denied = lower.contains("not in allowlist")
        || (lower.contains("rejected")
            && ["shell", "command", "tool call", "terminal"]
                .iter()
                .any(|word| lower.contains(word)));
    denied.then_some(
        "WARNING: Cursor reported a rejected tool or shell call. Its commands may not have run, but it may still have changed files directly, so rejected does not mean unchanged; verify the effect (use checks).",
    )
}

/// Machine-readable final output after a successful exit. Human-readable fallback is excluded:
/// it is not completion evidence.
pub(super) fn extract_result(output: &str, exit_code: i32) -> Option<String> {
    if exit_code != 0 {
        return None;
    }
    let value: Value = serde_json::from_str(output.trim()).ok()?;
    ["result", "text", "response"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_string))
}

/// The chat id of a headless run (`session_id` in the `--output-format json` result object), which
/// `--resume <id>` accepts to continue the same conversation. Verified live: a resumed run reports
/// the same id.
pub(super) fn extract_provider_session(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output.trim()).ok()?;
    value
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|id| !id.is_empty())
}

/// Recognises the trust screen a headless run writes before exiting in an untrusted folder. It is
/// surfaced as a prompt for the user; `--trust`, `--yolo` and `-f` are never passed on the user's
/// behalf.
pub(super) fn detect_trust(agent_type: &str, output: &str) -> Option<Value> {
    let lower = output.to_ascii_lowercase();
    if !(lower.contains("workspace trust required") || lower.contains("do you trust the contents"))
    {
        return None;
    }
    Some(json!({
        "agent_type": agent_type,
        "kind": "workspace_trust",
        "text": output.trim(),
        "choices": [],
        "headless": true,
        "resolution": "Trust this folder yourself in an interactive Cursor Agent (run `agent` in the project and accept the trust prompt), then retry with send_agent on this handle or a new run_agent. Puppet Master only passes --trust when the run was started with trust_workspace=true, and passes --force only when the run was started with allow_shell=true."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::profile_for;

    #[test]
    fn rejected_calls_are_flagged_and_other_backends_are_not() {
        let raw = r#"{"result":"Shell command rejected: Not in allowlist: python"}"#;
        let cursor = profile_for("cursor_agent");
        assert!(cursor.rejected_call_warning(raw).is_some());
        assert!(cursor.rejected_call_warning(r#"{"result":"PONG"}"#).is_none());
        assert!(profile_for("codex").rejected_call_warning(raw).is_none());
        // `run_warnings` finds this warning in a stored result by its marker text.
        assert!(rejected_call_warning(raw)
            .unwrap()
            .contains(REJECTED_CALL_MARKER));
    }

    #[test]
    fn extracts_structured_result_only_on_success() {
        assert_eq!(
            extract_result(r#"{"result":"ok"}"#, 0).as_deref(),
            Some("ok")
        );
        assert_eq!(extract_result(r#"{"result":"ok"}"#, 1), None);
        assert_eq!(extract_result("plain text", 0), None);
    }

    #[test]
    fn captures_the_chat_id_from_a_real_result() {
        let sample = include_str!("fixtures/headless_result.json");
        assert_eq!(
            extract_provider_session(sample).as_deref(),
            Some("c5ff69cc-81e6-4279-a20b-1c614522ce27")
        );
        assert_eq!(extract_result(sample, 0).as_deref(), Some("PONG"));
        assert_eq!(extract_provider_session(r#"{"session_id":""}"#), None);
        assert_eq!(extract_provider_session("plain text"), None);
    }

    #[test]
    fn headless_trust_screen_is_a_workspace_trust_prompt() {
        // Real stderr captured from a headless cursor_agent run in an untrusted folder.
        let stderr = include_str!("fixtures/workspace_trust_stderr.txt");
        let cursor = profile_for("cursor_agent");
        let prompt = cursor.detect_headless_trust(stderr).unwrap();
        assert_eq!(prompt["kind"], "workspace_trust");
        assert_eq!(prompt["choices"], json!([]));
        assert!(prompt["text"]
            .as_str()
            .unwrap()
            .contains("Workspace Trust Required"));
        assert!(profile_for("codex").detect_headless_trust(stderr).is_none());
        assert!(cursor.detect_headless_trust("some other failure").is_none());
    }
}
