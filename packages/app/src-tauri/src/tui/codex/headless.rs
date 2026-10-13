//! Headless `codex exec` runs: the command line, session resume and JSONL output.

use crate::tui::{HeadlessLaunch, LaunchSpec};
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn command(launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    Ok(LaunchSpec {
        program: crate::tui::executable("codex")?,
        args: exec_args(launch.task, launch.read_only, launch.resume_session),
        cwd: PathBuf::from(launch.project),
        read_only_enforced: launch.read_only,
        env: Vec::new(),
        env_remove: Vec::new(),
    })
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

pub(super) fn extract_result(output: &str, exit_code: i32) -> Option<String> {
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

/// `codex exec --json` never says which model ran; the best truthful answer is the top-level
/// `model = "..."` of the user's `config.toml` (flagged unconfirmed by the caller).
pub(super) fn default_model() -> Option<String> {
    let home = crate::project_path::home_dir()?;
    let dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    model_from_config(&std::fs::read_to_string(dir.join("config.toml")).ok()?)
}

fn model_from_config(config: &str) -> Option<String> {
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            break; // only the top-level table
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "model" {
            let value = value.split('#').next().unwrap_or("").trim().trim_matches(|c| matches!(c, '"' | '\''));
            return (!value.is_empty()).then(|| value.to_string());
        }
    }
    None
}

pub(super) fn extract_provider_session(output: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_comes_from_the_top_level_config_key() {
        let config = "model = \"gpt-6.1-sol\" # pinned
model_reasoning_effort = \"high\"
[profiles.x]
model = \"other\"
";
        assert_eq!(model_from_config(config).as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(model_from_config("[profiles.x]
model = \"other\"
"), None);
        assert_eq!(model_from_config("model_reasoning_effort = \"high\"
"), None);
    }

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
            extract_result(
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
            extract_provider_session(json).as_deref(),
            Some("0199thread")
        );
    }
}
