//! Headless `claude -p` runs: the command line and its JSON output.

use crate::tui::{HeadlessLaunch, LaunchSpec};
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn command(launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    Ok(LaunchSpec {
        program: crate::tui::executable("claude")?,
        args: vec![
            "-p".into(),
            launch.task.into(),
            "--output-format".into(),
            "json".into(),
            "--permission-prompts".into(),
            "none".into(),
            "--permission-mode".into(),
            if launch.read_only { "plan" } else { "acceptEdits" }.into(),
        ],
        cwd: PathBuf::from(launch.project),
        read_only_enforced: launch.read_only,
    })
}

pub(super) fn extract_result(output: &str, exit_code: i32) -> Option<String> {
    if exit_code != 0 {
        return None;
    }
    let value: Value = serde_json::from_str(output.trim()).ok()?;
    value.get("result")?.as_str().map(str::to_string)
}

pub(super) fn extract_provider_session(output: &str) -> Option<String> {
    serde_json::from_str::<Value>(output.trim())
        .ok()
        .and_then(|value| {
            value
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|id| !id.is_empty())
}

/// The model(s) `claude -p --output-format json` reports in `modelUsage`; with several (e.g. a
/// helper model) the one that produced the most output tokens wins.
pub(super) fn extract_model(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output.trim()).ok()?;
    let usage = value.get("modelUsage")?.as_object()?;
    usage
        .iter()
        .max_by_key(|(_, entry)| entry.get("outputTokens").and_then(Value::as_u64).unwrap_or(0))
        .map(|(model, _)| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepts_structured_success_output() {
        assert_eq!(
            extract_result(r#"{"result":"done"}"#, 0).as_deref(),
            Some("done")
        );
        assert_eq!(extract_result("I am done", 0), None);
        assert_eq!(extract_result(r#"{"result":"done"}"#, 1), None);
    }

    #[test]
    fn reads_the_model_from_model_usage() {
        let single = r#"{"result":"x","modelUsage":{"claude-sonnet-5-5":{"outputTokens":14}}}"#;
        assert_eq!(extract_model(single).as_deref(), Some("claude-sonnet-5-5"));
        let two = r#"{"modelUsage":{"claude-haiku":{"outputTokens":3},"claude-opus":{"outputTokens":90}}}"#;
        assert_eq!(extract_model(two).as_deref(), Some("claude-opus"));
        assert_eq!(extract_model(r#"{"result":"x"}"#), None);
        assert_eq!(extract_model("not json"), None);
    }

    #[test]
    fn captures_the_session_id_for_resume() {
        assert_eq!(
            extract_provider_session(r#"{"result":"x","session_id":"abc"}"#).as_deref(),
            Some("abc")
        );
        assert_eq!(extract_provider_session(r#"{"session_id":""}"#), None);
    }

    #[test]
    fn read_only_uses_plan_mode() {
        let launch = HeadlessLaunch {
            read_only: true,
            ..HeadlessLaunch::new(".", "inspect")
        };
        let Ok(spec) = command(&launch) else {
            return; // claude.exe is not on PATH on this machine
        };
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "plan"]));
        assert!(spec.read_only_enforced);
    }
}
