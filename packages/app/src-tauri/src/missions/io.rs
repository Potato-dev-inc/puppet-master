//! The live `RoundIo`: turns are ordinary `run_agent` / `followup_task` operations in visible panes,
//! budgets are the budgets module, the loop guard is the loop guard.

use super::engine::{RoundIo, Started};
use super::model::{Mission, Round, RoundEnd};
use crate::agent_runs::persist::FollowupOpts;
use crate::agent_runs::{AgentRunRequest, AgentRunView};
use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus};
use std::path::Path;
use std::time::Duration;

pub struct LiveIo;

fn no_app() -> OperationError {
    OperationError::new("APP_NOT_READY", "Puppet Master is still starting", true)
}

fn started(view: &AgentRunView) -> Started {
    Started { operation_id: view.operation_id.clone(), handle: view.handle.clone(), pane_id: view.pane_id.clone() }
}

/// What is left of the mission's budget for the next agent: the turns not yet spent and the
/// minutes left on the mission's clock.
fn budget_left(m: &Mission) -> (Option<u32>, Option<u32>) {
    let record = crate::budgets::record_of(&m.project, &m.handle());
    let turns = record.as_ref().and_then(|r| r.turns).map(|limit| limit.saturating_sub(m.turns_used()).max(1));
    let minutes = record.as_ref().and_then(|r| r.minutes.map(|mins| (mins, r.started_ms))).map(|(mins, started)| {
        let elapsed_min = super::store::now_ms().saturating_sub(started).div_ceil(60_000) as u32;
        mins.saturating_sub(elapsed_min).max(1)
    });
    (turns, minutes)
}

fn end_of(snapshot: &OperationSnapshot) -> RoundEnd {
    let status = match snapshot.status {
        OperationStatus::Completed => "completed",
        OperationStatus::Cancelled => "cancelled",
        _ => "failed",
    };
    RoundEnd {
        status: status.into(),
        code: snapshot.error.as_ref().map(|e| e.code.clone()),
        message: snapshot.error.as_ref().map(|e| e.message.clone()),
        pane_id: snapshot.pane_id.clone(),
    }
}

impl RoundIo for LiveIo {
    fn start_round(&self, m: &Mission, r: &Round, prompt: &str) -> Result<Started, OperationError> {
        let registry = crate::bridge::live_registry().ok_or_else(no_app)?;
        let app = crate::bridge::live_app().ok_or_else(no_app)?;
        let agent = m.agents.get(r.agent).cloned().ok_or_else(|| OperationError::new("INVALID_ARGUMENT", "no such agent in the mission", false))?;
        if let (true, Some(handle)) = (r.kind.same_handle(), r.handle.as_deref()) {
            let view = crate::agent_runs::send_agent(
                &m.project,
                handle,
                prompt.to_string(),
                None,
                registry,
                app,
                FollowupOpts {
                    background: true,
                    wait_ms: Some(0),
                    idempotency_key: Some(r.idempotency_key.clone()),
                    ..Default::default()
                },
            )?;
            return Ok(started(&view));
        }
        let (turns, minutes) = budget_left(m);
        let mut request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "project_path": m.project,
            "task": prompt,
            "agent_type": agent.agent_type,
            "account": agent.account,
            "model": agent.model,
            "headless": false,
            "background": true,
            "keep_pane": true,
            "wait_ms": 0,
            "timeout_ms": m.round_timeout_ms,
            "idempotency_key": r.idempotency_key,
            "budget_turns": turns,
            "budget_minutes": minutes,
            "budget_parent": m.handle(),
        }))
        .map_err(|e| OperationError::new("INVALID_ARGUMENT", e.to_string(), false))?;
        request.join_worktree = m.worktree.clone();
        let view = crate::agent_runs::run_agent(request, registry, app, None)?;
        Ok(started(&view))
    }

    fn find_by_key(&self, project: &str, key: &str) -> Option<Started> {
        let found = operations::find_by_idempotency_key(project, key).ok().flatten()?;
        let handle = if found.agent_run_id.is_empty() { found.operation_id.clone() } else { found.agent_run_id.clone() };
        Some(Started { operation_id: found.operation_id, handle, pane_id: found.pane_id })
    }

    fn wait_round(&self, project: &str, operation_id: &str, interrupt: &dyn Fn() -> bool) -> Option<RoundEnd> {
        let mut after = 0;
        loop {
            if interrupt() {
                return None;
            }
            match operations::wait_for_operation(project, operation_id, after, Some(vec![OperationStatus::Completed, OperationStatus::Failed, OperationStatus::Cancelled]), 1_000) {
                Ok(result) => {
                    if result.snapshot.status.terminal() {
                        return Some(end_of(&result.snapshot));
                    }
                    after = result.snapshot.revision;
                }
                Err(error) => {
                    // The record is gone: the turn can't be read any more; treat it as ended.
                    return Some(RoundEnd { status: "failed".into(), code: Some(error.code), message: Some(error.message), pane_id: None });
                }
            }
        }
    }

    fn cancel_round(&self, project: &str, handle: &str) {
        if let (Some(registry), Some(app)) = (crate::bridge::live_registry(), crate::bridge::live_app()) {
            let _ = crate::agent_runs::cancel_agent(project, handle, None, registry, app);
        }
    }

    fn budget_gate(&self, project: &str, key: &str) -> Result<(), OperationError> {
        crate::budgets::check_next_turn(project, key)
    }

    fn budget_stopped(&self, project: &str, key: &str) -> bool {
        crate::budgets::record_of(project, key).is_some_and(|r| r.stopped)
    }

    fn budget_stop(&self, project: &str, handle: &str) {
        let _ = crate::budgets::stop(project, handle);
    }

    fn budget_add_turns(&self, project: &str, key: &str, turns: u32) {
        let _ = crate::budgets::add_turns(project, key, turns);
    }

    fn loop_signal(&self, pane_id: &str) -> Option<String> {
        crate::loop_guard::status(pane_id).map(|s| s.summary)
    }

    fn dismiss_loop(&self, pane_id: &str) {
        crate::loop_guard::dismiss(pane_id);
    }

    fn panes_running_in(&self, copy: &Path) -> bool {
        let Some(registry) = crate::bridge::live_registry() else { return false };
        let copy = copy.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        let panes = registry.lock().list();
        panes.iter().any(|p| p.status == "running" && p.cwd.replace('\\', "/").to_ascii_lowercase().starts_with(&copy))
    }

    fn stall(&self) -> u32 {
        crate::loop_guard::settings().turns.clamp(3, 10)
    }

    fn poll(&self) -> Duration {
        Duration::from_secs(2)
    }
}
