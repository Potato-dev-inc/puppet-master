//! Install / verify Puppet Master MCP registration for CLI orchestrator backends.

use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const MCP_SERVER_NAME: &str = "puppet-master";
const MCP_COMMAND: &str = "npx";
const MCP_ARGS: &[&str] = &["-y", "@puppet-master/mcp"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureMcpResult {
    pub installed: bool,
    pub changed: bool,
    pub backend: String,
    pub message: String,
}

pub fn ensure_orchestrator_mcp(backend: &str, cwd: &Path) -> Result<EnsureMcpResult, String> {
    if cwd.as_os_str().is_empty() {
        return Err("project path is required to install orchestrator MCP".into());
    }
    match backend {
        "claude_cli" => ensure_claude_mcp(cwd),
        "codex_cli" => ensure_codex_mcp(cwd),
        "opencode_cli" => ensure_opencode_mcp(cwd),
        other => Err(format!("unsupported orchestrator backend: {other}")),
    }
}

fn puppet_master_mcp_server_json() -> Value {
    json!({
        "command": MCP_COMMAND,
        "args": MCP_ARGS,
    })
}

fn read_json_safe(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_json_pretty(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!("could not create {}: {err}", parent.display())
        })?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())? + "\n";
    fs::write(path, text).map_err(|err| format!("could not write {}: {err}", path.display()))
}

fn merge_mcp_servers(existing: &mut Value) {
    if !existing.is_object() {
        *existing = json!({});
    }
    let root = existing.as_object_mut().expect("object");
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    servers
        .as_object_mut()
        .expect("mcpServers object")
        .insert(MCP_SERVER_NAME.into(), puppet_master_mcp_server_json());
}

fn claude_mcp_installed(cwd: &Path) -> bool {
    let Some(servers) = read_mcp_server_names(cwd) else {
        return false;
    };
    if !servers.iter().any(|name| name == MCP_SERVER_NAME) {
        return false;
    }
    mcp_settings_approves_any(cwd, &servers)
}

fn read_mcp_server_names(cwd: &Path) -> Option<Vec<String>> {
    let parsed = read_json_safe(&cwd.join(".mcp.json"))?;
    let servers = parsed.get("mcpServers")?.as_object()?;
    if servers.is_empty() {
        return None;
    }
    Some(servers.keys().cloned().collect())
}

fn mcp_settings_approves(settings: Option<&Value>, server_names: &[String]) -> bool {
    let Some(settings) = settings else {
        return false;
    };
    if settings
        .get("enableAllProjectMcpServers")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return true;
    }
    let Some(list) = settings
        .get("enabledMcpjsonServers")
        .and_then(Value::as_array)
    else {
        return false;
    };
    server_names.iter().all(|name| {
        list.iter()
            .any(|value| value.as_str().is_some_and(|enabled| enabled == name))
    })
}

fn mcp_settings_approves_any(cwd: &Path, server_names: &[String]) -> bool {
    let mut sources = vec![
        cwd.join(".claude").join("settings.json"),
        cwd.join(".claude").join("settings.local.json"),
    ];
    if let Some(home) = home_dir() {
        sources.insert(0, home.join(".claude").join("settings.json"));
    }
    sources
        .iter()
        .any(|path| mcp_settings_approves(read_json_safe(path).as_ref(), server_names))
}

fn enable_all_project_mcp(cwd: &Path) -> Result<bool, String> {
    let dir = cwd.join(".claude");
    let path = dir.join("settings.json");
    fs::create_dir_all(&dir).map_err(|err| {
        format!("could not create {}: {err}", dir.display())
    })?;
    let mut settings = read_json_safe(&path).unwrap_or_else(|| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    if settings
        .get("enableAllProjectMcpServers")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(false);
    }
    settings
        .as_object_mut()
        .expect("settings object")
        .insert("enableAllProjectMcpServers".into(), Value::Bool(true));
    write_json_pretty(&path, &settings)?;
    Ok(true)
}

fn ensure_claude_mcp(cwd: &Path) -> Result<EnsureMcpResult, String> {
    let mcp_path = cwd.join(".mcp.json");
    let mut changed = false;

    let mut doc = read_json_safe(&mcp_path).unwrap_or_else(|| json!({ "mcpServers": {} }));
    let before = doc.clone();
    merge_mcp_servers(&mut doc);
    if doc != before {
        write_json_pretty(&mcp_path, &doc)?;
        changed = true;
    }

    if enable_all_project_mcp(cwd)? {
        changed = true;
    }

    let installed = claude_mcp_installed(cwd);
    Ok(EnsureMcpResult {
        installed,
        changed,
        backend: "claude_cli".into(),
        message: if installed {
            "Claude Code MCP configured for puppet-master".into()
        } else {
            "Claude Code MCP install incomplete".into()
        },
    })
}

fn codex_mcp_block() -> String {
    format!(
        "\n[mcp_servers.{}]\ncommand = \"{}\"\nargs = {:?}\n",
        MCP_SERVER_NAME, MCP_COMMAND, MCP_ARGS
    )
}

fn codex_has_puppet_master(content: &str) -> bool {
    content.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]"))
        || content.contains(&format!("[mcp_servers.\"{MCP_SERVER_NAME}\"]"))
}

fn ensure_codex_mcp(cwd: &Path) -> Result<EnsureMcpResult, String> {
    let dir = cwd.join(".codex");
    let path = dir.join("config.toml");
    fs::create_dir_all(&dir).map_err(|err| {
        format!("could not create {}: {err}", dir.display())
    })?;

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let changed = if codex_has_puppet_master(&existing) {
        false
    } else {
        let mut next = existing;
        if !next.ends_with('\n') && !next.is_empty() {
            next.push('\n');
        }
        next.push_str(&codex_mcp_block());
        fs::write(&path, next).map_err(|err| format!("could not write {}: {err}", path.display()))?;
        true
    };

    let installed = fs::read_to_string(&path)
        .map(|content| codex_has_puppet_master(&content))
        .unwrap_or(false);

    Ok(EnsureMcpResult {
        installed,
        changed,
        backend: "codex_cli".into(),
        message: if installed {
            "Codex MCP configured for puppet-master".into()
        } else {
            "Codex MCP install incomplete".into()
        },
    })
}

fn merge_opencode_mcp(existing: &mut Value) {
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
        .insert(
            MCP_SERVER_NAME.into(),
            json!({
                "type": "local",
                "command": [MCP_COMMAND, MCP_ARGS[0], MCP_ARGS[1]],
                "enabled": true,
            }),
        );
}

fn opencode_mcp_installed(cwd: &Path) -> bool {
    let path = cwd.join("opencode.json");
    let Some(doc) = read_json_safe(&path) else {
        return false;
    };
    doc.pointer(&format!("/mcp/{MCP_SERVER_NAME}/enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || doc.pointer(&format!("/mcp/{MCP_SERVER_NAME}/command"))
            .and_then(Value::as_array)
            .is_some()
}

fn ensure_opencode_mcp(cwd: &Path) -> Result<EnsureMcpResult, String> {
    let path = cwd.join("opencode.json");
    let mut doc = read_json_safe(&path).unwrap_or_else(|| json!({}));
    let before = doc.clone();
    merge_opencode_mcp(&mut doc);
    let changed = if doc != before {
        write_json_pretty(&path, &doc)?;
        true
    } else {
        false
    };
    let installed = opencode_mcp_installed(cwd);
    Ok(EnsureMcpResult {
        installed,
        changed,
        backend: "opencode_cli".into(),
        message: if installed {
            "OpenCode MCP configured for puppet-master".into()
        } else {
            "OpenCode MCP install incomplete".into()
        },
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("pm-mcp-{name}-{nonce}"))
    }

    #[test]
    fn claude_install_writes_mcp_json_and_approves_project_servers() {
        let cwd = temp_dir("claude");
        fs::create_dir_all(&cwd).expect("cwd");

        let result = ensure_claude_mcp(&cwd).expect("install");
        assert!(result.installed);
        assert!(result.changed);
        assert!(cwd.join(".mcp.json").is_file());
        assert!(cwd.join(".claude").join("settings.json").is_file());

        let again = ensure_claude_mcp(&cwd).expect("reinstall");
        assert!(again.installed);
        assert!(!again.changed);
    }

    #[test]
    fn codex_install_appends_project_config_toml() {
        let cwd = temp_dir("codex");
        fs::create_dir_all(&cwd).expect("cwd");

        let result = ensure_codex_mcp(&cwd).expect("install");
        assert!(result.installed);
        assert!(result.changed);

        let content = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("toml");
        assert!(codex_has_puppet_master(&content));
    }

    #[test]
    fn opencode_install_writes_project_config() {
        let cwd = temp_dir("opencode");
        fs::create_dir_all(&cwd).expect("cwd");

        let result = ensure_opencode_mcp(&cwd).expect("install");
        assert!(result.installed);
        assert!(result.changed);
        assert!(opencode_mcp_installed(&cwd));
    }
}
