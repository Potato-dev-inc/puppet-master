//! `GET /agents`: which runs and adoptable panes a caller sees, with the optional list filter.

use crate::agent_runs::runtime::AgentRunView;
use crate::operations::{self, OperationError, OperationStatus};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

use super::wire::query_value;

/// Ended runs (completed/failed/cancelled) stay listed this long after they finish.
const ENDED_RUN_RETENTION_MS: u64 = 24 * 60 * 60 * 1000;

/// Latest turn of every run that is active or ended within the retention window, paired with
/// whether `session_id` owns it. Runs started by other connections are listed with
/// `owned == false`; listing them grants no control (take_over still needs grant=true).
pub(crate) fn visible_runs(
    operations: Vec<crate::operations::OperationSnapshot>,
    session_id: Option<&str>,
    now_ms: u64,
) -> Vec<(crate::operations::OperationSnapshot, bool)> {
    let mut latest = BTreeMap::<String, crate::operations::OperationSnapshot>::new();
    for snapshot in operations {
        let handle = if snapshot.agent_run_id.is_empty() {
            snapshot.operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        };
        let should_replace = latest
            .get(&handle)
            .map_or(true, |existing| snapshot.turn_index >= existing.turn_index);
        if should_replace {
            latest.insert(handle, snapshot);
        }
    }
    latest
        .into_values()
        .filter(|snapshot| {
            let ended = matches!(
                snapshot.status,
                OperationStatus::Completed | OperationStatus::Failed | OperationStatus::Cancelled
            );
            !ended
                || snapshot.finished_at_ms.map_or(true, |finished| {
                    now_ms.saturating_sub(finished) <= ENDED_RUN_RETENTION_MS
                })
        })
        .map(|snapshot| {
            let handle = if snapshot.agent_run_id.is_empty() {
                snapshot.operation_id.as_str()
            } else {
                snapshot.agent_run_id.as_str()
            };
            let owned = session_id.map_or(true, |session| {
                snapshot.owner_session_id.as_deref() == Some(session)
                    || crate::mcp_sessions::check_run_access(session, handle).is_ok()
            });
            (snapshot, owned)
        })
        .collect()
}

/// Optional `list_agents` narrowing; the default lists everything visible.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct AgentListFilter {
    /// Only runs/panes this connection controls.
    pub mine_only: bool,
    /// Only runs that have not ended.
    pub active_only: bool,
    /// Hide ended runs that finished longer ago than this many ms.
    pub ended_within_ms: Option<u64>,
}

impl AgentListFilter {
    pub(super) fn from_query(query: &str) -> Self {
        let flag = |key: &str| {
            query_value(query, key).is_some_and(|value| matches!(value.as_str(), "1" | "true"))
        };
        Self {
            mine_only: flag("mine_only"),
            active_only: flag("active_only"),
            ended_within_ms: query_value(query, "ended_within_ms")
                .and_then(|value| value.parse().ok()),
        }
    }

    pub(crate) fn keeps(
        &self,
        snapshot: &crate::operations::OperationSnapshot,
        owned: bool,
        now_ms: u64,
    ) -> bool {
        if self.mine_only && !owned {
            return false;
        }
        if snapshot.status.terminal() {
            if self.active_only {
                return false;
            }
            if let (Some(window), Some(finished)) = (self.ended_within_ms, snapshot.finished_at_ms)
            {
                return now_ms.saturating_sub(finished) <= window;
            }
        }
        true
    }
}

pub(super) fn list_agents(
    project: &str,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    explicit_project: bool,
    filter: AgentListFilter,
) -> Result<Value, OperationError> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let operations = if explicit_project {
        operations::list_operations(project)?
    } else {
        operations::list_indexed_operations(project)?
    };
    let live: BTreeMap<String, crate::pty::PaneInfo> = registry
        .lock()
        .list()
        .into_iter()
        .filter(|pane| !pane.id.starts_with("puppet-master-orchestrator-"))
        .map(|pane| (pane.id.clone(), pane))
        .collect();
    let mut claimed = std::collections::BTreeSet::new();
    let mut live_workers = Vec::new();
    let mut other: Vec<(crate::operations::OperationSnapshot, Value)> = Vec::new();
    for (snapshot, owned) in visible_runs(operations, session_id, now_ms) {
        if should_hide_from_agent_list(&snapshot) || !filter.keeps(&snapshot, owned, now_ms) {
            continue;
        }
        let live_pane = snapshot
            .pane_id
            .as_deref()
            .and_then(|pane_id| live.get(pane_id));
        if let Some(pane) = live_pane {
            if snapshot.status.terminal() {
                continue;
            }
            claimed.insert(pane.id.clone());
            live_workers.push(run_worker_entry(&snapshot, owned, session_id, registry));
            continue;
        }
        // Ended runs within the retention window are listed whether or not the caller named a
        // project: an agent that just finished a run and asks "what do I have?" must find it
        // again without its handle.
        other.push((snapshot.clone(), run_worker_entry(&snapshot, owned, session_id, registry)));
    }
    // Live panes first (below), then runs that are still active without a live pane, then ended
    // runs, newest first.
    other.sort_by_key(|(snapshot, _)| {
        (
            snapshot.status.terminal(),
            std::cmp::Reverse(snapshot.finished_at_ms.unwrap_or(snapshot.created_at_ms)),
        )
    });
    let other: Vec<Value> = other.into_iter().map(|(_, entry)| entry).collect();
    for pane in live.values() {
        let hidden_by_mine = filter.mine_only
            && !session_id.is_some_and(|session| {
                crate::mcp_sessions::session_controls_pane(session, &pane.id)
            });
        if !claimed.contains(&pane.id) && !hidden_by_mine {
            live_workers.push(pane_worker_entry(pane, session_id));
        }
    }
    live_workers.extend(other);
    Ok(json!({"workers": live_workers}))
}

pub(super) fn should_hide_from_agent_list(snapshot: &crate::operations::OperationSnapshot) -> bool {
    matches!(
        snapshot.stage.as_deref(),
        Some("interrupted") | Some("closed")
    )
}

pub(super) fn run_worker_entry(
    snapshot: &crate::operations::OperationSnapshot,
    owned: bool,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Value {
    let view = AgentRunView::present(snapshot.clone(), Some(registry));
    let grant_required = snapshot.pane_id.as_deref().is_some_and(|pane_id| {
        session_id
            .is_some_and(|session| !crate::mcp_sessions::session_controls_pane(session, pane_id))
    });
    let workspace = snapshot
        .worker
        .workspace
        .clone()
        .unwrap_or_else(|| snapshot.project_path.clone());
    let pane = snapshot.pane_id.as_deref().and_then(|id| {
        registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == id)
    });
    json!({
        "worker_id": view.handle,
        "handle": view.handle,
        "name": snapshot.worker.name,
        "pane_id": snapshot.pane_id,
        "backend": snapshot.agent_type,
        "workspace": pane.as_ref().map(|pane| pane.cwd.clone()).unwrap_or(workspace),
        "status": view.status,
        "model": snapshot.worker.resolved_model.clone().or(snapshot.worker.requested_model.clone()),
        "owned": owned,
        "grant_required": grant_required,
        "read_only_supported": crate::agent_runs::capabilities::read_only_supported_for(&snapshot.agent_type, snapshot.pane_id.as_deref()),
        "adoptable": pane
            .as_ref()
            .map(|pane| pane.status != "error")
            .unwrap_or(owned || !grant_required),
    })
}

pub(super) fn pane_worker_entry(pane: &crate::pty::PaneInfo, session_id: Option<&str>) -> Value {
    let grant_required = session_id
        .is_some_and(|session| !crate::mcp_sessions::session_controls_pane(session, &pane.id));
    json!({
        "worker_id": pane.id,
        "handle": pane.id,
        "pane_id": pane.id,
        "backend": pane.agent_type,
        "workspace": pane.cwd,
        "status": pane.status,
        "model": Value::Null,
        "owned": !grant_required,
        "grant_required": grant_required,
        "read_only_supported": crate::agent_runs::capabilities::read_only_supported_for(&pane.agent_type, Some(&pane.id)),
        "adoptable": pane.status != "error",
    })
}
