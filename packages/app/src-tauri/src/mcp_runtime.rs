//! Resolve how CLI hosts should launch the Puppet Master MCP stdio server.

use once_cell::sync::OnceCell;
use std::path::{Path, PathBuf};

static BUNDLED_MCP_SCRIPT: OnceCell<PathBuf> = OnceCell::new();

pub fn set_bundled_mcp_script(path: PathBuf) {
    let _ = BUNDLED_MCP_SCRIPT.set(path);
}

pub fn bundled_mcp_script() -> Option<&'static Path> {
    BUNDLED_MCP_SCRIPT.get().map(|p| p.as_path())
}

pub fn using_bundled_mcp() -> bool {
    bundled_mcp_script().is_some()
}

pub struct McpLaunchSpec {
    pub command: String,
    pub args: Vec<String>,
}

/// Command + args written into Claude / Codex / OpenCode MCP config.
pub fn mcp_launch_spec() -> McpLaunchSpec {
    if let Some(script) = bundled_mcp_script() {
        return McpLaunchSpec {
            command: resolve_node_executable(),
            args: vec![script.to_string_lossy().into_owned()],
        };
    }
    McpLaunchSpec {
        command: "npx".into(),
        args: vec!["-y".into(), "@puppet-master/mcp".into()],
    }
}

fn resolve_node_executable() -> String {
    which_node_with_login_shell().unwrap_or_else(|| "node".into())
}

fn which_node_with_login_shell() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = std::process::Command::new(&shell)
        .args(["-l", "-c", "command -v node"])
        .env("PATH", crate::shell_env::path_for_spawn())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_uses_npx_when_no_bundle() {
        let spec = mcp_launch_spec();
        assert_eq!(spec.command, "npx");
        assert!(spec.args.contains(&"@puppet-master/mcp".to_string()));
    }
}
