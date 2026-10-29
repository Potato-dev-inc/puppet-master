//! Linux Inside in the end-to-end tests: an agent set to run in Linux starts through `pm-fake-wsl` (a stand-in for
//! `wsl.exe`, `tools/pm-fake-tui/src/bin/fake_wsl.rs`) instead of the real tool. The app's own code does all the work
//! (settings, probe, place, argv, env, stop); only the program at the end is the fake agent, and the "Linux home"
//! is a temp Windows folder. The real `wsl.exe` is never started: `PM_FAKE_WSL_EXE` is the only `wsl.exe` the app
//! knows in these tests.

use super::world::Session;
use serde_json::Value;
use std::path::PathBuf;

/// The one distro `pm-fake-wsl` lists.
pub const DISTRO: &str = "Ubuntu-Test";

/// The fake agents a test can switch to Linux.
const FAKE_AGENTS: [&str; 2] = ["fake_claude", "fake_codex"];

/// Process-wide settings the fake `wsl.exe` reads. Set while a test runs agents in Linux, removed when the session ends.
const ENV_NAMES: [&str; 5] = [
    "PM_FAKE_WSL_LOG",
    "PM_FAKE_WSL_HOME",
    "PM_FAKE_TUI_EXE",
    "PM_FAKE_TUI_SCRIPT",
    "PM_FAKE_TUI_TRANSCRIPT_ROOT",
];

/// The real `wsl.exe` must never start in these tests, not even from a late background call (a pane's stop runs on a
/// worker thread after the test is over). While `PM_FAKE_WSL_EXE` is set the app uses only that path, so between tests it
/// points at a file that is not there: the app then reports WSL as not installed.
pub(super) fn block_real_wsl() {
    std::env::set_var("PM_FAKE_WSL_EXE", std::env::temp_dir().join("pm-fake-wsl-is-not-installed.exe"));
}

fn store() -> crate::wsl::settings::Store {
    crate::wsl::settings::Store::at(&crate::app_paths::app_data_dir())
}

impl Session {
    /// The temp folder that stands for the distro's home as Windows reads it (`\wsl.localhost\Ubuntu-Test\home\you`).
    pub fn fake_wsl_home(&self) -> PathBuf {
        self.aux.join("wsl-home")
    }

    /// Sets `agent` (`fake_claude`, `fake_codex`) to run in the fake distro, the way Settings > Agents does, and points the
    /// app at `pm-fake-wsl`. The script written by `script()` is what the fake agent plays inside.
    pub fn set_linux(&self, agent: &str, distro: &str) {
        let wsl_exe = self.world.exe.with_file_name(if cfg!(windows) { "pm-fake-wsl.exe" } else { "pm-fake-wsl" });
        assert!(wsl_exe.is_file(), "pm-fake-wsl is not built at {} (build tools/pm-fake-tui)", wsl_exe.display());
        let home = self.fake_wsl_home();
        std::fs::create_dir_all(&home).expect("the fake Linux home");
        let log = self.aux.join("fake-wsl.log");
        std::env::set_var("PM_FAKE_WSL_EXE", &wsl_exe);
        std::env::set_var("PM_FAKE_WSL_LOG", &log);
        std::env::set_var("PM_FAKE_WSL_HOME", &home);
        std::env::set_var("PM_FAKE_TUI_EXE", &self.world.exe);
        std::env::set_var("PM_FAKE_TUI_SCRIPT", self.script_path());
        std::env::set_var("PM_FAKE_TUI_TRANSCRIPT_ROOT", &home);
        let name = crate::wsl::model::DistroName::parse(distro).expect("a valid distro name");
        store().set(agent, Some(&name)).expect("save the place");
        // Facts of an earlier test (its fake home) must not be reused.
        crate::wsl::probe::invalidate(distro);
    }

    /// Back to Windows for `agent`: the setting is removed.
    pub fn set_windows(&self, agent: &str) {
        store().set(agent, None).expect("save the place");
    }

    /// Everything `pm-fake-wsl` logged (`list`, `probe`, `launch`, `stop`), oldest first.
    pub fn fake_wsl_log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.aux.join("fake-wsl.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The test project's folder as the agent sees it inside Linux (`/mnt/c/Users/you/...`).
    pub fn linux_project_cwd(&self) -> String {
        let mounts = crate::wsl::path::Mounts {
            distro: DISTRO.to_string(),
            mount_root: "/mnt/".to_string(),
            unc_root: format!(r"\wsl.localhost\{DISTRO}"),
        };
        crate::wsl::path::to_linux(&self.project.to_string_lossy(), &mounts).expect("the test project is on a Windows drive")
    }

    /// The pane as the registry lists it (`PaneInfo`, snake_case JSON, `place` included).
    pub fn pane_info(&self, pane_id: &str) -> Option<Value> {
        let info = self.world.registry.lock().list().into_iter().find(|p| p.id == pane_id)?;
        serde_json::to_value(info).ok()
    }

    /// `spawn_pane` with an env guard (a guarded launch, as Assign an issue starts one).
    pub fn spawn_pane_guarded(&self, agent_type: &str, guard: &str) -> Result<String, String> {
        crate::pty::registry_spawn_pane(
            &self.world.registry,
            &self.world.app,
            crate::pty::SpawnPaneArgs {
                agent_type: agent_type.to_string(),
                cwd: Some(self.project.to_string_lossy().into_owned()),
                cols: Some(120),
                rows: Some(30),
                extra_args: None,
                pane_id: None,
                read_only: false,
                account: None,
                guard_env: Some(guard.to_string()),
            },
        )
    }

    /// The Chat tab of a pane that writes a session file, read the way the app reads it (Linux home of a Linux pane).
    /// `source_agent` is the real agent whose file format the fake writes (`claude`).
    pub fn open_file_chat(&self, pane_id: &str, source_agent: &str) -> super::ChatProbe {
        super::ChatProbe::open_files(self.world.registry.clone(), pane_id, source_agent)
    }

    /// What the History tab shows for the pane (`GET /panes/{id}/checkpoints`), as JSON.
    pub fn checkpoints(&self, pane_id: &str) -> Value {
        let view = crate::checkpoints::list_view(pane_id).expect("the checkpoint list");
        serde_json::to_value(view).unwrap_or(Value::Null)
    }

    /// The files checkpoint `id` changed compared with the one before it.
    pub fn checkpoint_paths(&self, pane_id: &str, id: &str) -> Vec<String> {
        crate::checkpoints::live()
            .diff(pane_id, id)
            .map(|changes| changes.into_iter().map(|change| change.path).collect())
            .unwrap_or_default()
    }

    /// Undo: put the project's files back to checkpoint `id` of the pane.
    pub fn restore_checkpoint(&self, pane_id: &str, id: &str) -> Result<Value, String> {
        crate::checkpoints::live()
            .restore(pane_id, id)
            .map(|report| serde_json::to_value(report).unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    }
}

/// The end of a session: every agent back on Windows, no setting left in the process.
pub(super) fn end() {
    for agent in FAKE_AGENTS {
        let _ = store().set(agent, None);
    }
    crate::wsl::probe::invalidate(DISTRO);
    block_real_wsl();
    for name in ENV_NAMES {
        std::env::remove_var(name);
    }
}
