//! Claude Code target: project `.mcp.json`, approvals in settings, and user/global scope via the `claude mcp` CLI.

use super::*;

pub(super) fn merge_mcp_servers(existing: &mut Value, source: McpLaunchSource) {
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

pub(super) fn claude_mcp_installed(cwd: &Path) -> bool {
    let Some(servers) = read_mcp_server_names(cwd) else {
        return false;
    };
    if !servers.iter().any(|name| name == MCP_SERVER_NAME) {
        return false;
    }
    mcp_settings_approves_any(cwd, &servers)
}

pub(super) fn read_mcp_server_names(cwd: &Path) -> Option<Vec<String>> {
    let parsed = read_json_safe(&cwd.join(".mcp.json"))?;
    let servers = parsed.get("mcpServers")?.as_object()?;
    if servers.is_empty() {
        return None;
    }
    Some(servers.keys().cloned().collect())
}

pub(super) fn mcp_settings_approves(settings: Option<&Value>, server_names: &[String]) -> bool {
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

pub(super) fn mcp_settings_approves_any(cwd: &Path, server_names: &[String]) -> bool {
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

pub(super) fn enable_all_project_mcp(cwd: &Path) -> Result<bool, String> {
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

pub(super) fn claude_mcp_needs_refresh(cwd: &Path, source: McpLaunchSource) -> bool {
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

pub(super) fn json_server_uses_npm_package(server: &Value) -> bool {
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

pub(super) fn ensure_claude_mcp(
    cwd: &Path,
    source: McpLaunchSource,
) -> Result<EnsureMcpResult, String> {
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

pub(super) fn ensure_claude_user_mcp_with_source(source: McpLaunchSource) -> EnsureMcpResult {
    let env_value = format!(
        "{}={}",
        crate::app_paths::BRIDGE_PORT_FILE_ENV,
        crate::app_paths::bridge_port_file_env_value()
    );
    let launch = launch_spec(source);
    let mut add_args = vec![
        "mcp".to_string(),
        "add".to_string(),
        "--scope".to_string(),
        "user".to_string(),
        MCP_SERVER_NAME.to_string(),
        "-e".to_string(),
        env_value,
        "--".to_string(),
        launch.command,
    ];
    add_args.extend(launch.args);
    let remove = run_claude_mcp_command(["mcp", "remove", "--scope", "user", MCP_SERVER_NAME]);
    let add = run_claude_mcp_command_dynamic(&add_args);

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

pub(super) fn run_claude_mcp_command<const N: usize>(args: [&str; N]) -> Result<(), String> {
    run_claude_mcp_command_dynamic(&args)
}

pub(super) fn run_claude_mcp_command_dynamic<S: AsRef<str>>(args: &[S]) -> Result<(), String> {
    let output = std::process::Command::new("claude")
        .args(args.iter().map(AsRef::as_ref))
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

pub(super) fn uninstall_claude_mcp(cwd: &Path) -> Result<EnsureMcpResult, String> {
    let mcp_path = cwd.join(".mcp.json");
    let Some(mut doc) = read_json_safe(&mcp_path) else {
        return Ok(EnsureMcpResult {
            installed: false,
            changed: false,
            backend: "claude_cli".into(),
            message: "No .mcp.json — puppet-master was not registered".into(),
        });
    };
    let had_entry = doc
        .get("mcpServers")
        .and_then(|servers| servers.get(MCP_SERVER_NAME))
        .is_some();
    if !had_entry {
        return Ok(EnsureMcpResult {
            installed: false,
            changed: false,
            backend: "claude_cli".into(),
            message: "puppet-master was not in project .mcp.json".into(),
        });
    }
    if let Some(servers) = doc.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.remove(MCP_SERVER_NAME);
    }
    if doc
        .get("mcpServers")
        .and_then(Value::as_object)
        .is_some_and(|servers| servers.is_empty())
    {
        if let Some(root) = doc.as_object_mut() {
            root.remove("mcpServers");
        }
    }
    if doc.as_object().is_some_and(|root| root.is_empty()) {
        fs::remove_file(&mcp_path)
            .map_err(|err| format!("could not remove {}: {err}", mcp_path.display()))?;
    } else {
        write_json_pretty(&mcp_path, &doc)?;
    }
    Ok(EnsureMcpResult {
        installed: false,
        changed: true,
        backend: "claude_cli".into(),
        message: "Removed puppet-master from project .mcp.json".into(),
    })
}

pub(super) fn uninstall_claude_global_mcp_configs() -> Vec<EnsureMcpResult> {
    let mut results = vec![
        uninstall_claude_mcp_scope("local"),
        uninstall_claude_mcp_scope("user"),
    ];
    if let Some(home) = crate::project_path::home_dir() {
        match uninstall_claude_mcp(&home) {
            Ok(result) => results.push(EnsureMcpResult {
                backend: "claude_cli_global_home".into(),
                ..result
            }),
            Err(err) => results.push(EnsureMcpResult {
                installed: true,
                changed: false,
                backend: "claude_cli_global_home".into(),
                message: format!("Could not remove ~/.mcp.json: {err}"),
            }),
        }
    }
    results
}

pub(super) fn uninstall_claude_mcp_scope(scope: &str) -> EnsureMcpResult {
    let backend = format!("claude_cli_global_{scope}");
    let args = ["mcp", "remove", "--scope", scope, MCP_SERVER_NAME];
    match run_claude_mcp_command(args) {
        Ok(()) => EnsureMcpResult {
            installed: false,
            changed: true,
            backend,
            message: format!("Removed puppet-master from Claude Code {scope} MCP"),
        },
        Err(err) if claude_mcp_remove_not_found(&err) => EnsureMcpResult {
            installed: false,
            changed: false,
            backend,
            message: format!("puppet-master was not registered in Claude Code {scope} MCP"),
        },
        Err(err) => EnsureMcpResult {
            installed: true,
            changed: false,
            backend,
            message: format!("Could not remove Claude Code {scope} MCP: {err}"),
        },
    }
}

pub(super) fn claude_mcp_remove_not_found(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("not found")
        || lower.contains("does not exist")
        || lower.contains("no user-scoped mcp server found")
        || lower.contains("no project-local mcp server found")
}
