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
    launch_command(profile.as_ref(), &request, &crate::tui::LaunchEnv::default()).expect("launch command")
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
    // Cursor's launcher depends on what is installed, so the test installs a stub one (the real
    // PATH is not read) and the run is the same on a machine with and without Cursor.
    let cursor_cli = crate::tui::cursor::binary::stub::installed();
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
            let stub_dir = cursor_cli.dir().unwrap().to_string_lossy().into_owned();
            assert!(got.iter().any(|arg| arg.contains(&stub_dir)), "{id} runs the installed CLI: {got:?}");
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
            env: Vec::new(),
            env_remove: Vec::new(),
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
        .and_then(|profile| launch_command(profile.as_ref(), &request, &crate::tui::LaunchEnv::default()));
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

// ---- env channel: LaunchSpec.env / env_remove and the account env reach every launch path ----

fn env_pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn env_str(cmd: &CommandBuilder, key: &str) -> Option<String> {
    cmd.get_env(key).and_then(|v| v.to_str()).map(str::to_string)
}

/// Run `cmd` on a PTY and return everything it printed until `PM_DONE` (or 10 seconds).
fn run_in_pty(cmd: CommandBuilder) -> String {
    let mut child = open_pty_child(cmd, 24, 120).expect("spawn");
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
            if String::from_utf8_lossy(&seen).contains("PM_DONE") {
                break;
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&seen).into_owned());
    });
    let out = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_default();
    let _ = child.child.kill();
    let _ = child.child.wait();
    out
}

/// A shell one-liner that prints `VAL=<$name>` then `PM_DONE`.
fn print_env_command(name: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        ("cmd.exe".into(), vec!["/C".into(), format!("echo VAL=%{name}%&& echo PM_DONE")])
    } else {
        ("sh".into(), vec!["-c".into(), format!("echo VAL=${name}; echo PM_DONE")])
    }
}

#[test]
fn launch_spec_env_reaches_the_pty_child() {
    let (program, args) = print_env_command("PM_ENV_T");
    let mut spec = crate::tui::LaunchSpec::new(program, args, std::env::temp_dir(), false);
    spec.env = env_pairs(&[("PM_ENV_T", "spec-env-7731")]);
    let cmd = build_launch_spec_command(&spec, &crate::tui::LaunchEnv::default());
    assert_eq!(env_str(&cmd, "PM_ENV_T").as_deref(), Some("spec-env-7731"));
    assert!(run_in_pty(cmd).contains("VAL=spec-env-7731"), "child saw the spec env");
}

#[test]
fn launch_spec_env_remove_and_account_env_override() {
    std::env::set_var("PM_DROP_T", "leaked-value");
    let (program, args) = print_env_command("PM_DROP_T");
    let mut spec = crate::tui::LaunchSpec::new(program, args, std::env::temp_dir(), false);
    spec.env_remove = vec!["PM_DROP_T".to_string()];
    let out = run_in_pty(build_launch_spec_command(&spec, &crate::tui::LaunchEnv::default()));
    assert!(out.contains("PM_DONE") && !out.contains("leaked-value"), "unset variable must not reach the child: {out}");

    // account env wins over the spec's own env
    let mut spec = crate::tui::LaunchSpec::new("x", vec![], std::env::temp_dir(), false);
    spec.env = env_pairs(&[("HOME_VAR", "from-spec")]);
    let account = crate::tui::LaunchEnv { set: env_pairs(&[("HOME_VAR", "from-account")]), remove: vec![] };
    let cmd = build_launch_spec_command(&spec, &account);
    assert_eq!(env_str(&cmd, "HOME_VAR").as_deref(), Some("from-account"));
    assert_eq!(env_str(&cmd, "TERM").as_deref(), Some("xterm-256color"), "common PTY env is kept");
}

/// The legacy `AgentType` builders (the ones with no `spawn_command`) take the env too, on the
/// Windows PowerShell-hosted path as well as the direct one.
#[test]
fn legacy_agent_type_builders_apply_env() {
    let env = crate::tui::LaunchEnv {
        set: env_pairs(&[("CLAUDE_CONFIG_DIR", "C:/accounts/work")]),
        remove: vec!["PM_DROP_T".into()],
    };
    for agent in [AgentType::Claude, AgentType::Codex, AgentType::Opencode, AgentType::CursorAgent] {
        let cmd = build_command(agent, &std::env::temp_dir().to_string_lossy(), &[], &env);
        assert_eq!(env_str(&cmd, "CLAUDE_CONFIG_DIR").as_deref(), Some("C:/accounts/work"), "{agent:?}");
        assert!(cmd.get_env("PM_DROP_T").is_none(), "{agent:?}");
        assert_eq!(env_str(&cmd, "TERM").as_deref(), Some("xterm-256color"), "{agent:?}");
        #[cfg(windows)]
        if windows_powershell_host_agent(agent) && agent != AgentType::CursorAgent {
            // (a machine with the native Cursor CLI launches it directly instead)
            assert_eq!(argv_of(&cmd)[0], "powershell.exe", "{agent:?} is hosted by PowerShell");
        }
    }
}

/// `launch_command` is the spawn path for both kinds of profile: a code profile gets the account
/// env through the legacy builder.
#[test]
fn launch_command_passes_the_account_env_for_code_profiles() {
    let account = crate::tui::LaunchEnv { set: env_pairs(&[("CODEX_HOME", "C:/accounts/codex-1")]), remove: vec![] };
    let cwd = std::env::temp_dir().to_string_lossy().into_owned();
    let request = crate::tui::SpawnRequest { cwd: &cwd, extra_args: &[], cols: 80, rows: 24, read_only: false };
    let codex = crate::tui::require_registered("codex").expect("codex is registered");
    let cmd = launch_command(codex.as_ref(), &request, &account).unwrap();
    assert_eq!(env_str(&cmd, "CODEX_HOME").as_deref(), Some("C:/accounts/codex-1"));
}

/// Windows: the PowerShell host is a separate process; the agent it starts inherits what the
/// `CommandBuilder` set. Proven with a real PowerShell on a PTY.
#[cfg(windows)]
#[test]
fn powershell_host_inherits_the_launch_env() {
    let mut cmd = CommandBuilder::new("powershell.exe");
    for arg in ["-NoLogo", "-NoProfile", "-Command", "Write-Output (\"VAL=\" + $env:PM_ENV_PS); Write-Output PM_DONE"] {
        cmd.arg(arg);
    }
    cmd.cwd(std::env::temp_dir());
    apply_common_pty_env(&mut cmd);
    apply_launch_env(&mut cmd, &env_pairs(&[("PM_ENV_PS", "ps-host-4410")]), &[]);
    assert!(run_in_pty(cmd).contains("VAL=ps-host-4410"));
}

/// WSL-11 golden test: a Windows pane's JSON is byte-identical to before the `place` field existed,
/// because `place` is left out when it is `None`. The text lists the fields in struct order.
#[test]
fn windows_pane_json_is_unchanged_by_place_field() {
    let info = PaneInfo {
        id: "pane-1".into(),
        agent_type: "claude".into(),
        pid: 42,
        status: "running".into(),
        created_at: 1,
        last_output_at: Some(2),
        cwd: r"C:\Users\you\work\shop".into(),
        cols: 120,
        rows: 30,
        viewer_held: false,
        reprints_on_resize: false,
        account: None,
        account_label: None,
        title: None,
        parent_pane_id: None,
        place: None,
    };
    assert_eq!(
        serde_json::to_string(&info).unwrap(),
        r#"{"id":"pane-1","agent_type":"claude","pid":42,"status":"running","created_at":1,"last_output_at":2,"cwd":"C:\\Users\\you\\work\\shop","cols":120,"rows":30,"viewer_held":false,"reprints_on_resize":false,"account":null,"account_label":null}"#
    );
}

/// JSON written before WSL-11 (no `place` key) still reads, as a Windows pane.
#[test]
fn pane_json_without_place_key_reads_as_windows_pane() {
    let old = r#"{"id":"pane-1","agent_type":"claude","pid":42,"status":"running","created_at":1,"last_output_at":2,"cwd":"C:\\Users\\you\\work\\shop","cols":120,"rows":30,"account":null,"account_label":null}"#;
    let info: PaneInfo = serde_json::from_str(old).unwrap();
    assert!(info.place.is_none());
}

/// A Linux pane carries its `place` as an object (snake_case keys, like the rest of PaneInfo).
#[test]
fn linux_pane_json_carries_place() {
    let mut info = PaneInfo {
        id: "pane-2".into(),
        agent_type: "claude".into(),
        pid: 7,
        status: "running".into(),
        created_at: 1,
        last_output_at: None,
        cwd: r"D:\work\shop".into(),
        cols: 120,
        rows: 30,
        viewer_held: false,
        reprints_on_resize: false,
        account: None,
        account_label: None,
        title: None,
        parent_pane_id: None,
        place: None,
    };
    info.place = Some(crate::wsl::model::PanePlace {
        kind: "linux".into(),
        distro: "Ubuntu-Test".into(),
        cwd: "/mnt/d/work/shop".into(),
        mount_root: "/mnt/".into(),
        unc_root: r"\wsl.localhost\Ubuntu-Test".into(),
        home_windows: String::new(),
        warnings: vec![],
    });
    let json: serde_json::Value = serde_json::to_value(&info).unwrap();
    assert_eq!(json["place"]["kind"], "linux");
    assert_eq!(json["place"]["distro"], "Ubuntu-Test");
    assert_eq!(json["place"]["cwd"], "/mnt/d/work/shop");
    assert!(json["place"].get("warnings").is_none());
}
