//! Branch from here: the live app behind [`LiveIo`] (registry, save points, copies, runs, the audit log).
//!
//! Nothing here decides anything; `live.rs` does. This file only reaches the real world, and the
//! pure part of that (`to_run_request`) is tested here. The source pane is only read.

use super::copy::{self, Made};
use super::live_io::{CopyRequest, InFlight, LiveIo, PaneFacts, StartRequest, Started};
use super::model::{ERR_BRANCH_COPY_FAILED, MSG_BRANCH_COPY_FAILED};
use super::offers::Candidate;
use super::store::Store;
use crate::audit::{Kind, Who};
use crate::checkpoints::Checkpoint;
use crate::handoff::keep_going::live_io::label_with;
use crate::handoff::keep_going::model::Target;
use crate::handoff::reset::{Clock, SystemClock};
use crate::operations::OperationError;
use crate::turn_summary::TurnSummary;
use crate::worktrees::{self, harden};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub struct AppIo {
    store: Store,
    flight: InFlight,
}

static APP_IO: Lazy<Arc<AppIo>> = Lazy::new(|| {
    Arc::new(AppIo {
        #[cfg(not(test))]
        store: Store::live(),
        // Unit tests never touch the real app-data folder; nothing in a test reaches this store.
        #[cfg(test)]
        store: Store::at(std::env::temp_dir().join(format!("pm-branch-from-unused-{}", std::process::id()))),
        flight: InFlight::default(),
    })
});

/// The live app, for the desktop commands.
pub fn app_io() -> Arc<dyn LiveIo> {
    APP_IO.clone()
}

impl LiveIo for AppIo {
    fn now_ms(&self) -> u64 {
        SystemClock.now_ms().max(0) as u64
    }

    fn store(&self) -> &Store {
        &self.store
    }

    fn in_flight(&self) -> &InFlight {
        &self.flight
    }

    fn pane(&self, pane_id: &str) -> Option<PaneFacts> {
        let registry = crate::bridge::live_registry()?;
        let info = registry.lock().list().into_iter().find(|pane| pane.id == pane_id)?;
        let origin = origin_of(&info.cwd);
        let mission = worktree_of(&info.cwd).is_some_and(|copy| copy.handle.starts_with(crate::missions::model::HANDLE_PREFIX));
        Some(PaneFacts {
            pane_id: info.id.clone(),
            agent_label: crate::tui::descriptor_for(&info.agent_type).map(|d| d.label).unwrap_or_else(|| info.agent_type.clone()),
            takes_part: crate::tui::profile_for(&info.agent_type).checkpoints(),
            agent_id: info.agent_type,
            account: info.account,
            account_label: info.account_label,
            cwd: info.cwd,
            origin,
            companion: crate::companion::service::is_pane_id(pane_id),
            mission,
            checkpoints_on: checkpoints_enabled(),
        })
    }

    fn checkpoints(&self, pane_id: &str) -> Option<Vec<Checkpoint>> {
        crate::checkpoints::live().list(pane_id).ok()
    }

    fn summaries(&self, pane_id: &str) -> Vec<TurnSummary> {
        crate::turn_summary::tracker().summaries(pane_id)
    }

    fn git_state(&self, origin: &str) -> &'static str {
        worktrees::folder::git_state(Path::new(origin))
    }

    fn candidates(&self, source: &PaneFacts) -> Vec<Candidate> {
        let folder = source.cwd.as_str();
        let agents: Vec<_> =
            crate::tui::descriptors().into_iter().filter(|d| d.dispatchable && d.kind == crate::tui::Kind::Cli.as_str()).collect();
        let mut found: Vec<Candidate> = Vec::new();
        for agent in &agents {
            found.push(Candidate {
                target: Target { agent_id: agent.id.to_string(), account: None },
                label: agent.label.clone(),
                installed: agent.installed,
                runnable: runnable(agent.id, folder),
                limited_until: crate::handoff::agent_limit(agent.id, None),
            });
        }
        // The source's own saved login is offered too; no other logins in v1.
        if let Some(account) = source.account.as_deref() {
            if let Some(agent) = agents.iter().find(|d| d.id == source.agent_id) {
                let usable = runnable(agent.id, folder).and_then(|_| {
                    crate::accounts::validate_for_run(agent.id, account)
                        .map(|_| ())
                        .map_err(|_| "That saved login isn't available any more".to_string())
                });
                found.push(Candidate {
                    target: Target { agent_id: agent.id.to_string(), account: Some(account.to_string()) },
                    label: label_with(&agent.label, source.account_label.as_deref()),
                    installed: agent.installed,
                    runnable: usable,
                    limited_until: crate::handoff::agent_limit(agent.id, Some(account)),
                });
            }
        }
        found
    }

    fn when_text(&self, ms: u64) -> String {
        crate::handoff::reset::when_text(i64::try_from(ms).unwrap_or(i64::MAX), &SystemClock)
    }

    fn clean(&self, text: &str) -> String {
        let ctx = crate::redact::known::context();
        crate::handoff::keep_going::brief::clean_text(text, &ctx, ctx.home.as_deref())
    }

    fn make_copy(&self, request: &CopyRequest) -> Result<Made, OperationError> {
        // PM's own git calls on the copy run with no hooks (S8). The empty hooks folder is in the private store.
        let hooks = self.store.empty_hooks_dir().map_err(|_| {
            OperationError::new(
                ERR_BRANCH_COPY_FAILED,
                MSG_BRANCH_COPY_FAILED.replace("<git reason, cleaned>", "Puppet Master couldn't prepare its private folder"),
                true,
            )
        })?;
        let env = harden::git_env(&hooks);
        // A source that works in a copy: the new copy starts from that copy's branch.
        let start_point = copy::start_point_for(&request.source_cwd);
        copy::make(
            &request.origin,
            start_point.as_deref(),
            &request.handle,
            Path::new(&request.source_cwd),
            &request.pane_id,
            &request.checkpoint_id,
            &request.commit_message,
            &env,
        )
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

    fn worktree_closed(&self, origin: &str, handle: &str) -> Option<&'static str> {
        if let Some(copy) = worktrees::find_by_handle(origin, handle) {
            match copy.closed.as_deref() {
                None if Path::new(&copy.path).is_dir() => return None,
                Some("merged") => return Some("kept"),
                _ => return Some("thrown_away"),
            }
        }
        // The description goes with the copy; the run's record remembers how it was closed.
        match crate::operations::latest_agent_run(origin, handle).ok().and_then(|run| run.worker.worktree).and_then(|w| w.closed) {
            Some(how) if how == "merged" => Some("kept"),
            _ => Some("thrown_away"),
        }
    }

    fn audit(&self, who: Who, summary: &str, detail: Value) {
        crate::audit::record(Kind::Branch, who, summary, detail);
    }

    fn changed(&self, pane_ids: &[&str]) {
        #[cfg(not(test))]
        if let Some(app) = crate::bridge::live_app() {
            use tauri::Emitter;
            let _ = app.emit(CHANGED_EVENT, json!({ "paneIds": pane_ids }));
        }
        #[cfg(test)]
        let _ = pane_ids;
    }
}

/// The window event (`src/lib/branch-from.ts`: `BRANCH_CHANGED_EVENT`).
pub const CHANGED_EVENT: &str = "branch://changed";

/// The origin project of a pane's folder: the project its copy belongs to, else the folder itself.
fn origin_of(cwd: &str) -> String {
    let folder = worktrees::origin_of_cwd(cwd).unwrap_or_else(|| cwd.to_string());
    crate::project_path::normalize_project_path(Path::new(&folder)).map(|path| path.to_string_lossy().to_string()).unwrap_or(folder)
}

/// Whether an agent could start in `folder`, in plain words when not.
fn runnable(agent_id: &str, folder: &str) -> Result<(), String> {
    crate::companion::agents::agent_ok(agent_id, folder).map_err(str::to_string)
}

/// The separate copy whose checkout holds `cwd`, if any.
fn worktree_of(cwd: &str) -> Option<worktrees::WorktreeRef> {
    let origin = worktrees::origin_of_cwd(cwd)?;
    worktrees::list(&origin)
        .into_iter()
        .find(|entry| {
            entry.exists && entry.worktree.closed.is_none() && crate::project_path::workspace_covers(Path::new(&entry.worktree.path), Path::new(cwd))
        })
        .map(|entry| entry.worktree)
}

/// Save points are on unless the person turned them off in Settings (`checkpoints::runtime::LiveEnv`).
fn checkpoints_enabled() -> bool {
    crate::bridge::checkpoint_public_settings()
        .as_ref()
        .and_then(|settings| settings.get("checkpoints_enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
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
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// The run request for a branch's new agent. Only fields a branch sets; everything else keeps the run
/// default, so the new agent has no extra launch arguments and no extra rights. It runs in the copy
/// (`join_worktree`) under the no-push guard, with trust and shell off.
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
        "handle": request.handle,
        "trust_workspace": request.trust_workspace,
        "allow_shell": request.allow_shell,
        "allowed_commands": request.allowed_commands,
        "read_only": request.read_only,
        "context_policy": request.context_policy,
    }))
    .map_err(|error| OperationError::new("INVALID_ARGUMENT", error.to_string(), false))?;
    run.join_worktree = Some(request.join_worktree.clone());
    run.guard_env = Some(request.guard_env.clone());
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktrees::WorktreeRef;

    fn copy() -> WorktreeRef {
        WorktreeRef {
            path: "C:\\Users\\you\\proj\\.puppet-master\\worktrees\\bf-0123456789ab".into(),
            cwd: "C:\\Users\\you\\proj\\.puppet-master\\worktrees\\bf-0123456789ab".into(),
            branch: "pm/bf-0123456789ab".into(),
            base: "main".into(),
            base_commit: "abc".into(),
            repo: "C:\\Users\\you\\proj".into(),
            handle: "bf-0123456789ab".into(),
            created_at_ms: 0,
            closed: None,
            changes: None,
            setup: None,
            pr: None,
        }
    }

    fn request() -> StartRequest {
        StartRequest {
            project_path: "C:\\Users\\you\\proj".into(),
            task: "Branch: notes".into(),
            agent_type: "fake_b".into(),
            account: Some("work".into()),
            headless: false,
            background: true,
            keep_pane: true,
            wait_ms: 0,
            timeout_ms: 1_800_000,
            idempotency_key: "branch:bf-0123456789ab".into(),
            name: "Branch: try it".into(),
            handle: "bf-0123456789ab".into(),
            trust_workspace: false,
            allow_shell: false,
            allowed_commands: None,
            read_only: false,
            context_policy: "fresh".into(),
            guard_env: crate::pty::env_guard::NO_PUSH.into(),
            join_worktree: copy(),
        }
    }

    #[test]
    fn the_run_request_runs_in_the_copy_with_every_gate_closed_and_no_push() {
        let run = to_run_request(&request()).unwrap();
        assert_eq!(run.project_path.as_deref(), Some("C:\\Users\\you\\proj"));
        assert_eq!(run.task, "Branch: notes");
        assert_eq!(run.agent_type, "fake_b");
        assert_eq!(run.account.as_deref(), Some("work"));
        assert!(!run.headless && run.background && run.keep_pane);
        assert!(!run.trust_workspace && !run.allow_shell && !run.read_only);
        assert!(run.allowed_commands.is_none());
        assert_eq!(run.wait_ms, 0);
        assert_eq!(run.timeout_ms, 1_800_000);
        assert_eq!(run.idempotency_key.as_deref(), Some("branch:bf-0123456789ab"));
        assert_eq!(run.handle.as_deref(), Some("bf-0123456789ab"));
        assert_eq!(run.join_worktree.as_ref().map(|w| w.handle.as_str()), Some("bf-0123456789ab"));
        assert_eq!(run.guard_env.as_deref(), Some(crate::pty::env_guard::NO_PUSH));
        assert_eq!(format!("{:?}", run.context_policy).to_lowercase(), "fresh");
        assert!(run.pane_id.is_none() && run.worker_id.is_none() && run.model.is_none());
        assert!(run.budget_turns.is_none() && run.budget_minutes.is_none() && !run.until_checks_pass);
    }
}
