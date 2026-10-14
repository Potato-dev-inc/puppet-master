//! The Hook Lane, v0 (D:\pm-night\DESIGN-HOOK-LANE.md; probes in docs/specs/hook-lane/PROBES.md).
//!
//! Agents that declare `[hooks]` (`TuiProfile::hook_scheme`) run PM's one fixed command,
//! `puppet-master-mcp hook <agent id>`, at their lifecycle points (session start, prompt submitted,
//! permission requested, file edited, turn stopped). The command forwards the vendor's JSON to
//! `POST /hooks/event` with a per-pane token and exits 0 with no output, whatever happens.
//!
//! v0 is **shadow only**: events are projected into a per-pane ledger and recorded next to the
//! screen's verdicts (`shadow`). Nothing reads them back to decide anything. `evidence` mode is a
//! setting value that exists, but in v0 it behaves exactly like `shadow`.
//!
//! What this module can never do (§7.3): type, press a key, answer or approve a prompt, take over a
//! pane, start or stop a run, unlock anything, or mark anything verified. It depends on none of the
//! modules that can (`architecture_test` in `tests.rs` greps for them).
//!
//! Generic throughout: every per-agent fact comes from the `[hooks]` table.

pub mod health;
pub mod ingest;
pub mod install;
pub mod ledger;
pub mod model;
pub mod render;
pub mod route;
pub mod shadow;
pub mod token;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::Value;

use crate::tui::declarative::hooks::HooksTable;

/// The setting `agent_hooks_mode` (global) and `agent_hooks_agents` (per-agent override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// No hooks installed, no token minted: exactly the behaviour before the Hook Lane.
    Off,
    /// Hooks installed and recorded beside the screen; never used in a decision.
    Shadow,
    /// v1: hooks used as evidence. In v0 it is accepted and behaves exactly like `Shadow`.
    Evidence,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "shadow" => Some(Self::Shadow),
            "evidence" => Some(Self::Evidence),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::Evidence => "evidence",
        }
    }
}

/// Founder decision (2026-10-14 15:40): shadow is the default.
pub const DEFAULT_MODE: Mode = Mode::Shadow;

#[derive(Debug, Clone)]
struct Settings {
    mode: Mode,
    agents: BTreeMap<String, Mode>,
}

static SETTINGS: Lazy<RwLock<Settings>> = Lazy::new(|| RwLock::new(Settings { mode: DEFAULT_MODE, agents: BTreeMap::new() }));
static BRIDGE_PORT: AtomicU16 = AtomicU16::new(0);
static REJECTIONS: Lazy<Mutex<BTreeMap<&'static str, u64>>> = Lazy::new(|| Mutex::new(BTreeMap::new()));
static SCHEMES: Lazy<Mutex<HashMap<String, Option<Arc<HooksTable>>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// What the newest spawn of each agent installed, and where (for the doctor row).
static AGENT_INSTALLS: Lazy<Mutex<HashMap<String, (InstallState, String)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Read `agent_hooks_mode` / `agent_hooks_agents` from the public settings. Called whenever the
/// settings mirror changes. Turning an agent off revokes its tokens and removes PM's project entries.
pub fn apply_settings(public: &Value) {
    let mode = public.get("agent_hooks_mode").and_then(Value::as_str).and_then(Mode::parse).unwrap_or(DEFAULT_MODE);
    let agents: BTreeMap<String, Mode> = public
        .get("agent_hooks_agents")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), Mode::parse(v.as_str()?)?))).collect())
        .unwrap_or_default();
    let was_any_on = {
        let s = SETTINGS.read();
        s.mode != Mode::Off || s.agents.values().any(|m| *m != Mode::Off)
    };
    *SETTINGS.write() = Settings { mode, agents };
    SCHEMES.lock().clear();
    let all_off = mode == Mode::Off && SETTINGS.read().agents.values().all(|m| *m == Mode::Off);
    if was_any_on && all_off {
        token::revoke_all();
        let dir = hooks_dir();
        let _ = std::thread::Builder::new().name("pm-hooks-uninstall".into()).spawn(move || {
            let removed = install::project::uninstall_all(&dir, |agent| {
                scheme_for(agent).and_then(|s| s.project_file.as_ref().map(|f| f.root_key.clone()))
            });
            tracing::info!(removed, "hooks turned off: removed Puppet Master's project hook entries");
        });
    }
}

/// The mode the user chose for `agent` (per-agent override, else the global one).
pub fn requested_mode(agent: &str) -> Mode {
    let s = SETTINGS.read();
    s.agents.get(agent).copied().unwrap_or(s.mode)
}

/// The mode in force for `agent`. v0: `evidence` runs as `shadow`.
pub fn effective_mode(agent: &str) -> Mode {
    match requested_mode(agent) {
        Mode::Evidence => Mode::Shadow,
        other => other,
    }
}

/// Called once the bridge is listening (`bridge::start_embedded_bridge`).
pub fn set_bridge_port(port: u16) {
    BRIDGE_PORT.store(port, Ordering::Relaxed);
}

fn bridge_addr() -> Option<String> {
    match BRIDGE_PORT.load(Ordering::Relaxed) {
        0 => None,
        port => Some(format!("127.0.0.1:{port}")),
    }
}

/// `<app data>/hooks` (override: `PUPPET_MASTER_HOOKS_DIR`, for dev runs that must not touch the
/// installed app's folder).
pub fn hooks_dir() -> PathBuf {
    if cfg!(test) {
        // Unit tests never touch the installed app's folder.
        return std::env::temp_dir().join(format!("pm-hooks-unit-tests-{}", std::process::id()));
    }
    std::env::var_os("PUPPET_MASTER_HOOKS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::app_paths::app_data_dir().join("hooks"))
}

/// The agent's `[hooks]` table, parsed once per settings change.
pub fn scheme_for(agent: &str) -> Option<Arc<HooksTable>> {
    let mut cache = SCHEMES.lock();
    cache
        .entry(agent.to_string())
        .or_insert_with(|| crate::tui::profile_for(agent).hook_scheme().map(Arc::new))
        .clone()
}

fn count_rejection(kind: &'static str) {
    *REJECTIONS.lock().entry(kind).or_default() += 1;
}

pub fn rejection_counts() -> BTreeMap<&'static str, u64> {
    REJECTIONS.lock().clone()
}

/// What a spawn did about hooks, for the doctor row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    /// The agent has no `[hooks]` table.
    NotSupported,
    Off,
    /// No bridge port or no MCP exe to run as the hook command.
    Unavailable,
    /// The user passes the launch flag themselves; PM never replaces their settings.
    UserFlag,
    /// Installed (launch flag, or the project file written/unchanged).
    Installed,
    /// The project's hook file is tracked by git: PM writes nothing.
    BlockedTracked,
    /// The project's hook file could not be read or written.
    BlockedFile,
}

/// What `prepare_spawn` adds to a pane's launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookLaunch {
    pub env: Vec<(String, String)>,
    pub extra_args: Vec<String>,
}

/// Mint the pane's token and install PM's hooks for this spawn (§4.1, §5). Returns nothing to add
/// when the agent has no hooks, hooks are off, or anything is missing: the pane then starts exactly
/// as it did before the Hook Lane. Never fails a spawn.
pub fn prepare_spawn(agent: &str, pane_id: &str, cwd: &str, user_args: &[String]) -> HookLaunch {
    let (state, launch) = prepare_spawn_in(agent, pane_id, cwd, user_args, &hooks_dir(), crate::mcp_runtime::bundled_mcp_binary(), bridge_addr());
    if state != InstallState::NotSupported {
        AGENT_INSTALLS.lock().insert(agent.to_string(), (state, cwd.to_string()));
    }
    launch
}

/// [`prepare_spawn`] with every input explicit (tests, the live harness).
pub fn prepare_spawn_in(
    agent: &str,
    pane_id: &str,
    cwd: &str,
    user_args: &[String],
    dir: &Path,
    source_exe: Option<&Path>,
    bridge: Option<String>,
) -> (InstallState, HookLaunch) {
    let Some(scheme) = scheme_for(agent) else { return (InstallState::NotSupported, HookLaunch::default()) };
    if effective_mode(agent) == Mode::Off {
        token::revoke(pane_id);
        return (InstallState::Off, HookLaunch::default());
    }
    let (Some(bridge), Some(source)) = (bridge, source_exe) else {
        return (InstallState::Unavailable, HookLaunch::default());
    };
    let Some(exe) = install::stable_exe::ensure(source, dir) else {
        return (InstallState::Unavailable, HookLaunch::default());
    };
    let mut extra_args = Vec::new();
    let state = match scheme.install.as_str() {
        "launch_flag" => match install::launch::args_for(&scheme, &exe, agent, dir, user_args) {
            Some(args) => {
                extra_args = args;
                InstallState::Installed
            }
            None if scheme.launch.as_ref().is_some_and(|l| user_args.iter().any(|a| a == &l.flag)) => InstallState::UserFlag,
            None => InstallState::Unavailable,
        },
        "project_file" => match install::project::ensure(&scheme, &exe, agent, Path::new(cwd), dir) {
            install::project::Outcome::Written { .. } | install::project::Outcome::Unchanged => InstallState::Installed,
            install::project::Outcome::BlockedTracked => InstallState::BlockedTracked,
            _ => InstallState::BlockedFile,
        },
        _ => InstallState::NotSupported,
    };
    if state != InstallState::Installed {
        token::revoke(pane_id);
        return (state, HookLaunch::default());
    }
    let token = token::mint(pane_id, agent, cwd);
    let env = vec![
        ("PM_HOOK_TOKEN".to_string(), token),
        ("PM_HOOK_PANE".to_string(), pane_id.to_string()),
        ("PM_HOOK_BRIDGE".to_string(), bridge),
    ];
    (state, HookLaunch { env, extra_args })
}

/// The pane exited or was replaced: its token stops working at once.
pub fn revoke(pane_id: &str) {
    token::revoke(pane_id);
    shadow::forget(pane_id);
    ledger::forget(pane_id);
}

/// What the newest spawn of `agent` installed, and in which folder.
pub fn last_install(agent: &str) -> Option<(InstallState, String)> {
    AGENT_INSTALLS.lock().get(agent).cloned()
}
