//! TUI profiles: everything that is specific to one agent CLI/TUI lives in its own submodule here.
//!
//! The generic machinery (pane registry, run lifecycle, wait, approvals, steering) never matches
//! on an agent-type string. It asks `profile_for(agent_type)` and calls a `TuiProfile` method, so a
//! fix for one TUI stays inside that TUI's folder. See `README.md` in this folder for how to add one.

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod generic;
pub mod opencode;
pub mod shell;

use crate::pty::status::PaneStatus;
use serde_json::Value;
use std::path::PathBuf;

/// How a backend is driven; decides which capability contract it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Interactive agent CLI observed through its terminal and/or run headless.
    Cli,
    /// Agent driven over its own protocol (OpenCode's HTTP API) instead of screen scraping.
    NativeProtocol,
    /// Plain shell pane: no agent, no model.
    Shell,
    /// Not a registered profile; behaves like a bare pane with generic heuristics.
    Unknown,
}

/// Declarative facts about a profile. Generic code reads these instead of listing agent names.
#[derive(Debug, Clone, Copy)]
pub struct Features {
    pub kind: Kind,
    /// `delegate_work`/`run_agent` may start this backend.
    pub dispatchable: bool,
    /// A headless run can enforce `read_only`.
    pub read_only_headless: bool,
    /// A pane this app spawns for the run can enforce `read_only` (headless is separate).
    pub read_only_spawned_pane: bool,
    /// A headless run can resume a provider session by id.
    pub resumable_headless: bool,
}

impl Features {
    pub const CLI: Features = Features {
        kind: Kind::Cli,
        dispatchable: true,
        read_only_headless: false,
        read_only_spawned_pane: false,
        resumable_headless: false,
    };
}

/// Opt-in per-run flags a profile may accept.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunOptionSupport {
    /// `trust_workspace`: headless-only; pre-trusts the project folder.
    pub trust_workspace: bool,
    /// `allow_shell`: headless-only, never with `read_only`; lets the agent run shell commands.
    pub allow_shell: bool,
    /// `allowed_commands`: visible-pane-only; auto-approve named programs.
    pub allowed_commands: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// True only where the CLI exposes a documented read-only execution policy.
    pub read_only_enforced: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HeadlessLaunch<'a> {
    pub project: &'a str,
    pub task: &'a str,
    pub read_only: bool,
    pub resume_session: Option<&'a str>,
    pub trust_workspace: bool,
    pub allow_shell: bool,
    /// Model requested for this run; only profiles with `accepts_headless_model` use it.
    pub model: Option<&'a str>,
}

#[cfg(test)]
impl<'a> HeadlessLaunch<'a> {
    pub fn new(project: &'a str, task: &'a str) -> Self {
        Self {
            project,
            task,
            read_only: false,
            resume_session: None,
            trust_workspace: false,
            allow_shell: false,
            model: None,
        }
    }
}

/// What to send a pane to answer a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub key: Option<String>,
    pub text: Option<String>,
}

impl Reply {
    pub fn key(key: &str) -> Self {
        Self {
            key: Some(key.into()),
            text: None,
        }
    }
}

/// A warning a profile attaches to a finished run (surfaced next to the result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub code: &'static str,
    pub source: &'static str,
    pub detail: &'static str,
}

/// Facts about a finished run that a profile may turn into warnings.
#[derive(Debug, Clone, Copy)]
pub struct RunFacts<'a> {
    pub result: Option<&'a str>,
    pub has_pane: bool,
    pub completed: bool,
    /// The result was read off the terminal screen rather than a structured stream.
    pub result_inferred: bool,
    /// The caller asked for a specific model for this run.
    pub model_requested: bool,
    /// `resolved_model` is the CLI's default (or a request), not read from the live TUI.
    pub model_unconfirmed: bool,
}

/// One agent CLI/TUI. Every method has a generic default, so a profile overrides only what its
/// TUI does differently. Keep patterns, fixtures and tests for a TUI in that TUI's folder.
pub trait TuiProfile: Sync {
    /// The `agent_type` string (matches `pty::agents::AgentType::as_str`).
    fn id(&self) -> &'static str;

    fn features(&self) -> Features {
        Features::CLI
    }

    /// Why this backend cannot be dispatched; shown to the caller. Only read when
    /// `features().dispatchable` is false.
    fn not_dispatchable_reason(&self) -> &'static str {
        "this backend cannot run delegated work"
    }

    fn run_options(&self) -> RunOptionSupport {
        RunOptionSupport::default()
    }

    /// Whether the CLI this profile launches can be found on this machine. Defaults to true for
    /// backends with nothing to look up; CLI profiles override it with a PATH lookup.
    fn is_installed(&self) -> bool {
        true
    }

    // ---- headless runs ------------------------------------------------------------------

    /// Command line for a headless run. Argument policy (read-only support, flag conflicts) is
    /// checked by `headless_launch` before this is called.
    fn headless_command(&self, _launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        Err(format!(
            "headless launch is unsupported for agent type: {}",
            self.id()
        ))
    }

    /// True when `headless_command` passes `HeadlessLaunch::model` to the CLI (a per-run model).
    fn accepts_headless_model(&self) -> bool {
        false
    }

    /// Final machine-readable result after a successful exit; never a human-readable fallback.
    fn extract_result(&self, _output: &str, _exit_code: i32) -> Option<String> {
        None
    }

    /// Provider conversation id from the CLI output, so a follow-up can resume it.
    fn extract_provider_session(&self, _output: &str) -> Option<String> {
        None
    }

    /// Warning to attach to a result when the CLI reports a rejected tool call yet exits 0.
    fn rejected_call_warning(&self, _output: &str) -> Option<&'static str> {
        None
    }

    /// A trust/approval screen a headless run printed before exiting; surfaced as a prompt.
    fn detect_headless_trust(&self, _output: &str) -> Option<Value> {
        None
    }

    /// True when `default_model` can answer (it may shell out, so callers run it off-thread).
    fn probes_default_model(&self) -> bool {
        false
    }

    /// The model the CLI uses when none is requested, for runs that never report one.
    fn default_model(&self) -> Option<String> {
        None
    }

    /// Warnings for a finished run, in addition to `rejected_call_warning`.
    fn run_warnings(&self, _facts: &RunFacts) -> Vec<Warning> {
        Vec::new()
    }

    // ---- visible pane -------------------------------------------------------------------

    /// Classify the last visible lines when this TUI has screens the generic prompt heuristic
    /// misreads. `None` falls through to the generic heuristic.
    fn classify_screen(&self, _visible_tail: &str) -> Option<PaneStatus> {
        None
    }

    /// A prompt blocking the pane, with the choices `prompt_reply` accepts.
    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<Value> {
        generic::detect_prompt(self.id(), screen, allow_broad)
    }

    fn prompt_reply(
        &self,
        prompt: &Value,
        choice: &str,
        allow_broad: bool,
    ) -> Result<Reply, String> {
        generic::prompt_reply(prompt, choice, allow_broad)
    }

    /// An open menu/selector screen (numbered yes/no, "enter submit"); used by wait `settled`.
    fn looks_like_menu(&self, _text: &str) -> bool {
        false
    }

    /// Input that empties the TUI's prompt box without side effects, when it has one.
    fn clear_input_sequence(&self) -> Option<&'static str> {
        None
    }

    /// Whether prompts typed into this TUI are verified on screen before Enter and a started turn
    /// is verified after it (`agent_runs::delivery`). Off by default: the check needs a TUI whose
    /// input box and transcript echo are readable from the screen.
    fn verifies_prompt_delivery(&self) -> bool {
        false
    }

    /// Whether `prompt_visible` is meaningful (shells only).
    fn prompt_visible(&self, _screen: &str) -> bool {
        false
    }

    // ---- turn-result capture from the screen -------------------------------------------

    /// True for a line that is TUI chrome (banner, footer, input placeholder), never reply text.
    /// `lower` is trimmed and lowercased.
    fn is_chrome_line(&self, _lower: &str) -> bool {
        false
    }

    /// True for a tool-activity row printed above a reply (shell header, hidden-output notice).
    fn is_tool_activity_line(&self, _lower: &str) -> bool {
        false
    }

    /// Remove boxed dialogs (trust, approval) from screen text. Their text is never reply
    /// evidence, and the box stays in the scrollback after it was answered.
    fn strip_dialogs(&self, text: &str) -> String {
        text.to_string()
    }

    /// Live model name shown in the TUI footer, when the TUI prints one.
    fn detect_footer_model(&self, _buffer: &str) -> Option<String> {
        None
    }

    // ---- capabilities -------------------------------------------------------------------

    /// Caveats appended to this backend's capability contract.
    fn capability_notes(&self, _has_pane: bool) -> Vec<String> {
        Vec::new()
    }
}

/// Registry order is the order `read_only_backends` reports; keep it stable.
static PROFILES: &[&dyn TuiProfile] = &[
    &claude::Claude,
    &codex::Codex,
    &opencode::OpenCode,
    &opencode::OpenCodeNative,
    &cursor::CursorAgent,
    &cursor::CursorIde,
    &shell::Cmd,
    &shell::Powershell,
    &shell::Bash,
];

static UNKNOWN: generic::Unknown = generic::Unknown;

#[cfg(test)]
pub fn profiles() -> &'static [&'static dyn TuiProfile] {
    PROFILES
}

/// Profile for an `agent_type`, case-insensitively; unregistered types get generic behaviour.
pub fn profile_for(agent_type: &str) -> &'static dyn TuiProfile {
    PROFILES
        .iter()
        .copied()
        .find(|profile| profile.id().eq_ignore_ascii_case(agent_type))
        .unwrap_or(&UNKNOWN)
}

pub fn is_registered(agent_type: &str) -> bool {
    PROFILES
        .iter()
        .any(|profile| profile.id().eq_ignore_ascii_case(agent_type))
}

/// A registered agent backend (not a plain shell) that `delegate_work` may be asked for.
pub fn is_agent_backend(agent_type: &str) -> bool {
    is_registered(agent_type) && profile_for(agent_type).features().kind != Kind::Shell
}

pub fn is_shell(agent_type: &str) -> bool {
    profile_for(agent_type).features().kind == Kind::Shell
}

/// Agent types whose headless run enforces `read_only`.
pub fn read_only_backends() -> Vec<&'static str> {
    PROFILES
        .iter()
        .filter(|profile| profile.features().read_only_headless)
        .map(|profile| profile.id())
        .collect()
}

/// Agent types whose run-spawned pane enforces `read_only`.
pub fn read_only_spawned_pane_backends() -> Vec<&'static str> {
    PROFILES
        .iter()
        .filter(|profile| profile.features().read_only_spawned_pane)
        .map(|profile| profile.id())
        .collect()
}

/// Headless command line for `agent_type`, after the checks every backend shares.
pub fn headless_launch(agent_type: &str, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    if launch.allow_shell && launch.read_only {
        return Err("allow_shell cannot be combined with read_only".into());
    }
    let profile = profile_for(agent_type);
    if launch.read_only && !profile.features().read_only_headless {
        return Err(format!(
            "READ_ONLY_UNSUPPORTED: read-only headless execution is unsupported for agent type: {agent_type}"
        ));
    }
    profile.headless_command(launch)
}

/// Whether any profile treats `text` as an open menu/selector screen.
pub fn looks_like_menu(text: &str) -> bool {
    PROFILES.iter().any(|profile| profile.looks_like_menu(text))
}

/// Whether any profile treats this (trimmed, lowercased) line as TUI chrome.
pub fn is_chrome_line(lower: &str) -> bool {
    PROFILES.iter().any(|profile| profile.is_chrome_line(lower))
}

/// Screen text with every profile's boxed dialogs removed.
pub fn strip_dialogs(text: &str) -> String {
    PROFILES
        .iter()
        .fold(text.to_string(), |acc, profile| profile.strip_dialogs(&acc))
}

/// Whether any profile treats this (trimmed, lowercased) line as a tool-activity row.
pub fn is_tool_activity_line(lower: &str) -> bool {
    PROFILES
        .iter()
        .any(|profile| profile.is_tool_activity_line(lower))
}

/// Whether an executable called `name` is on PATH (`name.exe` on Windows).
pub(crate) fn program_on_path(name: &str) -> bool {
    #[cfg(windows)]
    {
        find_path_file(&format!("{name}.exe")).is_some()
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
        })
    }
}

#[cfg(windows)]
pub(crate) fn find_path_file(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Program to spawn for a CLI called `name`. Windows needs the full path to the `.exe` because
/// ConPTY/CreateProcess does not search for it the way a shell does.
pub(crate) fn executable(name: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let file = format!("{name}.exe");
        find_path_file(&file)
            .map(|path| path.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{file} was not found on PATH"))
    }
    #[cfg(not(windows))]
    {
        Ok(name.to_string())
    }
}

#[cfg(test)]
mod conformance;
