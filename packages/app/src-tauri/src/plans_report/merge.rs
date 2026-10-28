//! One list of turns from two records of the same thing. A run's turn is both an operation record
//! (agent, account, start, end) and a turn summary (files changed); a turn typed into a pane by hand
//! has only a summary. They are joined by pane and time so no turn is counted twice.

use super::model::{FilesFact, TurnFact};
use crate::operations::OperationSnapshot;
use crate::turn_summary::TurnSummary;
use std::collections::HashSet;

/// A summary and a record of the same turn start and end within this long of each other.
const SLACK_MS: u64 = 15_000;

/// An operation record cut down to what the report needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpTurn {
    pub pane_id: Option<String>,
    pub agent_id: String,
    pub account: Option<String>,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub merged: bool,
}

impl OpTurn {
    /// `None` for a turn that never started (the same rule the budgets use).
    pub fn of(op: &OperationSnapshot) -> Option<Self> {
        if !crate::budgets::counts_as_turn(op) {
            return None;
        }
        let start_ms = op.started_at_ms.unwrap_or(op.created_at_ms);
        let merged = op.worker.worktree.as_ref().and_then(|w| w.closed.as_deref()) == Some("merged");
        Some(Self {
            pane_id: op.pane_id.clone(),
            agent_id: op.agent_type.clone(),
            account: op.worker.account.clone().filter(|a| !a.trim().is_empty()),
            start_ms,
            end_ms: op.finished_at_ms,
            merged,
        })
    }

    fn busy_ms(&self) -> Option<u64> {
        self.end_ms.map(|end| end.saturating_sub(self.start_ms))
    }
}

fn overlaps(op: &OpTurn, summary: &TurnSummary) -> bool {
    let op_end = op.end_ms.unwrap_or(op.start_ms);
    summary.started_at_ms <= op_end.saturating_add(SLACK_MS) && summary.ended_at_ms.saturating_add(SLACK_MS) >= op.start_ms
}

fn files_of(summary: &TurnSummary) -> Option<FilesFact> {
    summary.files.as_ref().map(|f| FilesFact { count: f.count, added: f.added, deleted: f.deleted })
}

/// The same turn can arrive from the memory of this run and from a project's ring file.
fn unique(summaries: &[TurnSummary]) -> Vec<&TurnSummary> {
    let mut seen = HashSet::new();
    summaries.iter().filter(|s| seen.insert((s.pane_id.clone(), s.turn, s.started_at_ms))).collect()
}

/// Every turn once. `agent_of_label` turns a summary's display name into an agent id for summaries
/// saved before they carried the id.
pub fn merge(ops: &[OpTurn], summaries: &[TurnSummary], agent_of_label: &dyn Fn(&str) -> Option<String>) -> Vec<TurnFact> {
    let summaries = unique(summaries);
    let mut taken = vec![false; ops.len()];
    let mut out = Vec::with_capacity(ops.len() + summaries.len());

    for summary in &summaries {
        let same_turn = ops
            .iter()
            .enumerate()
            .filter(|(i, op)| !taken[*i] && op.pane_id.is_some() && op.pane_id.as_deref() == Some(summary.pane_id.as_str()) && overlaps(op, summary))
            .min_by_key(|(_, op)| op.start_ms.abs_diff(summary.started_at_ms));
        match same_turn {
            Some((i, op)) => {
                taken[i] = true;
                out.push(TurnFact {
                    agent_id: op.agent_id.clone(),
                    account: op.account.clone().or_else(|| summary.account.clone()),
                    started_ms: op.start_ms,
                    busy_ms: op.busy_ms().unwrap_or(summary.duration_ms),
                    files: files_of(summary),
                    merged: op.merged,
                });
            }
            None => out.push(TurnFact {
                agent_id: summary
                    .agent_id
                    .clone()
                    .or_else(|| agent_of_label(&summary.agent))
                    .unwrap_or_else(|| summary.agent.clone()),
                account: summary.account.clone(),
                started_ms: summary.started_at_ms,
                busy_ms: summary.duration_ms,
                files: files_of(summary),
                merged: false,
            }),
        }
    }
    for op in ops.iter().enumerate().filter(|(i, _)| !taken[*i]).map(|(_, op)| op) {
        out.push(TurnFact {
            agent_id: op.agent_id.clone(),
            account: op.account.clone(),
            started_ms: op.start_ms,
            busy_ms: op.busy_ms().unwrap_or(0),
            files: None,
            merged: op.merged,
        });
    }
    out
}
