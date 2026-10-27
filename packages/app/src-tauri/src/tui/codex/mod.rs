//! OpenAI Codex CLI (`codex`). Everything Codex-specific lives in this folder:
//!
//! - `headless` — `codex exec` command line, session resume, JSONL output parsing
//! - `screen` — whether the interactive TUI is working, waiting on a dialog, or at an empty composer
//! - `prompt` — the folder-trust dialog as an answerable card (Trust / Don't trust)
//!
//! Interactive-pane heuristics beyond standby use the generic defaults.

mod headless;
mod prompt;
pub mod session_file;
mod session_extra;
pub mod session_source;
mod screen;

use super::{AccountScheme, ComposerState, Features, HeadlessLaunch, InstructionFile, LaunchSpec, RunFacts, StandbyScreen, StandbySupport, StartupGate, SubmitSequence, TuiProfile, Warning};

pub struct Codex;

/// CLI versions the Codex fixtures and live checks cover (see `TuiProfile::tested_versions`).
pub(crate) const TESTED_VERSIONS: &str = ">=0.159, <0.170";

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

/// Where Codex reads skills ("Add Puppet Master to your agents").
const SKILLS_HOST: &str = r#"
dir = "~/.agents/skills"
"#;

impl TuiProfile for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    /// What the screen says when the plan runs out (`limits.toml`, read by `crate::handoff`).
    fn limits(&self) -> Option<&crate::handoff::CompiledLimits> {
        static LIMITS: once_cell::sync::Lazy<Option<crate::handoff::CompiledLimits>> =
            once_cell::sync::Lazy::new(|| crate::handoff::CompiledLimits::from_toml(include_str!("limits.toml")));
        LIMITS.as_ref()
    }

    fn mcp_host(&self) -> Option<crate::tui::declarative::schema::McpHostTable> {
        crate::toolbelt::host_from_toml(MCP_HOST)
    }

    fn hook_scheme(&self) -> Option<crate::tui::declarative::hooks::HooksTable> {
        crate::tui::declarative::hooks::parse_hooks_toml(include_str!("hooks.toml"))
    }

    fn session_chat(&self) -> Option<&'static dyn crate::session_chat::SessionSource> {
        Some(&session_source::SOURCE)
    }

    fn skills_host(&self) -> Option<crate::agent_pack::SkillsHostTable> {
        crate::agent_pack::skills_host_from_toml(SKILLS_HOST)
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

    /// Fixtures are live captures from 0.159.x; the canary (`crate::canary`) re-checks the start
    /// screens whenever the installed version changes.
    fn tested_versions(&self) -> Option<super::TestedRange> {
        super::TestedRange::parse(TESTED_VERSIONS).ok()
    }

    fn install_hint(&self) -> Option<String> {
        Some(if cfg!(windows) {
            "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\""
        } else {
            "curl -fsSL https://chatgpt.com/codex/install.sh | sh"
        }
        .into())
    }

    fn install_fallback_hint(&self) -> Option<String> {
        Some("npm i -g @openai/codex".into())
    }

    fn login_hint(&self) -> Option<String> {
        Some("Run: codex login".into())
    }

    fn install_spec(&self) -> Option<super::install::InstallSpec> {
        super::install::parse_install_toml(include_str!("install.toml")).ok().flatten()
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

    /// Codex reads characters that arrive in a rapid burst as a paste, and an Enter within
    /// `PASTE_ENTER_SUPPRESS_WINDOW` (120 ms, codex-rs/tui/src/bottom_pane/paste_burst.rs) of the
    /// burst becomes a line break in the composer instead of a submit. Live on 0.159.3 (see
    /// `docs/features/fix-live-codex.md`): Enter 0 and 50 ms after the text left it unsent, Enter
    /// 100 ms after submitted. 400 ms leaves a wide margin for a loaded machine.
    fn submit_sequence(&self) -> SubmitSequence {
        SubmitSequence {
            settle: std::time::Duration::from_millis(400),
            keys: "\r",
        }
    }

    fn reads_composer(&self) -> bool {
        true
    }

    fn composer_state(&self, screen: &str) -> ComposerState {
        screen::composer_state(screen)
    }

    /// The composer without its dimmed placeholder (`screen::without_placeholder`).
    fn input_screen(&self, screen: &vt100::Screen) -> String {
        screen::without_placeholder(screen)
    }

    /// The composer, but not while Codex is still loading (it draws the box before it takes keys).
    fn ready_recogniser(&self, screen: &str) -> Option<ComposerState> {
        if screen::still_loading(screen) {
            return Some(ComposerState::Unknown);
        }
        Some(screen::composer_state(screen))
    }

    fn startup_gate(&self, screen: &str) -> Option<StartupGate> {
        screen::startup_gate(screen)
    }

    /// A startup screen only the person can pass (folder trust, sign-in, onboarding) is a pane waiting
    /// for them. The generic heuristic reads Codex's trust screen as idle (it draws no `y/n` text and the
    /// busy marker says "not working"), so the pane stayed `idle`, `deriveNeedsYou` raised no card and
    /// the Trust / Don't trust buttons never reached the Inbox (live, v0.2.0: the run's own
    /// `required_action` had them, `GET /attention` was empty). Gates Puppet Master dismisses itself
    /// (update notice, hooks review) are not waits.
    fn classify_screen(&self, visible_tail: &str) -> Option<crate::pty::status::PaneStatus> {
        screen::startup_gate(visible_tail)
            .filter(|gate| gate.dismiss.is_none())
            .map(|_| crate::pty::status::PaneStatus::WaitingInput)
    }

    /// The folder-trust dialog as a card with Trust / Don't trust (see `prompt`), then the generic
    /// wording. Without it the trust card had no buttons and the person had to open the terminal.
    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<serde_json::Value> {
        prompt::trust_prompt(self.id(), screen).or_else(|| super::generic::detect_prompt(self.id(), screen, allow_broad))
    }

    fn prompt_reply(&self, prompt: &serde_json::Value, choice: &str, allow_broad: bool) -> Result<super::Reply, String> {
        prompt::prompt_reply(prompt, choice, allow_broad)
    }

    fn clear_input_sequence(&self) -> Option<&'static str> {
        Some(screen::CLEAR_COMPOSER)
    }

    /// The banner row and the folder row under it are not part of a reply (`screen::strip_banner_block`).
    fn strip_dialogs(&self, text: &str) -> String {
        screen::strip_banner_block(text)
    }

    /// The footer, the shortcuts hint and the `Worked for` separator are not part of a reply.
    fn is_chrome_line(&self, lower: &str) -> bool {
        screen::is_chrome_line(lower)
    }

    /// Codex draws `Working (12s • esc to interrupt)` above the composer for as long as a turn
    /// runs, and no such row otherwise (live captures `working_live.txt` / `standby_ready_live.txt`).
    /// So a redraw of an idle pane (selecting it remounts the terminal, the resize makes Codex reprint)
    /// is provably not work and the pane stays idle instead of flashing "Working 0s".
    fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        Some(screen::shows_work_in_progress(visible_tail))
    }

    /// The composer is readable (`composer_state`), so a typed task is confirmed on screen before
    /// Enter and a started turn after it.
    fn verifies_prompt_delivery(&self) -> bool {
        true
    }

    fn probes_default_model(&self) -> bool {
        true
    }

    fn default_model(&self) -> Option<String> {
        headless::default_model()
    }

    /// A turn whose own tool call failed (live 0.162.1: `Read README.md (exit -1)`, "the workspace
    /// command runner failed to start") still ends cleanly, so the run is `completed`. Say so.
    fn run_warnings(&self, facts: &RunFacts) -> Vec<Warning> {
        if facts.completed && facts.result.is_some_and(screen::reports_tool_failure) {
            return vec![Warning {
                code: "AGENT_REPORTED_ERROR",
                source: "heuristic",
                detail: "Codex reported that one of its own tool calls failed, so work its reply describes may not have happened. Check the result before relying on it.",
            }];
        }
        Vec::new()
    }
}
