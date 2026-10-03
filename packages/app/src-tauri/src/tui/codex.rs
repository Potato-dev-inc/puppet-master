//! OpenAI Codex CLI (`codex`).

use super::{Features, HeadlessLaunch, LaunchSpec, TuiProfile};
use serde_json::Value;
use std::path::PathBuf;

pub struct Codex;

impl TuiProfile for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            resumable_headless: true,
            ..Features::CLI
        }
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        Ok(LaunchSpec {
            program: super::executable("codex")?,
            args: exec_args(launch.task, launch.read_only, launch.resume_session),
            cwd: PathBuf::from(launch.project),
            read_only_enforced: launch.read_only,
        })
    }

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        if exit_code != 0 {
            return None;
        }
        let mut final_text = None;
        for line in output.lines() {
            let value: Value = serde_json::from_str(line).ok()?;
            let event = value.get("type")?.as_str()?;
            if event == "item.completed"
                && value.pointer("/item/type").and_then(Value::as_str) == Some("agent_message")
            {
                final_text = value
                    .pointer("/item/text")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }
        final_text
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
        let mut found = None;
        for line in output.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) == Some("thread.started") {
                if let Some(id) = value
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    found = Some(id.to_string());
                    continue;
                }
            }
            for key in ["thread_id", "session_id"] {
                if let Some(id) = value
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    found = Some(id.to_string());
                }
            }
        }
        found
    }
}

/// `codex exec --help` does not accept `--ask-for-approval`; it is a top-level flag.
/// Resume is `codex exec resume <SESSION_ID> [PROMPT]`.
fn exec_args(task: &str, read_only: bool, resume_session: Option<&str>) -> Vec<String> {
    let mut args = vec!["--ask-for-approval".into(), "never".into(), "exec".into()];
    if let Some(session) = resume_session
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push("resume".into());
        args.push(session.to_string());
        args.push("--json".into());
        args.push(task.into());
        return args;
    }
    args.push("--json".into());
    args.push("--sandbox".into());
    args.push(if read_only { "read-only" } else { "workspace-write" }.into());
    args.push(task.into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_with_approval_before_exec() {
        let args = exec_args("inspect", false, None);
        let exec = args.iter().position(|arg| arg == "exec").unwrap();
        let ask = args
            .iter()
            .position(|arg| arg == "--ask-for-approval")
            .unwrap();
        assert!(ask < exec, "approval flag must precede exec: {args:?}");
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"]));
        let read_only = exec_args("inspect", true, None);
        assert!(read_only
            .windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"]));
    }

    #[test]
    fn extracts_the_final_agent_message() {
        assert_eq!(
            Codex
                .extract_result(
                    r#"{"type":"item.completed","item":{"type":"agent_message","text":"ok"}}"#,
                    0
                )
                .as_deref(),
            Some("ok")
        );
    }

    #[test]
    fn resumes_by_captured_thread_id() {
        let args = exec_args("ACK", false, Some("0199thread"));
        let exec = args.iter().position(|arg| arg == "exec").unwrap();
        let resume = args.iter().position(|arg| arg == "resume").unwrap();
        assert!(exec < resume, "{args:?}");
        assert!(args.windows(2).any(|pair| pair == ["resume", "0199thread"]));
        assert!(args.contains(&"--json".to_string()));
        assert!(!args.iter().any(|arg| arg == "--sandbox"));
        let json = r#"{"type":"thread.started","thread_id":"0199thread"}
{"type":"item.completed","item":{"type":"agent_message","text":"ACK"}}"#;
        assert_eq!(
            Codex.extract_provider_session(json).as_deref(),
            Some("0199thread")
        );
    }
}
