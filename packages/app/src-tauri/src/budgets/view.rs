//! The budget as the cockpit shows it: the header chip and its popover.

use super::accounting::{standing, Standing};
use super::adjust;
use super::model::{BudgetRecord, NEARLY_PERCENT};
use super::store;
use crate::operations;
use serde_json::{json, Value};

/// `ok` | `nearly` (80% or more) | `used_up` | `stopped`
fn state_of(standing: &Standing) -> &'static str {
    if standing.stopped {
        return "stopped";
    }
    if standing.exhausted().is_some() {
        return "used_up";
    }
    let turns_nearly = standing
        .turns_limit
        .is_some_and(|limit| limit > 0 && standing.committed() * 100 >= limit * NEARLY_PERCENT);
    let time_nearly = standing
        .minutes_limit
        .is_some_and(|minutes| minutes > 0 && standing.elapsed_ms * 100 >= u64::from(minutes) * 60_000 * u64::from(NEARLY_PERCENT));
    if turns_nearly || time_nearly {
        "nearly"
    } else {
        "ok"
    }
}

fn label(agent_type: &str) -> String {
    crate::tui::descriptors()
        .into_iter()
        .find(|descriptor| descriptor.id == agent_type)
        .map(|descriptor| descriptor.label)
        .unwrap_or_else(|| agent_type.to_string())
}

pub fn view_of(record: &BudgetRecord, now_ms: u64) -> Value {
    let state = standing(record, now_ms);
    let running = operations::latest_agent_run(&record.project, &record.handle)
        .map(|turn| !turn.status.terminal())
        .unwrap_or(false);
    let helpers: Vec<Value> = state
        .helpers
        .iter()
        .map(|helper| {
            json!({
                "agent": label(&helper.agent_type),
                "used": helper.used,
                "limit": helper.limit,
                "closed": helper.closed,
            })
        })
        .collect();
    json!({
        "state": state_of(&state),
        "depth": record.depth,
        "is_helper": record.parent.is_some(),
        "turns": record.turns.map(|limit| json!({
            "used": state.committed(),
            "own": state.turns_own,
            "held_for_helpers": state.turns_reserved,
            "limit": limit,
            "add_step": adjust::step_turns(record),
        })),
        "minutes": record.minutes.map(|limit| json!({
            "used": state.elapsed_ms / 60_000,
            "limit": limit,
            "add_step": adjust::step_minutes(record),
        })),
        // The chip reads "Time's up · pauses after this turn" while a turn is still running.
        "time_up_in_turn": running && state.ms_left() == Some(0) && !state.stopped,
        "helpers": helpers,
    })
}

/// The budget of a run, or `None` when it has none.
pub fn view(project: &str, handle: &str) -> Option<Value> {
    let record = store::get(project, handle)?;
    Some(view_of(&record, crate::questions::now_ms()))
}
