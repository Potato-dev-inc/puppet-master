//! OpenCode: the terminal TUI (`opencode`) and the same binary driven over its HTTP API
//! (`opencode_native`). Everything TUI-facing lives in this folder:
//!
//! - `screen` — the interactive TUI's menus and selectors
//!
//! The HTTP API client (keys, quota, messages) is a protocol backend, not a screen-scraped TUI,
//! and stays in `crate::opencode`.

pub mod read_only;
mod screen;

use super::{Features, InstructionFile, Kind, StandbyScreen, StandbySupport, TuiProfile};

/// OpenCode reads `AGENTS.md` (falling back to `CLAUDE.md` only when it is missing): opencode.ai/docs/rules.
fn agents_md() -> Vec<InstructionFile> {
    vec![InstructionFile::new("AGENTS.md")]
}

pub struct OpenCode;

/// Where OpenCode keeps MCP servers (Toolbelt).
const MCP_HOST: &str = r#"
config_path = "~/.config/opencode/opencode.json"
format = "json"
key_path = "mcp.puppet-master"
project_config_path = "opencode.json"
[entry]
type = "local"
command = ["{command}", "{args}"]
environment = "{env}"
enabled = true
[entry_http]
type = "remote"
url = "{url}"
headers = "{headers}"
enabled = true
"#;

/// Where OpenCode reads skills ("Add Puppet Master to your agents").
const SKILLS_HOST: &str = r#"
dir = "~/.agents/skills"
"#;

impl TuiProfile for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn is_installed(&self) -> bool {
        super::program_on_path("opencode")
    }

    fn version_command(&self) -> Option<super::VersionCommand> {
        Some(super::VersionCommand {
            program: super::executable("opencode").ok()?,
            args: vec!["--version".into()],
        })
    }

    fn install_hint(&self) -> Option<String> {
        Some("npm i -g opencode-ai".into())
    }

    fn login_hint(&self) -> Option<String> {
        Some("Open OpenCode and type /connect".into())
    }

    fn mcp_host(&self) -> Option<crate::tui::declarative::schema::McpHostTable> {
        crate::toolbelt::host_from_toml(MCP_HOST)
    }

    fn skills_host(&self) -> Option<crate::agent_pack::SkillsHostTable> {
        crate::agent_pack::skills_host_from_toml(SKILLS_HOST)
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        agents_md()
    }

    /// Visible panes only: the TUI takes its task typed into the pane, not on stdin.
    fn standby_support(&self) -> StandbySupport {
        StandbySupport {
            visible: true,
            headless: false,
            experimental: false,
        }
    }

    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        screen::standby_screen(screen)
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        screen::looks_like_menu(text)
    }
}

pub struct OpenCodeNative;

impl TuiProfile for OpenCodeNative {
    fn id(&self) -> &'static str {
        "opencode_native"
    }

    fn mcp_host(&self) -> Option<crate::tui::declarative::schema::McpHostTable> {
        crate::toolbelt::host_from_toml(MCP_HOST)
    }

    fn skills_host(&self) -> Option<crate::agent_pack::SkillsHostTable> {
        crate::agent_pack::skills_host_from_toml(SKILLS_HOST)
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        agents_md()
    }

    fn features(&self) -> Features {
        Features {
            kind: Kind::NativeProtocol,
            read_only_spawned_pane: true,
            ..Features::CLI
        }
    }

    fn run_options(&self) -> super::RunOptionSupport {
        // Verified live: the native worker's bash tool runs without any permission opt-in.
        super::RunOptionSupport {
            allow_shell: true,
            shell_default_on: true,
            ..Default::default()
        }
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        screen::looks_like_menu(text)
    }
}
