//! Cursor: the headless/interactive Cursor Agent CLI (`cursor_agent`) and its interactive-only
//! alias (`cursor`). Everything Cursor-specific lives in this folder:
//!
//! - `binary`   — which launcher is used (`agent` preferred, `cursor-agent` fallback)
//! - `launch`   — command lines for headless runs and the CLI's configured default model
//! - `headless` — reading a headless run's output (result, rejected calls, trust screen)
//! - `screen`   — the interactive TUI: status classification, approval/trust prompts, footer model
//! - `chrome`   — which screen lines are TUI furniture rather than reply text
//! - `fixtures` — real captures the tests replay

pub(crate) mod approval;
pub(crate) mod binary;
mod chrome;
mod headless;
mod launch;
mod screen;

#[cfg(test)]
pub use launch::parse_default_model;

use super::{
    Features, HeadlessLaunch, InstructionFile, Kind, LaunchSpec, Reply, RunFacts, RunOptionSupport, StandbyScreen,
    StandbySupport, TuiProfile, VersionCommand, Warning,
};
use crate::pty::status::PaneStatus;
use serde_json::Value;

/// Cursor loads `.cursor/rules/*.mdc`; a rule needs front-matter, and `alwaysApply: true` puts it in
/// every session (cursor.com/docs/context/rules; the CLI "supports the same rules system",
/// cursor.com/docs/cli/using).
fn cursor_rules() -> Vec<InstructionFile> {
    vec![InstructionFile::with_header(
        ".cursor/rules/puppet-master.mdc",
        "---
description: Shared project memory kept by Puppet Master
alwaysApply: true
---",
    )]
}

const NOTE_BOUNDARIES: &str =
    "cursor TUI response boundaries are version-specific screen heuristics, not protocol events";
const NOTE_QUESTIONS: &str =
    "cursor questions (trust / allow-once) are version-specific screen matches, not a stable question API";
const NOTE_INTERRUPT: &str =
    "cursor interrupt is Ctrl+C on a visible TUI or child stop when headless; not a graceful provider abort";
const NOTE_EXTRACT: &str =
    "cursor output extraction is headless JSON (--print) or TUI screen delta; exact parity across versions is not guaranteed";
const NOTE_HEADLESS_SHELL: &str =
    "headless cursor_agent runs without --force/--trust, so its shell commands are rejected by Cursor and it cannot run tests; completed only means the process exited. Use a visible pane or opencode_native when the task must execute commands";
const NOTE_HEADLESS_MODEL: &str =
    "headless cursor_agent passes a requested model as --model but never reports which model ran, so resolved_model is the requested value (unconfirmed) or, when none was requested, the CLI configured default (from `cursor-agent about`); neither is a per-run confirmation";

fn capability_notes(has_pane: bool) -> Vec<String> {
    let mut notes: Vec<String> = [
        NOTE_BOUNDARIES,
        NOTE_QUESTIONS,
        NOTE_INTERRUPT,
        NOTE_EXTRACT,
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if !has_pane {
        notes.push(NOTE_HEADLESS_SHELL.into());
        notes.push(NOTE_HEADLESS_MODEL.into());
    }
    notes
}

pub struct CursorAgent;

/// Where Cursor keeps MCP servers (Toolbelt).
const MCP_HOST: &str = r#"
config_path = "~/.cursor/mcp.json"
format = "json"
key_path = "mcpServers.puppet-master"
project_config_path = ".cursor/mcp.json"
[entry]
command = "{command}"
args = "{args}"
env = "{env}"
[entry_http]
url = "{url}"
headers = "{headers}"
"#;

impl TuiProfile for CursorAgent {
    fn id(&self) -> &'static str {
        "cursor_agent"
    }

    fn mcp_host(&self) -> Option<crate::tui::declarative::schema::McpHostTable> {
        crate::toolbelt::host_from_toml(MCP_HOST)
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        cursor_rules()
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            resumable_headless: true,
            ..Features::CLI
        }
    }

    fn run_options(&self) -> RunOptionSupport {
        RunOptionSupport {
            trust_workspace: true,
            allow_shell: true,
            shell_default_on: false,
            allowed_commands: true,
        }
    }

    fn is_installed(&self) -> bool {
        launch::is_installed()
    }

    fn version_command(&self) -> Option<VersionCommand> {
        let cli = binary::installed()?;
        let (program, args) = binary::visible_launch(&cli, &["--version".to_string()]);
        Some(VersionCommand { program, args })
    }

    fn install_hint(&self) -> Option<String> {
        Some(if cfg!(windows) {
            "irm 'https://cursor.com/install?win32=true' | iex"
        } else {
            "curl https://cursor.com/install -fsS | bash"
        }
        .into())
    }

    fn login_hint(&self) -> Option<String> {
        Some("Run: agent login".into())
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        launch::headless_command(launch)
    }

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        headless::extract_result(output, exit_code)
    }

    fn accepts_headless_model(&self) -> bool {
        true
    }

    fn headless_result_ready(&self, output: &str) -> bool {
        headless::has_result_event(output)
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
        headless::extract_provider_session(output)
    }

    fn rejected_call_warning(&self, output: &str) -> Option<&'static str> {
        headless::rejected_call_warning(output)
    }

    fn detect_headless_trust(&self, output: &str) -> Option<Value> {
        headless::detect_trust(self.id(), output)
    }

    fn probes_default_model(&self) -> bool {
        true
    }

    fn default_model(&self) -> Option<String> {
        launch::default_model()
    }

    fn run_warnings(&self, facts: &RunFacts) -> Vec<Warning> {
        let mut warnings = Vec::new();
        if facts
            .result
            .is_some_and(|result| result.contains(headless::REJECTED_CALL_MARKER))
        {
            warnings.push(Warning {
                code: "SHELL_COMMAND_REJECTED",
                source: "heuristic",
                detail: "Cursor reported a rejected tool or shell call; its commands may not have run. Verify with `checks` (command_exits).",
            });
        }
        if !facts.has_pane && facts.completed && facts.model_requested {
            warnings.push(Warning {
                code: "MODEL_UNCONFIRMED",
                source: "capture",
                detail: "The requested model was passed to Cursor as --model, but headless Cursor does not report which model ran, so resolved_model is the request, not a confirmation.",
            });
        }
        if facts.has_pane && facts.completed && facts.model_unconfirmed {
            warnings.push(Warning {
                code: "MODEL_UNCONFIRMED",
                source: "capture",
                detail: "The Cursor footer model was not visible when the turn finished, so resolved_model is the CLI's configured default, not a confirmation of what ran.",
            });
        }
        if facts.has_pane && facts.completed && facts.result_inferred {
            warnings.push(Warning {
                code: "PROVENANCE_INFERRED",
                source: "capture",
                detail: "Result was read from the terminal screen, not a structured stream. Prefer headless, or confirm with `checks`.",
            });
        }
        warnings
    }

    /// Visible panes only: headless Cursor takes its prompt as an argument and ignores stdin
    /// (`No prompt provided for print mode`), so it cannot be pre-launched.
    fn standby_support(&self) -> StandbySupport {
        StandbySupport {
            visible: true,
            headless: false,
            experimental: false,
        }
    }

    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        screen::standby_screen(self.id(), screen)
    }

    fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        Some(screen::shows_work_in_progress(visible_tail))
    }

    /// The framed input box read from the screen (`screen::input_box`).
    fn ready_recogniser(&self, screen: &str) -> Option<crate::tui::ComposerState> {
        Some(screen::input_box(screen))
    }

    /// With no spinner and no stop hint on screen, 700ms of stillness is a finished turn: the
    /// spinner repaints every ~250ms and the stop hint stays up for the whole turn, including
    /// tool runs (frames recorded live, an 8s shell command included).
    fn quiet_idle_after(&self) -> std::time::Duration {
        std::time::Duration::from_millis(700)
    }

    fn classify_screen(&self, visible_tail: &str) -> Option<PaneStatus> {
        Some(screen::classify(visible_tail))
    }

    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<Value> {
        screen::detect_prompt(self.id(), screen, allow_broad)
    }

    fn prompt_reply(
        &self,
        prompt: &Value,
        choice: &str,
        allow_broad: bool,
    ) -> Result<Reply, String> {
        screen::prompt_reply(prompt, choice, allow_broad)
    }

    /// Ctrl+U empties Cursor's input box (verified live). Never Ctrl+C: on Cursor that only
    /// arms "press again to exit".
    fn clear_input_sequence(&self) -> Option<&'static str> {
        Some("\u{15}")
    }

    fn verifies_prompt_delivery(&self) -> bool {
        true
    }

    fn is_chrome_line(&self, lower: &str) -> bool {
        chrome::is_chrome_line(lower)
    }

    fn is_tool_activity_line(&self, lower: &str) -> bool {
        chrome::is_tool_line(lower)
    }

    fn strip_dialogs(&self, text: &str) -> String {
        chrome::strip_search_results(&chrome::join_wrapped_shell_headers(&chrome::strip_footer_cwd(
            &chrome::strip_dialog_boxes(text),
        )))
    }

    fn tidy_reply(&self, text: &str) -> String {
        chrome::tidy_reply(text)
    }

    fn detect_footer_model(&self, buffer: &str) -> Option<String> {
        screen::footer_model(buffer)
    }

    fn capability_notes(&self, has_pane: bool) -> Vec<String> {
        capability_notes(has_pane)
    }
}

/// The `cursor` choice. Interactive panes run the same agent CLI as `cursor_agent` (the Cursor IDE
/// launcher `cursor.cmd` is not an agent and exits straight away in a pane); it is not a
/// delegated-work backend (`run_agent` uses `cursor_agent`), and its panes share the CLI's screen
/// heuristics.
pub struct CursorIde;

impl TuiProfile for CursorIde {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        cursor_rules()
    }

    fn features(&self) -> Features {
        Features {
            kind: Kind::Cli,
            dispatchable: false,
            ..Features::CLI
        }
    }

    fn not_dispatchable_reason(&self) -> &'static str {
        "cursor is an interactive pane of the Cursor agent CLI and cannot run delegated work; use cursor_agent for agent runs"
    }

    fn classify_screen(&self, visible_tail: &str) -> Option<PaneStatus> {
        Some(screen::classify(visible_tail))
    }

    fn capability_notes(&self, has_pane: bool) -> Vec<String> {
        capability_notes(has_pane)
    }
}
