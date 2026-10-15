//! Codex target: the `[mcp_servers.puppet-master]` block in project and global `config.toml`.

use super::*;

pub(super) fn codex_mcp_block(source: McpLaunchSource) -> String {
    let launch = launch_spec(source);
    let port_file = crate::app_paths::bridge_port_file_env_value();
    format!(
        "\n[mcp_servers.{MCP_SERVER_NAME}]\ncommand = \"{}\"\nargs = {:?}\nenabled = true\n\n[mcp_servers.{MCP_SERVER_NAME}.env]\n{} = \"{}\"\n",
        launch.command,
        launch.args,
        crate::app_paths::BRIDGE_PORT_FILE_ENV,
        port_file,
    )
}

pub(super) fn codex_needs_port_env(content: &str) -> bool {
    content.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]"))
        && !content.contains(crate::app_paths::BRIDGE_PORT_FILE_ENV)
}

pub(super) fn codex_needs_refresh(content: &str, source: McpLaunchSource) -> bool {
    if !codex_has_puppet_master(content) {
        return true;
    }
    if codex_needs_port_env(content) {
        return true;
    }
    if source == McpLaunchSource::NpmPackage {
        return !content.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]"))
            || !content.contains("command = \"npx\"")
            || !content.contains("@puppet-master/mcp");
    }
    if content.contains("//?/") || content.contains(r"\\?\") {
        return true;
    }
    if crate::mcp_runtime::using_bundled_mcp() && content.contains("@puppet-master/mcp") {
        return true;
    }
    false
}

pub(super) fn strip_codex_puppet_master_block(content: &str) -> String {
    let mut out = Vec::new();
    let mut skip = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[mcp_servers.") && trimmed.contains(MCP_SERVER_NAME) {
            skip = true;
            continue;
        }
        if skip
            && trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && !trimmed.contains(MCP_SERVER_NAME)
        {
            skip = false;
        }
        if skip {
            continue;
        }
        out.push(line);
    }
    let mut text = out.join("\n");
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

pub(super) fn codex_has_puppet_master(content: &str) -> bool {
    content.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]"))
        || content.contains(&format!("[mcp_servers.\"{MCP_SERVER_NAME}\"]"))
}

pub(super) fn ensure_codex_mcp(
    cwd: &Path,
    source: McpLaunchSource,
) -> Result<EnsureMcpResult, String> {
    let dir = cwd.join(".codex");
    let path = dir.join("config.toml");
    ensure_codex_mcp_at(&path, source, "codex_cli")
}

pub(super) fn ensure_codex_global_mcp_with_source(
    source: McpLaunchSource,
) -> Result<EnsureMcpResult, String> {
    let home = crate::project_path::home_dir()
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    ensure_codex_mcp_at(
        &home.join(".codex").join("config.toml"),
        source,
        "codex_cli_global",
    )
}

pub(super) fn ensure_codex_mcp_at(
    path: &Path,
    source: McpLaunchSource,
    backend: &str,
) -> Result<EnsureMcpResult, String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    let changed = if codex_has_puppet_master(&existing) && !codex_needs_refresh(&existing, source) {
        false
    } else if codex_has_puppet_master(&existing) {
        let mut next = strip_codex_puppet_master_block(&existing);
        next.push_str(&codex_mcp_block(source));
        fs::write(path, next)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
        true
    } else {
        let mut next = existing;
        if !next.ends_with('\n') && !next.is_empty() {
            next.push('\n');
        }
        next.push_str(&codex_mcp_block(source));
        fs::write(path, next)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
        true
    };

    let installed = fs::read_to_string(path)
        .map(|content| codex_has_puppet_master(&content))
        .unwrap_or(false);

    Ok(EnsureMcpResult {
        installed,
        changed,
        backend: backend.into(),
        message: if installed {
            "Codex MCP configured for puppet-master".into()
        } else {
            "Codex MCP install incomplete".into()
        },
    })
}

pub(super) fn uninstall_codex_mcp(cwd: &Path) -> Result<EnsureMcpResult, String> {
    uninstall_codex_at(&cwd.join(".codex").join("config.toml"), "codex_cli")
}

pub(super) fn uninstall_codex_global_mcp() -> Result<EnsureMcpResult, String> {
    let home = crate::project_path::home_dir()
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    uninstall_codex_at(&home.join(".codex").join("config.toml"), "codex_cli_global")
}

pub(super) fn uninstall_codex_at(path: &Path, backend: &str) -> Result<EnsureMcpResult, String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if !codex_has_puppet_master(&existing) {
        return Ok(EnsureMcpResult {
            installed: false,
            changed: false,
            backend: backend.into(),
            message: format!(
                "puppet-master was not in {}",
                crate::app_paths::path_for_host_config(path)
            ),
        });
    }
    let next = strip_codex_puppet_master_block(&existing);
    if next.trim().is_empty() {
        if path.is_file() {
            fs::remove_file(path)
                .map_err(|err| format!("could not remove {}: {err}", path.display()))?;
        }
    } else {
        fs::write(path, next)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
    }
    Ok(EnsureMcpResult {
        installed: false,
        changed: true,
        backend: backend.into(),
        message: format!(
            "Removed puppet-master from {}",
            crate::app_paths::path_for_host_config(path)
        ),
    })
}
