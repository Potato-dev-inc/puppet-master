//! The canary engine: keeps one record per agent, decides when to check, runs the check, and turns
//! the outcome into strict mode, an Inbox card and the status the UI reads.
//!
//! Everything that touches the machine (which agents exist, their versions, the live PTY check, the
//! clock) is injected through [`Deps`], so the whole flow is tested with a faked version bump.

use super::model::*;
use super::state;
use crate::tui::VersionCommand;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::PathBuf;

/// One installed (or missing) CLI agent, as the engine needs it.
#[derive(Debug, Clone)]
pub struct Source {
    pub facts: Facts,
    pub version_command: Option<VersionCommand>,
}

pub struct Deps {
    pub sources: Box<dyn Fn() -> Vec<Source> + Send + Sync>,
    /// The first line of the agent's `--version`, `None` when it could not be read.
    pub read_version: Box<dyn Fn(&Source) -> Option<String> + Send + Sync>,
    /// Drive the real CLI (or a fake) to its input box. Blocks.
    pub run_check: Box<dyn Fn(&Source, Option<String>) -> CanaryResult + Send + Sync>,
    /// Unrecognised screens saved for an agent.
    pub captures: Box<dyn Fn(Option<&str>) -> usize + Send + Sync>,
    pub now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
}

type Listener = Box<dyn Fn(&Status, &[String]) + Send + Sync>;

pub struct Engine {
    path: Option<PathBuf>,
    deps: Deps,
    data: Mutex<Persisted>,
    checking: Mutex<HashSet<String>>,
    /// Only one live check at a time: each launches a real CLI.
    one_at_a_time: Mutex<()>,
    listeners: Mutex<Vec<Listener>>,
}

impl Engine {
    pub fn new(path: Option<PathBuf>, deps: Deps) -> Self {
        let data = path.as_deref().and_then(load).unwrap_or_default();
        Self {
            path,
            deps,
            data: Mutex::new(data),
            checking: Mutex::new(HashSet::new()),
            one_at_a_time: Mutex::new(()),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// `listener(status, strict_agents)` runs after every change that can alter the picture.
    pub fn on_change(&self, listener: Listener) {
        self.listeners.lock().push(listener);
    }

    pub fn status(&self) -> Status {
        let sources = (self.deps.sources)();
        let data = self.data.lock();
        let checking = self.checking.lock();
        let agents: Vec<AgentView> = sources
            .iter()
            .map(|s| {
                state::view_of(
                    &s.facts,
                    data.agents.get(&s.facts.id),
                    checking.contains(&s.facts.id),
                    (self.deps.captures)(Some(&s.facts.id)),
                )
            })
            .collect();
        Status {
            agents,
            auto_check: data.auto_check,
            notices: data
                .agents
                .values()
                .filter_map(|r| r.notice.clone())
                .filter(|n| !n.dismissed)
                .collect(),
            captures: (self.deps.captures)(None),
        }
    }

    /// The version last read for `agent`, as a label (`0.162.1`).
    pub fn version_of(&self, agent: &str) -> Option<String> {
        let data = self.data.lock();
        data.agents
            .get(&agent.to_ascii_lowercase())
            .and_then(|r| r.version.as_deref())
            .map(super::version::label)
    }

    pub fn publish(&self) {
        let status = self.status();
        let strict: Vec<String> = status.agents.iter().filter(|a| a.strict).map(|a| a.agent.clone()).collect();
        for listener in self.listeners.lock().iter() {
            listener(&status, &strict);
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let data = self.data.lock().clone();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec_pretty(&data) {
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// Read every agent's version. Returns the agents whose change calls for a check.
    pub fn refresh(&self) -> Vec<String> {
        let sources = (self.deps.sources)();
        let mut wanted = Vec::new();
        for source in sources.iter().filter(|s| s.facts.installed) {
            let version = (self.deps.read_version)(source);
            let now = (self.deps.now_ms)();
            let mut data = self.data.lock();
            let auto = data.auto_check;
            let before = data.agents.get(&source.facts.id).cloned();
            if state::should_check(auto, &source.facts, before.as_ref(), version.as_deref()) {
                wanted.push(source.facts.id.clone());
            }
            let record = data.agents.entry(source.facts.id.clone()).or_default();
            // An unreadable version keeps the last good one: the CLI may just have been slow.
            if version.is_some() {
                record.version = version;
            }
            record.probed_ms = now;
        }
        self.save();
        self.publish();
        wanted
    }

    /// Check one agent now (the version trigger or "Check this agent"). Blocks while the CLI runs.
    pub fn check(&self, agent: &str) -> Result<AgentView, String> {
        let agent = agent.to_ascii_lowercase();
        let source = (self.deps.sources)()
            .into_iter()
            .find(|s| s.facts.id.eq_ignore_ascii_case(&agent))
            .ok_or_else(|| format!("{agent} is not an agent Puppet Master knows"))?;
        if !source.facts.installed {
            return Err(format!("{} is not installed on this computer", source.facts.label));
        }
        if !self.checking.lock().insert(source.facts.id.clone()) {
            return Err(format!("{} is already being checked", source.facts.label));
        }
        let _checking = Checking { set: &self.checking, id: source.facts.id.clone() };
        self.publish();
        let _turn = self.one_at_a_time.lock();
        let version = (self.deps.read_version)(&source);
        if version.is_some() {
            let mut data = self.data.lock();
            data.agents.entry(source.facts.id.clone()).or_default().version = version.clone();
        }
        let version_for_run = version.or_else(|| self.data.lock().agents.get(&source.facts.id).and_then(|r| r.version.clone()));
        let result = (self.deps.run_check)(&source, version_for_run);
        {
            let mut data = self.data.lock();
            let record = data.agents.entry(source.facts.id.clone()).or_default();
            state::apply_result(record, &source.facts, result);
        }
        drop(_checking);
        self.save();
        self.publish();
        let status = self.status();
        status
            .agents
            .into_iter()
            .find(|a| a.agent == source.facts.id)
            .ok_or_else(|| "the agent disappeared".to_string())
    }

    /// The version trigger: read versions, then check every agent that changed.
    pub fn run_due(&self) -> Vec<AgentView> {
        self.refresh().into_iter().filter_map(|agent| self.check(&agent).ok()).collect()
    }

    pub fn dismiss_notice(&self, id: &str) {
        {
            let mut data = self.data.lock();
            for record in data.agents.values_mut() {
                if let Some(notice) = record.notice.as_mut().filter(|n| n.id == id) {
                    notice.dismissed = true;
                }
            }
        }
        self.save();
        self.publish();
    }

    pub fn set_auto_check(&self, on: bool) {
        self.data.lock().auto_check = on;
        self.save();
        self.publish();
    }
}

/// Clears the "being checked" mark however the check ends (a panic included).
struct Checking<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
}

impl Drop for Checking<'_> {
    fn drop(&mut self) {
        self.set.lock().remove(&self.id);
    }
}

fn load(path: &std::path::Path) -> Option<Persisted> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
