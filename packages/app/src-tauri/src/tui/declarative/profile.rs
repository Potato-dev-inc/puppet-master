//! `DeclarativeProfile`: a validated `Definition` behind the `TuiProfile` trait (headless half).
//!
//! Headless methods map onto `[headless]` (argv via `template`, parsing via `output`), the binary
//! via `resolve`; everything screen-shaped goes through the `[tui]` table engine (`screen`).
//! Constructible from any parsed definition; nothing here loads files or registers anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::agent_contexts::{AgentCapability, AgentContextProfile, ModelDetection};
use crate::standby::key::canonical_project;
use crate::tui::types::{
    DefinitionRef, Features, GateError, HeadlessLaunch, Kind, LaunchSpec, RunOptionSupport,
    SpawnRequest, StandbySupport,
};
use crate::pty::status::PaneStatus;
use crate::tui::types::{Reply, StandbyScreen};
use crate::tui::{generic, AgentId, TuiProfile};
use serde_json::Value;

use super::approvals::{ApprovalState, Approvals};
use super::loader::load_text;
use super::output::OutputParser;
use super::sources::{source_name, Origin};
use super::validate::Source;
use super::resolve::resolve_binary;
use super::screen::TuiEngine;
use super::schema;
use super::schema::{Cwd, DefError, Definition, ModelSource};
use super::template;

static INTERNED: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// `TuiProfile` returns `&'static str` for ids and warning codes; declarative values are runtime
/// strings. Each distinct string is leaked once (a handful of short strings per definition, and
/// re-loading the same text adds nothing).
fn intern(text: &str) -> &'static str {
    let mut set = INTERNED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = set.get(text) {
        return hit;
    }
    let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// Where a registered definition came from, kept so every spawn can re-verify it (APR-4, SEC-4).
#[derive(Debug, Clone)]
pub struct DefinitionOrigin {
    pub source: Source,
    /// The fingerprint that was approved and registered.
    pub fingerprint: String,
    pub path: PathBuf,
    /// Canonical project path of a project definition.
    pub project: Option<String>,
}

pub struct DeclarativeProfile {
    origin: Option<DefinitionOrigin>,
    def: Definition,
    id: &'static str,
    not_dispatchable_reason: &'static str,
    warning_code: Option<&'static str>,
    parser: Option<OutputParser>,
    engine: Option<TuiEngine>,
}

impl DeclarativeProfile {
    /// Build from a definition that passed `validate`. Fails only when a selector regex does not
    /// compile (which `validate` already rules out).
    pub fn new(mut def: Definition) -> Result<Self, DefError> {
        // GEN-1/GEN-2: a generic agent has no tables of its own; an empty `[tui]` (no argv, no
        // regexes) makes it a visible pane that runs on the shared heuristics.
        if def.agent.kind == schema::Kind::Generic && def.tui.is_none() {
            def.tui = Some(Default::default());
        }
        let parser = match def.headless.as_ref().filter(|h| h.output.is_some()) {
            Some(headless) => Some(OutputParser::compile(headless)?),
            None => None,
        };
        let warning_code = def
            .headless
            .as_ref()
            .and_then(|h| h.output.as_ref())
            .and_then(|o| o.rejected.as_ref())
            .and_then(|r| r.warning_code.as_deref())
            .map(intern);
        let engine = match &def.tui {
            Some(tui) => {
                let clear = tui.clear_input_sequence.as_deref().or(tui.clear_input_keys.as_deref());
                Some(TuiEngine::compile(&def.agent.id, tui, clear.map(intern))?)
            }
            None => None,
        };
        Ok(Self {
            engine,
            origin: None,
            id: intern(&def.agent.id),
            not_dispatchable_reason: intern(&def.features.not_dispatchable_reason),
            warning_code,
            parser,
            def,
        })
    }

    pub fn definition(&self) -> &Definition {
        &self.def
    }

    /// Bind the file this profile was loaded from; without it `spawn_gate` has nothing to verify.
    pub fn with_origin(mut self, origin: DefinitionOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// The spawn-time check against explicit approvals (pure apart from reading the file and
    /// resolving the binary): the file still fingerprints as registered and is still approved
    /// under that fingerprint, and the binary resolves to the path pinned at approval.
    pub(crate) fn gate_with(
        &self,
        approvals: &Approvals,
        code_ids: &[&str],
        project: &str,
    ) -> Result<Option<DefinitionRef>, GateError> {
        let Some(origin) = &self.origin else { return Ok(None) };
        let mut reference = DefinitionRef {
            id: self.id.to_string(),
            source: source_name(origin.source).to_string(),
            fingerprint: origin.fingerprint.clone(),
            binary: None,
        };
        if origin.source == Source::Bundled {
            return Ok(Some(reference));
        }
        // SRC-7: a project definition runs only for the project it was consented for.
        if origin.source == Source::Project
            && origin.project.as_deref().map(canonical_project) != Some(canonical_project(project))
        {
            return Err(GateError {
                code: "AGENT_DEF_NO_CONSENT",
                message: format!("{} belongs to another project and is not available in {project}", self.id),
            });
        }
        let stale = |message: String| GateError { code: "AGENT_DEF_APPROVAL_STALE", message };
        let text = std::fs::read_to_string(&origin.path).map_err(|e| {
            stale(format!("definition file {} cannot be read: {e}", origin.path.display()))
        })?;
        let file = load_text(
            &origin.path,
            &text,
            text.len(),
            Origin::new(origin.source),
            origin.project.as_deref(),
            code_ids,
        );
        let current = match &file.fingerprint {
            Some(current) => current.clone(),
            None => {
                let first = file.errors.first().map(|e| e.to_string()).unwrap_or_default();
                return Err(stale(format!("definition file changed and is no longer valid: {first}")));
            }
        };
        if current != origin.fingerprint {
            return Err(stale(format!(
                "definition {} changed after it was approved; review and approve it again",
                self.id
            )));
        }
        let record = match approvals.state(&origin.path, &current) {
            ApprovalState::Approved => approvals.get(&origin.path),
            ApprovalState::Stale => {
                return Err(stale(format!("approval of {} is stale; approve it again", self.id)))
            }
            ApprovalState::Unapproved => {
                return Err(GateError {
                    code: "AGENT_DEF_UNAPPROVED",
                    message: format!("{} is not approved (or the approval was revoked)", self.id),
                })
            }
        };
        let resolved = resolve_binary(&self.def.binary, Path::new(project))
            .map_err(|e| GateError { code: e.code(), message: e.to_string() })?;
        if let Some(record) = record {
            if canonical_project(&resolved.to_string_lossy()) != canonical_project(&record.binary) {
                return Err(GateError {
                    code: "AGENT_DEF_BINARY_CHANGED",
                    message: format!(
                        "{} now resolves to {} but {} was approved",
                        self.id,
                        resolved.display(),
                        record.binary
                    ),
                });
            }
        }
        reference.binary = Some(resolved.to_string_lossy().into_owned());
        Ok(Some(reference))
    }

    fn has_segment(&self, when: &str) -> bool {
        self.def.headless.as_ref().is_some_and(|h| h.optional.iter().any(|s| s.when == when))
    }

    fn has_stdin(&self) -> bool {
        self.def.headless.as_ref().is_some_and(|h| h.stdin.is_some())
    }

    /// The launch for an already resolved `program`: rendered argv (the task is never part of it
    /// for stdin delivery) and the cwd the definition asks for.
    pub(crate) fn headless_spec(
        &self,
        launch: &HeadlessLaunch<'_>,
        program: PathBuf,
    ) -> Result<LaunchSpec, String> {
        let headless = self
            .def
            .headless
            .as_ref()
            .ok_or_else(|| format!("headless launch is unsupported for agent type: {}", self.id))?;
        let cwd = match headless.cwd {
            Cwd::Project => PathBuf::from(launch.project),
            Cwd::Inherit => std::env::current_dir()
                .map_err(|e| format!("cannot read the current directory: {e}"))?,
        };
        Ok(LaunchSpec {
            program: program.to_string_lossy().into_owned(),
            args: template::render(headless, launch),
            cwd,
            read_only_enforced: launch.read_only && self.def.features.read_only_headless,
        })
    }

    /// The pane launch for an already resolved `program`.
    pub(crate) fn tui_spec(&self, tui: &super::schema::TuiTable, request: &SpawnRequest<'_>, program: PathBuf) -> LaunchSpec {
        let mut args = template::render_tui(tui, request.cwd, request.read_only, None, None);
        args.extend(request.extra_args.iter().cloned());
        LaunchSpec {
            program: program.to_string_lossy().into_owned(),
            args,
            cwd: PathBuf::from(request.cwd),
            read_only_enforced: request.read_only && self.def.features.read_only_spawned_pane,
        }
    }

    fn resolve_program(&self, project: &str) -> Result<PathBuf, String> {
        resolve_binary(&self.def.binary, Path::new(project))
            .map_err(|e| format!("{}: {e}", e.code()))
    }
}

fn capability(name: &str) -> Option<AgentCapability> {
    Some(match name {
        "codebase-reasoning" => AgentCapability::CodebaseReasoning,
        "implementation" => AgentCapability::Implementation,
        "review" => AgentCapability::Review,
        "debugging" => AgentCapability::Debugging,
        "terminal-ops" => AgentCapability::TerminalOps,
        "ui-orchestration" => AgentCapability::UiOrchestration,
        _ => return None,
    })
}

impl TuiProfile for DeclarativeProfile {
    fn id(&self) -> &'static str {
        self.id
    }

    fn features(&self) -> Features {
        let f = &self.def.features;
        let has_session = self
            .def
            .headless
            .as_ref()
            .and_then(|h| h.output.as_ref())
            .is_some_and(|o| o.session.is_some());
        Features {
            // `generic` definitions behave as CLIs; native_protocol/shell are rejected by validate.
            kind: Kind::Cli,
            // GEN-1: a generic agent is dispatchable only when it is marked experimental.
            dispatchable: f.dispatchable && (self.def.agent.kind != schema::Kind::Generic || self.def.agent.experimental),
            read_only_headless: f.read_only_headless,
            read_only_spawned_pane: f.read_only_spawned_pane,
            resumable_headless: f
                .resumable_headless
                .unwrap_or(self.has_segment("resume") && has_session),
        }
    }

    fn reprints_on_resize(&self) -> bool {
        self.def.features.reprints_on_resize
    }

    fn experimental(&self) -> bool {
        self.def.agent.experimental || self.def.agent.kind == schema::Kind::Generic
    }

    fn not_dispatchable_reason(&self) -> &'static str {
        if self.def.agent.kind == schema::Kind::Generic && !self.def.agent.experimental {
            return "a generic agent can run delegated work only when it is marked experimental = true";
        }
        if self.not_dispatchable_reason.is_empty() {
            "this backend cannot run delegated work"
        } else {
            self.not_dispatchable_reason
        }
    }

    fn run_options(&self) -> RunOptionSupport {
        let r = &self.def.run_options;
        RunOptionSupport {
            trust_workspace: r.trust_workspace,
            allow_shell: r.allow_shell,
            shell_default_on: r.shell_default_on,
            allowed_commands: r.allowed_commands,
        }
    }

    /// Visible pane launch (TUI-1): resolved program + `[tui]` argv + the caller's extra args, in the
    /// project directory. `None` without a `[tui]` table or when the binary does not resolve (the
    /// spawn path then reports that no launch command exists).
    fn spawn_command(&self, request: &SpawnRequest) -> Option<LaunchSpec> {
        let tui = self.def.tui.as_ref()?;
        let program = self.resolve_program(request.cwd).ok()?;
        Some(self.tui_spec(tui, request, program))
    }

    fn spawn_gate(&self, project: &str) -> Result<Option<DefinitionRef>, GateError> {
        if self.origin.is_none() {
            return Ok(None);
        }
        let code_ids = crate::tui::registry::registry().code_ids();
        self.gate_with(&Approvals::load(&super::approvals::default_path()), &code_ids, project)
    }

    fn classify_headless_failure(&self, output: &str, exit_code: i32) -> Option<(&'static str, String)> {
        let parser = self.parser.as_ref()?;
        if let Some(message) = parser.error_message(output) {
            return Some(("AGENT_REPORTED_ERROR", message));
        }
        (exit_code == 0).then(|| {
            (
                "AGENT_OUTPUT_UNPARSEABLE",
                format!("{} exited cleanly but no result could be read from its output", self.id),
            )
        })
    }

    fn is_installed(&self) -> bool {
        let here = std::env::current_dir().unwrap_or_default();
        resolve_binary(&self.def.binary, &here).is_ok()
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        if self.def.headless.is_none() {
            return Err(format!("headless launch is unsupported for agent type: {}", self.id));
        }
        if self.has_stdin() {
            // LaunchSpec has no stdin channel; stdin delivery runs through the standby path until
            // the run integration (T20) wires it into the headless runner.
            return Err(format!(
                "headless launch with stdin task delivery is not wired into the runner yet for agent type: {}",
                self.id
            ));
        }
        let program = self.resolve_program(launch.project)?;
        self.headless_spec(launch, program)
    }

    fn accepts_headless_model(&self) -> bool {
        self.has_segment("model")
    }

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        self.parser.as_ref()?.extract_result(output, exit_code)
    }

    fn headless_result_ready(&self, output: &str) -> bool {
        self.parser.as_ref().is_some_and(|p| p.result_ready(output))
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
        self.parser.as_ref()?.extract_provider_session(output)
    }

    fn rejected_call_warning(&self, output: &str) -> Option<&'static str> {
        let code = self.warning_code?;
        self.parser.as_ref()?.rejected_call_warning(output).map(|_| code)
    }

    fn extract_model(&self, output: &str) -> Option<String> {
        self.parser.as_ref()?.extract_model(output)
    }

    fn default_model(&self) -> Option<String> {
        self.def.model.default.clone()
    }

    /// A visible standby needs a TUI pane (`[tui]`). With `[tui.ready]` the definition must opt in
    /// (`[standby] visible = true`) and is judged by its ready screen. Without it, visible standby is
    /// on unless the definition says `visible = false`, and runs on the experimental generic path.
    fn standby_support(&self) -> StandbySupport {
        let has_tui = self.def.tui.is_some();
        let has_ready = self.def.tui.as_ref().is_some_and(|t| t.ready.is_some());
        let visible = has_tui
            && if has_ready {
                self.def.standby.visible == Some(true)
            } else {
                self.def.standby.visible != Some(false)
            };
        StandbySupport {
            visible,
            headless: self.def.standby.headless && self.has_stdin(),
            experimental: visible && !has_ready,
        }
    }

    fn headless_standby_command(
        &self,
        launch: &HeadlessLaunch,
    ) -> Option<Result<LaunchSpec, String>> {
        if !self.standby_support().headless {
            return None;
        }
        Some(
            self.resolve_program(launch.project)
                .and_then(|program| self.headless_spec(launch, program)),
        )
    }

    fn headless_standby_input(&self, task: &str) -> Option<String> {
        let headless = self.def.headless.as_ref()?;
        let launch = HeadlessLaunch {
            project: "",
            task,
            read_only: false,
            resume_session: None,
            trust_workspace: false,
            allow_shell: false,
            model: None,
        };
        template::render_stdin(headless, &launch)
    }

    fn idle_after(&self) -> std::time::Duration {
        match &self.engine {
            Some(engine) => engine.idle_after(),
            None => crate::pty::registry::DEFAULT_IDLE_AFTER,
        }
    }

    fn quiet_idle_after(&self) -> std::time::Duration {
        match &self.engine {
            Some(engine) => engine.quiet_idle_after(),
            None => self.idle_after(),
        }
    }

    fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        self.engine.as_ref()?.busy_marker(visible_tail)
    }

    fn classify_screen(&self, visible_tail: &str) -> Option<PaneStatus> {
        self.engine.as_ref()?.classify_screen(visible_tail)
    }

    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        if self.standby_support().experimental {
            return generic::experimental_standby_screen(screen);
        }
        match &self.engine {
            Some(engine) => engine.standby_screen(screen),
            None => StandbyScreen::NotReady,
        }
    }

    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<Value> {
        if let Some(engine) = &self.engine {
            if let Some(prompt) = engine.detect_prompt(screen, allow_broad) {
                return Some(prompt);
            }
            if engine.claims(screen) {
                // The definition recognised the screen and suppressed it (an answered dialog).
                return None;
            }
        }
        generic::detect_prompt(self.id, screen, allow_broad)
    }

    fn prompt_reply(&self, prompt: &Value, choice: &str, allow_broad: bool) -> Result<Reply, String> {
        match self.engine.as_ref().and_then(|engine| engine.prompt_reply(prompt, choice, allow_broad)) {
            Some(reply) => reply,
            None => generic::prompt_reply(prompt, choice, allow_broad),
        }
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        self.engine.as_ref().is_some_and(|engine| engine.looks_like_menu(text))
    }

    fn clear_input_sequence(&self) -> Option<&'static str> {
        self.engine.as_ref()?.clear_input_sequence()
    }

    fn verifies_prompt_delivery(&self) -> bool {
        self.engine.as_ref().is_some_and(|engine| engine.verifies_prompt_delivery())
    }

    fn is_chrome_line(&self, lower: &str) -> bool {
        self.engine.as_ref().is_some_and(|engine| engine.is_chrome_line(lower))
    }

    fn is_tool_activity_line(&self, lower: &str) -> bool {
        self.engine.as_ref().is_some_and(|engine| engine.is_tool_activity_line(lower))
    }

    fn sidebar_crop(&self) -> Option<crate::tui::types::SidebarCrop> {
        self.engine.as_ref().and_then(|engine| engine.sidebar_crop())
    }

    fn strip_dialogs(&self, text: &str) -> String {
        match &self.engine {
            Some(engine) => engine.strip_dialogs(text),
            None => text.to_string(),
        }
    }

    fn tidy_reply(&self, text: &str) -> String {
        match &self.engine {
            Some(engine) => engine.tidy_reply(text),
            None => text.to_string(),
        }
    }

    fn detect_footer_model(&self, buffer: &str) -> Option<String> {
        self.engine.as_ref()?.detect_footer_model(buffer)
    }

    fn capability_notes(&self, _has_pane: bool) -> Vec<String> {
        let note = self.def.features.read_only_note.trim();
        if note.is_empty() { Vec::new() } else { vec![note.to_string()] }
    }

    fn context(&self) -> Option<AgentContextProfile> {
        let c = &self.def.context;
        Some(AgentContextProfile {
            agent_type: AgentId::new(self.id),
            label: self.def.agent.label.clone().into(),
            default_model: self.def.model.default.clone().map(Into::into),
            model_detection: match self.def.model.source {
                ModelSource::None => ModelDetection::Unknown,
                ModelSource::FooterRegex | ModelSource::OutputPath => ModelDetection::CliBanner,
                ModelSource::ProbeCommand => ModelDetection::Configuration,
            },
            smartness: c.smartness.clamp(1, 10) as u8,
            strengths: c.strengths.iter().filter_map(|s| capability(s)).collect::<Vec<_>>().into(),
            context_notes: c.notes.iter().cloned().map(Into::into).collect(),
            best_for: c.best_for.iter().cloned().map(Into::into).collect(),
            planned_sidebar_actions: Vec::new(),
        })
    }
}
