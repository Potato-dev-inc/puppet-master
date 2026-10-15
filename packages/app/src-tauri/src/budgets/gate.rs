//! The pause gate: no turn starts on a run whose budget is used up. A turn that is running is
//! never touched; the gate only decides whether the NEXT one may begin.

use super::accounting::{standing, Reason};
use super::card;
use super::errors;
use super::store;
use crate::operations::{OperationError, OperationSnapshot};

fn now_ms() -> u64 {
    crate::questions::now_ms()
}

/// Whether the run may start another turn now. A run without a budget always may (today's
/// behaviour). When it may not, the person's card is raised (once) and the caller gets
/// `BUDGET_EXHAUSTED` with guidance.
pub fn check_next_turn(project: &str, handle: &str) -> Result<(), OperationError> {
    check_next_turn_at(project, handle, now_ms())
}

pub fn check_next_turn_at(project: &str, handle: &str, now: u64) -> Result<(), OperationError> {
    let Some(record) = store::get(project, handle) else { return Ok(()) };
    let state = standing(&record, now);
    match state.exhausted() {
        None => Ok(()),
        Some(reason) => {
            // The card is in the Inbox (and `GET /attention`) before the caller is told it is there:
            // the 409 used to land seconds ahead of the card (VERIFY-R13 #6).
            card::raise(&record, &state, reason);
            crate::attention::settle();
            Err(errors::run_exhausted(handle, reason, &state))
        }
    }
}

/// A turn reached a final state. If that used up the budget, the person hears about it now, not
/// only when someone tries to start the next turn: the run sits paused after its last turn.
pub fn turn_ended(turn: &OperationSnapshot) {
    if !turn.status.terminal() || turn.agent_run_id.is_empty() {
        return;
    }
    let Some(record) = store::get(&turn.project_path, &turn.agent_run_id) else { return };
    let state = standing(&record, now_ms());
    if let Some(reason) = state.exhausted() {
        if reason != Reason::Stopped {
            card::raise(&record, &state, reason);
        }
    }
}
