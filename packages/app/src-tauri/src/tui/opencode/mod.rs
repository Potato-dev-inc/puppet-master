//! OpenCode: the terminal TUI (`opencode`) and the same binary driven over its HTTP API
//! (`opencode_native`). Everything TUI-facing lives in this folder:
//!
//! - `screen` — the interactive TUI's menus and selectors
//! - `transcript` — how the conversation is drawn (message rows, tool rows, the input box, the sidebar)
//! - `chat` — the same conversation read from OpenCode's local HTTP API, with tool input, output and diffs
//! - `history` — old chats, read from OpenCode's own SQLite database (read-only), for "Bring all my chats"
//!
//! The HTTP API client (keys, quota, messages) is a protocol backend, not a screen-scraped TUI,
//! and stays in `crate::opencode`.

mod chat;
mod history;
pub mod read_only;
mod screen;
mod transcript;

use super::{Features, InstructionFile, Kind, LinuxLaunch, SpawnRequest, StandbyScreen, StandbySupport, TuiProfile};
use crate::pty::agents::{linux_command, AgentType};

/// OpenCode reads `AGENTS.md` (falling back to `CLAUDE.md` only when it is missing): opencode.ai/docs/rules.
fn agents_md() -> Vec<InstructionFile> {
    vec![InstructionFile::new("AGENTS.md")]
}

pub struct OpenCode;

/// What both OpenCode profiles (the TUI and the TUI attached to the native server) read off the same screen.
macro_rules! transcript_methods {
    () => {
        fn transcript_text(&self, text: &str) -> String {
            transcript::transcript_text(text)
        }

        fn user_message_row(&self, row: &str) -> Option<String> {
            transcript::user_message_row(row)
        }

        fn reads_user_messages(&self) -> bool {
            true
        }

        fn is_chrome_line(&self, lower: &str) -> bool {
            transcript::is_chrome_line(lower)
        }

        fn is_tool_activity_line(&self, lower: &str) -> bool {
            transcript::is_tool_activity_line(lower)
        }

        fn is_thinking_line(&self, lower: &str) -> bool {
            transcript::is_thinking_line(lower)
        }

        fn is_summary_line(&self, lower: &str) -> bool {
            transcript::is_summary_line(lower)
        }

        fn detect_footer_model(&self, buffer: &str) -> Option<String> {
            transcript::footer_model(buffer)
        }
    };
}

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

    /// The TUI only: `opencode_native` (the HTTP protocol backend) has no Linux launch.
    fn linux_launch(&self, request: &SpawnRequest) -> Option<LinuxLaunch> {
        Some(LinuxLaunch::from_command(linux_command(AgentType::Opencode), request.extra_args))
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

    /// The same order as the doctor's Install plan (`install.toml`: winget, then npm). The hint
    /// said npm while the Install button used winget (live v0.2.0 hunt).
    fn install_hint(&self) -> Option<String> {
        Some(if cfg!(windows) { "winget install SST.opencode" } else { "npm i -g opencode-ai" }.into())
    }

    fn install_fallback_hint(&self) -> Option<String> {
        cfg!(windows).then(|| "npm i -g opencode-ai".into())
    }

    fn login_hint(&self) -> Option<String> {
        Some("Open OpenCode and type /connect".into())
    }

    fn install_spec(&self) -> Option<super::install::InstallSpec> {
        super::install::parse_install_toml(include_str!("install.toml")).ok().flatten()
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

    /// OpenCode's old chats: its database under `~/.local/share/opencode` (or `XDG_DATA_HOME`), read-only (`history.rs`).
    fn session_history(&self) -> Option<&'static dyn crate::session_chat::SessionHistory> {
        Some(&history::HISTORY)
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

    /// The TUI is started with `--port`, so it serves its own API on a port Puppet Master knows.
    fn session_api(&self) -> Option<&'static dyn crate::session_chat::SessionApi> {
        Some(&chat::API)
    }

    fn session_api_endpoint(&self, pane: &crate::pty::registry::PaneState) -> Option<crate::session_chat::ApiEndpoint> {
        chat::tui_endpoint(pane)
    }

    fn prepare_session_api(&self, pane_id: &str, user_args: &[String]) -> Vec<String> {
        chat::prepare(pane_id, user_args)
    }

    transcript_methods!();
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

    /// The worker's server (`opencode serve`) is the pane's own and Puppet Master made its session.
    fn session_api(&self) -> Option<&'static dyn crate::session_chat::SessionApi> {
        Some(&chat::API)
    }

    fn session_api_endpoint(&self, pane: &crate::pty::registry::PaneState) -> Option<crate::session_chat::ApiEndpoint> {
        chat::native_endpoint(pane)
    }

    transcript_methods!();
}

#[cfg(all(test, windows))]
mod install_hint_tests {
    use crate::tui::TuiProfile;

    /// The doctor's hint and its Install plan name the same route first (live v0.2.0 hunt: the
    /// hint said npm while the plan installed with winget).
    #[test]
    fn the_hint_names_the_install_plans_first_route_and_the_fallback_its_second() {
        let profile = super::OpenCode;
        let spec = profile.install_spec().expect("OpenCode has an install plan");
        let order = spec.ordered_methods();
        assert_eq!(&order[..2], ["winget", "npm"]);
        let hint = profile.install_hint().unwrap();
        assert!(hint.contains("winget") && hint.contains(spec.winget.as_deref().unwrap()), "{hint}");
        let fallback = profile.install_fallback_hint().unwrap();
        assert!(fallback.contains(spec.npm.as_deref().unwrap()), "{fallback}");
    }
}

#[cfg(test)]
mod linux_launch_tests {
    use super::*;

    fn request<'a>(cwd: &'a str, extra: &'a [String]) -> SpawnRequest<'a> {
        SpawnRequest { cwd, extra_args: extra, cols: 80, rows: 24, read_only: false }
    }

    #[test]
    fn the_tui_runs_as_a_bare_program_with_no_flags() {
        let launch = OpenCode.linux_launch(&request("/mnt/d/work/shop", &[])).unwrap();
        assert_eq!(launch, LinuxLaunch { program: "opencode".into(), args: vec![] });
    }

    #[test]
    fn extra_args_follow_in_order() {
        let extra = vec!["--port".to_string(), "4096".to_string()];
        let launch = OpenCode.linux_launch(&request("/mnt/d/work/shop", &extra)).unwrap();
        assert_eq!(launch.args, ["--port", "4096"]);
    }

    #[test]
    fn the_native_protocol_backend_has_no_linux_launch() {
        assert!(OpenCodeNative.linux_launch(&request("/mnt/d/work/shop", &[])).is_none());
    }
}
