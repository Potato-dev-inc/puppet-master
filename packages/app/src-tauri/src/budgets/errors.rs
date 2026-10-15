//! The `BUDGET_EXHAUSTED` error and its guidance for the agent that called.

use super::accounting::{Reason, Standing};
use crate::operations::OperationError;
use serde_json::json;

pub const CODE: &str = "BUDGET_EXHAUSTED";

fn error(message: String, context: serde_json::Value) -> OperationError {
    let mut error = OperationError::new(CODE, message, true);
    error.context = context;
    error
}

/// A run asked for another turn and its budget is used up (or the person stopped it).
pub fn run_exhausted(handle: &str, reason: Reason, standing: &Standing) -> OperationError {
    let used = standing.committed();
    let message = match reason {
        Reason::Stopped => format!(
            "The person chose Stop here for {handle}: it has reached its budget and will not get more turns. Do not send it another turn; use what it already produced."
        ),
        Reason::Turns => format!(
            "Budget used up for {handle}: {used} of {} turns. The run is paused, not killed, and no turn was cut short. Puppet Master has put a card in the person's Inbox (Add turns / Stop here). Do not retry in a loop: tell the person you are waiting for them, or finish with what you have. Once they add turns, call followup_task again.",
            standing.turns_limit.unwrap_or(used)
        ),
        Reason::Time => format!(
            "Time budget used up for {handle}: {} of {} minutes. The run is paused, not killed, and no turn was cut short. Puppet Master has put a card in the person's Inbox (Add time / Stop here). Do not retry in a loop: tell the person you are waiting for them, or finish with what you have.",
            standing.elapsed_ms / 60_000,
            standing.minutes_limit.unwrap_or(0)
        ),
    };
    error(
        message,
        json!({
            "handle": handle,
            "reason": reason.as_str(),
            "turns_used": used,
            "turns_limit": standing.turns_limit,
            "minutes_used": standing.elapsed_ms / 60_000,
            "minutes_limit": standing.minutes_limit,
        }),
    )
}

/// A helper could not be started because the budget it would draw from is used up.
pub fn helper_parent_exhausted(parent: &str, reason: Reason, standing: &Standing) -> OperationError {
    let mut e = run_exhausted(parent, reason, standing);
    e.message = format!(
        "Couldn't start a helper: budget used up ({parent} has nothing left to share). {}",
        if reason == Reason::Stopped {
            "The person stopped this run; do not start more helpers."
        } else {
            "Finish the work yourself, or ask the person to add turns, then try again."
        }
    );
    e
}

/// The helper asked for more than the parent has left.
pub fn more_than_left(what: &str, asked: u32, left: u32, parent: &str) -> OperationError {
    let flag = if what == "turns" { "budget_turns" } else { "budget_minutes" };
    error(
        format!(
            "Couldn't start a helper: you asked for {asked} {what} but only {left} are left in your budget. Ask for at most {left} ({flag}), or leave {flag} out to get half of what is left."
        ),
        json!({"reason": "more_than_left", "unit": what, "asked": asked, "left": left, "parent": parent}),
    )
}

/// Helpers may only go `max_depth` levels below the first budget.
pub fn depth_limit(parent: &str, max_depth: u32) -> OperationError {
    error(
        format!(
            "Couldn't start a helper: helpers can't start their own helpers more than {max_depth} levels deep. Do this work yourself, or ask the person to start it."
        ),
        json!({"reason": "depth_limit", "parent": parent, "max_depth": max_depth}),
    )
}
