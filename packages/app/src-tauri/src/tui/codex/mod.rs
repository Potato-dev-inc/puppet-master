//! OpenAI Codex CLI (`codex`). Everything Codex-specific lives in this folder:
//!
//! - `headless` — `codex exec` command line, session resume, JSONL output parsing
//! - `screen` — whether the interactive TUI is working, waiting on a dialog, or at an empty composer
//!
//! Interactive-pane heuristics beyond standby use the generic defaults.

mod headless;
mod screen;

use super::{AccountScheme, Features, HeadlessLaunch, InstructionFile, LaunchSpec, StandbyScreen, StandbySupport, TuiProfile};

pub struct Codex;

/// Where Codex keeps MCP servers (Toolbelt).
const MCP_HOST: &str = r#"
config_path = "~/.codex/config.toml"
format = "toml"
key_path = "mcp_servers.puppet-master"
project_config_path = ".codex/config.toml"
[entry]
command = "{command}"
args = "{args}"
env = "{env}"
[entry_http]
url = "{url}"
http_headers = "{headers}"
"#;

impl TuiProfile for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn mcp_host(&self) -> Option<crate::tui::declarative::schema::McpHostTable> {
        crate::toolbelt::host_from_toml(MCP_HOST)
    }

    /// Codex reads `AGENTS.md` from the project root down to the cwd (developers.openai.com/codex/guides/agents-md).
    fn instruction_files(&self) -> Vec<InstructionFile> {
        vec![InstructionFile::new("AGENTS.md")]
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            resumable_headless: true,
            ..Features::CLI
        }
    }

    /// `CODEX_HOME` is the documented directory for this CLI's config and login.
    fn account_scheme(&self) -> Option<AccountScheme> {
        Some(AccountScheme {
            home_env: "CODEX_HOME".into(),
            extra_env: Vec::new(),
            login_hint: "Run `codex` and sign in, then close it.".into(),
        })
    }

    fn is_installed(&self) -> bool {
        super::program_on_path("codex")
    }

    fn version_command(&self) -> Option<super::VersionCommand> {
        Some(super::VersionCommand {
            program: super::executable("codex").ok()?,
            args: vec!["--version".into()],
        })
    }

    fn install_hint(&self) -> Option<String> {
        Some("npm i -g @openai/codex".into())
    }

    fn login_hint(&self) -> Option<String> {
        Some("Run: codex login".into())
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        headless::command(launch)
    }

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        headless::extract_result(output, exit_code)
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
        headless::extract_provider_session(output)
    }

    /// Visible panes only: headless Codex takes its task as an argument, not on stdin.
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

    fn probes_default_model(&self) -> bool {
        true
    }

    fn default_model(&self) -> Option<String> {
        headless::default_model()
    }
}
