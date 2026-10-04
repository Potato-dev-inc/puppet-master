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
fn rust_preference_writes_auto_runtime_when_available() {
    let cwd = temp_dir("rust-preference");
    fs::create_dir_all(&cwd).expect("cwd");

    let results = install_mcp_configs_with_preference(&cwd, true).expect("install all");
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|result| result.installed));

    let codex = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("codex");
    assert!(codex.contains("[mcp_servers.puppet-master]"));
    assert!(codex.contains(crate::app_paths::BRIDGE_PORT_FILE_ENV));
}

#[test]
fn uninstall_removes_all_project_backends() {
    let cwd = temp_dir("uninstall");
    fs::create_dir_all(&cwd).expect("cwd");
    install_npm_mcp_configs(&cwd).expect("install");

    let results = uninstall_npm_mcp_configs(&cwd).expect("uninstall");
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|result| result.changed));
    assert!(results.iter().all(|result| !result.installed));
    assert!(!claude_mcp_installed(&cwd));
    let codex = fs::read_to_string(cwd.join(".codex").join("config.toml")).unwrap_or_default();
    assert!(!codex_has_puppet_master(&codex));
    assert!(!opencode_mcp_installed_path(
        &cwd.join("opencode.json"),
        McpLaunchSource::NpmPackage
    ));
}

#[test]
fn global_uninstall_removes_home_mcp_json() {
    let home = temp_dir("home-mcp");
    fs::create_dir_all(&home).expect("home");
    let mut doc = json!({ "mcpServers": {} });
    merge_mcp_servers(&mut doc, McpLaunchSource::NpmPackage);
    write_json_pretty(&home.join(".mcp.json"), &doc).expect("write");

    let result = uninstall_claude_mcp(&home).expect("uninstall home mcp");
    assert!(result.changed);
    assert!(!result.installed);
    assert!(!home.join(".mcp.json").is_file());
}

#[test]
fn claude_mcp_remove_not_found_matches_cli_errors() {
    assert!(claude_mcp_remove_not_found(
        "No user-scoped MCP server found with name: puppet-master"
    ));
    assert!(claude_mcp_remove_not_found(
        "No project-local MCP server found with name: puppet-master"
    ));
    assert!(!claude_mcp_remove_not_found(
        "could not run claude: No such file"
    ));
}

#[test]
fn all_cli_backends_reject_root_project_path() {
    for backend in ["claude_cli", "codex_cli", "opencode_cli"] {
        let err =
            ensure_orchestrator_mcp_with_preference(backend, Path::new("/"), false).unwrap_err();
        assert!(
            err.contains("Pick a project folder"),
            "backend {backend}: {err}"
        );
    }
}
