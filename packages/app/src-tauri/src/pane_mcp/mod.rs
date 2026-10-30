//! Puppet Master's helper tools, ready inside every agent pane with no setup.
//!
//! An agent that runs in a pane gets Puppet Master's own MCP server (`run_agent`, `wait_agents`, ...)
//! for that one launch, pointed at THIS app's bridge (the port file of this profile, the way the
//! shim reads it) and naming the pane it runs in, so a helper it starts nests under it. How an agent
//! takes a server without touching the person's config is that agent's own knowledge
//! (`TuiProfile::pane_mcp`, `tui/<agent>/` or the definition's `[pane_mcp]`); this module only
//! decides whether to, builds the server entry and writes the launch's files. It never names an agent.
//!
//! The person's own config files are never written. Settings > Agents has a switch
//! (`pane_mcp_enabled`, default on); off = every pane starts exactly as before.

use crate::tui::pane_mcp::{PaneMcpFile, PaneMcpLaunch, PaneMcpRequest, PaneMcpServer};
use crate::tui::TuiProfile;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// The name the agent shows the tools under (the same as the one registered by hand).
pub const SERVER_NAME: &str = crate::mcp_install::MCP_SERVER_NAME;

/// One or two sentences for the agent, for the agents that can take one without replacing the
/// person's own instructions.
pub const NOTE: &str = "Puppet Master helpers: when you are asked to use or spawn Puppet Master helpers, agents or workers (not your own built-in subagents), call the run_agent tool of the puppet-master MCP server to start one with a task, then wait_agents to collect the answers. Do not look for its command line or HTTP bridge.";

/// `*.mcp.json` files older than this are removed when a new launch writes its own.
const STALE_AFTER: Duration = Duration::from_secs(3 * 24 * 3600);

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Read `pane_mcp_enabled` from the public settings (default on). Called whenever they change.
pub fn apply_settings(public: &Value) {
    ENABLED.store(public.get("pane_mcp_enabled").and_then(Value::as_bool).unwrap_or(true), Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// `<app data>/pane-mcp`: this launch's config files (no secret: the bridge secret stays in its own
/// file beside the port file, which the shim reads).
pub fn files_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("pm-pane-mcp-unit-tests-{}", std::process::id()));
    }
    crate::app_paths::app_data_dir().join("pane-mcp")
}

/// The server entry for a pane, or `None` when this build has no MCP server to start (a dev run
/// that never built it): then no pane gets a dangling entry that fails at start.
pub fn server_for(pane_id: &str) -> Option<PaneMcpServer> {
    if !crate::mcp_runtime::using_bundled_mcp() {
        return None;
    }
    let launch = crate::mcp_runtime::mcp_launch_spec();
    Some(server_with(launch.command, launch.args, pane_id, crate::app_paths::bridge_port_file_env_value()))
}

fn server_with(command: String, args: Vec<String>, pane_id: &str, port_file: String) -> PaneMcpServer {
    PaneMcpServer {
        name: SERVER_NAME.to_string(),
        command,
        args,
        env: vec![
            (crate::app_paths::BRIDGE_PORT_FILE_ENV.to_string(), port_file),
            (crate::pty::PANE_ID_ENV.to_string(), pane_id.to_string()),
        ],
    }
}

/// What `pane_id`'s launch adds so its agent has the helper tools. Empty when the switch is off, the
/// agent has no way to take a server for one launch, or anything is missing; never fails a spawn.
pub fn prepare(profile: &dyn TuiProfile, pane_id: &str, user_args: &[String]) -> PaneMcpLaunch {
    if !enabled() {
        return PaneMcpLaunch::default();
    }
    let Some(server) = server_for(pane_id) else { return PaneMcpLaunch::default() };
    prepare_in(profile, pane_id, user_args, &server, &files_dir())
}

/// [`prepare`] with every input explicit (tests).
pub fn prepare_in(
    profile: &dyn TuiProfile,
    pane_id: &str,
    user_args: &[String],
    server: &PaneMcpServer,
    dir: &Path,
) -> PaneMcpLaunch {
    let request = PaneMcpRequest { pane_id, user_args, server, dir, note: NOTE };
    let launch = profile.pane_mcp(&request);
    if launch.is_empty() {
        return PaneMcpLaunch::default();
    }
    if let Err(err) = write_files(dir, &launch.files) {
        tracing::warn!(pane = %pane_id, %err, "pane MCP files not written; the pane starts without the helper tools");
        return PaneMcpLaunch::default();
    }
    launch
}

fn write_files(dir: &Path, files: &[PaneMcpFile]) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    prune(dir);
    for file in files {
        // A profile only names files inside the folder it was given.
        if file.path.parent() != Some(dir) {
            return Err(format!("{} is outside {}", file.path.display(), dir.display()));
        }
        if std::fs::read_to_string(&file.path).is_ok_and(|have| have == file.content) {
            continue;
        }
        std::fs::write(&file.path, &file.content).map_err(|e| format!("{}: {e}", file.path.display()))?;
    }
    Ok(())
}

/// Remove this folder's own stale per-pane files. Best effort.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_ours = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".mcp.json"));
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if is_ours && old {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests;
