//! Puppet Master's MCP server for one Codex launch: `-c mcp_servers.<name>.<key>=<toml>` overrides.
//!
//! Checked live on 0.162.1 (`codex -c ... mcp list --json`): the overrides add the server for this
//! run only, next to the servers in the person's own `config.toml`, which is never written. Codex
//! has no append-only way to add a note (`developer_instructions` would replace the person's own),
//! so no note is added; the server's own `instructions` describe the tools.

use crate::tui::pane_mcp::{PaneMcpLaunch, PaneMcpRequest};

/// A TOML basic string. JSON string escapes are valid TOML ones for every character we write.
fn toml_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// A bare TOML key, or a quoted one.
fn toml_key(key: &str) -> String {
    if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        key.to_string()
    } else {
        toml_string(key)
    }
}

pub(super) fn launch(request: &PaneMcpRequest<'_>) -> PaneMcpLaunch {
    // The person already named a server with this name on the command line: leave it alone.
    let prefix = format!("mcp_servers.{}.", request.server.name);
    if request.user_args.iter().any(|a| a.starts_with(&prefix)) {
        return PaneMcpLaunch::default();
    }
    let server = request.server;
    let name = toml_key(&server.name);
    let args = server.args.iter().map(|a| toml_string(a)).collect::<Vec<_>>().join(", ");
    let env = server
        .env
        .iter()
        .map(|(k, v)| format!("{}={}", toml_key(k), toml_string(v)))
        .collect::<Vec<_>>()
        .join(", ");
    let overrides = [
        format!("mcp_servers.{name}.command={}", toml_string(&server.command)),
        format!("mcp_servers.{name}.args=[{args}]"),
        format!("mcp_servers.{name}.env={{{env}}}"),
    ];
    PaneMcpLaunch {
        env: Vec::new(),
        args: overrides.into_iter().flat_map(|o| ["-c".to_string(), o]).collect(),
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::pane_mcp::PaneMcpServer;
    use std::path::Path;

    fn server() -> PaneMcpServer {
        PaneMcpServer {
            name: "puppet-master".into(),
            command: "C:/Program Files/Puppet Master/bin/puppet-master-mcp.exe".into(),
            args: vec![],
            env: vec![
                ("PUPPET_MASTER_BRIDGE_PORT_FILE".into(), "C:/Users/a b/puppet-master.bridge.port".into()),
                ("PUPPET_MASTER_PANE_ID".into(), "pane-7".into()),
            ],
        }
    }

    fn request<'a>(server: &'a PaneMcpServer, user_args: &'a [String]) -> PaneMcpRequest<'a> {
        PaneMcpRequest { pane_id: "pane-7", user_args, server, dir: Path::new("C:/d"), note: "note" }
    }

    #[test]
    fn three_overrides_name_the_command_args_and_this_panes_env() {
        let s = server();
        let launch = launch(&request(&s, &[]));
        assert!(launch.env.is_empty() && launch.files.is_empty());
        assert_eq!(
            launch.args,
            vec![
                "-c",
                "mcp_servers.puppet-master.command=\"C:/Program Files/Puppet Master/bin/puppet-master-mcp.exe\"",
                "-c",
                "mcp_servers.puppet-master.args=[]",
                "-c",
                "mcp_servers.puppet-master.env={PUPPET_MASTER_BRIDGE_PORT_FILE=\"C:/Users/a b/puppet-master.bridge.port\", PUPPET_MASTER_PANE_ID=\"pane-7\"}",
            ]
        );
    }

    #[test]
    fn every_override_is_valid_toml_with_the_right_values() {
        let s = server();
        for pair in launch(&request(&s, &[])).args.chunks(2) {
            let (key, value) = pair[1].split_once('=').expect("key=value");
            let parsed: toml::Value = toml::from_str(&format!("v = {value}")).unwrap_or_else(|e| panic!("{key}: {e}"));
            assert!(parsed.get("v").is_some());
        }
    }

    #[test]
    fn a_server_the_person_already_named_on_the_command_line_is_left_alone() {
        let s = server();
        let mine = vec!["-c".to_string(), "mcp_servers.puppet-master.command=\"x\"".to_string()];
        assert_eq!(launch(&request(&s, &mine)), PaneMcpLaunch::default());
    }

    #[test]
    fn a_quote_or_backslash_in_a_value_is_escaped() {
        let mut s = server();
        s.env = vec![("K".into(), "a\"b\\c".into())];
        let args = launch(&request(&s, &[])).args;
        let (_, value) = args[5].split_once('=').unwrap();
        let parsed: toml::Value = toml::from_str(&format!("v = {value}")).unwrap();
        assert_eq!(parsed["v"]["K"].as_str(), Some("a\"b\\c"));
    }
}
