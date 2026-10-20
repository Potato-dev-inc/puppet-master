//! No console window flashes while a lead runs the Cursor agent (zero tokens, a fake launcher).
//!
//! On Windows the Cursor agent's CLI is a PowerShell script behind `agent.cmd`. Puppet Master is a
//! GUI program, so every helper process it starts without `CREATE_NO_WINDOW` (the headless
//! `powershell.exe`, the `about` model probe, `tasklist` and `taskkill`, `git`) opens a console
//! window for as long as it lives. The fix is one door, `proc_util`; this test runs a real headless
//! Cursor run through the real `/agents/*` routes with a stand-in `agent.cmd` +
//! `cursor-agent.ps1` and counts the helper processes it took. Included from `fake_tui_e2e.rs`.

use puppet_master_app_lib::fake_tui::World;
use puppet_master_app_lib::proc_util::spawn_log;
use serde_json::{json, Value};

const FAKE_PS1: &str = r#"
if ($args.Count -gt 0 -and $args[0] -eq 'about') { 'Model               Composer 2.5'; exit 0 }
if ($args -contains 'linger') { Start-Sleep -Seconds 60; exit 0 }
'{"type":"result","subtype":"success","is_error":false,"result":"pong","session_id":"fake-chat-1"}'
"#;

const FAKE_CMD: &str = "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0cursor-agent.ps1\" %*\r\n";

/// Puts the stand-in Cursor launcher first on PATH; the returned guard restores PATH.
struct FakeCursor {
    old_path: std::ffi::OsString,
}

impl FakeCursor {
    fn install(session: &puppet_master_app_lib::fake_tui::Session) -> Self {
        let launcher = session.aux().join("cursor-bin");
        std::fs::create_dir_all(&launcher).expect("launcher dir");
        std::fs::write(launcher.join("cursor-agent.ps1"), FAKE_PS1).expect("fake ps1");
        std::fs::write(launcher.join("agent.cmd"), FAKE_CMD).expect("fake cmd");
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![launcher];
        dirs.extend(std::env::split_paths(&old_path));
        std::env::set_var("PATH", std::env::join_paths(dirs).expect("PATH"));
        FakeCursor { old_path }
    }
}

impl Drop for FakeCursor {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.old_path);
    }
}

/// Helper processes started (by program and call site) while one lead runs Cursor and polls it.
fn spawns_for_one_cursor_run(polls: usize) -> (Value, Vec<String>) {
    let session = World::get().session("no-windows-cursor");
    let _cursor = FakeCursor::install(&session);
    // The MCP shim sends its pid with every request; the bridge then asks "is that process still alive?"
    // on each access check. This process stands in for the lead's MCP shim.
    puppet_master_app_lib::mcp_sessions::note_session_pid("lead", std::process::id());

    session.run_mode("cursor_agent", false, true);
    spawn_log::start();
    let started = session
        .run_as("lead", "cursor_agent", "say pong", json!({ "headless": true, "keep_pane": false }))
        .unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let handle = started["handle"].as_str().expect("handle").to_string();
    let project = session.project().to_string_lossy().into_owned();
    // The lead polls like `wait_agents` in a loop while the run is going, and a second MCP connection
    // (another window's lead) looks at the same run: that one is not the controller, so every look asks
    // "is the controller's process still alive?" (it used to start `tasklist` each time).
    for _ in 0..polls {
        let body = json!({ "project_path": project, "handles": [handle], "until": ["completed", "failed", "cancelled"], "wait_ms": 0 });
        let _ = session.agents_as(Some("lead"), "POST", "/agents/wait", body.clone());
        // The bridge's access gate in front of the same call (the lead passes it, the watcher is told it is not the controller).
        assert_eq!(session.authorize_as("lead", "POST", "/agents/wait", body.clone()), Ok(()));
        let denied = session.authorize_as("watcher", "POST", "/agents/wait", body).expect_err("the watcher is not the controller");
        assert_eq!(denied["code"], "AUTHORIZATION_DENIED", "{denied}");
    }
    let done = session.wait(&handle, &["completed", "failed", "cancelled"], 90_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    // The model probe runs off-thread; give it a moment to finish.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let spawned = spawn_log::finish();
    (done, spawned)
}

#[test]
fn a_cursor_headless_run_and_its_polling_start_few_processes() {
    const POLLS: usize = 30;
    let (done, spawned) = spawns_for_one_cursor_run(POLLS);
    println!("-- {} helper process(es) for one Cursor run polled {POLLS} times:\n{}", spawned.len(), spawned.join("\n"));
    assert_eq!(done["status"], "completed", "the fake Cursor run should finish: {done}");
    let count = |program: &str| spawned.iter().filter(|line| line.to_ascii_lowercase().starts_with(program)).count();
    // The run itself (powershell -File cursor-agent.ps1) and the one-time `about` model probe.
    assert_eq!(count("powershell.exe"), 2, "{spawned:?}");
    // Polling must cost nothing: no tasklist/taskkill/where per wait call.
    assert_eq!(count("tasklist"), 0, "{POLLS} access checks of a non-controller must not start tasklist: {spawned:?}");
    assert_eq!(count("taskkill"), 0, "{spawned:?}");
    assert_eq!(count("cmd.exe"), 0, "{spawned:?}");
    // Bounded overall: the budget is the two PowerShells plus a handful of git calls (checkpoints).
    assert!(spawned.len() <= 12, "a Cursor run should not start more than 12 helper processes, took {}: {spawned:?}", spawned.len());
}

#[test]
fn stopping_a_cursor_run_starts_one_hidden_taskkill_and_nothing_else() {
    let session = World::get().session("no-windows-cursor-stop");
    let _cursor = FakeCursor::install(&session);
    puppet_master_app_lib::mcp_sessions::note_session_pid("lead", std::process::id());
    session.run_mode("cursor_agent", false, true);
    let started = session
        .run_as("lead", "cursor_agent", "linger", json!({ "headless": true, "keep_pane": false }))
        .unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let handle = started["handle"].as_str().expect("handle").to_string();
    let project = session.project().to_string_lossy().into_owned();
    // Wait until the process exists (the transcript has begun), then stop it.
    session.until(30, || {
        let view = session.view(&handle);
        (view["status"] == "running").then_some(())
    });
    std::thread::sleep(std::time::Duration::from_millis(1500));
    spawn_log::start();
    let stopped = session.agents_as(Some("lead"), "POST", "/agents/cancel", json!({ "project_path": project, "handle": handle }));
    let spawned = spawn_log::finish();
    println!("-- cancel: {stopped:?}\n{}", spawned.join("\n"));
    assert!(stopped.is_ok(), "{stopped:?}");
    assert_eq!(spawned.iter().filter(|s| s.starts_with("taskkill")).count(), 1, "{spawned:?}");
    assert!(spawned.len() <= 2, "{spawned:?}");
}
