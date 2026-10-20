//! Resolve how CLI hosts should launch the Puppet Master MCP stdio server.

use once_cell::sync::OnceCell;
use std::path::{Path, PathBuf};

static BUNDLED_MCP_SCRIPT: OnceCell<PathBuf> = OnceCell::new();
static BUNDLED_MCP_BINARY: OnceCell<PathBuf> = OnceCell::new();

pub fn set_bundled_mcp_script(path: PathBuf) {
    let _ = BUNDLED_MCP_SCRIPT.set(path);
}

pub fn bundled_mcp_script() -> Option<&'static Path> {
    BUNDLED_MCP_SCRIPT.get().map(|p| p.as_path())
}

pub fn set_bundled_mcp_binary(path: PathBuf) {
    let _ = BUNDLED_MCP_BINARY.set(path);
}

pub fn bundled_mcp_binary() -> Option<&'static Path> {
    BUNDLED_MCP_BINARY.get().map(|p| p.as_path())
}

/// The MCP binary's file name on this platform.
fn mcp_binary_name() -> &'static str {
    if cfg!(windows) {
        "puppet-master-mcp.exe"
    } else {
        "puppet-master-mcp"
    }
}

/// Where a dev build (no installed resource folder) may find the Rust MCP binary, best first: the
/// package's `dist` (where it is built), the staged `src-tauri/resources/bin`, `<exe dir>/bin` (what
/// Tauri's resource folder is in a dev run, e.g. `target/debug/bin`).
pub fn dev_mcp_binary_candidates(manifest_dir: &Path, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let name = mcp_binary_name();
    let mut list = vec![
        manifest_dir.join("../../mcp-server/dist").join(name),
        manifest_dir.join("resources/bin").join(name),
    ];
    if let Some(dir) = exe_dir {
        list.push(dir.join("bin").join(name));
    }
    list
}

/// The first of [`dev_mcp_binary_candidates`] that exists.
pub fn dev_mcp_binary(manifest_dir: &Path, exe_dir: Option<&Path>) -> Option<PathBuf> {
    dev_mcp_binary_candidates(manifest_dir, exe_dir).into_iter().find(|path| path.is_file())
}

pub fn using_bundled_mcp() -> bool {
    bundled_mcp_binary().is_some() || bundled_mcp_script().is_some()
}

pub struct McpLaunchSpec {
    pub command: String,
    pub args: Vec<String>,
}

pub fn npm_mcp_launch_spec() -> McpLaunchSpec {
    McpLaunchSpec {
        command: "npx".into(),
        args: vec!["-y".into(), "@puppet-master/mcp".into()],
    }
}

/// Command + args written into Claude / Codex / OpenCode MCP config.
pub fn mcp_launch_spec() -> McpLaunchSpec {
    if let Some(binary) = bundled_mcp_binary() {
        return McpLaunchSpec {
            command: crate::app_paths::path_for_host_config(binary),
            args: vec![],
        };
    }
    if let Some(script) = bundled_mcp_script() {
        return McpLaunchSpec {
            command: resolve_node_executable(),
            args: vec![crate::app_paths::path_for_host_config(script)],
        };
    }
    npm_mcp_launch_spec()
}

fn resolve_node_executable() -> String {
    which_node_executable()
        .map(|path| crate::app_paths::path_for_host_config(Path::new(&path)))
        .unwrap_or_else(|| "node".into())
}

#[cfg(unix)]
fn which_node_executable() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = crate::proc_util::hidden_command(&shell)
        .args(["-l", "-c", "command -v node"])
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

#[cfg(windows)]
fn which_node_executable() -> Option<String> {
    let output = crate::proc_util::hidden_command("cmd.exe")
        .args(["/C", "where node"])
        .env("Path", crate::shell_env::path_for_spawn())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(all(not(unix), not(windows)))]
fn which_node_executable() -> Option<String> {
    None
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

#[cfg(test)]
mod dev_fallback_tests {
    use super::*;

    #[test]
    fn a_dev_build_finds_the_mcp_binary_in_the_staged_resources_folder() {
        // VERIFY-R13 #3: the exe sat in src-tauri/resources/bin only, so Hook Lane stayed inactive.
        let root = std::env::temp_dir().join(format!("pm-mcp-dev-{}", uuid::Uuid::new_v4()));
        let manifest = root.join("packages/app/src-tauri");
        let staged = manifest.join("resources/bin");
        std::fs::create_dir_all(&staged).unwrap();
        assert_eq!(dev_mcp_binary(&manifest, None), None, "nothing built or staged yet");
        std::fs::write(staged.join(mcp_binary_name()), "x").unwrap();
        assert_eq!(dev_mcp_binary(&manifest, None), Some(manifest.join("resources/bin").join(mcp_binary_name())));
        // The exe-side `bin` folder (a dev run's resource dir) counts too, after the other two.
        let exe_dir = root.join("target/debug");
        std::fs::create_dir_all(exe_dir.join("bin")).unwrap();
        std::fs::write(exe_dir.join("bin").join(mcp_binary_name()), "x").unwrap();
        let only_exe = root.join("elsewhere");
        assert_eq!(dev_mcp_binary(&only_exe, Some(&exe_dir)), Some(exe_dir.join("bin").join(mcp_binary_name())));
        std::fs::remove_dir_all(&root).ok();
    }
}
