//! Data types the profiles and the generic machinery exchange (kinds, features, launch specs, replies, warnings).

use super::*;
use serde::{Deserialize, Serialize};

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

impl Kind {
    /// Wire name used by `GET /agents/types`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Cli => "cli",
            Kind::NativeProtocol => "native_protocol",
            Kind::Shell => "shell",
            Kind::Unknown => "unknown",
        }
    }
}

/// How to ask an agent CLI for its version: a resolved program plus arguments (agent doctor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCommand {
    pub program: String,
    pub args: Vec<String>,
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
#[derive(Debug, Clone, Copy, Default, Serialize)]
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

/// Which kinds of warm standby worker (`crate::standby`) a backend supports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StandbySupport {
    /// A TUI pane can be started ahead of time and given its task later.
    pub visible: bool,
    /// A headless process can be started ahead of time and given its task over stdin later.
    pub headless: bool,
    /// The visible standby has no agent-specific ready screen (no `[tui.ready]`): readiness comes
    /// from the generic screen heuristics and from the screen holding still (`standby::stillness`).
    pub experimental: bool,
}

/// What a standby pane's screen shows, for the claim-time health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandbyScreen {
    /// An empty input box with no dialog on top of it: a task can be typed now.
    Ready,
    /// A trust or approval dialog is up. A standby never answers one: trust is the caller's call.
    Dialog,
    /// A login, update or first-run screen the user has to deal with.
    NeedsAttention,
    /// Still starting, or showing something this profile does not recognise as ready.
    NotReady,
}

/// How a TUI turns typed text into a submitted prompt (`TuiProfile::submit_sequence`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmitSequence {
    /// Least time between the last typed character and the submit keys. Some TUIs treat a burst of
    /// characters as a paste and read an Enter that follows within a short window as a line break
    /// in the pasted text; Enter sent inside that window does not submit. Zero leaves the generic
    /// pause (`pty::registry::submit_delay`) alone.
    pub settle: std::time::Duration,
    /// What is written to submit: Enter for every TUI seen so far.
    pub keys: &'static str,
}

impl SubmitSequence {
    pub const ENTER: SubmitSequence = SubmitSequence {
        settle: std::time::Duration::ZERO,
        keys: "\r",
    };
}

/// What a TUI's input box holds, read from the screen. Prompt delivery uses it to confirm that
/// Enter submitted instead of leaving the text waiting (`TuiProfile::composer_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerState {
    /// This profile cannot tell from the screen.
    Unknown,
    /// The input box is empty and no turn is running.
    Empty,
    /// Text is typed in the input box and not submitted.
    HasText,
    /// A turn is running (the TUI shows its working marker).
    Working,
}

/// A screen a freshly started TUI shows before it takes a prompt: an update notice, a trust
/// question, a login or onboarding page (`TuiProfile::startup_gate`). Typing a task into one loses
/// the task, answers the screen with the task's letters, or (worse) ends a run with the screen as
/// its "answer".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupGate {
    /// Stable machine name, e.g. `update_available`.
    pub id: &'static str,
    /// One plain sentence for what the user would see.
    pub summary: &'static str,
    /// Keys that close the screen WITHOUT choosing anything that changes the machine or grants
    /// trust (a "skip" for a notice). `None` means only the user may move past it: Puppet Master
    /// parks the run and shows the screen instead of answering for them.
    pub dismiss: Option<&'static str>,
}

impl StartupGate {
    /// The id of the folder-trust gate: its prompt kind is `workspace_trust`.
    pub const WORKSPACE_TRUST: &'static str = "workspace_trust";

    /// The kind of the prompt card a gate that only the user may pass becomes (`pane_prompt`):
    /// `workspace_trust` for the folder-trust question, `startup_screen` for any other (sign-in,
    /// onboarding). A dismissible gate is closed with its keys and never becomes a card.
    pub fn prompt_kind(&self) -> &'static str {
        if self.id == Self::WORKSPACE_TRUST {
            "workspace_trust"
        } else {
            "startup_screen"
        }
    }
}

/// A file an agent reads its project instructions from (`TuiProfile::instruction_files`). Puppet
/// Master keeps the shared project memory inside a managed block of each such file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    /// Relative to the project root, forward slashes (`AGENTS.md`, `.cursor/rules/x.mdc`).
    pub path: String,
    /// Text written once at the top when Puppet Master creates the file (for formats that need
    /// front-matter). Never added to a file that already exists.
    pub header: Option<String>,
}

impl InstructionFile {
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string(), header: None }
    }
    pub fn with_header(path: &str, header: &str) -> Self {
        Self { path: path.to_string(), header: Some(header.to_string()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// True only where the CLI exposes a documented read-only execution policy.
    pub read_only_enforced: bool,
    /// Environment variables set for the child, on top of the inherited environment (the TOML
    /// `[env] set` table, for definitions). Every launch path applies them.
    pub env: Vec<(String, String)>,
    /// Variables removed from the child's environment (the TOML `[env] unset` list).
    pub env_remove: Vec<String>,
}

impl LaunchSpec {
    /// A launch with no environment changes.
    pub fn new(program: impl Into<String>, args: Vec<String>, cwd: PathBuf, read_only_enforced: bool) -> Self {
        Self { program: program.into(), args, cwd, read_only_enforced, env: Vec::new(), env_remove: Vec::new() }
    }

    /// Apply `env` then `env_remove` to a std `Command` (headless runs and standby processes).
    pub fn apply_to_command(&self, command: &mut std::process::Command) {
        for (key, value) in &self.env {
            command.env(key, value);
        }
        for key in &self.env_remove {
            command.env_remove(key);
        }
    }
}

/// Environment a launch gets on top of the profile's own: what pointing an agent at a user
/// account adds (`accounts::launch_env`). Applied after `LaunchSpec::env`, so it wins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchEnv {
    pub set: Vec<(String, String)>,
    pub remove: Vec<String>,
}

/// How an agent is pointed at a different account: declared by the profile, never matched by
/// agent name in generic code. Only declare one where the env var is documented by the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountScheme {
    /// The env var naming the directory that holds the agent's config and login (`CODEX_HOME`).
    pub home_env: String,
    /// Further variables every account of this agent gets.
    pub extra_env: Vec<(String, String)>,
    /// Plain-language steps for the one-time login in an account's home.
    pub login_hint: String,
}

/// A TUI that draws a fixed-width sidebar on the right once the pane is wider than `min_cols`
/// (Kilo: more than 120 columns). Screen text readers drop the rightmost `width` columns of every
/// row, so the sidebar's text never joins the reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarCrop {
    pub min_cols: u16,
    pub width: u16,
}

/// What a visible pane spawn hands `TuiProfile::spawn_command`.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // no code profile reads size/read_only yet; declarative profiles will (T16)
pub struct SpawnRequest<'a> {
    pub cwd: &'a str,
    /// Caller-supplied arguments appended after the profile's own.
    pub extra_args: &'a [String],
    pub cols: u16,
    pub rows: u16,
    /// The pane was asked to start with enforced read-only permissions.
    pub read_only: bool,
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

/// Which definition a run was started from (SPEC OBS-2), recorded on the run and in its receipt so a
/// result can be tied to the exact file contents and binary that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionRef {
    pub id: String,
    pub source: String,
    pub fingerprint: String,
    /// Absolute path of the binary the approval pinned (absent for bundled definitions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
}

/// A spawn refused before anything started: SPEC 7.4 code and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateError {
    pub code: &'static str,
    pub message: String,
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

/// How a run of this agent can be shown: headless child process and/or visible TUI pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Modes {
    pub headless: bool,
    pub visible: bool,
}

/// The `Features` flags listed by `GET /agents/types`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FeatureFlags {
    pub read_only_headless: bool,
    pub read_only_spawned_pane: bool,
    pub resumable_headless: bool,
}

/// One registered agent as listed by `GET /agents/types` (SPEC 7.5). Built from the profile only;
/// declarative-definition fields (`definition`, non-code `source`) arrive with the registry.
#[derive(Debug, Clone, Serialize)]
pub struct Descriptor {
    pub id: &'static str,
    pub label: String,
    pub icon: Option<String>,
    pub kind: &'static str,
    pub source: &'static str,
    pub experimental: bool,
    pub dispatchable: bool,
    pub not_dispatchable_reason: Option<&'static str>,
    pub installed: bool,
    pub install_hint: Option<String>,
    pub login_hint: Option<String>,
    pub modes: Modes,
    pub features: FeatureFlags,
    pub run_options: RunOptionSupport,
    pub standby: StandbySupport,
    pub definition: Option<Value>,
}

impl Descriptor {
    pub fn of(profile: &dyn TuiProfile) -> Self {
        let id = profile.id();
        let features = profile.features();
        let label = crate::pty::agents::AgentType::parse(id)
            .map(|agent| crate::agent_contexts::get_agent_context_profile(agent).label.to_string())
            .unwrap_or_else(|| id.to_string());
        Self {
            id,
            label,
            icon: None,
            kind: features.kind.as_str(),
            source: "code",
            experimental: profile.experimental(),
            dispatchable: features.dispatchable,
            not_dispatchable_reason: (!features.dispatchable)
                .then(|| profile.not_dispatchable_reason()),
            installed: profile.is_installed(),
            install_hint: profile.install_hint(),
            login_hint: profile.login_hint(),
            modes: Modes {
                headless: supports_headless(profile),
                visible: true,
            },
            features: FeatureFlags {
                read_only_headless: features.read_only_headless,
                read_only_spawned_pane: features.read_only_spawned_pane,
                resumable_headless: features.resumable_headless,
            },
            run_options: profile.run_options(),
            standby: profile.standby_support(),
            definition: None,
        }
    }
}

/// A dispatchable CLI profile that implements `headless_command`. Probing builds the command line
/// only (no process starts); a missing binary still counts as headless-capable, since `installed`
/// reports that separately. Only the trait's default "unsupported" error means no headless mode.
fn supports_headless(profile: &dyn TuiProfile) -> bool {
    let features = profile.features();
    if !features.dispatchable || features.kind != Kind::Cli {
        return false;
    }
    let probe = HeadlessLaunch {
        project: ".",
        task: "probe",
        read_only: false,
        resume_session: None,
        trust_workspace: false,
        allow_shell: false,
        model: None,
    };
    match profile.headless_command(&probe) {
        Ok(_) => true,
        Err(error) => {
            error != format!("headless launch is unsupported for agent type: {}", profile.id())
        }
    }
}
