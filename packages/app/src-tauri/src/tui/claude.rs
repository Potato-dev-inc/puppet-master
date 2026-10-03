//! Claude Code CLI (`claude`).

use super::{Features, HeadlessLaunch, LaunchSpec, TuiProfile};
use serde_json::Value;
use std::path::PathBuf;

pub struct Claude;

impl TuiProfile for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            ..Features::CLI
        }
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        Ok(LaunchSpec {
            program: super::executable("claude")?,
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

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        if exit_code != 0 {
            return None;
        }
        let value: Value = serde_json::from_str(output.trim()).ok()?;
        value.get("result")?.as_str().map(str::to_string)
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepts_structured_success_output() {
        assert_eq!(
            Claude.extract_result(r#"{"result":"done"}"#, 0).as_deref(),
            Some("done")
        );
        assert_eq!(Claude.extract_result("I am done", 0), None);
        assert_eq!(Claude.extract_result(r#"{"result":"done"}"#, 1), None);
    }

    #[test]
    fn captures_the_session_id_for_resume() {
        assert_eq!(
            Claude
                .extract_provider_session(r#"{"result":"x","session_id":"abc"}"#)
                .as_deref(),
            Some("abc")
        );
        assert_eq!(Claude.extract_provider_session(r#"{"session_id":""}"#), None);
    }

    #[test]
    fn read_only_uses_plan_mode() {
        let launch = HeadlessLaunch {
            read_only: true,
            ..HeadlessLaunch::new(".", "inspect")
        };
        let Ok(spec) = Claude.headless_command(&launch) else {
            return; // claude.exe is not on PATH on this machine
        };
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "plan"]));
        assert!(spec.read_only_enforced);
    }
}
