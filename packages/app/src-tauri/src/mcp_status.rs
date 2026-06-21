//! Live MCP + bridge readiness probes for the settings UI.

use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use crate::mcp_install::{self, EnsureMcpResult};

const NPM_PACKAGE: &str = "@puppet-master/mcp";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpBackendStatus {
    pub backend: String,
    pub label: String,
    pub installed: bool,
    pub uses_npm: bool,
    pub config_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusReport {
    pub bridge_reachable: bool,
    pub bridge_url: Option<String>,
    pub bridge_version: Option<String>,
    pub port_file_exists: bool,
    pub port_file_path: String,
    pub node_available: bool,
    pub npm_available: bool,
    pub npm_package_version: Option<String>,
    pub launch_command: String,
    pub backends: Vec<McpBackendStatus>,
    pub overall_ready: bool,
    pub repair_results: Vec<EnsureMcpResult>,
}

pub fn get_mcp_status(cwd: &Path, auto_repair: bool) -> Result<McpStatusReport, String> {
    let cwd = crate::project_path::normalize_project_path(cwd)?;
    let port_file_path = crate::app_paths::bridge_port_file();
    let port_file_exists = port_file_path.is_file();
    let (bridge_reachable, bridge_url, bridge_version) = probe_bridge_health(&port_file_path);
    let (node_available, npm_available, npm_package_version) = probe_node_and_npm();
    let launch = crate::mcp_runtime::npm_mcp_launch_spec();
    let launch_command = format!(
        "{} {}",
        launch.command,
        launch.args.join(" ")
    );

    let mut repair_results = Vec::new();
    if auto_repair {
        repair_results = mcp_install::install_npm_mcp_configs(&cwd)?;
    }

    let backends = vec![
        inspect_claude_status(&cwd),
        inspect_codex_status(&cwd),
        inspect_opencode_status(&cwd),
    ];

    let orchestrators_ready = backends.iter().all(|backend| backend.installed);
    let overall_ready =
        bridge_reachable && npm_available && node_available && orchestrators_ready;

    Ok(McpStatusReport {
        bridge_reachable,
        bridge_url,
        bridge_version,
        port_file_exists,
        port_file_path: crate::app_paths::path_for_host_config(&port_file_path),
        node_available,
        npm_available,
        npm_package_version,
        launch_command,
        backends,
        overall_ready,
        repair_results,
    })
}

fn inspect_claude_status(cwd: &Path) -> McpBackendStatus {
    let config_path = cwd.join(".mcp.json");
    let installed = mcp_install::claude_mcp_installed_public(cwd);
    let uses_npm = mcp_install::claude_mcp_uses_npm_public(cwd);
    McpBackendStatus {
        backend: "claude_cli".into(),
        label: "Claude Code (project)".into(),
        installed,
        uses_npm,
        config_path: crate::app_paths::path_for_host_config(&config_path),
        message: if installed {
            if uses_npm {
                "Configured via npx @puppet-master/mcp".into()
            } else {
                "Configured with a local/bundled script — reinstall to switch to npm".into()
            }
        } else {
            "Missing .mcp.json approval or puppet-master entry".into()
        },
    }
}

fn inspect_codex_status(cwd: &Path) -> McpBackendStatus {
    let config_path = cwd.join(".codex").join("config.toml");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let installed = mcp_install::codex_has_puppet_master_public(&content)
        && !mcp_install::codex_needs_refresh_public(&content);
    let uses_npm = content.contains("command = \"npx\"")
        && content.contains(NPM_PACKAGE);
    McpBackendStatus {
        backend: "codex_cli".into(),
        label: "Codex CLI (project)".into(),
        installed,
        uses_npm,
        config_path: crate::app_paths::path_for_host_config(&config_path),
        message: if installed {
            if uses_npm {
                "Configured via npx @puppet-master/mcp".into()
            } else {
                "Configured with a local/bundled script — reinstall to switch to npm".into()
            }
        } else {
            "Missing or outdated .codex/config.toml entry".into()
        },
    }
}

fn inspect_opencode_status(cwd: &Path) -> McpBackendStatus {
    let config_path = cwd.join("opencode.json");
    let installed = mcp_install::opencode_mcp_installed_public(cwd);
    let uses_npm = mcp_install::opencode_mcp_uses_npm_public(cwd);
    McpBackendStatus {
        backend: "opencode_cli".into(),
        label: "OpenCode (project)".into(),
        installed,
        uses_npm,
        config_path: crate::app_paths::path_for_host_config(&config_path),
        message: if installed {
            if uses_npm {
                "Configured via npx @puppet-master/mcp".into()
            } else {
                "Configured with a local/bundled script — reinstall to switch to npm".into()
            }
        } else {
            "Missing or outdated opencode.json entry".into()
        },
    }
}

fn probe_bridge_health(port_file: &Path) -> (bool, Option<String>, Option<String>) {
    if !port_file.is_file() {
        return (false, None, None);
    }
    let raw = match fs::read_to_string(port_file) {
        Ok(text) => text,
        Err(_) => return (false, None, None),
    };
    let (host, port) = match parse_bridge_endpoint(raw.trim()) {
        Some(value) => value,
        None => return (false, None, None),
    };
    let url = format!("http://{host}:{port}");
    match http_get(&format!("{url}/health")) {
        Ok(body) => {
            let ok = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|json| json.get("ok").and_then(Value::as_bool))
                .unwrap_or(false);
            let version = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|json| {
                    json.get("version")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            (ok, Some(url), version)
        }
        Err(_) => (false, Some(url), None),
    }
}

fn parse_bridge_endpoint(raw: &str) -> Option<(String, u16)> {
    if raw.contains(':') {
        let (host, port_str) = raw.split_once(':')?;
        let port = port_str.parse().ok()?;
        return Some((host.to_string(), port));
    }
    raw.parse::<u16>().ok().map(|port| ("127.0.0.1".into(), port))
}

fn http_get(url: &str) -> Result<String, String> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported URL: {url}"))?;
    let (authority, path) = without_scheme
        .split_once('/')
        .map(|(host_port, rest)| (host_port, format!("/{rest}")))
        .unwrap_or((without_scheme, "/".into()));
    let addr = resolve_socket_addr(authority)?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .map_err(|err| format!("connect {authority}: {err}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1500)))
        .map_err(|err| err.to_string())?;
    let host = authority.split(':').next().unwrap_or(authority);
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&response);
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or(text.as_ref())
        .trim()
        .to_string();
    Ok(body)
}

fn resolve_socket_addr(authority: &str) -> Result<SocketAddr, String> {
    if let Ok(addr) = authority.parse::<SocketAddr>() {
        return Ok(addr);
    }
    if let Some((host, port_str)) = authority.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            let mut addrs = (host, port)
                .to_socket_addrs()
                .map_err(|err| format!("resolve {authority}: {err}"))?;
            if let Some(addr) = addrs.next() {
                return Ok(addr);
            }
        }
    }
    let mut addrs = (authority, 80u16)
        .to_socket_addrs()
        .map_err(|err| format!("resolve {authority}: {err}"))?;
    addrs
        .next()
        .ok_or_else(|| format!("no address for {authority}"))
}

fn probe_node_and_npm() -> (bool, bool, Option<String>) {
    let node_available = which_executable("node").is_some();
    let npm_available = which_executable("npm").is_some();
    let npm_package_version = if npm_available {
        npm_view_version()
    } else {
        None
    };
    (node_available, npm_available, npm_package_version)
}

fn which_executable(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "where", name])
            .env("Path", crate::shell_env::path_for_spawn())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        return String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string);
    }
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = std::process::Command::new(&shell)
            .args(["-l", "-c", &format!("command -v {name}")])
            .env("PATH", crate::shell_env::path_for_spawn())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = name;
        None
    }
}

fn npm_view_version() -> Option<String> {
    run_npm_view(&["view", NPM_PACKAGE, "version"])
        .or_else(|| run_npm_view(&["view", NPM_PACKAGE, "version", "--registry", "https://registry.npmjs.org"]))
}

fn run_npm_view(args: &[&str]) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let output = std::process::Command::new("cmd.exe")
            .arg("/C")
            .arg("npm")
            .args(args)
            .env("Path", crate::shell_env::path_for_spawn())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return if version.is_empty() { None } else { Some(version) };
    }
    #[cfg(not(windows))]
    {
        let output = std::process::Command::new("npm")
            .args(args)
            .env("PATH", crate::shell_env::path_for_spawn())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if version.is_empty() {
            None
        } else {
            Some(version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_port_endpoint() {
        assert_eq!(
            parse_bridge_endpoint("127.0.0.1:17321"),
            Some(("127.0.0.1".into(), 17321))
        );
        assert_eq!(parse_bridge_endpoint("17321"), Some(("127.0.0.1".into(), 17321)));
    }

    #[test]
    fn resolves_host_port_socket_addr() {
        let addr = resolve_socket_addr("127.0.0.1:17321").expect("addr");
        assert_eq!(addr.port(), 17321);
        assert!(addr.ip().is_loopback());
    }
}
