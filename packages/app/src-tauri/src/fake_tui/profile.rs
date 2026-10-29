//! The dev-only agent types `fake_claude` and `fake_codex`.
//!
//! A `FakeAgent` is the REAL `Claude` / `Codex` profile with two differences: its id, and the
//! program it launches (`pm-fake-tui`, under a PowerShell host shell exactly like the real agents'
//! panes). Every screen-reading method is forwarded to the real profile, so what the end-to-end
//! tests exercise is the real recogniser, not a copy of it. Generic code never matches on the id.

use crate::pty::status::PaneStatus;
use crate::tui::{
    Activity, AccountScheme, ComposerState, DefinitionRef, Features, GateError, HeadlessLaunch, InstructionFile,
    LaunchSpec, LinuxLaunch, LaunchSession, Reply, RunFacts, RunOptionSupport, SidebarCrop, SpawnRequest, StandbyScreen, StandbySupport,
    StartupGate, SubmitSequence, TestedRange, TuiProfile, VersionCommand, Warning,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

pub struct FakeAgent {
    id: &'static str,
    inner: Arc<dyn TuiProfile>,
    exe: PathBuf,
    /// The script the NEXT spawned pane plays (set by the session before each run).
    script: Arc<parking_lot::Mutex<Option<PathBuf>>>,
}

impl FakeAgent {
    pub fn new(
        id: &'static str,
        inner: Arc<dyn TuiProfile>,
        exe: PathBuf,
        script: Arc<parking_lot::Mutex<Option<PathBuf>>>,
    ) -> Self {
        Self { id, inner, exe, script }
    }
}

/// The script this agent plays: `script.<id>.toml` beside the shared script when a test wrote one for it
/// (a night with both fakes needs two layouts at once), else the shared one.
fn own_script(shared: &std::path::Path, id: &str) -> PathBuf {
    let own = shared.with_file_name(format!("script.{id}.toml"));
    if own.is_file() {
        own
    } else {
        shared.to_path_buf()
    }
}

fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

impl TuiProfile for FakeAgent {
    fn id(&self) -> &'static str {
        self.id
    }

    /// A CLI with a visible pane and no headless mode: the fake only has a TUI.
    fn features(&self) -> Features {
        Features::CLI
    }

    fn reprints_on_resize(&self) -> bool {
        self.inner.reprints_on_resize()
    }

    fn checkpoints(&self) -> bool {
        self.inner.checkpoints()
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        self.inner.instruction_files()
    }

    fn run_options(&self) -> RunOptionSupport {
        RunOptionSupport::default()
    }

    fn account_scheme(&self) -> Option<AccountScheme> {
        None
    }

    fn is_installed(&self) -> bool {
        self.exe.is_file()
    }

    /// `pm-fake-tui --version --script <script>` once a session has written a script, so the canary's
    /// end-to-end test reads the version line the script plays (`version_output`) the way it reads a
    /// real CLI's. Without a script there is nothing to ask.
    fn version_command(&self) -> Option<VersionCommand> {
        let script = self.script.lock().clone()?;
        Some(VersionCommand {
            program: self.exe.to_string_lossy().into_owned(),
            args: vec!["--version".into(), "--script".into(), script.to_string_lossy().into_owned()],
        })
    }

    /// The fake's "tested" range: version 1.x. A script that prints version 2 is an update the profile
    /// was not tested with.
    fn tested_versions(&self) -> Option<TestedRange> {
        TestedRange::parse(">=1.0, <2.0").ok()
    }

    fn risk_rules(&self) -> Vec<crate::prompt_risk::RiskRule> {
        self.inner.risk_rules()
    }

    /// `powershell -NoLogo -NoExit -Command "& 'pm-fake-tui.exe' --script '<script>'"`: the shape of
    /// `pty::registry::command::powershell_host_command`, so the host shell's prompt is what a pane
    /// shows when the fake exits or crashes, as for a real agent.
    fn spawn_command(&self, request: &SpawnRequest) -> Option<LaunchSpec> {
        let mut invocation = format!("& {}", ps_quote(&self.exe.to_string_lossy()));
        if let Some(script) = self.script.lock().as_ref() {
            let script = own_script(script, self.id);
            invocation.push_str(&format!(" --script {}", ps_quote(&script.to_string_lossy())));
        }
        Some(LaunchSpec::new(
            "powershell.exe",
            vec!["-NoLogo".into(), "-NoExit".into(), "-Command".into(), invocation],
            PathBuf::from(request.cwd),
            false,
        ))
    }

    /// The real profile's Linux launch (a bare program name and its flags): the fake only changes what runs
    /// at the end of it, `pm-fake-wsl` swaps the program for `pm-fake-tui`.
    fn linux_launch(&self, request: &SpawnRequest) -> Option<LinuxLaunch> {
        self.inner.linux_launch(request)
    }

    /// A pane that runs in Linux is launched with the real profile's session id (`--session-id <id>`), so its
    /// Chat tab finds the session file by name. Windows panes of the fake keep launching without one.
    fn launch_session(&self, user_args: &[String], fresh_id: &str) -> LaunchSession {
        let in_linux = crate::wsl::settings::Store::at(&crate::app_paths::app_data_dir()).get(self.id).is_some();
        if in_linux {
            self.inner.launch_session(user_args, fresh_id)
        } else {
            LaunchSession::Unmanaged
        }
    }

    fn spawn_gate(&self, _project: &str) -> Result<Option<DefinitionRef>, GateError> {
        Ok(None)
    }

    fn idle_after(&self) -> std::time::Duration {
        self.inner.idle_after()
    }

    /// The real profile's usage-limit rules: without them a fake pane that draws a limit is never
    /// seen to be at one, and the run settles as `completed` on the notice.
    fn limits(&self) -> Option<&crate::handoff::CompiledLimits> {
        self.inner.limits()
    }

    fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        self.inner.busy_marker(visible_tail)
    }

    fn quiet_idle_after(&self) -> std::time::Duration {
        self.inner.quiet_idle_after()
    }

    fn activity(&self, visible_tail: &str, status: PaneStatus) -> Option<Activity> {
        self.inner.activity(visible_tail, status)
    }

    fn is_error_line(&self, line: &str) -> bool {
        self.inner.is_error_line(line)
    }

    fn classify_screen(&self, visible_tail: &str) -> Option<PaneStatus> {
        self.inner.classify_screen(visible_tail)
    }

    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<Value> {
        let mut prompt = self.inner.detect_prompt(screen, allow_broad)?;
        // The real profile stamps its own id; the pane's agent type is this one.
        if prompt.get("agent_type").is_some() {
            prompt["agent_type"] = Value::String(self.id.to_string());
        }
        Some(prompt)
    }

    fn prompt_reply(&self, prompt: &Value, choice: &str, allow_broad: bool) -> Result<Reply, String> {
        self.inner.prompt_reply(prompt, choice, allow_broad)
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        self.inner.looks_like_menu(text)
    }

    fn clear_input_sequence(&self) -> Option<&'static str> {
        self.inner.clear_input_sequence()
    }

    fn verifies_prompt_delivery(&self) -> bool {
        self.inner.verifies_prompt_delivery()
    }

    fn submit_sequence(&self) -> SubmitSequence {
        self.inner.submit_sequence()
    }

    fn composer_state(&self, screen: &str) -> ComposerState {
        self.inner.composer_state(screen)
    }

    fn reads_composer(&self) -> bool {
        self.inner.reads_composer()
    }

    fn ready_recogniser(&self, screen: &str) -> Option<ComposerState> {
        self.inner.ready_recogniser(screen)
    }

    fn input_screen(&self, screen: &vt100::Screen) -> String {
        self.inner.input_screen(screen)
    }

    fn startup_gate(&self, screen: &str) -> Option<StartupGate> {
        self.inner.startup_gate(screen)
    }

    fn prompt_visible(&self, screen: &str) -> bool {
        self.inner.prompt_visible(screen)
    }

    fn is_chrome_line(&self, lower: &str) -> bool {
        self.inner.is_chrome_line(lower)
    }

    fn is_tool_activity_line(&self, lower: &str) -> bool {
        self.inner.is_tool_activity_line(lower)
    }

    fn is_thinking_line(&self, lower: &str) -> bool {
        self.inner.is_thinking_line(lower)
    }

    fn sidebar_crop(&self) -> Option<SidebarCrop> {
        self.inner.sidebar_crop()
    }

    fn side_panel(&self) -> bool {
        self.inner.side_panel()
    }

    fn strip_dialogs(&self, text: &str) -> String {
        self.inner.strip_dialogs(text)
    }

    fn tidy_reply(&self, text: &str) -> String {
        self.inner.tidy_reply(text)
    }

    fn detect_footer_model(&self, buffer: &str) -> Option<String> {
        self.inner.detect_footer_model(buffer)
    }

    fn footer_model_readings(&self, buffer: &str) -> Vec<crate::tui::ModelReading> {
        self.inner.footer_model_readings(buffer)
    }

    fn is_summary_line(&self, lower: &str) -> bool {
        self.inner.is_summary_line(lower)
    }

    fn user_message_row(&self, row: &str) -> Option<String> {
        self.inner.user_message_row(row)
    }

    fn reads_user_messages(&self) -> bool {
        self.inner.reads_user_messages()
    }

    fn run_warnings(&self, facts: &RunFacts) -> Vec<Warning> {
        self.inner.run_warnings(facts)
    }

    fn standby_support(&self) -> StandbySupport {
        StandbySupport::default()
    }

    fn standby_screen(&self, _screen: &str) -> StandbyScreen {
        StandbyScreen::NotReady
    }

    fn headless_command(&self, _launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        Err(format!("{} has no headless mode", self.id))
    }

    fn capability_notes(&self, has_pane: bool) -> Vec<String> {
        self.inner.capability_notes(has_pane)
    }
}
