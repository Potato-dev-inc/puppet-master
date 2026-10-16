//! The Agent Wizard against a real ConPTY, for `tests/fake_tui_e2e.rs`: a wizard whose panes are the real
//! pane registry of the windowless test app, whose work folders live in the test's scratch folder, and
//! whose "unknown agent" is `pm-fake-tui` copied to `<scratch>/bin/<name>.exe` and found through `PATH`.
//!
//! Nothing here opens a window (the wizard's pane window option is off) and nothing touches the real
//! agents folder: finished definitions land in `<scratch>/agents`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::WizardEnv;
use crate::tui::declarative::wizard::visible::capture::Config;
use crate::tui::declarative::wizard::visible::runner::RunnerCfg;
use crate::tui::declarative::wizard::{api, Wizard};

use super::world::Session;

/// A wizard over the test app. Restores `PATH` and `PM_FAKE_TUI_SCRIPT` when dropped.
pub struct WizardRig {
    wizard: Wizard,
    agents: PathBuf,
    saved_path: Option<std::ffi::OsString>,
    saved_script: Option<std::ffi::OsString>,
}

impl Session {
    /// A wizard for the program `name` (a copy of `pm-fake-tui` named `<name>.exe`, put first on `PATH`).
    /// `script` is what the fake plays (see `tools/pm-fake-tui/README.md`).
    pub fn wizard_rig(&self, name: &str, script: &str) -> WizardRig {
        self.script(script);
        let bin = self.aux().join("bin");
        std::fs::create_dir_all(&bin).expect("bin folder");
        let exe = bin.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
        std::fs::copy(self.fake_exe(), &exe).expect("copy the fake agent");
        let saved_path = std::env::var_os("PATH");
        let mut dirs = vec![bin];
        dirs.extend(std::env::split_paths(&saved_path.clone().unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(dirs).expect("a PATH"));
        let saved_script = std::env::var_os("PM_FAKE_TUI_SCRIPT");
        std::env::set_var("PM_FAKE_TUI_SCRIPT", self.script_path());

        let agents = self.aux().join("agents");
        let temp = self.aux().join("wizard-temp");
        std::fs::create_dir_all(&temp).expect("wizard temp folder");
        let mut env = WizardEnv::app();
        env.agents_dir = agents.clone();
        env.temp_root = temp;
        env.path_dirs = None;
        env.app_build = "fake-tui-e2e".into();
        env.visible = RunnerCfg {
            capture: Config { stable_ms: 1200, ..Config::default() },
            tick: Duration::from_millis(100),
            window: false,
            ..RunnerCfg::default()
        };
        WizardRig { wizard: Wizard::new(move || env.clone()), agents, saved_path, saved_script }
    }
}

impl WizardRig {
    /// One wizard action; the error is the route's JSON error.
    pub fn call(&self, action: &str, body: Value) -> Result<Value, Value> {
        api::handle(&self.wizard, action, &body).map_err(|e| e.to_json())
    }

    pub fn agents_dir(&self) -> &Path {
        &self.agents
    }

    /// The session's random test words `(one, two, sky)`: a test may look, an AI may not.
    pub fn words(&self, session: &str) -> (String, String, String) {
        self.wizard.read(session, |s| (s.words.one.clone(), s.words.two.clone(), s.words.sky.clone())).expect("the session exists")
    }

    pub fn session_count(&self) -> usize {
        self.wizard.session_count()
    }

    /// "Check with fixtures" on a written definition: `{status, results: [{fixture, passed, failures}]}`.
    pub fn check_definition(&self, file: &Path) -> Value {
        let code_ids: Vec<String> = crate::tui::registry::registry().code_ids().into_iter().map(String::from).collect();
        let refs: Vec<&str> = code_ids.iter().map(String::as_str).collect();
        match crate::tui::declarative::check::check_report(file, None, &refs) {
            Ok(report) => json!({"status": report.status, "results": report.results}),
            Err(e) => json!({"error": e}),
        }
    }

    /// The transient profile ids the wizard may still hold (none after a clean run).
    pub fn is_registered(&self, agent_type: &str) -> bool {
        crate::tui::is_registered(agent_type)
    }
}

impl Drop for WizardRig {
    fn drop(&mut self) {
        match self.saved_path.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        match self.saved_script.take() {
            Some(script) => std::env::set_var("PM_FAKE_TUI_SCRIPT", script),
            None => std::env::remove_var("PM_FAKE_TUI_SCRIPT"),
        }
    }
}
