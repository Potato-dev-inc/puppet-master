//! Headless `claude -p` runs: the command line and its JSON output.

use crate::tui::{HeadlessLaunch, LaunchSpec};
use serde_json::Value;
use std::path::PathBuf;

/// Flags both launch forms share: no permission prompts, and the permission mode the run's
/// `read_only` asks for.
fn policy_args(launch: &HeadlessLaunch) -> Vec<String> {
    vec![
        "--permission-prompts".into(),
        "none".into(),
        "--permission-mode".into(),
        if launch.read_only { "plan" } else { "acceptEdits" }.into(),
    ]
}

pub(super) fn command(launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    let mut args: Vec<String> = vec![
        "-p".into(),
        launch.task.into(),
        "--output-format".into(),
        "json".into(),
    ];
    args.extend(policy_args(launch));
    Ok(LaunchSpec {
        program: crate::tui::executable("claude")?,
        args,
        cwd: PathBuf::from(launch.project),
        read_only_enforced: launch.read_only,
        env: Vec::new(),
        env_remove: Vec::new(),
    })
}

/// A process started ahead of time: the same policy flags, but the task arrives later as one
/// stream-json user message on stdin. Verified live (claude 2.1.287): after the message and EOF it
/// runs that turn, prints its events (the last is the same `result` object `--output-format json`
/// prints) and exits; started with no input it just waits, with its startup already paid.
pub(super) fn standby_command(launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    args.extend(policy_args(launch));
    Ok(LaunchSpec {
        program: crate::tui::executable("claude")?,
        args,
        cwd: PathBuf::from(launch.project),
        read_only_enforced: launch.read_only,
        env: Vec::new(),
        env_remove: Vec::new(),
    })
}

/// The stdin bytes that give a standby process its task.
pub(super) fn standby_input(task: &str) -> String {
    let message = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": task}]},
    });
    format!("{message}\n")
}

/// The run's terminal `result` object: the whole output when it is one JSON document
/// (`--output-format json`), else the last `{"type":"result",...}` line of a stream-json run.
fn result_object(output: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(output.trim()) {
        return Some(value);
    }
    output.lines().rev().find_map(|line| {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        (value.get("type").and_then(Value::as_str) == Some("result")).then_some(value)
    })
}

pub(super) fn extract_result(output: &str, exit_code: i32) -> Option<String> {
    if exit_code != 0 {
        return None;
    }
    let value = result_object(output)?;
    value.get("result")?.as_str().map(str::to_string)
}

pub(super) fn extract_provider_session(output: &str) -> Option<String> {
    result_object(output)
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
    let value = result_object(output)?;
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

    const STREAM: &str = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s1\"}\n",
        "{\"type\":\"assistant\",\"message\":{}}\n",
        "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"PONG\",\"session_id\":\"s1\",\"modelUsage\":{\"claude-sonnet-5-5\":{\"outputTokens\":5}}}\n",
    );

    #[test]
    fn a_stream_json_run_yields_the_same_result_facts() {
        assert_eq!(extract_result(STREAM, 0).as_deref(), Some("PONG"));
        assert_eq!(extract_result(STREAM, 1), None);
        assert_eq!(extract_provider_session(STREAM).as_deref(), Some("s1"));
        assert_eq!(extract_model(STREAM).as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(extract_result("{\"type\":\"system\"}\n{\"type\":\"assistant\"}\n", 0), None, "no result event yet");
    }

    #[test]
    fn standby_input_is_one_stream_json_user_line() {
        let line = standby_input("say \"hi\"\nthen stop");
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1, "one line: {line:?}");
        let value: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["content"][0]["text"], "say \"hi\"\nthen stop");
    }

    #[test]
    fn the_standby_command_carries_the_policy_flags_and_no_task() {
        let launch = HeadlessLaunch {
            read_only: true,
            ..HeadlessLaunch::new(".", "ignored")
        };
        let Ok(spec) = standby_command(&launch) else {
            return; // claude.exe is not on PATH on this machine
        };
        assert!(spec.args.windows(2).any(|pair| pair == ["--permission-mode", "plan"]));
        assert!(spec.args.windows(2).any(|pair| pair == ["--input-format", "stream-json"]));
        assert!(!spec.args.iter().any(|arg| arg == "ignored"), "the task is not known yet");
        assert!(spec.read_only_enforced);
        let writable = standby_command(&HeadlessLaunch::new(".", "x")).unwrap();
        assert!(writable.args.windows(2).any(|pair| pair == ["--permission-mode", "acceptEdits"]));
    }
}
