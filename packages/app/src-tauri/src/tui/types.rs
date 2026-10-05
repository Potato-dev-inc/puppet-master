//! Data types the profiles and the generic machinery exchange (kinds, features, launch specs, replies, warnings).

use super::*;

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
    /// Shell commands already run without opting in (OpenCode native), so `allow_shell` is an
    /// accepted no-op there (still never with `read_only`).
    pub shell_default_on: bool,
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
