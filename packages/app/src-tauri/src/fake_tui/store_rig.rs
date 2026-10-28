//! The Agent Store against a real ConPTY, for `tests/fake_tui_store.rs`: a test-only catalog entry for the
//! made-up agent `nimbus` (its program is `pm-fake-tui`), the Store's own `add` / `remove` on the e2e app-data
//! folder, and `pm-fake-tui` put on `PATH` through a folder of the test.
//!
//! The entry joins the catalog only here (`catalog::shipped` asks `store_catalog_files` in a `dev-fake-tui` build
//! that is not a unit-test build), so the shipped catalog is never changed. Nothing is installed (the entry has
//! no install spec), nothing leaves the machine, and the app-data folder is the process's own temp folder
//! (`World::start`), never the person's.

use std::path::PathBuf;

use serde_json::Value;

use super::world::Session;
use crate::agent_store::{self, catalog, record};
use crate::tui::declarative::{loader, service};

/// The id of the made-up agent.
pub const ID: &str = "nimbus";

const ENTRY: &str = r#"schema_version = 1

[store]
id = "nimbus"
tagline = "A made-up agent for the Store's end-to-end test."
tier = "starter"
builtin = false
categories = ["general"]
status = "active"
first_listed = "2026-10-28"

[vendor]
name = "Nimbus Test Vendor"
homepage = "https://example.com/nimbus"
license = "Apache-2.0"
license_kind = "open_source"

[sign_in]
kind = "none"
summary = "No sign-in needed."

[checked]
date = "2026-10-28"
version = "0.4.2"
how = "vendor_docs"
windows_tested = false
"#;

/// `tests/agent-definitions/nimbus.toml` with the program `pm-fake-tui` and the transcript echo the Chat tab reads.
const DEFINITION: &str = r##"schema_version = 1

[agent]
id = "nimbus"
label = "Nimbus"
description = "Nimbus: a made-up agent for the Store's end-to-end test."
experimental = true

[binary]
names = ["pm-fake-tui"]
install_hint = "install nimbus"

[tui]
argv = []
idle_after_ms = 1500
quiet_idle_after_ms = 800

[tui.ready]
regex = '\|\s*>\s*type a task'
not_regex = ['ctrl\+c to stop', 'allow nimbus', 'do you trust']

[tui.busy]
working_regex = ['ctrl\+c to stop']
idle_regex = ['\|\s*>\s*type a task']

[tui.chrome]
line_regex = [
  '^nimbus\s+[\d.]+',
  '^[+|\-\s]+$',
  '^\|\s*>\s*type a task',
  '^/help for commands',
  '^model:\s',
  '^done in \d+s$',
  '^\W?\s*generating\.\.\.',
]

[tui.tool_activity]
line_regex = ['^tool:\s', '^->\s']

[tui.thinking]
line_regex = ['^[~*]\s*(thinking|generating)', '^thought for \d+s']

[tui.delivery]
input_echo_regex = '^\|\s*>\s*\S'
transcript_echo_regex = '^>\s+\S'

[[tui.prompt]]
kind = "command_approval"
detect_regex = 'allow nimbus to .*\[y/n\]'
choices = [
  { id = "allow_once", key = "y" },
  { id = "deny", key = "n" },
]
"##;

/// The catalog folder of the test entry, as the generator lists a folder. Read by `catalog::shipped`.
pub(crate) fn catalog_files() -> Vec<(String, String)> {
    vec![
        ("agents/nimbus/entry.toml".into(), ENTRY.into()),
        ("agents/nimbus/definition.toml".into(), DEFINITION.into()),
        ("agents/nimbus/fixtures/idle.txt".into(), "NIMBUS 0.4.2\n| > Type a task...\n".into()),
    ]
}

/// The Store over the test app: `pm-fake-tui` on or off `PATH`, `add`, `remove`, the cards and the listing.
/// Starts and ends with nothing of `nimbus` in the e2e app-data folder; restores `PATH` and the script on drop.
pub struct StoreRig {
    bin: PathBuf,
    saved_path: Option<std::ffi::OsString>,
    saved_script: Option<std::ffi::OsString>,
}

impl Session {
    /// A Store rig for this test. `script` is what `pm-fake-tui` plays when a pane of the agent starts.
    /// The program is NOT on `PATH` yet ([`StoreRig::program_on_path`]).
    pub fn store_rig(&self, script: &str) -> StoreRig {
        self.script(script);
        let bin = self.aux().join("bin");
        std::fs::create_dir_all(&bin).expect("bin folder");
        // The copy exists but its folder is not on PATH: a test chooses when the agent's program "is installed".
        std::fs::copy(self.fake_exe(), bin.join("pm-fake-tui.exe")).expect("copy the fake agent");
        let saved_script = std::env::var_os("PM_FAKE_TUI_SCRIPT");
        std::env::set_var("PM_FAKE_TUI_SCRIPT", self.script_path());
        let rig = StoreRig { bin, saved_path: std::env::var_os("PATH"), saved_script };
        rig.wipe();
        rig
    }
}

impl StoreRig {
    /// `pm-fake-tui` is installed: its folder is first on `PATH`.
    pub fn program_on_path(&self) {
        let mut dirs = vec![self.bin.clone()];
        dirs.extend(std::env::split_paths(&self.saved_path.clone().unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(dirs).expect("a PATH"));
    }

    /// The program is not installed: `PATH` as it was.
    pub fn program_off_path(&self) {
        match &self.saved_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }

    /// `agent_store::add("nimbus", confirmed)`: the app's own wrapper over the e2e app-data folder.
    pub fn add(&self, confirmed: bool) -> Result<Value, String> {
        agent_store::add(ID, confirmed)
    }

    /// `agent_store::remove("nimbus")`.
    pub fn remove(&self) -> Result<Value, String> {
        agent_store::remove(ID)
    }

    /// The Store's card for `nimbus`, as the grid reads it (the version probe is not run).
    pub fn card(&self) -> Value {
        let record_path = record::default_path();
        let env = agent_store::Env { record_path: &record_path, version: &|_, _| None };
        let listed = agent_store::list_in(catalog::all(), &catalog::catalog_version(), &env).expect("the Store lists");
        listed["entries"].as_array().and_then(|cards| cards.iter().find(|card| card["id"] == ID)).cloned().unwrap_or(Value::Null)
    }

    /// The agent definitions listing's row for `nimbus` (what Settings > Agents shows), or `Null`.
    pub fn definition_row(&self) -> Value {
        let listing = service::listing_global();
        listing["definitions"].as_array().and_then(|rows| rows.iter().find(|row| row["id"] == ID)).cloned().unwrap_or(Value::Null)
    }

    /// `<agents folder>/nimbus.toml`.
    pub fn agent_file(&self) -> PathBuf {
        crate::app_paths::agents_dir().join("nimbus.toml")
    }

    /// `<agents folder>/nimbus.fixtures`.
    pub fn fixtures_dir(&self) -> PathBuf {
        crate::app_paths::agents_dir().join("nimbus.fixtures")
    }

    /// What `.removed/` holds (Settings > Agents > Removed).
    pub fn removed(&self) -> Value {
        service::removed_global()
    }

    /// Undo a removal (the same function Settings > Agents calls).
    pub fn restore(&self, removed_id: &str) -> Result<Value, String> {
        service::restore_global(removed_id).map_err(|e| format!("{}: {}", e.code, e.message))
    }

    /// The registry as the app reads the agents folder now (the file watcher's job in the app).
    pub fn reload(&self) {
        loader::reload();
    }

    /// Takes everything of `nimbus` out of the e2e app-data folder and reloads, so a test starts and ends clean.
    fn wipe(&self) {
        let dir = crate::app_paths::agents_dir();
        let _ = std::fs::remove_file(dir.join("nimbus.toml"));
        let _ = std::fs::remove_dir_all(dir.join("nimbus.fixtures"));
        let _ = std::fs::remove_dir_all(dir.join(".removed"));
        let _ = std::fs::remove_file(record::default_path());
        let _ = std::fs::remove_file(crate::tui::declarative::approvals::default_path());
        loader::reload();
    }
}

impl Drop for StoreRig {
    fn drop(&mut self) {
        self.program_off_path();
        match &self.saved_script {
            Some(script) => std::env::set_var("PM_FAKE_TUI_SCRIPT", script),
            None => std::env::remove_var("PM_FAKE_TUI_SCRIPT"),
        }
        self.wipe();
    }
}
