//! What the person can do to a budget: add turns, add time, stop here.

use super::model::{BudgetRecord, MAX_MINUTES, MAX_TURNS};
use super::store;
use crate::operations::OperationError;

/// The default "Add" step: half of the limit the run started with, rounded up, at least one.
pub fn step_turns(record: &BudgetRecord) -> Option<u32> {
    record.first_turns.map(|turns| turns.div_ceil(2).max(1))
}

pub fn step_minutes(record: &BudgetRecord) -> Option<u32> {
    record.first_minutes.map(|minutes| minutes.div_ceil(2).max(1))
}

fn not_found(handle: &str) -> OperationError {
    OperationError::new("BUDGET_NOT_FOUND", format!("{handle} has no budget"), false)
}

/// Runs `change` on the run's record and on every record above it. A helper's share can never
/// be larger than its parent's, so extra turns for a helper are extra turns for its parent too.
fn raise_chain(
    project: &str,
    handle: &str,
    change: &dyn Fn(&mut BudgetRecord) -> bool,
) -> Result<BudgetRecord, OperationError> {
    let first = store::update(project, handle, |record| {
        change(&mut *record);
        record.stopped = false;
        record.clone()
    })
    .ok_or_else(|| not_found(handle))?;
    let mut up = (first.parent.clone(), first.parent_project.clone().unwrap_or_else(|| first.project.clone()));
    // Depth is capped, so this ends; the bound only guards against a damaged file with a loop.
    for _ in 0..8 {
        let (Some(parent), parent_project) = up else { break };
        let next = store::update(&parent_project, &parent, |record| {
            change(&mut *record);
            (record.parent.clone(), record.parent_project.clone().unwrap_or_else(|| record.project.clone()))
        });
        match next {
            Some(next) => up = (next.0, next.1),
            None => break,
        }
    }
    Ok(first)
}

/// Adds turns to a run's limit. Fails when the run is not limited by turns.
pub fn add_turns(project: &str, handle: &str, turns: u32) -> Result<BudgetRecord, OperationError> {
    let current = store::get(project, handle).ok_or_else(|| not_found(handle))?;
    if current.turns.is_none() {
        return Err(OperationError::new("INVALID_ARGUMENT", format!("{handle} has no turn limit to add to"), false));
    }
    raise_chain(project, handle, &|record| match record.turns.as_mut() {
        Some(limit) => {
            *limit = limit.saturating_add(turns).min(MAX_TURNS * 10);
            true
        }
        None => false,
    })
}

/// Adds minutes to a run's time limit. Fails when the run is not limited by time.
pub fn add_minutes(project: &str, handle: &str, minutes: u32) -> Result<BudgetRecord, OperationError> {
    let current = store::get(project, handle).ok_or_else(|| not_found(handle))?;
    if current.minutes.is_none() {
        return Err(OperationError::new("INVALID_ARGUMENT", format!("{handle} has no time limit to add to"), false));
    }
    raise_chain(project, handle, &|record| match record.minutes.as_mut() {
        Some(limit) => {
            *limit = limit.saturating_add(minutes).min(MAX_MINUTES * 10);
            true
        }
        None => false,
    })
}

/// "Stop here": no more turns for this run, whatever is left of its budget.
pub fn stop(project: &str, handle: &str) -> Result<BudgetRecord, OperationError> {
    store::update(project, handle, |record| {
        record.stopped = true;
        record.clone()
    })
    .ok_or_else(|| not_found(handle))
}
