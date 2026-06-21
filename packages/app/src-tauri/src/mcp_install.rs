//! Install / verify Puppet Master MCP registration for CLI orchestrator backends.

use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const MCP_SERVER_NAME: &str = "puppet-master";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpLaunchSource {
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

pub fn ensure_orchestrator_mcp(backend: &str, cwd: &Path) -> Result<EnsureMcpResult, String> {
    ensure_orchestrator_mcp_with_source(backend, cwd, McpLaunchSource::Auto)
}

pub fn install_npm_mcp_configs(cwd: &Path) -> Result<Vec<EnsureMcpResult>, String> {
    let mut results = Vec::new();
    for backend in ["claude_cli", "codex_cli", "opencode_cli"] {
        results.push(ensure_orchestrator_mcp_with_source(
            backend,
            cwd,
            McpLaunchSource::NpmPackage,
        )?);
    }
    Ok(results)
}

pub fn install_global_npm_mcp_configs() -> Result<Vec<EnsureMcpResult>, String> {
    let mut results = Vec::new();
    results.push(ensure_claude_user_mcp());
    results.push(ensure_codex_global_mcp()?);
    results.push(ensure_opencode_global_mcp()?);
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

fn opencode_mcp_entry(source: McpLaunchSource) -> Value {
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

fn merge_mcp_servers(existing: &mut Value, source: McpLaunchSource) {
    if !existing.is_object() {
        *existing = json!({});
    }
    let root = existing.as_object_mut().expect("object");
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    servers.as_object_mut().expect("mcpServers object").insert(
        MCP_SERVER_NAME.into(),
        puppet_master_mcp_server_json(source),
    );
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
    if let Some(home) = crate::project_path::home_dir() {
        sources.insert(0, home.join(".claude").join("settings.json"));
    }
    sources
        .iter()
        .any(|path| mcp_settings_approves(read_json_safe(path).as_ref(), server_names))
}

fn enable_all_project_mcp(cwd: &Path) -> Result<bool, String> {
    let dir = cwd.join(".claude");
    let path = dir.join("settings.json");
    fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;
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

fn claude_mcp_needs_refresh(cwd: &Path, source: McpLaunchSource) -> bool {
    let Some(doc) = read_json_safe(&cwd.join(".mcp.json")) else {
        return true;
    };
    let Some(server) = doc.get("mcpServers").and_then(|v| v.get(MCP_SERVER_NAME)) else {
        return true;
    };
    if source == McpLaunchSource::NpmPackage && !json_server_uses_npm_package(server) {
        return true;
    }
    if server
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|args| {
            args.iter().any(|value| {
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
    server
        .get("env")
        .and_then(|env| env.get(crate::app_paths::BRIDGE_PORT_FILE_ENV))
        .is_none()
}

fn json_server_uses_npm_package(server: &Value) -> bool {
    server.get("command").and_then(Value::as_str) == Some("npx")
        && server
            .get("args")
            .and_then(Value::as_array)
            .is_some_and(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .eq(["-y", "@puppet-master/mcp"])
            })
}

fn opencode_entry_uses_npm_package(entry: &Value) -> bool {
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

fn ensure_claude_mcp(cwd: &Path, source: McpLaunchSource) -> Result<EnsureMcpResult, String> {
    let mcp_path = cwd.join(".mcp.json");
    let mut changed = false;

    let needs_refresh = claude_mcp_needs_refresh(cwd, source);
    let mut doc = read_json_safe(&mcp_path).unwrap_or_else(|| json!({ "mcpServers": {} }));
    let before = doc.clone();
    merge_mcp_servers(&mut doc, source);
    if doc != before || needs_refresh {
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

fn codex_mcp_block(source: McpLaunchSource) -> String {
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

fn codex_needs_port_env(content: &str) -> bool {
    content.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]"))
        && !content.contains(crate::app_paths::BRIDGE_PORT_FILE_ENV)
}

fn codex_needs_refresh(content: &str, source: McpLaunchSource) -> bool {
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
    if crate::mcp_runtime::using_bundled_mcp() && !content.contains("mcp-stdio.bundle.cjs") {
        return true;
    }
    false
}

fn strip_codex_puppet_master_block(content: &str) -> String {
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

fn codex_has_puppet_master(content: &str) -> bool {
    content.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]"))
        || content.contains(&format!("[mcp_servers.\"{MCP_SERVER_NAME}\"]"))
}

fn ensure_codex_mcp(cwd: &Path, source: McpLaunchSource) -> Result<EnsureMcpResult, String> {
    let dir = cwd.join(".codex");
    let path = dir.join("config.toml");
    ensure_codex_mcp_at(&path, source, "codex_cli")
}

fn ensure_codex_global_mcp() -> Result<EnsureMcpResult, String> {
    let home = crate::project_path::home_dir()
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    ensure_codex_mcp_at(
        &home.join(".codex").join("config.toml"),
        McpLaunchSource::NpmPackage,
        "codex_cli_global",
    )
}

fn ensure_codex_mcp_at(
    path: &Path,
    source: McpLaunchSource,
    backend: &str,
) -> Result<EnsureMcpResult, String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let changed = if codex_has_puppet_master(&existing) && !codex_needs_refresh(&existing, source) {
        false
    } else if codex_has_puppet_master(&existing) {
        let mut next = strip_codex_puppet_master_block(&existing);
        next.push_str(&codex_mcp_block(source));
        fs::write(&path, next)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
        true
    } else {
        let mut next = existing;
        if !next.ends_with('\n') && !next.is_empty() {
            next.push('\n');
        }
        next.push_str(&codex_mcp_block(source));
        fs::write(&path, next)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
        true
    };

    let installed = fs::read_to_string(&path)
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

fn merge_opencode_mcp(existing: &mut Value, source: McpLaunchSource) {
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

fn opencode_mcp_needs_refresh_path(path: &Path, source: McpLaunchSource) -> bool {
    let Some(doc) = read_json_safe(&path) else {
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

fn opencode_mcp_installed_path(path: &Path, source: McpLaunchSource) -> bool {
    let Some(doc) = read_json_safe(&path) else {
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

fn ensure_opencode_mcp(cwd: &Path, source: McpLaunchSource) -> Result<EnsureMcpResult, String> {
    ensure_opencode_mcp_at(&cwd.join("opencode.json"), source, "opencode_cli")
}

fn ensure_opencode_global_mcp() -> Result<EnsureMcpResult, String> {
    ensure_opencode_mcp_at(
        &opencode_global_config_path()?,
        McpLaunchSource::NpmPackage,
        "opencode_cli_global",
    )
}

fn opencode_global_config_path() -> Result<PathBuf, String> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(appdata)
            .join("opencode")
            .join("opencode.json"));
    }
    let home = crate::project_path::home_dir()
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    Ok(home.join(".config").join("opencode").join("opencode.json"))
}

fn ensure_opencode_mcp_at(
    path: &Path,
    source: McpLaunchSource,
    backend: &str,
) -> Result<EnsureMcpResult, String> {
    let mut doc = read_json_safe(&path).unwrap_or_else(|| json!({}));
    let needs_refresh = opencode_mcp_needs_refresh_path(path, source);
    let before = doc.clone();
    merge_opencode_mcp(&mut doc, source);
    let changed = if doc != before || needs_refresh {
        write_json_pretty(&path, &doc)?;
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

fn ensure_claude_user_mcp() -> EnsureMcpResult {
    let env_value = format!(
        "{}={}",
        crate::app_paths::BRIDGE_PORT_FILE_ENV,
        crate::app_paths::bridge_port_file_env_value()
    );
    let remove = run_claude_mcp_command(["mcp", "remove", "--scope", "user", MCP_SERVER_NAME]);
    let add = run_claude_mcp_command([
        "mcp",
        "add",
        "--scope",
        "user",
        MCP_SERVER_NAME,
        "-e",
        env_value.as_str(),
        "--",
        "npx",
        "-y",
        "@puppet-master/mcp",
    ]);

    match add {
        Ok(()) => EnsureMcpResult {
            installed: true,
            changed: true,
            backend: "claude_cli_global".into(),
            message: "Claude Code user MCP configured for puppet-master".into(),
        },
        Err(add_err) => EnsureMcpResult {
            installed: false,
            changed: remove.is_ok(),
            backend: "claude_cli_global".into(),
            message: format!("Claude Code global MCP install incomplete: {add_err}"),
        },
    }
}

fn run_claude_mcp_command<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let output = std::process::Command::new("claude")
        .args(args)
        .env("PATH", crate::shell_env::path_for_spawn())
        .output()
        .map_err(|err| format!("could not run claude: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(if detail.is_empty() {
        format!("claude exited with status {}", output.status)
    } else {
        detail
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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

        let result = ensure_claude_mcp(&cwd, McpLaunchSource::Auto).expect("install");
        assert!(result.installed);
        assert!(result.changed);
        assert!(cwd.join(".mcp.json").is_file());
        assert!(cwd.join(".claude").join("settings.json").is_file());

        let again = ensure_claude_mcp(&cwd, McpLaunchSource::Auto).expect("reinstall");
        assert!(again.installed);
        assert!(!again.changed);
    }

    #[test]
    fn codex_install_appends_project_config_toml() {
        let cwd = temp_dir("codex");
        fs::create_dir_all(&cwd).expect("cwd");

        let result = ensure_codex_mcp(&cwd, McpLaunchSource::Auto).expect("install");
        assert!(result.installed);
        assert!(result.changed);

        let content = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("toml");
        assert!(codex_has_puppet_master(&content));
    }

    #[test]
    fn opencode_install_writes_project_config() {
        let cwd = temp_dir("opencode");
        fs::create_dir_all(&cwd).expect("cwd");

        let result = ensure_opencode_mcp(&cwd, McpLaunchSource::Auto).expect("install");
        assert!(result.installed);
        assert!(result.changed);
        assert!(opencode_mcp_installed_path(
            &cwd.join("opencode.json"),
            McpLaunchSource::Auto
        ));
    }

    #[test]
    fn npm_install_rewrites_all_backends_to_npx_package() {
        let cwd = temp_dir("npm");
        fs::create_dir_all(&cwd).expect("cwd");
        write_json_pretty(
            &cwd.join(".mcp.json"),
            &json!({
                "mcpServers": {
                    MCP_SERVER_NAME: {
                        "command": "/usr/local/bin/node",
                        "args": ["/tmp/mcp-stdio.bundle.cjs"]
                    }
                }
            }),
        )
        .expect("claude seed");
        fs::create_dir_all(cwd.join(".codex")).expect("codex dir");
        fs::write(
            cwd.join(".codex").join("config.toml"),
            format!(
                "\n[mcp_servers.{MCP_SERVER_NAME}]\ncommand = \"/usr/local/bin/node\"\nargs = [\"/tmp/mcp-stdio.bundle.cjs\"]\nenabled = true\n"
            ),
        )
        .expect("codex seed");
        write_json_pretty(
            &cwd.join("opencode.json"),
            &json!({
                "mcp": {
                    MCP_SERVER_NAME: {
                        "type": "local",
                        "command": ["/usr/local/bin/node", "/tmp/mcp-stdio.bundle.cjs"],
                        "enabled": true
                    }
                }
            }),
        )
        .expect("opencode seed");

        let results = install_npm_mcp_configs(&cwd).expect("install all");
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|result| result.installed));

        let claude = read_json_safe(&cwd.join(".mcp.json")).expect("claude");
        assert!(json_server_uses_npm_package(
            claude
                .pointer(&format!("/mcpServers/{MCP_SERVER_NAME}"))
                .expect("server")
        ));

        let codex = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("codex");
        assert!(codex.contains("command = \"npx\""));
        assert!(codex.contains("@puppet-master/mcp"));
        assert!(!codex.contains("mcp-stdio.bundle.cjs"));

        let opencode = read_json_safe(&cwd.join("opencode.json")).expect("opencode");
        assert!(opencode_entry_uses_npm_package(
            opencode
                .pointer(&format!("/mcp/{MCP_SERVER_NAME}"))
                .expect("entry")
        ));
    }

    #[test]
    fn all_cli_backends_reject_root_project_path() {
        for backend in ["claude_cli", "codex_cli", "opencode_cli"] {
            let err = ensure_orchestrator_mcp(backend, Path::new("/")).unwrap_err();
            assert!(
                err.contains("Pick a project folder"),
                "backend {backend}: {err}"
            );
        }
    }
}
