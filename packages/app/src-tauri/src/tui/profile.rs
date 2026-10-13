//! The `TuiProfile` trait: every per-agent behaviour, each with a generic default.

use super::*;

/// One agent CLI/TUI. Every method has a generic default, so a profile overrides only what its
/// TUI does differently. Keep patterns, fixtures and tests for a TUI in that TUI's folder.
pub trait TuiProfile: Send + Sync {
    /// The `agent_type` string (matches `pty::agents::AgentType::as_str`).
    fn id(&self) -> &'static str;

    fn features(&self) -> Features {
        Features::CLI
    }

    /// This TUI reprints its whole conversation on every PTY resize without clearing scrollback
    /// (`ESC[3J`), so each resize leaves another copy in a viewer's history. Viewers wipe their
    /// scrollback at the resize point for such agents; plain shells and TUIs that only redraw
    /// their screen keep their history. Default false.
    fn reprints_on_resize(&self) -> bool {
        false
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

    // ---- visible pane spawn -------------------------------------------------------------

    /// Program, arguments and cwd for an interactive PTY pane. `None` (the default) keeps the
    /// code launch path (`pty::agents::resolve_command`), which only knows the built-in ids; a
    /// profile outside that set returns `Some` to be spawnable. The program runs directly (no
    /// shell); the spawn path adds the common PTY environment.
    fn spawn_command(&self, _request: &SpawnRequest) -> Option<LaunchSpec> {
        None
    }

    /// Re-checked at every spawn (APR-4, SEC-4): a definition-backed profile verifies that its file
    /// still has the approved fingerprint and that the binary is still the pinned one, and returns
    /// the reference to record on the run. Code profiles have nothing to check (`Ok(None)`).
    fn spawn_gate(&self, _project: &str) -> Result<Option<DefinitionRef>, GateError> {
        Ok(None)
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

    /// Why a headless turn that produced no result failed, when the profile can tell: the CLI
    /// reported an error itself (`AGENT_REPORTED_ERROR`, message preserved) or exited cleanly with
    /// output the definition cannot read (`AGENT_OUTPUT_UNPARSEABLE`). `None` keeps the generic
    /// failure handling.
    fn classify_headless_failure(&self, _output: &str, _exit_code: i32) -> Option<(&'static str, String)> {
        None
    }

    /// True when `headless_command` passes `HeadlessLaunch::model` to the CLI (a per-run model).
    fn accepts_headless_model(&self) -> bool {
        false
    }

    /// Final machine-readable result after a successful exit; never a human-readable fallback.
    fn extract_result(&self, _output: &str, _exit_code: i32) -> Option<String> {
        None
    }

    /// True once the CLI's output already holds its terminal result, so the turn is over even if
    /// the child process lingers instead of exiting.
    fn headless_result_ready(&self, _output: &str) -> bool {
        false
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

    /// The model a headless run reports in its own structured output (a confirmation, unlike
    /// `default_model`). `None` when the CLI does not say.
    fn extract_model(&self, _output: &str) -> Option<String> {
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

    // ---- warm standby (see `crate::standby`) --------------------------------------------

    /// Whether this backend can be pre-launched and handed its task later.
    fn standby_support(&self) -> StandbySupport {
        StandbySupport::default()
    }

    /// What a pre-launched TUI pane's screen shows. A standby is only handed out while this is
    /// `Ready`; dialogs are never answered by the pool.
    fn standby_screen(&self, _screen: &str) -> StandbyScreen {
        StandbyScreen::NotReady
    }

    /// Command line of a pre-launched headless process that waits for its task on stdin. The launch
    /// carries every option fixed at spawn (read-only, shell, model); the task is not known yet.
    /// `None` when the CLI has no stdin task mode.
    fn headless_standby_command(&self, _launch: &HeadlessLaunch) -> Option<Result<LaunchSpec, String>> {
        None
    }

    /// The bytes that hand `task` to a process started by `headless_standby_command`; the pool
    /// closes stdin after writing them.
    fn headless_standby_input(&self, _task: &str) -> Option<String> {
        None
    }

    // ---- visible pane -------------------------------------------------------------------

    /// How long a running pane's screen must hold still before it reads `idle`. The default is
    /// conservative; a TUI whose busy states always animate (a spinner, a timer) can shorten it so
    /// a finished turn is noticed sooner. Completion detection waits for `idle`.
    fn idle_after(&self) -> std::time::Duration {
        crate::pty::registry::DEFAULT_IDLE_AFTER
    }

    /// Whether the last visible lines show the TUI actively working (a spinner, a stop hint):
    /// `Some(true)`, `Some(false)` when it provably is not, `None` when the profile cannot tell.
    fn busy_marker(&self, _visible_tail: &str) -> Option<bool> {
        None
    }

    /// The shorter window used instead of `idle_after` while `busy_marker` says `Some(false)`.
    /// A screen with no sign of work that holds still this long is a finished turn.
    fn quiet_idle_after(&self) -> std::time::Duration {
        self.idle_after()
    }

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

    /// Labelled "experimental" in every surface (GEN-3): generic zero-config agents.
    fn experimental(&self) -> bool {
        false
    }

    /// A right-hand sidebar the screen readers should crop away (see `SidebarCrop`).
    fn sidebar_crop(&self) -> Option<crate::tui::types::SidebarCrop> {
        None
    }

    /// Remove boxed dialogs (trust, approval) from screen text. Their text is never reply
    /// evidence, and the box stays in the scrollback after it was answered.
    fn strip_dialogs(&self, text: &str) -> String {
        text.to_string()
    }

    /// Final pass over text extracted from the screen as the reply, after chrome and tool rows
    /// were removed: drop artifacts of how this TUI redraws (repeated lines).
    fn tidy_reply(&self, text: &str) -> String {
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

    /// Capabilities, model detection and routing notes for `list_agent_contexts`. `None` (no
    /// context) keeps the agent out of that list. The default is the code table in
    /// `agent_contexts::profiles`, keyed by `id()`.
    fn context(&self) -> Option<ContextProfile> {
        crate::agent_contexts::context_profile(self.id())
    }
}

/// What `TuiProfile::context` returns.
pub type ContextProfile = crate::agent_contexts::AgentContextProfile;

/// An agent id (`agent_type` string) outside the spawn/launch layer, where `AgentType` stays
/// (SPEC REG-5). Lowercase; serialises as the bare string, exactly like `AgentType` did.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(transparent)]
pub struct AgentId(Arc<str>);

impl AgentId {
    pub fn new(id: &str) -> Self {
        if id.bytes().any(|b| b.is_ascii_uppercase()) {
            Self(Arc::from(id.to_ascii_lowercase()))
        } else {
            Self(Arc::from(id))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for AgentId {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for AgentId {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}
