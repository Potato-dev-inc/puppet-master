//! `DeclarativeProfile`: a validated `Definition` behind the `TuiProfile` trait (headless half).
//!
//! Headless methods map onto `[headless]` (argv via `template`, parsing via `output`), the binary
//! via `resolve`. Everything TUI-shaped keeps the trait default until the table engine (T22).
//! Constructible from any parsed definition; nothing here loads files or registers anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::agent_contexts::{AgentCapability, AgentContextProfile, ModelDetection};
use crate::tui::types::{
    Features, HeadlessLaunch, Kind, LaunchSpec, RunOptionSupport, StandbySupport,
};
use crate::tui::{AgentId, TuiProfile};

use super::output::OutputParser;
use super::resolve::resolve_binary;
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

pub struct DeclarativeProfile {
    def: Definition,
    id: &'static str,
    not_dispatchable_reason: &'static str,
    warning_code: Option<&'static str>,
    parser: Option<OutputParser>,
}

impl DeclarativeProfile {
    /// Build from a definition that passed `validate`. Fails only when a selector regex does not
    /// compile (which `validate` already rules out).
    pub fn new(def: Definition) -> Result<Self, DefError> {
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
        Ok(Self {
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
            dispatchable: f.dispatchable,
            read_only_headless: f.read_only_headless,
            read_only_spawned_pane: f.read_only_spawned_pane,
            resumable_headless: f
                .resumable_headless
                .unwrap_or(self.has_segment("resume") && has_session),
        }
    }

    fn not_dispatchable_reason(&self) -> &'static str {
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

    fn standby_support(&self) -> StandbySupport {
        StandbySupport {
            visible: self.def.standby.visible
                && self.def.tui.as_ref().is_some_and(|t| t.ready.is_some()),
            headless: self.def.standby.headless && self.has_stdin(),
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
