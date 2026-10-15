//! OpenCode target: the `mcp` entry in project and global `opencode.json`.

use super::*;

pub(super) fn opencode_mcp_entry(source: McpLaunchSource) -> Value {
    let launch = launch_spec(source);
    let mut command = vec![launch.command];
    command.extend(launch.args);
    json!({
        "type": "local",
        "command": command,
        "enabled": true,
        "environment": {
            crate::app_paths::BRIDGE_PORT_FILE_ENV: crate::app_paths::bridge_port_file_env_value(),
        },
    })
}

pub(super) fn opencode_entry_uses_npm_package(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_array)
        .is_some_and(|command| {
            command
                .iter()
                .filter_map(Value::as_str)
                .eq(["npx", "-y", "@puppet-master/mcp"])
        })
}

pub(super) fn merge_opencode_mcp(existing: &mut Value, source: McpLaunchSource) {
    if !existing.is_object() {
        *existing = json!({});
    }
    let root = existing.as_object_mut().expect("object");
    root.entry("$schema")
        .or_insert_with(|| json!("https://opencode.ai/config.json"));
    let mcp = root.entry("mcp").or_insert_with(|| json!({}));
    if !mcp.is_object() {
        *mcp = json!({});
    }
    mcp.as_object_mut()
        .expect("mcp object")
        .insert(MCP_SERVER_NAME.into(), opencode_mcp_entry(source));
}

pub(super) fn opencode_mcp_needs_refresh_path(path: &Path, source: McpLaunchSource) -> bool {
    let Some(doc) = read_json_safe(path) else {
        return true;
    };
    let Some(entry) = doc.pointer(&format!("/mcp/{MCP_SERVER_NAME}")) else {
        return true;
    };
    if source == McpLaunchSource::NpmPackage && !opencode_entry_uses_npm_package(entry) {
        return true;
    }
    if source == McpLaunchSource::NpmPackage {
        return entry
            .pointer("/environment/PUPPET_MASTER_BRIDGE_PORT_FILE")
            .is_none();
    }
    if entry
        .get("command")
        .and_then(Value::as_array)
        .is_some_and(|command| {
            command.iter().any(|value| {
                value.as_str().is_some_and(|arg| {
                    arg.contains("//?/")
                        || arg.contains(r"\\?\")
                        || (crate::mcp_runtime::using_bundled_mcp()
                            && arg.contains("@puppet-master/mcp"))
                })
            })
        })
    {
        return true;
    }
    entry
        .pointer("/environment/PUPPET_MASTER_BRIDGE_PORT_FILE")
        .is_none()
}

pub(super) fn opencode_mcp_installed_path(path: &Path, source: McpLaunchSource) -> bool {
    let Some(doc) = read_json_safe(path) else {
        return false;
    };
    if opencode_mcp_needs_refresh_path(path, source) {
        return false;
    }
    doc.pointer(&format!("/mcp/{MCP_SERVER_NAME}/enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || doc
            .pointer(&format!("/mcp/{MCP_SERVER_NAME}/command"))
            .and_then(Value::as_array)
            .is_some()
}

pub(super) fn ensure_opencode_mcp(
    cwd: &Path,
    source: McpLaunchSource,
) -> Result<EnsureMcpResult, String> {
    ensure_opencode_mcp_at(&cwd.join("opencode.json"), source, "opencode_cli")
}

pub(super) fn ensure_opencode_global_mcp_with_source(
    source: McpLaunchSource,
) -> Result<EnsureMcpResult, String> {
    ensure_opencode_mcp_at(
        &opencode_global_config_path()?,
        source,
        "opencode_cli_global",
    )
}

pub(super) fn opencode_global_config_path() -> Result<PathBuf, String> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(appdata)
            .join("opencode")
            .join("opencode.json"));
    }
    let home = crate::project_path::home_dir()
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    Ok(home.join(".config").join("opencode").join("opencode.json"))
}

pub(super) fn ensure_opencode_mcp_at(
    path: &Path,
    source: McpLaunchSource,
    backend: &str,
) -> Result<EnsureMcpResult, String> {
    let mut doc = read_json_safe(path).unwrap_or_else(|| json!({}));
    let needs_refresh = opencode_mcp_needs_refresh_path(path, source);
    let before = doc.clone();
    merge_opencode_mcp(&mut doc, source);
    let changed = if doc != before || needs_refresh {
        write_json_pretty(path, &doc)?;
        true
    } else {
        false
    };
    let installed = opencode_mcp_installed_path(path, source);
    Ok(EnsureMcpResult {
        installed,
        changed,
        backend: backend.into(),
        message: if installed {
            "OpenCode MCP configured for puppet-master".into()
        } else {
            "OpenCode MCP install incomplete".into()
        },
    })
}

pub(super) fn uninstall_opencode_mcp(cwd: &Path) -> Result<EnsureMcpResult, String> {
    uninstall_opencode_at(&cwd.join("opencode.json"), "opencode_cli")
}

pub(super) fn uninstall_opencode_global_mcp() -> Result<EnsureMcpResult, String> {
    uninstall_opencode_at(&opencode_global_config_path()?, "opencode_cli_global")
}

pub(super) fn uninstall_opencode_at(path: &Path, backend: &str) -> Result<EnsureMcpResult, String> {
    let Some(mut doc) = read_json_safe(path) else {
        return Ok(EnsureMcpResult {
            installed: false,
            changed: false,
            backend: backend.into(),
            message: format!(
                "puppet-master was not in {}",
                crate::app_paths::path_for_host_config(path)
            ),
        });
    };
    let had_entry = doc
        .get("mcp")
        .and_then(|mcp| mcp.get(MCP_SERVER_NAME))
        .is_some();
    if !had_entry {
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
    if let Some(mcp) = doc.get_mut("mcp").and_then(Value::as_object_mut) {
        mcp.remove(MCP_SERVER_NAME);
    }
    if doc
        .get("mcp")
        .and_then(Value::as_object)
        .is_some_and(|mcp| mcp.is_empty())
    {
        if let Some(root) = doc.as_object_mut() {
            root.remove("mcp");
        }
    }
    if doc.as_object().is_some_and(|root| root.is_empty()) {
        fs::remove_file(path)
            .map_err(|err| format!("could not remove {}: {err}", path.display()))?;
    } else {
        write_json_pretty(path, &doc)?;
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
