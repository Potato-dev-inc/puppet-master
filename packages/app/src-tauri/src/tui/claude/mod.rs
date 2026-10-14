//! Claude Code CLI (`claude`). Everything Claude-specific lives in this folder:
//!
//! - `headless` — command line for headless runs and reading their JSON output
//! - `screen` — whether the interactive TUI is working (drives how soon a pane reads idle)
//! - `prompt` - the folder-trust dialog and the numbered permission menus: detection and answers
//!
//! The generic `Allow once` / `Always allow` wording is still read by the generic fallback.

mod headless;
mod prompt;
mod screen;
#[cfg(test)]
mod fullscreen_tests;

use super::{AccountScheme, Features, HeadlessLaunch, InstructionFile, LaunchSpec, PaneStatus, Reply, StandbyScreen, StandbySupport, StartupGate, TuiProfile};

use serde_json::Value;

pub struct Claude;

/// CLI versions the Claude fixtures and live checks cover (see `TuiProfile::tested_versions`).
pub(crate) const TESTED_VERSIONS: &str = ">=2.1.299, <2.2";

/// Where Claude Code keeps MCP servers (Toolbelt).
const MCP_HOST: &str = r#"
config_path = "~/.claude.json"
format = "json"
key_path = "mcpServers.puppet-master"
project_config_path = ".mcp.json"
[entry]
command = "{command}"
args = "{args}"
env = "{env}"
[entry_http]
type = "http"
url = "{url}"
headers = "{headers}"
"#;

/// Where Claude Code reads skills ("Add Puppet Master to your agents").
const SKILLS_HOST: &str = r#"
dir = "~/.claude/skills"
"#;

impl TuiProfile for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    /// `-p --permission-prompts none --permission-mode acceptEdits` (or `plan`): edits only; shell
    /// commands are denied, never asked about (`headless::policy_args`).
    fn headless_runs_commands_unattended(&self) -> bool {
        false
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

    fn skills_host(&self) -> Option<crate::agent_pack::SkillsHostTable> {
        crate::agent_pack::skills_host_from_toml(SKILLS_HOST)
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            ..Features::CLI
        }
    }

    /// Claude Code reprints the whole conversation after a resize and never sends `ESC[3J`
    /// (recorded under ConPTY and replayed into xterm.js, 2026-10-13).
    fn reprints_on_resize(&self) -> bool {
        true
    }

    /// Claude Code opens a diff panel to the right of the chat after an edit (`/diff`, 2.1.x).
    fn side_panel(&self) -> bool {
        true
    }

    /// Last line of defence for reply text that still carries the panel's column.
    fn tidy_reply(&self, text: &str) -> String {
        crate::tui::side_panel::strip_text(text)
    }

    /// `CLAUDE_CONFIG_DIR` is the documented directory for this CLI's config and login.
    fn account_scheme(&self) -> Option<AccountScheme> {
        Some(AccountScheme {
            home_env: "CLAUDE_CONFIG_DIR".into(),
            extra_env: Vec::new(),
            login_hint: "Run `claude` and log in, then close it.".into(),
        })
    }

    /// Claude Code loads `CLAUDE.md` from the project root (code.claude.com/docs/en/memory).
    fn instruction_files(&self) -> Vec<InstructionFile> {
        vec![InstructionFile::new("CLAUDE.md")]
    }

    fn is_installed(&self) -> bool {
        super::program_on_path("claude")
    }

    fn version_command(&self) -> Option<super::VersionCommand> {
        Some(super::VersionCommand {
            program: super::executable("claude").ok()?,
            args: vec!["--version".into()],
        })
    }

    /// Fixtures are live captures from 2.1.299; the canary (`crate::canary`) re-checks the start
    /// screens whenever the installed version changes.
    fn tested_versions(&self) -> Option<super::TestedRange> {
        super::TestedRange::parse(TESTED_VERSIONS).ok()
    }

    fn install_hint(&self) -> Option<String> {
        Some(if cfg!(windows) {
            "winget install Anthropic.ClaudeCode"
        } else {
            "curl -fsSL https://claude.ai/install.sh | bash"
        }
        .into())
    }

    fn install_fallback_hint(&self) -> Option<String> {
        Some("npm i -g @anthropic-ai/claude-code".into())
    }

    fn login_hint(&self) -> Option<String> {
        Some("Open Claude Code and type /login".into())
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

    fn extract_model(&self, output: &str) -> Option<String> {
        headless::extract_model(output)
    }

    /// Both forms: a TUI pane waits at an empty prompt, and `claude -p --input-format stream-json`
    /// waits for its task on stdin (see `headless::standby_command`).
    fn standby_support(&self) -> StandbySupport {
        StandbySupport {
            visible: true,
            headless: true,
            experimental: false,
        }
    }

    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        screen::standby_screen(screen)
    }

    fn headless_standby_command(&self, launch: &HeadlessLaunch) -> Option<Result<LaunchSpec, String>> {
        Some(headless::standby_command(launch))
    }

    fn headless_standby_input(&self, task: &str) -> Option<String> {
        Some(headless::standby_input(task))
    }

    fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        Some(screen::shows_work_in_progress(visible_tail))
    }

    /// The input box read from the screen (`screen::input_box`): a task is typed only into an empty
    /// box. There is no clear sequence, so a box that already holds text is not typed into either.
    fn ready_recogniser(&self, screen: &str) -> Option<super::ComposerState> {
        Some(screen::input_box(screen))
    }

    /// The screen without the dimmed suggestion / hint in the input box (`screen::without_suggestion`).
    fn input_screen(&self, screen: &vt100::Screen) -> String {
        screen::without_suggestion(screen)
    }

    /// No activity row and 800ms of stillness: the turn is over (live capture of a finished turn).
    fn quiet_idle_after(&self) -> std::time::Duration {
        std::time::Duration::from_millis(800)
    }

    fn is_chrome_line(&self, lower: &str) -> bool {
        screen::is_chrome_line(lower)
    }

    /// The folder-trust dialog draws no "y/n" or "allow once" text, so the generic heuristic reads
    /// it as a running pane; the permission menus are caught here too, whatever their wording.
    /// Both are waiting for the user.
    fn classify_screen(&self, visible_tail: &str) -> Option<PaneStatus> {
        (prompt::shows_trust_gate(visible_tail) || prompt::shows_permission_menu(visible_tail)).then_some(PaneStatus::WaitingInput)
    }

    /// Claude's dialogs by structure (see `prompt`), then the generic wording.
    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<Value> {
        prompt::detect_prompt(self.id(), screen, allow_broad)
            .or_else(|| crate::tui::generic::detect_prompt(self.id(), screen, allow_broad))
    }

    /// The folder-trust dialog is a startup gate only the user may pass (no `dismiss`). Its
    /// answerable card (trust / deny) comes from `detect_prompt`; this hook is how generic code
    /// learns that the pane cannot take a task yet.
    fn startup_gate(&self, screen: &str) -> Option<StartupGate> {
        prompt::shows_trust_gate(screen).then_some(StartupGate {
            id: StartupGate::WORKSPACE_TRUST,
            summary: "Claude Code asks whether to trust this folder",
            dismiss: None,
        })
    }

    fn prompt_reply(&self, prompt: &Value, choice: &str, allow_broad: bool) -> Result<Reply, String> {
        prompt::prompt_reply(prompt, choice, allow_broad)
    }

    /// Typing the prompt and Enter in one burst is taken for a paste: Claude drops the Enter as an
    /// "invisible character" and waits for the user to "review and press Enter to send" (live,
    /// v2.1.287). The verified protocol types the text, confirms it is in the box, then sends Enter
    /// on its own. There is no clear sequence: Ctrl+U is not a kill-line key there, it is typed
    /// into the box as another invisible character.
    fn verifies_prompt_delivery(&self) -> bool {
        true
    }

    /// Ctrl+U, one press per visual line, erases the text Puppet Master typed (live, 2.1.300: two
    /// wrapped copies of a follow-up left in the box). Not the readiness clear sequence: a
    /// person's draft is not Puppet Master's to erase.
    fn retype_clear_sequence(&self) -> Option<&'static str> {
        Some(screen::CLEAR_TYPED_TEXT)
    }
}
