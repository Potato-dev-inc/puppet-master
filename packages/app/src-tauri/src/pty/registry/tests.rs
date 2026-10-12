use super::*;

#[test]
fn write_enter_bytes_sends_cr() {
    let mut buf = Vec::new();
    write_enter_bytes(&mut buf, "powershell").expect("write enter");
    assert_eq!(buf, b"\r");
}

#[cfg(windows)]
#[test]
fn multi_line_prompts_go_through_the_api_but_keys_and_escapes_do_not() {
    // A task with acceptance criteria is several lines; it used to be classified as terminal
    // input, typed into the attached TUI and dropped (PROMPT_NOT_SUBMITTED after 45s).
    assert!(!is_pty_control_input(
        "Reply PONG.\n\nAcceptance criteria:\n- be brief"
    ));
    assert!(!is_pty_control_input("line one\r\nline two\ttabbed"));
    assert!(is_pty_control_input("\x1b[A"));
    assert!(is_pty_control_input("\x03"));
    assert!(is_pty_control_input("text with an escape \x1b inside"));
    assert!(is_pty_control_input("\x15"));
}

#[test]
fn cursor_agent_launches_through_interactive_powershell() {
    assert!(windows_powershell_host_agent(AgentType::CursorAgent));
    let (command, base_args) = resolve_command(AgentType::CursorAgent);
    assert_eq!(command, "cursor-agent.ps1");
    let invocation = powershell_cursor_agent_command(command, &base_args, &[]);
    assert!(invocation.contains("Get-Command -Name 'cursor-agent.ps1'"));
    assert!(invocation.starts_with("& (Get-Command"));
}

/// The PTY command a pane spawn builds for `agent_type`, as `spawn_pane` resolves it.
fn spawn_command_for(agent_type: &str, cwd: &str, extra_args: &[String]) -> CommandBuilder {
    let profile = crate::tui::require_registered(agent_type).expect("registered");
    let request = crate::tui::SpawnRequest {
        cwd,
        extra_args,
        cols: 120,
        rows: 30,
        read_only: false,
    };
    launch_command(profile.as_ref(), &request).expect("launch command")
}

fn argv_of(cmd: &CommandBuilder) -> Vec<String> {
    cmd.get_argv()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// The nine code ids build exactly the command they built before `spawn_command` existed.
#[test]
fn code_profiles_command_unchanged() {
    let cwd = std::env::temp_dir().to_string_lossy().into_owned();
    let extra = vec!["--extra".to_string()];
    #[cfg(windows)]
    let expected: [(&str, Vec<&str>); 9] = [
        ("claude", vec!["powershell.exe", "-NoLogo", "-NoExit", "-Command", "& 'claude' '--extra'"]),
        (
            "codex",
            vec![
                "powershell.exe",
                "-NoLogo",
                "-NoExit",
                "-Command",
                "& 'codex' '--sandbox' 'workspace-write' '--ask-for-approval' 'never' '--extra'",
            ],
        ),
        ("opencode", vec!["powershell.exe", "-NoLogo", "-NoExit", "-Command", "& 'opencode' '--extra'"]),
        ("opencode_native", vec!["opencode", "--extra"]),
        (
            "cursor_agent",
            vec![
                "powershell.exe",
                "-NoLogo",
                "-NoExit",
                "-Command",
                "& (Get-Command -Name 'cursor-agent.ps1' -CommandType ExternalScript -ErrorAction Stop).Source '--extra'",
            ],
        ),
        ("cursor", vec!["cmd.exe", "/C", "cursor.cmd", "--extra"]),
        ("cmd", vec!["cmd.exe", "/K", "--extra"]),
        ("powershell", vec!["powershell.exe", "-NoLogo", "-NoExit", "--extra"]),
        ("bash", vec!["bash.exe", "--login", "--extra"]),
    ];
    #[cfg(not(windows))]
    let shell_cmd = if cfg!(target_os = "macos") {
        vec!["zsh", "-l", "--extra"]
    } else {
        vec!["bash", "--login", "--extra"]
    };
    #[cfg(not(windows))]
    let expected: [(&str, Vec<&str>); 9] = [
        ("claude", vec!["claude", "--extra"]),
        (
            "codex",
            vec!["codex", "--sandbox", "workspace-write", "--ask-for-approval", "never", "--extra"],
        ),
        ("opencode", vec!["opencode", "--extra"]),
        ("opencode_native", vec!["opencode", "--extra"]),
        ("cursor_agent", vec!["cursor-agent", "--extra"]),
        ("cursor", vec!["cursor", "--extra"]),
        ("cmd", shell_cmd),
        ("powershell", vec!["pwsh", "-NoLogo", "-NoExit", "--extra"]),
        ("bash", vec!["bash", "--login", "--extra"]),
    ];

    let code_ids: Vec<&str> = crate::tui::registry::Registry::seeded()
        .ordered()
        .iter()
        .map(|profile| profile.id())
        .collect();
    let expected_ids: Vec<&str> = expected.iter().map(|(id, _)| *id).collect();
    assert_eq!(code_ids, expected_ids, "snapshot covers the nine code ids in registry order");

    for (id, argv) in expected {
        let cmd = spawn_command_for(id, &cwd, &extra);
        if id.starts_with("cursor") {
            // Cursor's launcher depends on what is installed (`agent` alias preferred): the
            // choice itself is tested in tui/cursor/binary.rs; here only that --extra reaches it.
            let got = argv_of(&cmd);
            assert!(got.iter().any(|arg| arg.contains("--extra")), "{id}: {got:?}");
            assert!(!got.iter().any(|arg| arg.contains("cursor.cmd")), "{id} must not launch the IDE: {got:?}");
            continue;
        }
        assert_eq!(argv_of(&cmd), argv, "{id}: argv");
        assert_eq!(cmd.get_cwd().map(|c| c.to_string_lossy().into_owned()), Some(cwd.clone()), "{id}: cwd");
        let path = crate::shell_env::path_for_spawn();
        assert_eq!(cmd.get_env("PATH").and_then(|v| v.to_str()), Some(path), "{id}: PATH");
        #[cfg(windows)]
        assert_eq!(cmd.get_env("Path").and_then(|v| v.to_str()), Some(path), "{id}: Path");
        assert_eq!(cmd.get_env("TERM").and_then(|v| v.to_str()), Some("xterm-256color"), "{id}: TERM");
        assert_eq!(cmd.get_env("COLORTERM").and_then(|v| v.to_str()), Some("truecolor"), "{id}: COLORTERM");
    }
}

/// A profile that only exists in this test, outside the `AgentType` enum, with its own launch.
struct EchoProfile;

const ECHO_MARKER: &str = "t09-spawn-ok";

impl crate::tui::TuiProfile for EchoProfile {
    fn id(&self) -> &'static str {
        "t09_echo_probe"
    }

    fn spawn_command(&self, request: &crate::tui::SpawnRequest) -> Option<crate::tui::LaunchSpec> {
        let (program, args) = if cfg!(windows) {
            ("cmd.exe", vec!["/C".to_string(), format!("echo {ECHO_MARKER}")])
        } else {
            ("sh", vec!["-c".to_string(), format!("echo {ECHO_MARKER}")])
        };
        Some(crate::tui::LaunchSpec {
            program: program.into(),
            args,
            cwd: request.cwd.into(),
            read_only_enforced: false,
        })
    }
}

/// A registered profile with `spawn_command` gets a PTY through the spawn path without any
/// `AgentType`: the gate resolves it, its launch spec becomes the command, and the child runs.
#[test]
fn test_profile_spawns_without_agent_type() {
    use crate::tui::{registry, TuiProfile};
    let id = EchoProfile.id();
    assert!(AgentType::parse(id).is_none());
    let cwd = std::env::temp_dir().to_string_lossy().into_owned();
    let request = crate::tui::SpawnRequest {
        cwd: &cwd,
        extra_args: &[],
        cols: 80,
        rows: 24,
        read_only: false,
    };

    // Inject, resolve, restore: other tests read the process-wide registry, keep the window short.
    let _serial = registry::test_serial();
    let before = registry::snapshot();
    let mut with_probe: Vec<Arc<dyn TuiProfile>> = before.iter().cloned().collect();
    with_probe.push(Arc::new(EchoProfile));
    registry::registry().swap(with_probe);
    let resolved = crate::tui::require_registered(id)
        .map_err(|unknown| unknown.id)
        .and_then(|profile| launch_command(profile.as_ref(), &request));
    registry::registry().swap(before.iter().cloned().collect());

    let cmd = resolved.expect("registered test profile resolves a launch command");
    assert_eq!(
        cmd.get_cwd().map(|c| c.to_string_lossy().into_owned()),
        Some(cwd.clone())
    );
    assert_eq!(cmd.get_env("TERM").and_then(|v| v.to_str()), Some("xterm-256color"));

    let mut child = open_pty_child(cmd, request.rows, request.cols).expect("spawn");
    let mut reader = child.master.try_clone_reader().expect("reader");
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut seen = Vec::new();
        let mut buf = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            seen.extend_from_slice(&buf[..n]);
            if String::from_utf8_lossy(&seen).contains(ECHO_MARKER) {
                let _ = tx.send(());
                break;
            }
        }
    });
    let echoed = rx.recv_timeout(Duration::from_secs(10)).is_ok();
    let _ = child.child.kill();
    let _ = child.child.wait();
    assert!(echoed, "child output reached the PTY");
}

/// A launcher that exported NO_COLOR (npm does when its output is redirected) must not make
/// Claude Code render monochrome in a pane: the pane env keeps color-capable TERM/COLORTERM and
/// drops every color-disabling variable.
#[test]
fn pane_env_enables_color_and_drops_no_color() {
    let mut cmd = CommandBuilder::new("claude");
    for name in COLOR_DISABLING_ENV {
        cmd.env(name, "1");
    }
    apply_common_pty_env(&mut cmd);
    for name in COLOR_DISABLING_ENV {
        assert!(cmd.get_env(name).is_none(), "{name} must not reach the pane");
    }
    assert_eq!(cmd.get_env("TERM").and_then(|v| v.to_str()), Some("xterm-256color"));
    assert_eq!(cmd.get_env("COLORTERM").and_then(|v| v.to_str()), Some("truecolor"));
}
