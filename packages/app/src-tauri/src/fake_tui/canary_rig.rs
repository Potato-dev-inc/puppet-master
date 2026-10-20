//! The CLI-update canary (`crate::canary`) driven against `pm-fake-tui`, for `tests/fake_tui_canary.rs`.
//!
//! What is real: the version line (`pm-fake-tui --version --script <script>` run as a process, like a
//! real CLI's `--version`), the engine (state, rules, notices, strict mode), the check itself (a hidden
//! ConPTY running the fake under the real Codex profile, driven to its input box with no prompt) and
//! the binary fingerprint. What is made up: the agent and its versions. A test rewrites the script
//! between phases (`Session::script`) to play "the CLI updated".

use crate::canary::engine::{Deps, Engine, Source};
use crate::canary::model::{Facts, Signal};
use crate::tui::VersionCommand;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub struct CanaryRig {
    engine: Arc<Engine>,
    agent: &'static str,
}

impl CanaryRig {
    pub(super) fn new(agent: &'static str, exe: PathBuf, script: PathBuf) -> Self {
        let profile = crate::tui::profile_for(agent);
        let command = VersionCommand {
            program: exe.to_string_lossy().into_owned(),
            args: vec!["--version".into(), "--script".into(), script.to_string_lossy().into_owned()],
        };
        let source = Source {
            facts: Facts {
                id: agent.to_string(),
                label: "Fake Codex".into(),
                installed: true,
                tested: profile.tested_versions().map(|range| range.as_str().to_string()),
            },
            version_command: Some(command),
        };
        let (listed, found) = (source.clone(), source.clone());
        let engine = Arc::new(Engine::new(
            None,
            Deps {
                sources: Box::new(move || vec![listed.clone()]),
                read_version: Box::new(|source| {
                    let command = source.version_command.as_ref()?;
                    match crate::agent_health::run_version(command, crate::agent_health::VERSION_TIMEOUT) {
                        crate::agent_health::VersionOutcome::Version(line) => Some(line),
                        _ => None,
                    }
                }),
                run_check: Box::new(|source, version| crate::canary::run_live(source, version)),
                captures: Box::new(|_| 0),
                now_ms: Box::new(crate::event_log::now_ms),
                fingerprint: Box::new(|source| {
                    crate::canary::binary::fingerprint(&source.version_command.as_ref()?.program)
                }),
                source_of: Box::new(move |id| id.eq_ignore_ascii_case(&found.facts.id).then(|| found.clone())),
            },
        ));
        // What `canary::init` wires in the app: the strict set follows the engine.
        engine.on_change(Box::new(|_, strict_agents| {
            crate::canary::strict::replace(strict_agents.iter().cloned());
        }));
        Self { engine, agent }
    }

    /// Read the version now. The ids of the agents whose change calls for a check.
    pub fn refresh(&self) -> Vec<String> {
        self.engine.refresh()
    }

    /// Check the agent now (starts the fake in a hidden pane and drives it to its input box).
    pub fn check(&self) -> Result<Value, String> {
        self.engine.check(self.agent).map(|view| to_value(&view))
    }

    /// The version trigger: read, then check what changed.
    pub fn run_due(&self) -> Vec<Value> {
        self.engine.run_due().iter().map(to_value).collect()
    }

    /// The agent as the UI shows it (`AgentView`).
    pub fn agent(&self) -> Value {
        to_value(&self.engine.status().agents[0])
    }

    /// The Inbox cards that were not dismissed.
    pub fn notices(&self) -> Vec<Value> {
        self.engine.status().notices.iter().map(to_value).collect()
    }

    pub fn set_auto_check(&self, on: bool) {
        self.engine.set_auto_check(on);
    }

    /// Strict mode as the run path sees it.
    pub fn is_strict(&self) -> bool {
        crate::canary::strict::is_strict(self.agent)
    }

    /// The keys a run would press on `screen` if it is a start-up notice, strict mode applied.
    pub fn dismiss_keys(&self, screen: &str) -> Option<String> {
        let profile = crate::tui::profile_for(self.agent);
        crate::canary::strict::startup_gate(profile.as_ref(), screen)?.dismiss.map(str::to_string)
    }

    /// A hint from a run that the screens may have changed; true when a check is due now (not started).
    pub fn signal(&self, kind: &str) -> bool {
        let signal = match kind {
            "unreadable_screen" => Signal::UnreadableScreen,
            "ambiguous_idle" => Signal::AmbiguousIdle,
            _ => Signal::NotReady,
        };
        self.engine.note_signal(self.agent, signal)
    }

    /// The spawn gate: true when the program changed and the new version calls for a check.
    pub fn before_run(&self) -> bool {
        self.engine.before_run(self.agent)
    }
}

impl Drop for CanaryRig {
    fn drop(&mut self) {
        // The strict set is process-wide.
        crate::canary::strict::replace(Vec::<String>::new());
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|e| json!({ "error": e.to_string() }))
}
