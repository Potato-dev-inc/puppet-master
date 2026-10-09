//! Install / verify Puppet Master MCP registration for CLI orchestrator backends.

use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

mod claude;
mod codex;
pub mod declarative;
mod opencode;

use claude::*;
use codex::*;
use opencode::*;

pub const MCP_SERVER_NAME: &str = "puppet-master";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpLaunchSource {
    Auto,
    NpmPackage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureMcpResult {
    pub installed: bool,
    pub changed: bool,
    pub backend: String,
    pub message: String,
}

pub fn launch_source_from_developer_setting(use_rust_mcp: bool) -> McpLaunchSource {
    if use_rust_mcp {
        McpLaunchSource::Auto
    } else {
        McpLaunchSource::NpmPackage
    }
}

pub fn ensure_orchestrator_mcp_with_preference(
    backend: &str,
    cwd: &Path,
    use_rust_mcp: bool,
) -> Result<EnsureMcpResult, String> {
    ensure_orchestrator_mcp_with_source(
        backend,
        cwd,
        launch_source_from_developer_setting(use_rust_mcp),
    )
}

#[cfg(test)]
pub fn install_npm_mcp_configs(cwd: &Path) -> Result<Vec<EnsureMcpResult>, String> {
    install_mcp_configs_with_preference(cwd, false)
}

pub fn install_mcp_configs_with_preference(
    cwd: &Path,
    use_rust_mcp: bool,
) -> Result<Vec<EnsureMcpResult>, String> {
    let mut results = Vec::new();
    let source = launch_source_from_developer_setting(use_rust_mcp);
    for backend in ["claude_cli", "codex_cli", "opencode_cli"] {
        results.push(ensure_orchestrator_mcp_with_source(backend, cwd, source)?);
    }
    Ok(results)
}

pub fn install_global_mcp_configs_with_preference(
    use_rust_mcp: bool,
) -> Result<Vec<EnsureMcpResult>, String> {
    let mut results = Vec::new();
    let source = launch_source_from_developer_setting(use_rust_mcp);
    results.push(ensure_claude_user_mcp_with_source(source));
    results.push(ensure_codex_global_mcp_with_source(source)?);
    results.push(ensure_opencode_global_mcp_with_source(source)?);
    Ok(results)
}

pub fn uninstall_npm_mcp_configs(cwd: &Path) -> Result<Vec<EnsureMcpResult>, String> {
    let cwd = crate::project_path::normalize_project_path(cwd)?;
    Ok(vec![
        uninstall_claude_mcp(&cwd)?,
        uninstall_codex_mcp(&cwd)?,
        uninstall_opencode_mcp(&cwd)?,
    ])
}

pub fn uninstall_global_npm_mcp_configs() -> Result<Vec<EnsureMcpResult>, String> {
    let mut results = uninstall_claude_global_mcp_configs();
    results.push(uninstall_codex_global_mcp()?);
    results.push(uninstall_opencode_global_mcp()?);
    Ok(results)
}

fn ensure_orchestrator_mcp_with_source(
    backend: &str,
    cwd: &Path,
    source: McpLaunchSource,
) -> Result<EnsureMcpResult, String> {
    // Shared guard for Claude (.mcp.json), Codex (.codex), and OpenCode (opencode.json).
    let cwd = crate::project_path::normalize_project_path(cwd)?;
    match backend {
        "claude_cli" => ensure_claude_mcp(&cwd, source),
        "codex_cli" => ensure_codex_mcp(&cwd, source),
        "opencode_cli" => ensure_opencode_mcp(&cwd, source),
        other => Err(format!("unsupported orchestrator backend: {other}")),
    }
}

fn launch_spec(source: McpLaunchSource) -> crate::mcp_runtime::McpLaunchSpec {
    match source {
        McpLaunchSource::Auto => crate::mcp_runtime::mcp_launch_spec(),
        McpLaunchSource::NpmPackage => crate::mcp_runtime::npm_mcp_launch_spec(),
    }
}

fn puppet_master_mcp_server_json(source: McpLaunchSource) -> Value {
    let launch = launch_spec(source);
    json!({
        "command": launch.command,
        "args": launch.args,
        "env": {
            crate::app_paths::BRIDGE_PORT_FILE_ENV: crate::app_paths::bridge_port_file_env_value(),
        },
    })
}

fn read_json_safe(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_json_pretty(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())? + "\n";
    fs::write(path, text).map_err(|err| format!("could not write {}: {err}", path.display()))
}

pub fn claude_mcp_installed_public(cwd: &Path) -> bool {
    claude_mcp_installed(cwd)
}

pub fn claude_mcp_uses_npm_public(cwd: &Path) -> bool {
    let Some(doc) = read_json_safe(&cwd.join(".mcp.json")) else {
        return false;
    };
    doc.get("mcpServers")
        .and_then(|servers| servers.get(MCP_SERVER_NAME))
        .is_some_and(json_server_uses_npm_package)
}

pub fn codex_has_puppet_master_public(content: &str) -> bool {
    codex_has_puppet_master(content)
}

pub fn codex_needs_refresh_with_source_public(content: &str, source: McpLaunchSource) -> bool {
    codex_needs_refresh(content, source)
}

pub fn opencode_mcp_installed_with_source_public(cwd: &Path, source: McpLaunchSource) -> bool {
    opencode_mcp_installed_path(&cwd.join("opencode.json"), source)
}

pub fn opencode_mcp_uses_npm_public(cwd: &Path) -> bool {
    let path = cwd.join("opencode.json");
    let Some(doc) = read_json_safe(&path) else {
        return false;
    };
    doc.pointer(&format!("/mcp/{MCP_SERVER_NAME}"))
        .is_some_and(opencode_entry_uses_npm_package)
}

#[cfg(test)]
mod tests;
