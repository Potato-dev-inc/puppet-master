use super::*;
use std::path::PathBuf;

struct TempDir(PathBuf);
impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tempdir_unique() -> TempDir {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("pm-doctor-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

/// A throwaway script that behaves like a CLI: `body` is the shell (unix) or batch (windows) text.
fn fake_cli(name: &str, unix: &str, windows: &str) -> (TempDir, VersionCommand) {
    let dir = tempdir_unique();
    let path: PathBuf;
    #[cfg(windows)]
    {
        let _ = unix;
        path = dir.path().join(format!("{name}.cmd"));
        std::fs::write(&path, format!("@echo off\r\n{windows}\r\n")).unwrap();
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = windows;
        path = dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{unix}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let command = VersionCommand { program: path.to_string_lossy().into_owned(), args: vec!["--version".into()] };
    (dir, command)
}

fn probe(command: Option<VersionCommand>, installed: bool, can_test: bool) -> Probe {
    Probe {
        id: "fake".into(),
        label: "Fake".into(),
        installed,
        version_command: command,
        install_hint: Some("npm i -g fake".into()),
        install_fallback: None,
        login_hint: Some("Run: fake login".into()),
        can_test,
    }
}

const SHORT: Duration = Duration::from_millis(400);
const LONG: Duration = Duration::from_secs(10);

#[test]
fn working_binary_reports_its_version() {
    let (_dir, cmd) = fake_cli("fake", "echo 'fake 1.2.3'", "echo fake 1.2.3");
    let health = check_probe(&probe(Some(cmd), true, true), LONG);
    assert_eq!(health.state, HealthState::Unchecked);
    assert_eq!(health.version.as_deref(), Some("fake 1.2.3"));
    assert!(health.reason.contains("Test"), "{}", health.reason);
    assert_eq!(health.install_hint.as_deref(), Some("npm i -g fake"));
}

#[test]
fn missing_binary_is_not_installed_and_keeps_both_hints() {
    let health = check_probe(&probe(None, false, false), LONG);
    assert_eq!(health.state, HealthState::NotInstalled);
    assert!(!health.installed);
    assert_eq!(health.login_hint.as_deref(), Some("Run: fake login"));
}

#[test]
fn failing_version_command_is_an_error_with_the_cli_message() {
    let (_dir, cmd) = fake_cli("broken", "echo 'boom: bad config' >&2; exit 3", "echo boom: bad config 1>&2\r\nexit /b 3");
    let health = check_probe(&probe(Some(cmd), true, false), LONG);
    assert_eq!(health.state, HealthState::Error);
    assert!(health.reason.contains("boom: bad config"), "{}", health.reason);
}

#[test]
fn unspawnable_program_is_an_error() {
    let cmd = VersionCommand { program: "Z:/definitely/not/here".into(), args: vec![] };
    let health = check_probe(&probe(Some(cmd), true, false), LONG);
    assert_eq!(health.state, HealthState::Error);
}

#[test]
fn hanging_version_command_times_out() {
    let (_dir, cmd) = fake_cli("hang", "sleep 5", "ping -n 6 127.0.0.1 >nul");
    let started = Instant::now();
    let health = check_probe(&probe(Some(cmd), true, false), SHORT);
    assert!(started.elapsed() < Duration::from_secs(4), "did not honour the timeout");
    assert_eq!(health.state, HealthState::Error);
    assert!(health.reason.contains("did not answer"), "{}", health.reason);
}

#[test]
fn silent_version_command_is_an_error() {
    let (_dir, cmd) = fake_cli("silent", "exit 0", "exit /b 0");
    assert!(matches!(run_version(&cmd, LONG), VersionOutcome::Failed(_)));
}

#[test]
fn login_failures_map_to_needs_login() {
    for message in [
        "Not logged in. Please run /login",
        "Error: 401 Unauthorized",
        "No API key found, set OPENAI_API_KEY",
        "Please sign in to continue",
    ] {
        assert_eq!(classify_failure("AGENT_REPORTED_ERROR", message).0, HealthState::NeedsLogin, "{message}");
    }
}

#[test]
fn other_failures_map_to_error_with_a_short_reason() {
    let (state, reason) = classify_failure("AGENT_REPORTED_ERROR", "model overloaded\nstack trace line 2");
    assert_eq!(state, HealthState::Error);
    assert_eq!(reason, "It failed: model overloaded");
    let long = "x".repeat(500);
    assert!(classify_failure("X", &long).1.chars().count() < 160);
    assert_eq!(classify_failure("AGENT_OUTPUT_UNPARSEABLE", "").0, HealthState::Error);
    assert_eq!(classify_failure("BACKEND_UNAVAILABLE", "not found on PATH").0, HealthState::NotInstalled);
}

#[test]
fn quick_ask_outcomes_map_to_plain_states() {
    use serde_json::json;
    assert_eq!(classify_test(&Ok(json!({"answer": "OK", "receipt": {}}))).0, HealthState::Ready);
    assert_eq!(classify_test(&Ok(json!({"answer": "  "}))).0, HealthState::Error);
    assert_eq!(classify_test(&Ok(json!({"status": "running", "handle": "h"}))).0, HealthState::Error);
    let needs = classify_test(&Ok(json!({"status": "needs_input"})));
    assert!(needs.1.contains("permission"), "{}", needs.1);
    let login = Err(OperationError::new("AGENT_REPORTED_ERROR", "Invalid API key", false));
    assert_eq!(classify_test(&login).0, HealthState::NeedsLogin);
}

#[test]
fn registered_agents_carry_hints_and_exclude_shells() {
    let probes = collect_probes();
    assert!(!probes.is_empty());
    for id in ["cmd", "powershell", "bash", "opencode_native"] {
        assert!(probes.iter().all(|p| p.id != id), "{id} is not an agent to set up");
    }
    let claude = probes.iter().find(|p| p.id == "claude").expect("claude listed");
    let expected = if cfg!(windows) { "winget install Anthropic.ClaudeCode" } else { "curl -fsSL https://claude.ai/install.sh | bash" };
    assert_eq!(claude.install_hint.as_deref(), Some(expected));
    assert_eq!(claude.install_fallback.as_deref(), Some("npm i -g @anthropic-ai/claude-code"));
    assert!(claude.login_hint.is_some());
    let codex = probes.iter().find(|p| p.id == "codex").expect("codex listed");
    let expected = if cfg!(windows) {
        "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\""
    } else {
        "curl -fsSL https://chatgpt.com/codex/install.sh | sh"
    };
    assert_eq!(codex.install_hint.as_deref(), Some(expected));
    assert_eq!(codex.install_fallback.as_deref(), Some("npm i -g @openai/codex"));
    for p in &probes {
        assert!(p.install_hint.is_some(), "{} has no install hint", p.id);
        if !p.installed {
            assert!(!p.can_test, "{} cannot be tested when missing", p.id);
        }
    }
}

#[test]
fn check_all_answers_for_every_probe() {
    let all = check_all();
    assert_eq!(all.len(), collect_probes().len());
    for health in &all {
        if !health.installed {
            assert_eq!(health.state, HealthState::NotInstalled);
        }
    }
}
