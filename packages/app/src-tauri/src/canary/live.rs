//! The canary on this machine: the real agents, the real `--version`, a real CLI on a hidden PTY,
//! the app's data folder and event bus.

use super::engine::{Deps, Engine, Source};
use super::model::*;
use super::run::{self, CanaryIo, Timing};
use super::strict;
use crate::pty::registry::{spawn_probe, ProbePty};
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Emitted with the whole `Status` whenever it changes.
pub const CHANGED_EVENT: &str = "canary://changed";
/// The first check waits this long so the app is up before a CLI is launched.
const STARTUP_GRACE: Duration = Duration::from_secs(20);
/// Versions are re-read this often, so a CLI updated while the app runs is noticed.
const REPROBE_EVERY: Duration = Duration::from_secs(15 * 60);

static ENGINE: OnceCell<Arc<Engine>> = OnceCell::new();

pub(super) fn engine() -> Option<&'static Arc<Engine>> {
    ENGINE.get()
}

fn state_path() -> PathBuf {
    crate::app_paths::app_data_dir().join("canary.json")
}

/// Where a check starts the CLI. A fixed folder per agent, so a trust answer the person gave once
/// sticks. `PM_CANARY_SCRATCH` overrides the root (live tests).
pub fn scratch_dir(agent: &str) -> PathBuf {
    let root = std::env::var_os("PM_CANARY_SCRATCH")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::app_paths::app_data_dir().join("canary").join("scratch"));
    root.join(agent)
}

fn sources() -> Vec<Source> {
    crate::agent_health::collect_probes()
        .into_iter()
        .map(|probe| {
            let tested = crate::tui::profile_for(&probe.id).tested_versions().map(|r| r.as_str().to_string());
            Source {
                facts: Facts { id: probe.id, label: probe.label, installed: probe.installed, tested },
                version_command: probe.version_command,
            }
        })
        .collect()
}

fn read_version(source: &Source) -> Option<String> {
    let command = source.version_command.as_ref()?;
    match crate::agent_health::run_version(command, crate::agent_health::VERSION_TIMEOUT) {
        crate::agent_health::VersionOutcome::Version(line) => Some(line),
        _ => None,
    }
}

impl CanaryIo for ProbePty {
    fn screen(&mut self) -> String {
        ProbePty::screen(self)
    }
    fn write(&mut self, keys: &str) -> Result<(), String> {
        self.send(keys)
    }
    fn alive(&mut self) -> bool {
        ProbePty::alive(self)
    }
    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
    fn elapsed(&mut self) -> Duration {
        ProbePty::elapsed(self)
    }
}

/// Start the real CLI in its scratch folder and drive it to the input box. Sends no prompt.
pub fn run_live(source: &Source, version: Option<String>) -> CanaryResult {
    let id = source.facts.id.as_str();
    let profile = crate::tui::profile_for(id);
    let now = crate::event_log::now_ms();
    let early = |outcome: Outcome| CanaryResult {
        agent: id.to_string(),
        version: version.clone(),
        at_ms: now,
        duration_ms: 0,
        outcome,
        passed: outcome.passed(),
        summary: run::summary(outcome, None, &source.facts.label),
        gate: None,
        dismissed: vec![],
        keys_sent: vec![],
        prompts_sent: 0,
        screens: vec![],
    };
    if !run::checkable(profile.as_ref()) {
        return early(Outcome::Inconclusive);
    }
    let scratch = scratch_dir(id);
    if std::fs::create_dir_all(&scratch).is_err() {
        return early(Outcome::StartFailed);
    }
    let mut probe = match spawn_probe(profile.as_ref(), &scratch.to_string_lossy(), 30, 120) {
        Ok(probe) => probe,
        Err(error) => {
            tracing::warn!(agent = id, %error, "canary could not start the CLI");
            return early(Outcome::StartFailed);
        }
    };
    let drive = run::drive(&mut probe, profile.as_ref(), &Timing::default());
    probe.kill();
    run::into_result(
        drive,
        id,
        &source.facts.label,
        version,
        now,
        profile.as_ref(),
        &super::captures::redact_screen,
    )
}

fn deps() -> Deps {
    Deps {
        sources: Box::new(sources),
        read_version: Box::new(read_version),
        run_check: Box::new(run_live),
        captures: Box::new(|agent| super::captures::count(&super::captures::root(), agent)),
        now_ms: Box::new(crate::event_log::now_ms),
    }
}

/// Start the canary: load what it knew, publish strict mode, and watch for version changes.
pub fn init(app: AppHandle) {
    let engine = Arc::new(Engine::new(Some(state_path()), deps()));
    engine.on_change(Box::new(move |status, strict_agents| {
        let entered = strict::replace(strict_agents.iter().cloned());
        crate::standby::set_strict_agents(strict_agents);
        if !entered.is_empty() {
            tracing::info!(agents = ?entered, "agents entered strict mode");
        }
        let _ = app.emit(CHANGED_EVENT, status);
    }));
    if ENGINE.set(engine.clone()).is_err() {
        return;
    }
    let _ = std::thread::Builder::new().name("canary".into()).spawn(move || {
        // What was known last time takes effect at once; the first probe follows the grace period.
        engine.publish();
        std::thread::sleep(STARTUP_GRACE);
        loop {
            engine.run_due();
            std::thread::sleep(REPROBE_EVERY);
        }
    });
}

pub fn version_of(agent: &str) -> Option<String> {
    engine()?.version_of(agent)
}

pub fn status() -> Status {
    match engine() {
        Some(engine) => engine.status(),
        None => Status { agents: vec![], auto_check: true, notices: vec![], captures: 0 },
    }
}

/// "Check this agent": blocks while the CLI starts and settles (up to a minute).
pub fn check(agent: &str) -> Result<AgentView, String> {
    engine().ok_or("the version check is not running yet")?.check(agent)
}

pub fn dismiss_notice(id: &str) {
    if let Some(engine) = engine() {
        engine.dismiss_notice(id);
    }
}

pub fn set_auto_check(on: bool) {
    if let Some(engine) = engine() {
        engine.set_auto_check(on);
    }
}

/// Open the folder of saved screens in the system file manager; returns its path.
pub fn open_captures() -> Result<String, String> {
    let root = super::captures::root();
    std::fs::create_dir_all(&root).map_err(|e| format!("could not create {}: {e}", root.display()))?;
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&root)
        .spawn()
        .map_err(|e| format!("could not open {}: {e}", root.display()))?;
    Ok(root.to_string_lossy().into_owned())
}
