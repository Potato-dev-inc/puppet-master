//! The live side of crews: the registry as a source of drafts, and the real `Starter`, which starts a
//! pane the way every in-app run does (`agent_runs::run_agent`, a visible pane that stays open) and then
//! records the helper link (`pty::pane_parent::set_parent`, the same link an MCP `run_agent` earns).

use super::agents::{pick_default, AgentChoice, Choices};
use super::draft::{draft, Draft, Env, TreePane};
use super::model::CrewError;
use super::start::{StartFailure, Starter, Step};
use super::validate::AgentState;
use crate::agent_runs::{run_agent, AgentRunRequest};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a started role may take to get its pane before the crew stops there.
const PANE_WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(250);

/// The registry's word on an agent type, for `validate`.
pub fn agent_state(agent_type: &str) -> AgentState {
    let Some(profile) = crate::tui::registry::registry().find(agent_type) else { return AgentState::Unknown };
    let features = profile.features();
    if features.dispatchable {
        AgentState::Ready
    } else {
        AgentState::NotDispatchable(profile.not_dispatchable_reason().to_string())
    }
}

pub fn label_of(agent_type: &str) -> String {
    crate::tui::descriptor_for(agent_type).map(|d| d.label).unwrap_or_else(|| agent_type.to_string())
}

/// The agents that can take a role here: dispatchable and installed, in registry order.
pub fn startable_agents() -> Vec<AgentChoice> {
    crate::tui::registry::snapshot()
        .iter()
        .filter(|profile| profile.features().dispatchable && profile.is_installed())
        .map(|profile| AgentChoice { id: profile.id().to_string(), label: label_of(profile.id()) })
        .collect()
}

/// The Start sheet's list and the agent `{default}` stands for (the Ask bar's default, see `agents::pick_default`).
pub fn choices() -> Choices {
    let agents = startable_agents();
    let settings = crate::askbar::settings();
    let default = pick_default(settings.default_agent.as_deref(), settings.last_agent.as_deref(), &agents);
    Choices { agents, default }
}

/// "Save this team" for the pane `root_id` and the helpers under it.
pub fn draft_for_pane(registry: &Arc<Mutex<PaneRegistry>>, root_id: &str) -> Result<Draft, CrewError> {
    let panes: Vec<TreePane> = registry
        .lock()
        .list()
        .into_iter()
        .map(|info| {
            let mut folders = vec![info.cwd.clone()];
            folders.extend(crate::worktrees::origin_of_cwd(&info.cwd));
            // A title the person did not choose is only the start of the task: not a role name.
            let title = info.title.clone().filter(|_| crate::ambient::store::is_user_set(&info.id));
            TreePane { id: info.id, agent_type: info.agent_type, title, folders, parent: info.parent_pane_id }
        })
        .collect();
    let first_task_of = |pane_id: &str| first_task(registry, pane_id);
    let redact = |text: &str| crate::redact::strict(text).into_inner();
    draft(&panes, root_id, &Env { first_task_of: &first_task_of, redact: &redact, label_of: &label_of })
}

/// The task of the first turn of the run bound to the pane.
fn first_task(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<String> {
    let (project, handle) = crate::agent_runs::pane_api::run_of_pane(registry, pane_id)?;
    let turns = crate::operations::turns_for_handle(&project, &handle).ok()?;
    turns.into_iter().next().map(|t| t.task).filter(|t| !t.trim().is_empty())
}

/// The folder a crew starts in must be a real project folder, not a drive root or the home folder.
pub fn check_folder(folder: &str) -> Result<(), CrewError> {
    let path = std::path::Path::new(folder);
    if folder.trim().is_empty() || !path.is_dir() {
        return Err(CrewError::new("CREW_FOLDER_MISSING", "Pick a folder that exists."));
    }
    let known = crate::plain_mode::guard::KnownDirs::from_env();
    if !crate::plain_mode::guard::verdict(folder, &known).is_ok() {
        return Err(CrewError::new("CREW_FOLDER_TOO_BROAD", "That folder is too broad for a crew. Pick a project folder."));
    }
    crate::path_limit::check(path).map_err(|e| CrewError::new("CREW_FOLDER_TOO_LONG", e.message()))
}

pub struct LiveStarter {
    registry: Arc<Mutex<PaneRegistry>>,
    app: tauri::AppHandle,
    /// Keeps the idempotency keys of this crew start apart from any other start.
    run: String,
    count: usize,
}

impl LiveStarter {
    pub fn new(registry: Arc<Mutex<PaneRegistry>>, app: tauri::AppHandle) -> Self {
        let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        Self { registry, app, run: format!("{run:x}"), count: 0 }
    }
}

fn failure(code: impl Into<String>, message: impl Into<String>) -> StartFailure {
    StartFailure { code: code.into(), message: message.into() }
}

impl Starter for LiveStarter {
    fn start(&mut self, step: &Step) -> Result<String, StartFailure> {
        self.count += 1;
        let request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "project_path": step.folder,
            "task": step.task,
            "agent_type": step.role.agent_type,
            "headless": false,
            "background": true,
            "keep_pane": true,
            "wait_ms": 0,
            "idempotency_key": super::start_key(&self.run, self.count),
        }))
        .map_err(|e| failure("INTERNAL", e.to_string()))?;
        let view = run_agent(request, self.registry.clone(), self.app.clone(), None).map_err(|e| failure(e.code, e.message))?;
        let pane_id = match view.pane_id {
            Some(id) => id,
            None => wait_for_pane(&step.folder, &view.handle)?,
        };
        if let Some(parent) = &step.parent_pane {
            crate::pty::pane_parent::set_parent(&self.registry, &pane_id, parent);
        }
        crate::ambient::store::set_user_title(&pane_id, &step.role.name);
        crate::bridge::notify_panes_changed();
        Ok(pane_id)
    }
}

/// The pane appears when the run's dispatch has made it; a run that failed first says why.
fn wait_for_pane(project: &str, handle: &str) -> Result<String, StartFailure> {
    let until = Instant::now() + PANE_WAIT;
    loop {
        match crate::operations::latest_agent_run(project, handle) {
            Ok(snapshot) => {
                if let Some(id) = snapshot.pane_id {
                    return Ok(id);
                }
                if snapshot.status.terminal() {
                    return Err(match snapshot.error {
                        Some(e) => failure(e.code, e.message),
                        None => failure("CREW_NO_PANE", "The agent ended before its pane opened."),
                    });
                }
            }
            Err(e) => return Err(failure(e.code, e.message)),
        }
        if Instant::now() >= until {
            return Err(failure("CREW_NO_PANE", "The agent's pane did not open in time."));
        }
        std::thread::sleep(POLL);
    }
}
