//! What a run has used. Turns are counted from the operation records (one operation per turn),
//! never from tokens or provider usage; time from the moment the run was admitted.

use super::model::{BudgetRecord, MAX_DEPTH};
use super::store;
use crate::operations::{self, OperationSnapshot, OperationStatus};

/// A turn that was created and not withdrawn before it ever started. A cancelled turn that never
/// started (a superseded trust prompt, a failed lock) used nothing.
fn counts_as_turn(turn: &OperationSnapshot) -> bool {
    !(turn.status == OperationStatus::Cancelled && turn.started_at_ms.is_none())
}

/// Turns this run (and only this run) has started or queued.
pub fn turns_used(project: &str, handle: &str) -> u32 {
    operations::turns_for_handle(project, handle)
        .map(|turns| turns.iter().filter(|turn| counts_as_turn(turn)).count() as u32)
        .unwrap_or(0)
}

/// Turns this run and every helper below it actually used.
pub fn turns_used_in_tree(project: &str, handle: &str) -> u32 {
    tree_sum(project, handle, 0)
}

fn tree_sum(project: &str, handle: &str, level: u32) -> u32 {
    let own = turns_used(project, handle);
    if level > MAX_DEPTH {
        return own;
    }
    own + store::children_of(project, handle)
        .iter()
        .map(|(child_project, child)| tree_sum(child_project, &child.handle, level + 1))
        .sum::<u32>()
}

fn is_closed(project: &str, handle: &str) -> bool {
    operations::latest_agent_run(project, handle).map(|turn| turn.worker.closed).unwrap_or(false)
}

fn agent_type_of(project: &str, handle: &str) -> String {
    operations::latest_agent_run(project, handle).map(|turn| turn.agent_type).unwrap_or_default()
}

/// One helper that draws from a run's budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Helper {
    pub handle: String,
    pub project: String,
    pub agent_type: String,
    /// Turns the helper (and its own helpers) used.
    pub used: u32,
    pub limit: Option<u32>,
    pub closed: bool,
}

impl Helper {
    /// What this helper holds back from its parent's budget: its whole share while it can still
    /// work, only what it spent once it is closed (the rest is refunded).
    fn reserved(&self) -> u32 {
        if self.closed {
            self.used
        } else {
            self.limit.unwrap_or(self.used).max(self.used)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The person chose "Stop here".
    Stopped,
    Turns,
    Time,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Turns => "turns",
            Self::Time => "time",
        }
    }
}

/// A run's budget set against what it has used right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub turns_limit: Option<u32>,
    pub turns_own: u32,
    /// Turns handed to helpers.
    pub turns_reserved: u32,
    pub minutes_limit: Option<u32>,
    pub elapsed_ms: u64,
    pub stopped: bool,
    pub helpers: Vec<Helper>,
}

impl Standing {
    /// Turns spent by this run plus the shares it handed out.
    pub fn committed(&self) -> u32 {
        self.turns_own + self.turns_reserved
    }

    pub fn turns_left(&self) -> Option<u32> {
        self.turns_limit.map(|limit| limit.saturating_sub(self.committed()))
    }

    pub fn ms_left(&self) -> Option<u64> {
        self.minutes_limit.map(|minutes| u64::from(minutes) * 60_000).map(|limit| limit.saturating_sub(self.elapsed_ms))
    }

    /// Why no further turn may start, or `None` when one may.
    pub fn exhausted(&self) -> Option<Reason> {
        if self.stopped {
            Some(Reason::Stopped)
        } else if self.turns_left() == Some(0) {
            Some(Reason::Turns)
        } else if self.ms_left() == Some(0) {
            Some(Reason::Time)
        } else {
            None
        }
    }
}

pub fn standing(record: &BudgetRecord, now_ms: u64) -> Standing {
    let helpers: Vec<Helper> = store::children_of(&record.project, &record.handle)
        .into_iter()
        .map(|(project, child)| Helper {
            used: tree_sum(&project, &child.handle, 1),
            agent_type: agent_type_of(&project, &child.handle),
            closed: is_closed(&project, &child.handle),
            limit: child.turns,
            handle: child.handle,
            project,
        })
        .collect();
    Standing {
        turns_limit: record.turns,
        turns_own: turns_used(&record.project, &record.handle),
        turns_reserved: helpers.iter().map(Helper::reserved).sum(),
        minutes_limit: record.minutes,
        elapsed_ms: now_ms.saturating_sub(record.started_ms),
        stopped: record.stopped,
        helpers,
    }
}
