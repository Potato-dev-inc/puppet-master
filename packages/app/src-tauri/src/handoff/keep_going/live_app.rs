//! Keep going: the live app behind [`LiveIo`] (registry, runs, the chat reader, the audit log).
//!
//! Nothing here decides anything; `live.rs` does. This file only reaches the real world, and the
//! pure part of that (`to_run_request`) is tested here.

use super::live_io::{label_with, Gathered, InFlight, LiveIo, PaneFacts, StartRequest, Started};
use super::model::{Candidate, Handover, Target, STOP_WAIT_MS};
use super::store::Store;
use crate::audit::{Kind, Who};
use crate::handoff::reset::{Clock, SystemClock};
use crate::operations::{OperationError, OperationSnapshot};
use crate::plans_report::limit_log::Entry;
use crate::pty::agent_stop::StopError;
use crate::questions::QuestionStore;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The panes' status lines are asked for every second, so the records are cached until one changes.
pub struct AppIo {
    store: Store,
    flight: InFlight,
    cache: Mutex<Option<Vec<Handover>>>,
}

static APP_IO: Lazy<AppIo> = Lazy::new(|| AppIo {
    #[cfg(not(test))]
    store: Store::live(),
    // Unit tests never touch the real app-data folder; nothing in a test reaches this store.
    #[cfg(test)]
    store: Store::at(std::env::temp_dir().join(format!("pm-keep-going-unused-{}", std::process::id()))),
    flight: InFlight::default(),
    cache: Mutex::new(None),
});

pub fn app_io() -> &'static AppIo {
    &APP_IO
}

impl LiveIo for AppIo {
    fn now_ms(&self) -> u64 {
        SystemClock.now_ms().max(0) as u64
    }

    fn clock(&self) -> &dyn Clock {
        &SystemClock
    }

    fn store(&self) -> &Store {
        &self.store
    }

    fn records(&self) -> Vec<Handover> {
        let mut cache = self.cache.lock();
        cache.get_or_insert_with(|| self.store.records()).clone()
    }

    fn questions(&self) -> &QuestionStore {
        crate::questions::store()
    }

    fn in_flight(&self) -> &InFlight {
        &self.flight
    }

    fn pane(&self, pane_id: &str) -> Option<PaneFacts> {
        let registry = crate::bridge::live_registry()?;
        let info = registry.lock().list().into_iter().find(|pane| pane.id == pane_id)?;
        let project = crate::project_path::normalize_project_path(Path::new(&info.cwd)).ok()?.to_string_lossy().to_string();
        let worktree = worktree_of(&info.cwd);
        let mission = worktree.as_ref().is_some_and(|copy| copy.handle.starts_with(crate::missions::model::HANDLE_PREFIX))
            || budget_parent_is_mission(&project, pane_id);
        Some(PaneFacts {
            pane_id: info.id.clone(),
            agent_label: crate::tui::descriptor_for(&info.agent_type).map(|d| d.label).unwrap_or_else(|| info.agent_type.clone()),
            agent_id: info.agent_type,
            account: info.account,
            account_label: info.account_label,
            cwd: info.cwd,
            project,
            status: info.status,
            companion: crate::companion::service::is_pane_id(pane_id),
            mission,
            worktree,
        })
    }

    fn vendor_of(&self, agent_id: &str) -> Option<String> {
        crate::tui::profile_for(agent_id).limits().map(|limits| limits.vendor().to_string())
    }

    fn candidates(&self, folder: &str, extra: &[Target]) -> Vec<Candidate> {
        let mut found: Vec<Candidate> = Vec::new();
        let agents: Vec<_> =
            crate::tui::descriptors().into_iter().filter(|d| d.dispatchable && d.kind == crate::tui::Kind::Cli.as_str()).collect();
        for agent in &agents {
            found.push(Candidate {
                target: Target { agent_id: agent.id.to_string(), account: None },
                label: agent.label.clone(),
                vendor: self.vendor_of(agent.id),
                runnable: runnable(agent.id, agent.installed, folder),
                limited_until: None,
            });
        }
        for target in extra {
            let Some(agent) = agents.iter().find(|d| d.id == target.agent_id) else { continue };
            let Some(account) = target.account.as_deref() else { continue };
            let runnable = runnable(agent.id, agent.installed, folder).and_then(|_| {
                crate::accounts::validate_for_run(agent.id, account)
                    .map(|_| ())
                    .map_err(|_| "That saved login isn't available any more".to_string())
            });
            found.push(Candidate {
                target: target.clone(),
                label: label_with(&agent.label, crate::accounts::label_of(account).as_deref()),
                vendor: self.vendor_of(agent.id),
                runnable,
                limited_until: None,
            });
        }
        found
    }

    fn engine_limit(&self, agent_id: &str, account: Option<&str>) -> Option<Option<u64>> {
        crate::handoff::agent_limit(agent_id, account)
    }

    fn at_limit(&self, pane_id: &str) -> bool {
        crate::handoff::is_parked(pane_id)
    }

    fn gather(&self, pane: &PaneFacts, run: Option<&OperationSnapshot>) -> Gathered {
        super::live_gather::gather(pane, run)
    }

    fn clean(&self, text: &str) -> String {
        let ctx = crate::redact::known::context();
        super::brief::clean_text(text, &ctx, ctx.home.as_deref())
    }

    fn stop_agent(&self, pane_id: &str) -> Result<(), StopError> {
        match crate::bridge::live_registry() {
            Some(registry) => crate::pty::agent_stop::stop_agent(&registry, pane_id, STOP_WAIT_MS),
            None => Err(StopError::NoPid),
        }
    }

    #[cfg(not(test))]
    fn start(&self, request: &StartRequest) -> Result<Started, OperationError> {
        let not_ready = || OperationError::new("APP_NOT_READY", "Puppet Master is still starting", true);
        let registry = crate::bridge::live_registry().ok_or_else(not_ready)?;
        let app = crate::bridge::live_app().ok_or_else(not_ready)?;
        let view = crate::agent_runs::run_agent(to_run_request(request)?, registry, app, None)?;
        let pane_id = view.pane_id.clone().or_else(|| pane_of_run(&request.project_path, &view.operation_id));
        Ok(Started { handle: view.handle, operation_id: view.operation_id, pane_id })
    }

    // Not in unit tests: the app handle's window code does not link into the test binary.
    #[cfg(test)]
    fn start(&self, _request: &StartRequest) -> Result<Started, OperationError> {
        Err(OperationError::new("APP_NOT_READY", "There is no app in a unit test", true))
    }

    fn set_parent(&self, child: &str, parent: &str) {
        if let Some(registry) = crate::bridge::live_registry() {
            crate::pty::pane_parent::set_parent(&registry, child, parent);
            #[cfg(not(test))]
            crate::bridge::notify_panes_changed();
        }
    }

    fn budget_left(&self, project: &str, handle: &str) -> (Option<u32>, Option<u32>) {
        let Some(view) = crate::budgets::view(project, handle) else { return (None, None) };
        let left = |section: &str| {
            let used = view.get(section)?.get("used")?.as_u64()?;
            let limit = view.get(section)?.get("limit")?.as_u64()?;
            u32::try_from(limit.saturating_sub(used).max(1)).ok()
        };
        (left("turns"), left("minutes"))
    }

    fn publish(&self, snapshot: &OperationSnapshot) {
        // Not in unit tests: the app handle's window code does not link into the test binary.
        #[cfg(not(test))]
        if let (Some(registry), Some(app)) = (crate::bridge::live_registry(), crate::bridge::live_app()) {
            let _ = crate::bridge::publish_operation(snapshot, &registry, &app);
        }
        #[cfg(test)]
        let _ = snapshot;
    }

    fn log_limit(&self, entry: Entry) {
        #[cfg(not(test))]
        {
            let _ = crate::plans_report::limit_log::append(&crate::plans_report::limit_log::default_path(), &entry);
        }
        #[cfg(test)]
        let _ = entry;
    }

    fn audit(&self, who: Who, summary: &str, detail: Value) {
        crate::audit::record(Kind::KeepGoing, who, summary, detail);
    }

    fn mark(&self, pane_id: &str, text: &str) {
        crate::flight::mark(pane_id, text);
    }

    fn changed(&self, pane_ids: &[&str]) {
        *self.cache.lock() = None;
        for pane_id in pane_ids {
            crate::pane_activity::invalidate(pane_id);
        }
        #[cfg(not(test))]
        if let Some(app) = crate::bridge::live_app() {
            use tauri::Emitter;
            let _ = app.emit("keep-going://changed", json!({ "paneIds": pane_ids }));
        }
    }
}

/// Whether an agent could start in `folder`, in plain words when not.
fn runnable(agent_id: &str, installed: bool, folder: &str) -> Result<(), String> {
    if !installed {
        return Err("Not installed on this computer".to_string());
    }
    crate::companion::agents::agent_ok(agent_id, folder).map_err(str::to_string)
}

/// The separate copy whose checkout holds `cwd`, if any.
fn worktree_of(cwd: &str) -> Option<crate::worktrees::WorktreeRef> {
    let origin = crate::worktrees::origin_of_cwd(cwd)?;
    crate::worktrees::list(&origin)
        .into_iter()
        .find(|entry| {
            entry.exists
                && entry.worktree.closed.is_none()
                && crate::project_path::workspace_covers(Path::new(&entry.worktree.path), Path::new(cwd))
        })
        .map(|entry| entry.worktree)
}

/// A mission round draws its budget from the mission's.
fn budget_parent_is_mission(project: &str, pane_id: &str) -> bool {
    // A separate copy's runs are stored in the project it came from (`handoff::PaneCtx::runs_folder`).
    let project = crate::worktrees::origin_of_cwd(project).unwrap_or_else(|| project.to_string());
    let Ok(Some(run)) = crate::operations::active_operation_for_pane(&project, pane_id) else { return false };
    let handle = if run.agent_run_id.is_empty() { run.operation_id } else { run.agent_run_id };
    crate::budgets::record_of(&project, &handle)
        .and_then(|record| record.parent)
        .is_some_and(|parent| parent.starts_with(crate::missions::model::HANDLE_PREFIX))
}

#[cfg(not(test))]
/// The new run's pane, when the run's answer did not carry it yet: the dispatch makes it a moment later.
fn pane_of_run(project: &str, operation_id: &str) -> Option<String> {
    for _ in 0..50 {
        match crate::operations::get_operation(project, operation_id) {
            Ok(run) if run.pane_id.is_some() => return run.pane_id,
            Ok(run) if run.status.terminal() => return None,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// The run request for a hand-over's new agent. Only fields the hand-over sets; everything else
/// keeps the run default, so the new agent has no extra launch arguments and no extra rights.
pub fn to_run_request(request: &StartRequest) -> Result<crate::agent_runs::AgentRunRequest, OperationError> {
    let mut run: crate::agent_runs::AgentRunRequest = serde_json::from_value(json!({
        "project_path": request.project_path,
        "task": request.task,
        "agent_type": request.agent_type,
        "account": request.account,
        "headless": request.headless,
        "background": request.background,
        "keep_pane": request.keep_pane,
        "wait_ms": request.wait_ms,
        "timeout_ms": request.timeout_ms,
        "idempotency_key": request.idempotency_key,
        "name": request.name,
        "trust_workspace": request.trust_workspace,
        "allow_shell": request.allow_shell,
        "allowed_commands": request.allowed_commands,
        "read_only": request.read_only,
        "budget_turns": request.budget_turns,
        "budget_minutes": request.budget_minutes,
    }))
    .map_err(|error| OperationError::new("INVALID_ARGUMENT", error.to_string(), false))?;
    run.join_worktree = request.join_worktree.clone();
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> StartRequest {
        StartRequest {
            project_path: "C:\\Users\\you\\proj".into(),
            task: "Continue: notes".into(),
            agent_type: "fake_b".into(),
            account: Some("work".into()),
            headless: false,
            background: true,
            keep_pane: true,
            wait_ms: 0,
            timeout_ms: 600_000,
            idempotency_key: "keep-going:kg-0123456789ab".into(),
            name: "Continue: notes · kg-0123456789ab".into(),
            trust_workspace: false,
            allow_shell: false,
            allowed_commands: None,
            read_only: false,
            budget_turns: Some(4),
            budget_minutes: Some(9),
            join_worktree: None,
        }
    }

    #[test]
    fn the_run_request_keeps_every_gate_closed() {
        let run = to_run_request(&request()).unwrap();
        assert_eq!(run.task, "Continue: notes");
        assert_eq!(run.agent_type, "fake_b");
        assert_eq!(run.account.as_deref(), Some("work"));
        assert!(!run.headless && run.background && run.keep_pane);
        assert!(!run.trust_workspace && !run.allow_shell && !run.read_only);
        assert!(run.allowed_commands.is_none());
        assert_eq!(run.wait_ms, 0);
        assert_eq!(run.idempotency_key.as_deref(), Some("keep-going:kg-0123456789ab"));
        assert_eq!((run.budget_turns, run.budget_minutes), (Some(4), Some(9)));
        assert!(run.join_worktree.is_none() && run.pane_id.is_none() && run.worker_id.is_none() && run.handle.is_none());
        assert!(run.model.is_none() && !run.until_checks_pass && run.backup_agents.is_none());
    }
}
