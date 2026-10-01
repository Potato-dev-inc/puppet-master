use super::command::{build, output_for_command, parse_completion, powershell_with_cwd};
use super::execute;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

#[test]
fn bash_wrapper_uses_integer_status_and_cwd_marker() {
    let script = build("bash", "false", "__PM_TEST__", None);
    assert!(script.starts_with("printf '__PM_TEST___START"));
    assert!(script.contains("__pm_ec=$?"));
    assert!(script.contains("pwd | base64"));
    assert_eq!(
        parse_completion("false\n__PM_TEST__|1|L3RtcA==\n", "__PM_TEST__", "bash"),
        Some((1, "/tmp".into()))
    );
}

#[test]
fn powershell_wrapper_encodes_multiline_input_and_captures_native_code() {
    let script = build(
        "powershell",
        "Write-Output 'hi'\nexit 3",
        "__PM_TEST__",
        None,
    );
    assert!(script.starts_with("[Console]::WriteLine('__PM_TEST___START')"));
    assert!(script.contains("FromBase64String"));
    assert!(script.contains("$global:LASTEXITCODE = 0"));
    assert!(script.contains("$pmOk = $?"));
}

#[test]
fn cmd_wrapper_captures_errorlevel_after_the_command() {
    let script = build("cmd", "whoami", "__PM_TEST__", None);
    assert!(script.starts_with("echo __PM_TEST___START"));
    assert!(script.contains("set \"__pm_ec=%errorlevel%\""));
    assert!(script.contains("__PM_TEST__^|%__pm_ec%^|%cd%"));
    assert_eq!(
        parse_completion("__PM_TEST__|7|C:\\repo\n", "__PM_TEST__", "cmd"),
        Some((7, "C:\\repo".into()))
    );
}

#[test]
fn explicit_cwd_is_applied_with_shell_safe_quoting() {
    let bash = build("bash", "pwd", "__PM_TEST__", Some("/tmp/it's here"));
    assert!(bash.contains("cd -- '/tmp/it'\\''s here' &&"));
    let powershell = powershell_with_cwd("Get-Location", Some("C:\\it's here"));
    assert!(powershell.contains("Set-Location -LiteralPath 'C:\\it''s here' -ErrorAction Stop"));
    let cmd = build("cmd", "cd", "__PM_TEST__", Some("C:\\work dir"));
    assert!(cmd.contains("cd /d \"C:\\work dir\""));
}

#[test]
fn output_uses_the_command_start_and_end_markers() {
    let output =
        "old output\n__PM_TEST___START\nhello\nworld\n__PM_TEST__|0|L3RtcA==\nPS C:\\repo>";
    assert_eq!(output_for_command(output, "__PM_TEST__"), "hello\nworld");
    assert_eq!(output_for_command("old output", "__PM_TEST__"), "");
}

#[test]
fn completion_requires_the_unique_marker_and_integer_status() {
    assert_eq!(
        parse_completion("stale output", "__PM_TEST__", "bash"),
        None
    );
    assert_eq!(
        parse_completion("__PM_TEST__|not-an-int|L3RtcA==", "__PM_TEST__", "bash"),
        None
    );
}

#[test]
fn execution_requires_an_explicit_existing_pane() {
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let error = execute(&registry, "", "echo test", 100).unwrap_err();
    assert_eq!(error.code, "PANE_REQUIRED");

    let error = execute(&registry, "missing", "echo test", 100).unwrap_err();
    assert_eq!(error.code, "PANE_NOT_FOUND");
}

#[cfg(windows)]
#[test]
fn powershell_pty_executes_commands_reports_failures_and_times_out() {
    let registry = disposable_powershell_registry();
    let pane_id = "shell-exec-integration";

    let success = super::execute(&registry, pane_id, "Write-Output 'shell-exec-ok'", 10_000)
        .expect("command should complete");
    assert_eq!(success["exit_code"], 0);
    assert!(
        success["stdout"]
            .as_str()
            .unwrap()
            .contains("shell-exec-ok"),
        "unexpected result: {success}; buffer: {}",
        crate::pty::registry_read_buffer(&registry, pane_id, 80).unwrap_or_default()
    );

    let failure = super::execute(&registry, pane_id, "cmd.exe /c exit 7", 10_000)
        .expect("non-zero child exit should still complete");
    assert_eq!(failure["exit_code"], 7);

    let missing = std::env::temp_dir().join(format!("missing-pm-cwd-{}", uuid::Uuid::new_v4()));
    let missing = missing.to_string_lossy();
    let bad_cwd = super::execute_in(
        &registry,
        pane_id,
        "Write-Output 'must-not-run'",
        10_000,
        Some(&missing),
    )
    .expect("failed Set-Location should return a command result");
    assert_eq!(bad_cwd["exit_code"], 1);
    assert!(!bad_cwd["stdout"].as_str().unwrap().contains("must-not-run"));

    let timed_out = super::execute(&registry, pane_id, "Start-Sleep -Seconds 20", 250)
        .expect_err("long command should time out");
    assert_eq!(timed_out.code, "COMMAND_TIMEOUT");
    assert_eq!(timed_out.context["status"], "timeout");

    let pane = registry.lock().take(pane_id);
    if let Some(pane) = pane {
        let mut child = pane.child;
        let _ = child.kill();
    }
}

#[cfg(windows)]
fn disposable_powershell_registry() -> Arc<Mutex<PaneRegistry>> {
    let system = native_pty_system();
    let pair = system
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open disposable PTY");
    let mut command = CommandBuilder::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-NoExit"]);
    let child = pair.slave.spawn_command(command).expect("spawn PowerShell");
    drop(pair.slave);
    let writer = pair.master.take_writer().expect("get PTY writer");
    let mut reader = pair.master.try_clone_reader().expect("clone PTY reader");
    let scrollback = Arc::new(Mutex::new(crate::pty::scrollback::Scrollback::new(1_000)));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(30, 120, 1_000)));
    let status = Arc::new(Mutex::new(crate::pty::status::PaneStatus::Running));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let exited = Arc::new(Mutex::new(false));

    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    registry.lock().panes.insert(
        "shell-exec-integration".into(),
        crate::pty::registry::PaneState {
            info: crate::pty::registry::PaneInfo {
                id: "shell-exec-integration".into(),
                agent_type: "powershell".into(),
                pid: child.process_id().unwrap_or(0),
                status: "running".into(),
                created_at: 0,
                last_output_at: None,
                cwd: std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                cols: 120,
                rows: 30,
            },
            scrollback: scrollback.clone(),
            screen: screen.clone(),
            status: status.clone(),
            last_output: last_output.clone(),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            child,
            exited: exited.clone(),
            opencode: None,
            opencode_key_event: None,
        },
    );

    thread::spawn(move || {
        let mut bytes = [0_u8; 4096];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    scrollback.lock().push_chunk(&bytes[..count]);
                    let visible = {
                        let mut parser = screen.lock();
                        parser.process(&bytes[..count]);
                        parser.screen().contents()
                    };
                    *last_output.lock() = Instant::now();
                    *status.lock() =
                        crate::pty::status::classify_agent_observation("powershell", &visible, "");
                }
            }
        }
        *exited.lock() = true;
        *status.lock() = crate::pty::status::PaneStatus::Error;
    });
    registry
}
