//! The Linux branch of a pane spawn: the Windows command is unchanged, the Linux command has the
//! exact `wsl.exe` shape, refusals stop the spawn, and closing a Linux pane stops what it left.

use super::*;
use crate::operations::OperationError;
use crate::tui::{LaunchEnv, SpawnRequest};
use crate::wsl::model::{
    Distro, DistroFacts, DistroName, PanePlace, WslStatus, WSL_ACCOUNT_NOT_YET, WSL_GUARD_NOT_SUPPORTED,
};
use crate::wsl::path::Mounts;
use crate::wsl::place::PlaceProbe;
use crate::wsl::settings::Store;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const WSL_EXE: &str = r"C:\Windows\System32\wsl.exe";

fn argv_of(cmd: &CommandBuilder) -> Vec<String> {
    cmd.get_argv().iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
}

fn request<'a>(cwd: &'a str, extra: &'a [String]) -> SpawnRequest<'a> {
    SpawnRequest { cwd, extra_args: extra, cols: 120, rows: 30, read_only: false }
}

fn place_for(program: &str) -> LinuxPlace {
    LinuxPlace {
        distro: "Ubuntu-Test".into(),
        linux_cwd: "/mnt/d/work/shop".into(),
        program_abs: format!("/usr/local/bin/{program}"),
        path: "/usr/local/bin:/usr/bin:/bin".into(),
        mounts: Mounts {
            distro: "Ubuntu-Test".into(),
            mount_root: "/mnt/".into(),
            unc_root: r"\\wsl.localhost\Ubuntu-Test".into(),
        },
        home: "/home/you".into(),
        home_windows: r"\\wsl.localhost\Ubuntu-Test\home\you".into(),
    }
}

fn pane_env(pane_id: &str) -> LaunchEnv {
    spawn::compose_pane_env(LaunchEnv::default(), vec![], pane_id, None)
}

fn value_of(cmd: &CommandBuilder, name: &str) -> Option<String> {
    cmd.get_env(name).map(|value| value.to_string_lossy().into_owned())
}

// ---- Windows stays byte for byte ----

#[test]
fn a_windows_pane_command_is_launch_command_unchanged() {
    let cwd = std::env::temp_dir().to_string_lossy().into_owned();
    let extra = vec!["--extra".to_string()];
    for agent in ["claude", "codex", "opencode"] {
        let profile = crate::tui::require_registered(agent).expect("registered");
        let before = launch_command(profile.as_ref(), &request(&cwd, &extra), &pane_env("pane-w")).unwrap();
        let after = linux::pane_command(profile.as_ref(), &request(&cwd, &extra), &pane_env("pane-w"), None).unwrap();
        assert_eq!(argv_of(&after), argv_of(&before), "{agent}");
        assert_eq!(after.get_cwd(), before.get_cwd(), "{agent}");
        assert_eq!(value_of(&after, "WSLENV"), value_of(&before, "WSLENV"), "{agent}: no WSLENV is added");
        assert_eq!(value_of(&after, "PM_WSL_PATH"), None, "{agent}");
    }
}

#[cfg(windows)]
#[test]
fn the_windows_command_lines_are_the_golden_ones() {
    let cwd = std::env::temp_dir().to_string_lossy().into_owned();
    let extra = vec!["--extra".to_string()];
    let expected: [(&str, Vec<&str>); 3] = [
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
    ];
    for (agent, argv) in expected {
        let profile = crate::tui::require_registered(agent).expect("registered");
        let cmd = linux::pane_command(profile.as_ref(), &request(&cwd, &extra), &pane_env("pane-w"), None).unwrap();
        assert_eq!(argv_of(&cmd), argv, "{agent}");
    }
}

// ---- the Linux command ----

#[test]
fn a_linux_pane_runs_wsl_exe_with_exec_and_the_fixed_launcher() {
    let profile = crate::tui::require_registered("claude").expect("registered");
    let extra = vec!["--session-id".to_string(), "abc".to_string(), r#"say "hi""#.to_string()];
    let windows_cwd = r"D:\work\shop";
    let cmd = linux::linux_command(
        Path::new(WSL_EXE),
        profile.as_ref(),
        &request(windows_cwd, &extra),
        &pane_env("pane-7"),
        &place_for("claude"),
    )
    .unwrap();
    assert_eq!(
        argv_of(&cmd),
        [
            WSL_EXE,
            "-d",
            "Ubuntu-Test",
            "--cd",
            "/mnt/d/work/shop",
            "--exec",
            "/bin/sh",
            "-c",
            crate::wsl::launch::LAUNCHER,
            crate::wsl::launch::ARGV0,
            "/usr/local/bin/claude",
            "--session-id",
            "abc",
            r#"say "hi""#,
        ]
    );
    assert_eq!(cmd.get_cwd().map(|c| c.to_string_lossy().into_owned()).as_deref(), Some(windows_cwd));
}

#[test]
fn only_the_pane_id_and_the_path_marker_cross_and_wslenv_is_always_ours() {
    let profile = crate::tui::require_registered("codex").expect("registered");
    let cmd = linux::linux_command(
        Path::new(WSL_EXE),
        profile.as_ref(),
        &request(r"D:\work\shop", &[]),
        &pane_env("pane-7"),
        &place_for("codex"),
    )
    .unwrap();
    assert_eq!(value_of(&cmd, "WSLENV").as_deref(), Some("PM_WSL_PATH/u:PUPPET_MASTER_PANE_ID"));
    assert_eq!(value_of(&cmd, "PUPPET_MASTER_PANE_ID").as_deref(), Some("pane-7"));
    assert_eq!(value_of(&cmd, "PM_WSL_PATH").as_deref(), Some("/usr/local/bin:/usr/bin:/bin"));
    // The agent's own Linux flags come from the profile, after the program.
    let argv = argv_of(&cmd);
    let at = argv.iter().position(|arg| arg == "/usr/local/bin/codex").unwrap();
    assert_eq!(&argv[at + 1..], ["--sandbox", "workspace-write", "--ask-for-approval", "never"]);
}

#[test]
fn a_windows_path_in_the_env_is_refused_with_a_plain_sentence() {
    let profile = crate::tui::require_registered("claude").expect("registered");
    let mut env = pane_env("pane-7");
    env.set.push(("SOME_HOME".into(), r"C:\Users\you".into()));
    let err = linux::linux_command(
        Path::new(WSL_EXE),
        profile.as_ref(),
        &request(r"D:\x", &[]),
        &env,
        &place_for("claude"),
    )
    .unwrap_err();
    assert_eq!(err, "A setting for this agent can't be passed into Linux.");
}

#[test]
fn an_agent_with_no_linux_launch_cannot_get_a_linux_command() {
    let profile = crate::tui::require_registered("cmd").expect("registered");
    let err = linux::linux_command(
        Path::new(WSL_EXE),
        profile.as_ref(),
        &request(r"D:\x", &[]),
        &pane_env("pane-7"),
        &place_for("cmd"),
    )
    .unwrap_err();
    assert!(err.ends_with("can't run in Linux from Puppet Master yet."), "{err}");
}

// ---- the decision ----

struct FakeProbe;

impl PlaceProbe for FakeProbe {
    fn status(&self, _max_age_ms: u64) -> WslStatus {
        WslStatus::Ready {
            distros: vec![Distro { name: "Ubuntu-Test".into(), default: true, version: 2, state: "Running".into() }],
        }
    }

    fn facts(&self, distro: &str, _max_age_ms: u64) -> Result<DistroFacts, OperationError> {
        Ok(DistroFacts {
            distro: distro.into(),
            checked_at_ms: 1,
            uid: 1000,
            home: "/home/you".into(),
            home_windows: r"\\wsl.localhost\Ubuntu-Test\home\you".into(),
            mount_root: "/mnt/".into(),
            path: "/usr/bin:/bin".into(),
            git: None,
            interop: true,
            networking: None,
            programs: BTreeMap::from([("claude".to_string(), Some("/usr/bin/claude".to_string()))]),
        })
    }
}

fn store_for_claude() -> Store {
    let dir = std::env::temp_dir().join(format!("pm-linux-spawn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::at(&dir);
    store.set("claude", Some(&DistroName::parse("Ubuntu-Test").unwrap())).unwrap();
    store
}

fn ask<'a>(pane_id: &'a str, account: Option<&'a str>, guard: Option<&'a str>) -> crate::wsl::place::Ask<'a> {
    crate::wsl::place::Ask {
        agent_type: "claude",
        pane_id,
        account,
        guard_env: guard,
        hidden: false,
        copy: false,
        headless: crate::wsl::place::Headless::No,
        kind: crate::tui::Kind::Cli,
        linux_program: crate::tui::linux_program("claude"),
    }
}

#[test]
fn a_spawn_with_no_setting_is_windows_and_reads_no_probe() {
    // In tests the settings file is a folder that does not exist: every agent is on Windows.
    let profile = crate::tui::require_registered("claude").expect("registered");
    assert_eq!(linux::plan(profile.as_ref(), "pane-plan", None, None, r"D:\x"), Ok(None));
}

#[test]
fn the_setting_puts_a_claude_pane_in_linux_and_the_place_maps_the_folder() {
    let place = crate::wsl::place::plan_with(&store_for_claude(), &FakeProbe, &ask("pane-1", None, None), None, r"D:\work\shop")
        .unwrap()
        .unwrap();
    assert_eq!(place.linux_cwd, "/mnt/d/work/shop");
    assert_eq!(place.program_abs, "/usr/bin/claude");
}

#[test]
fn an_account_or_a_guard_refuses_the_linux_spawn() {
    let store = store_for_claude();
    let account = crate::wsl::place::plan_with(&store, &FakeProbe, &ask("pane-1", Some("work"), None), None, r"D:\x");
    assert_eq!(account.unwrap_err().code, WSL_ACCOUNT_NOT_YET);
    let guarded = crate::wsl::place::plan_with(&store, &FakeProbe, &ask("pane-1", None, Some("no_push")), None, r"D:\x");
    assert_eq!(guarded.unwrap_err().code, WSL_GUARD_NOT_SUPPORTED);
}

#[test]
fn a_companion_pane_stays_on_windows_whatever_the_setting() {
    let place = crate::wsl::place::plan_with(&store_for_claude(), &FakeProbe, &ask("companion-1", None, None), None, r"D:\x");
    assert_eq!(place.unwrap(), None);
}

#[test]
fn a_hidden_pane_stays_on_windows_whatever_the_setting() {
    crate::pty::hidden::hide("linux-hidden-pane");
    let mut hidden = ask("linux-hidden-pane", None, None);
    hidden.hidden = crate::pty::hidden::is_hidden("linux-hidden-pane");
    let place = crate::wsl::place::plan_with(&store_for_claude(), &FakeProbe, &hidden, None, r"D:\x");
    crate::pty::hidden::reveal("linux-hidden-pane");
    assert_eq!(place.unwrap(), None);
}

// ---- closing ----

fn linux_info_place(distro: &str) -> PanePlace {
    PanePlace {
        kind: "linux".into(),
        distro: distro.into(),
        cwd: "/mnt/d/x".into(),
        mount_root: "/mnt/".into(),
        unc_root: "unc".into(),
        home_windows: String::new(),
        warnings: Vec::new(),
    }
}

#[test]
fn shutting_down_a_linux_pane_runs_the_stop_with_its_pane_id_and_a_windows_pane_does_not() {
    let mut linux_pane = PaneRegistry::test_pane_stub("shutdown-linux-pane");
    linux_pane.info.place = Some(linux_info_place("Ubuntu-Test"));
    shutdown_pane(linux_pane);
    shutdown_pane(PaneRegistry::test_pane_stub("shutdown-windows-pane"));
    let stops = crate::wsl::live::recorded_stops();
    assert!(stops.contains(&("Ubuntu-Test".to_string(), "shutdown-linux-pane".to_string())));
    assert!(!stops.iter().any(|(_, pane)| pane == "shutdown-windows-pane"));
}

#[test]
fn closing_a_linux_pane_through_the_registry_stops_it_too() {
    let registry = Mutex::new(PaneRegistry::default());
    let mut pane = PaneRegistry::test_pane_stub("close-linux-pane");
    pane.info.place = Some(linux_info_place("Ubuntu-Test"));
    registry.lock().panes.insert("close-linux-pane".into(), pane);
    kill_pane_and_wait(&registry, "close-linux-pane", Some("test")).unwrap();
    assert!(crate::wsl::live::recorded_stops()
        .contains(&("Ubuntu-Test".to_string(), "close-linux-pane".to_string())));
}

// ---- a pane that ends at once ----

#[test]
fn a_pane_that_exits_inside_the_window_drops_the_distro_facts() {
    let dropped = AtomicU32::new(0);
    let gone = AtomicBool::new(false);
    let done = drop_after(&gone, &dropped, Duration::from_millis(500), true);
    assert!(done);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn a_pane_that_lives_past_the_window_keeps_the_facts() {
    let dropped = AtomicU32::new(0);
    let gone = AtomicBool::new(false);
    let done = drop_after(&gone, &dropped, Duration::from_millis(150), false);
    assert!(!done);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
}

fn drop_after(gone: &AtomicBool, dropped: &AtomicU32, window: Duration, exits: bool) -> bool {
    if exits {
        gone.store(true, Ordering::SeqCst);
    }
    linux::drop_facts_on_fast_exit(
        &|| gone.load(Ordering::SeqCst),
        window,
        Duration::from_millis(10),
        &|| {
            dropped.fetch_add(1, Ordering::SeqCst);
        },
    )
}

// ---- generic code names no agent ----

/// The production part of a source file: everything before its test module.
fn production(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap()
}

#[test]
fn the_new_spawn_code_names_no_agent() {
    let needles = ["claude", "codex", "cursor", "opencode", "agy"];
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("pty").join("registry");
    let linux_src = std::fs::read_to_string(dir.join("linux.rs")).unwrap();
    for needle in needles {
        assert!(!production(&linux_src).to_lowercase().contains(needle), "linux.rs names {needle}");
    }
    // spawn.rs holds older agent names for its other jobs: only the lines about the Linux place count.
    let spawn_src = std::fs::read_to_string(dir.join("spawn.rs")).unwrap();
    let linux_lines: Vec<&str> = production(&spawn_src)
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("linux") || lower.contains("wsl")
        })
        .collect();
    assert!(!linux_lines.is_empty());
    for line in linux_lines {
        for needle in needles {
            assert!(!line.to_lowercase().contains(needle), "spawn.rs line names {needle}: {line}");
        }
    }
}
