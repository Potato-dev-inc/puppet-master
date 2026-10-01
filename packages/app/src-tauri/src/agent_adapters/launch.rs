use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// True only where the CLI exposes a documented read-only execution policy.
    pub read_only_enforced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub task: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: String,
    pub text: String,
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub key: Option<String>,
    pub text: Option<String>,
}

pub fn launch_spec(
    agent_type: &str,
    project: &str,
    task: &str,
    read_only: bool,
) -> Result<LaunchSpec, String> {
    launch_spec_resuming(agent_type, project, task, read_only, None)
}

pub fn launch_spec_resuming(
    agent_type: &str,
    project: &str,
    task: &str,
    read_only: bool,
    resume_session: Option<&str>,
) -> Result<LaunchSpec, String> {
    if read_only && !matches!(agent_type, "claude" | "codex" | "cursor_agent") {
        return Err(format!(
            "READ_ONLY_UNSUPPORTED: read-only headless execution is unsupported for agent type: {agent_type}"
        ));
    }
    #[cfg(windows)]
    if agent_type == "cursor_agent" {
        let shim = find_path_file("cursor-agent.ps1")
            .ok_or_else(|| "cursor-agent.ps1 was not found on PATH".to_string())?;
        return Ok(LaunchSpec {
            program: "powershell.exe".into(),
            args: [
                vec![
                    "-NoProfile".into(),
                    "-File".into(),
                    shim.to_string_lossy().into_owned(),
                ],
                vec!["--print".into(), "--output-format".into(), "json".into()],
                if read_only {
                    vec!["--mode".into(), "plan".into()]
                } else {
                    vec![]
                },
                vec![task.to_string()],
            ]
            .concat(),
            cwd: PathBuf::from(project),
            read_only_enforced: read_only,
        });
    }
    let (program, args) = match agent_type {
        "claude" => (
            if cfg!(windows) {
                resolve_windows_exe("claude.exe")?
            } else {
                "claude".into()
            },
            vec![
                "-p".into(),
                task.into(),
                "--output-format".into(),
                "json".into(),
                "--permission-prompts".into(),
                "none".into(),
                "--permission-mode".into(),
                if read_only { "plan" } else { "acceptEdits" }.into(),
            ],
        ),
        "codex" => (
            if cfg!(windows) {
                resolve_windows_exe("codex.exe")?
            } else {
                "codex".into()
            },
            codex_exec_args(task, read_only, resume_session),
        ),
        "cursor_agent" => {
            let args = if read_only {
                vec!["--print", "--output-format", "json", "--mode", "plan", task]
            } else {
                vec!["--print", "--output-format", "json", task]
            };
            let args = args.into_iter().map(str::to_string).collect();
            return Ok(LaunchSpec {
                program: "cursor-agent".into(),
                args,
                cwd: PathBuf::from(project),
                read_only_enforced: read_only,
            });
        }
        other => {
            return Err(format!(
                "headless launch is unsupported for agent type: {other}"
            ))
        }
    };
    Ok(LaunchSpec {
        program,
        args,
        cwd: PathBuf::from(project),
        read_only_enforced: read_only,
    })
}

/// `codex exec --help` does not accept `--ask-for-approval`; it is a top-level flag.
/// Resume is `codex exec resume <SESSION_ID> [PROMPT]`.
fn codex_exec_args(task: &str, read_only: bool, resume_session: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--ask-for-approval".into(),
        "never".into(),
        "exec".into(),
    ];
    if let Some(session) = resume_session.map(str::trim).filter(|value| !value.is_empty()) {
        args.push("resume".into());
        args.push(session.to_string());
        args.push("--json".into());
        args.push(task.into());
        return args;
    }
    args.push("--json".into());
    args.push("--sandbox".into());
    args.push(
        if read_only {
            "read-only"
        } else {
            "workspace-write"
        }
        .into(),
    );
    args.push(task.into());
    args
}

/// Capture the provider conversation id from CLI JSONL so a later follow-up can resume it.
pub fn extract_provider_session(agent_type: &str, output: &str) -> Option<String> {
    match agent_type {
        "codex" => extract_codex_session(output),
        "claude" => serde_json::from_str::<Value>(output.trim())
            .ok()
            .and_then(|value| {
                value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .filter(|id| !id.is_empty()),
        _ => None,
    }
}

fn extract_codex_session(output: &str) -> Option<String> {
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

#[cfg(windows)]
fn find_path_file(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(windows)]
fn resolve_windows_exe(name: &str) -> Result<String, String> {
    find_path_file(name)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{name} was not found on PATH"))
}

/// Extracts only machine-readable final output after a successful process exit.
/// Human-readable fallback is deliberately excluded as it is not completion evidence.
pub fn extract_result(agent_type: &str, output: &str, exit_code: i32) -> Option<String> {
    if exit_code != 0 {
        return None;
    }
    match agent_type {
        "claude" => {
            let value: Value = serde_json::from_str(output.trim()).ok()?;
            value.get("result")?.as_str().map(str::to_string)
        }
        "cursor_agent" => {
            let value: Value = serde_json::from_str(output.trim()).ok()?;
            ["result", "text", "response"]
                .iter()
                .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_string))
        }
        "codex" => {
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
        _ => None,
    }
}

/// Recognises the trust screen a headless `cursor-agent --print` run writes before exiting in an
/// untrusted folder. It is surfaced as a prompt for the user; `--trust`, `--yolo` and `-f` are
/// never passed on the user's behalf.
pub fn detect_headless_trust(agent_type: &str, output: &str) -> Option<Value> {
    if agent_type != "cursor_agent" {
        return None;
    }
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
        "resolution": "Trust this folder yourself in an interactive Cursor Agent (run `agent` in the project and accept the trust prompt), then retry with send_agent on this handle or a new run_agent. Puppet Master never passes --trust, --yolo or -f for you."
    }))
}

pub fn detect_prompt(agent_type: &str, screen: &str, allow_broad: bool) -> Option<Value> {
    let lower = screen.to_ascii_lowercase();
    let (kind, choices) = if agent_type == "cursor_agent" {
        if lower.contains("trust this workspace") || lower.contains("trust the authors") {
            (
                "workspace_trust",
                vec!["trust".to_string(), "deny".to_string()],
            )
        } else if lower.contains("allow once") || lower.contains("run this command") {
            (
                "command_approval",
                vec!["allow_once".to_string(), "deny".to_string()],
            )
        } else {
            return None;
        }
    } else if lower.contains("allow once") || lower.contains("approve this command") {
        (
            "command_approval",
            vec!["allow_once".to_string(), "deny".to_string()],
        )
    } else if allow_broad && (lower.contains("allow all") || lower.contains("always allow")) {
        (
            "broad_approval",
            vec![
                "allow_once".to_string(),
                "deny".to_string(),
                "allow_all".to_string(),
            ],
        )
    } else {
        return None;
    };
    Some(json!({"agent_type": agent_type, "kind": kind, "text": screen.trim(), "choices": choices}))
}

pub fn prompt_reply(
    agent_type: &str,
    prompt: &Value,
    choice: &str,
    allow_broad: bool,
) -> Result<Reply, String> {
    let kind = prompt
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if agent_type == "cursor_agent" && kind == "workspace_trust" {
        return match choice {
            "trust" => Ok(Reply {
                key: Some("y".into()),
                text: None,
            }),
            "deny" => Ok(Reply {
                key: Some("n".into()),
                text: None,
            }),
            _ => Err("unsupported workspace trust choice".into()),
        };
    }
    match choice {
        "allow_once" => Ok(Reply {
            key: Some("y".into()),
            text: None,
        }),
        "deny" => Ok(Reply {
            key: Some("n".into()),
            text: None,
        }),
        "allow_all" if allow_broad && kind == "broad_approval" => Ok(Reply {
            key: Some("a".into()),
            text: None,
        }),
        "allow_all" => Err("broad approval requires explicit allow_broad".into()),
        _ => Err("unsupported approval choice".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_cursor_agent_without_ide_and_uses_plan_for_read_only() {
        let spec = launch_spec("cursor_agent", ".", "inspect", true).unwrap();
        #[cfg(windows)]
        {
            assert_eq!(spec.program, "powershell.exe");
            assert_eq!(spec.args[0], "-NoProfile");
            assert_eq!(spec.args[1], "-File");
            assert!(spec.args[2].ends_with("cursor-agent.ps1"));
        }
        #[cfg(not(windows))]
        assert_eq!(spec.program, "cursor-agent");
        assert!(spec.args.windows(2).any(|pair| pair == ["--mode", "plan"]));
        assert!(!spec
            .args
            .iter()
            .any(|arg| arg == "--yolo" || arg == "--force"));
        assert!(spec.read_only_enforced);
    }

    #[test]
    fn launches_codex_with_approval_before_exec() {
        let args = super::codex_exec_args("inspect", false, None);
        let exec = args.iter().position(|arg| arg == "exec").unwrap();
        let ask = args
            .iter()
            .position(|arg| arg == "--ask-for-approval")
            .unwrap();
        assert!(ask < exec, "approval flag must precede exec: {args:?}");
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"]));
        let read_only = super::codex_exec_args("inspect", true, None);
        assert!(read_only
            .windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"]));
    }

    #[test]
    fn only_accepts_structured_success_outputs() {
        assert!(launch_spec("opencode", ".", "inspect", true)
            .unwrap_err()
            .starts_with("READ_ONLY_UNSUPPORTED:"));
        assert_eq!(
            extract_result("claude", r#"{"result":"done"}"#, 0).as_deref(),
            Some("done")
        );
        assert_eq!(extract_result("claude", "I am done", 0), None);
        assert_eq!(extract_result("claude", r#"{"result":"done"}"#, 1), None);
        assert_eq!(
            extract_result(
                "codex",
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"ok"}}"#,
                0
            )
            .as_deref(),
            Some("ok")
        );
        assert_eq!(
            extract_result("cursor_agent", r#"{"result":"ok"}"#, 0).as_deref(),
            Some("ok")
        );
    }

    #[test]
    fn cursor_prompts_are_narrow_and_broad_approval_requires_opt_in() {
        let prompt = detect_prompt("cursor_agent", "Trust this workspace?", false).unwrap();
        assert_eq!(prompt["kind"], "workspace_trust");
        assert!(detect_prompt("cursor_agent", "Are you sure?", true).is_none());
        let broad = detect_prompt("claude", "Always allow this command?", true).unwrap();
        assert!(prompt_reply("claude", &broad, "allow_all", false).is_err());
    }

    #[test]
    fn headless_cursor_trust_screen_is_a_workspace_trust_prompt() {
        // Real stderr captured from a headless cursor_agent run in an untrusted folder.
        let stderr = include_str!("fixtures/cursor_workspace_trust_stderr.txt");
        let prompt = detect_headless_trust("cursor_agent", stderr).unwrap();
        assert_eq!(prompt["kind"], "workspace_trust");
        assert_eq!(prompt["choices"], json!([]));
        assert!(prompt["text"]
            .as_str()
            .unwrap()
            .contains("Workspace Trust Required"));
        assert!(detect_headless_trust("codex", stderr).is_none());
        assert!(detect_headless_trust("cursor_agent", "some other failure").is_none());
    }

    #[test]
    fn headless_launch_never_passes_trust_bypass_flags() {
        for read_only in [true, false] {
            let spec = launch_spec("cursor_agent", ".", "inspect", read_only).unwrap();
            assert!(!spec
                .args
                .iter()
                .any(|arg| matches!(arg.as_str(), "--trust" | "--yolo" | "--force" | "-f")));
        }
    }

    #[test]
    fn resumes_codex_by_captured_thread_id() {
        let args = super::codex_exec_args("ACK", false, Some("0199thread"));
        let exec = args.iter().position(|arg| arg == "exec").unwrap();
        let resume = args.iter().position(|arg| arg == "resume").unwrap();
        assert!(exec < resume, "{args:?}");
        assert!(args.windows(2).any(|pair| pair == ["resume", "0199thread"]));
        assert!(args.contains(&"--json".to_string()));
        assert!(!args.iter().any(|arg| arg == "--sandbox"));
        let json = r#"{"type":"thread.started","thread_id":"0199thread"}
{"type":"item.completed","item":{"type":"agent_message","text":"ACK"}}"#;
        assert_eq!(
            super::extract_provider_session("codex", json).as_deref(),
            Some("0199thread")
        );
    }
}
