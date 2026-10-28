//! Budgets: a brake on how much an agent run may do before it must ask a person.
//!
//! - A run can hold a budget of turns and/or wall-clock minutes (`budget_turns` /
//!   `budget_minutes` on `run_agent`, or the defaults in Settings; both are OFF unless set).
//! - Helpers an agent starts with `run_agent` draw their share from the parent's remaining
//!   budget, at most two levels below the first budget.
//! - Turns are counted from the operation records. Provider usage and tokens are not read.
//! - When a budget is used up the run is PAUSED BEFORE ITS NEXT TURN, never stopped mid-turn:
//!   the caller gets `BUDGET_EXHAUSTED` and an Inbox card offers "Add 10 turns / Stop here".
//!
//! Layout: `model` (types), `store` (records on disk), `accounting` (what has been used),
//! `admit` (a new run's budget and the helper rules), `gate` (the pause), `card` (the Inbox
//! card), `adjust` (add / stop), `view` (the header chip), `errors`, `pane_usage` (turns and busy
//! time per helper pane, rolled up through `parent_pane_id`).
//!
//! Escalating a helper's questions to its parent first (roadmap T3, P2) is not built here.

mod accounting;
mod adjust;
mod admit;
mod card;
mod errors;
mod gate;
mod model;
mod pane_usage;
mod store;
mod view;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;

pub(crate) use accounting::counts_as_turn;
pub use adjust::{add_minutes, add_turns, step_minutes, step_turns, stop};
pub use admit::{admit, discard, validate};
pub use card::{on_answered, Outcome, KIND as CARD_KIND};
pub use gate::{check_next_turn, turn_ended};
pub use model::{Asked, Defaults, PARENT_ENV};
pub use pane_usage::{usage_of_pane, PaneUsage};
pub use store::get as record_of;
pub use view::view;

/// The popover's "Add 10 turns" / "Add 30 min" / "Stop here": the same step the Inbox card offers.
pub fn adjust_by_step(project: &str, handle: &str, action: &str) -> Result<String, crate::operations::OperationError> {
    let record = store::get(project, handle)
        .ok_or_else(|| crate::operations::OperationError::new("BUDGET_NOT_FOUND", format!("{handle} has no budget"), false))?;
    let invalid = |what: &str| crate::operations::OperationError::new("INVALID_ARGUMENT", format!("no {what} limit to add to"), false);
    match action {
        card::CHOICE_ADD_TURNS => {
            let step = step_turns(&record).ok_or_else(|| invalid("turn"))?;
            add_turns(project, handle, step)?;
            Ok(format!("Added {step} {}.", if step == 1 { "turn" } else { "turns" }))
        }
        card::CHOICE_ADD_MINUTES => {
            let step = step_minutes(&record).ok_or_else(|| invalid("time"))?;
            add_minutes(project, handle, step)?;
            Ok(format!("Added {step} min."))
        }
        card::CHOICE_STOP => {
            stop(project, handle)?;
            Ok("Stopped at its budget.".to_string())
        }
        other => Err(crate::operations::OperationError::new("INVALID_ARGUMENT", format!("unknown budget action {other}"), false)),
    }
}

/// Hook for the routes that answer a question: applies the choice when the question was a pause
/// card. Returns the plain sentence to show the person ("Added 10 turns."), if any.
pub fn answer_applied(question: &crate::questions::Question) -> Option<String> {
    on_answered(question).map(|outcome| match outcome {
        Outcome::AddedTurns(n) => format!("Added {n} {}.", if n == 1 { "turn" } else { "turns" }),
        Outcome::AddedMinutes(n) => format!("Added {n} min."),
        Outcome::Stopped => "Stopped at its budget.".to_string(),
    })
}
