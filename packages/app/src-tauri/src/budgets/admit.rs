//! Admitting a new run: which budget it gets (its own, the Settings default, or a share of the
//! run that started it) and the rules that keep a helper inside its parent.

use super::accounting::standing;
use super::errors;
use super::model::{Asked, BudgetRecord, Defaults, Source, MAX_DEPTH, MAX_MINUTES, MAX_TURNS};
use super::store;
use crate::operations::{self, OperationError};
use parking_lot::Mutex;

/// Admission reads the parent's remaining share and then reserves from it; two helpers admitted at
/// once must not both take the same turns.
static ADMIT_LOCK: Mutex<()> = Mutex::new(());

fn invalid(message: String) -> OperationError {
    OperationError::new("INVALID_ARGUMENT", message, false)
}

/// Rejects budget arguments outside what makes sense, before anything is created.
pub fn validate(asked: &Asked) -> Result<(), OperationError> {
    if let Some(turns) = asked.turns {
        if turns == 0 || turns > MAX_TURNS {
            return Err(invalid(format!("budget_turns must be between 1 and {MAX_TURNS}")));
        }
    }
    if let Some(minutes) = asked.minutes {
        if minutes == 0 || minutes > MAX_MINUTES {
            return Err(invalid(format!("budget_minutes must be between 1 and {MAX_MINUTES}")));
        }
    }
    Ok(())
}

/// The parent's record, when `parent` names a run that holds a budget. A parent without a budget
/// is no parent for budget purposes.
fn parent_record(project: &str, parent: &str) -> Option<BudgetRecord> {
    if let Some(found) = store::get(project, parent) {
        return Some(found);
    }
    let run = operations::resolve_agent_run(project, parent).ok()?;
    store::get(&run.project_path, parent)
}

/// Decides and records the budget of the run `handle`. `Ok(None)` means no budget (the default:
/// today's behaviour). An identical retry returns the record already made.
pub fn admit(
    project: &str,
    handle: &str,
    asked: &Asked,
    defaults: Defaults,
    now_ms: u64,
) -> Result<Option<BudgetRecord>, OperationError> {
    validate(asked)?;
    let _serial = ADMIT_LOCK.lock();
    if let Some(existing) = store::get(project, handle) {
        return Ok(Some(existing));
    }
    let record = match asked.parent.as_deref().and_then(|parent| parent_record(project, parent)) {
        Some(parent) => Some(helper_record(project, handle, &parent, asked, now_ms)?),
        None => root_record(project, handle, asked, defaults, now_ms),
    };
    if let Some(record) = &record {
        store::put(project, record.clone());
    }
    Ok(record)
}

/// The defaults for a run started by a companion (the always-on helper the person keeps in one pane): the Settings
/// default is kept as it is, and when the person set no turn limit there, a helper of a companion gets
/// `companion::delegate::HELPER_TURNS` turns. A limit asked for on the run itself (`Asked`) still wins, and so does
/// a share of the parent's budget: this only fills in what the person left unset, as the Settings default does.
pub fn with_companion_default(defaults: Defaults, from_companion: bool) -> Defaults {
    if from_companion && defaults.turns.is_none() {
        Defaults { turns: Some(crate::companion::delegate::HELPER_TURNS), ..defaults }
    } else {
        defaults
    }
}

fn root_record(project: &str, handle: &str, asked: &Asked, defaults: Defaults, now_ms: u64) -> Option<BudgetRecord> {
    // Each limit falls back to the Settings default on its own: asking for turns does not switch
    // the default time limit off.
    let turns = asked.turns.or(defaults.turns);
    let minutes = asked.minutes.or(defaults.minutes);
    if turns.is_none() && minutes.is_none() {
        return None;
    }
    Some(BudgetRecord {
        handle: handle.to_string(),
        project: project.to_string(),
        parent: None,
        parent_project: None,
        depth: 0,
        turns,
        minutes,
        first_turns: turns,
        first_minutes: minutes,
        started_ms: now_ms,
        source: if asked.any() { Source::Argument } else { Source::Default },
        stopped: false,
    })
}

fn helper_record(
    project: &str,
    handle: &str,
    parent: &BudgetRecord,
    asked: &Asked,
    now_ms: u64,
) -> Result<BudgetRecord, OperationError> {
    let depth = parent.depth + 1;
    if depth > MAX_DEPTH {
        return Err(errors::depth_limit(&parent.handle, MAX_DEPTH));
    }
    let share = standing(parent, now_ms);
    if let Some(reason) = share.exhausted() {
        return Err(errors::helper_parent_exhausted(&parent.handle, reason, &share));
    }
    let turns = match (share.turns_left(), asked.turns) {
        (Some(left), Some(want)) if want > left => {
            return Err(errors::more_than_left("turns", want, left, &parent.handle))
        }
        (Some(_), Some(want)) => Some(want),
        // Half of what is left, so one helper cannot take the whole budget by default.
        (Some(left), None) => Some(left.div_ceil(2).max(1)),
        (None, want) => want,
    };
    let minutes_left = share.ms_left().map(|ms| ms.div_ceil(60_000) as u32);
    let minutes = match (minutes_left, asked.minutes) {
        (Some(left), Some(want)) if want > left => {
            return Err(errors::more_than_left("minutes", want, left, &parent.handle))
        }
        (Some(_), Some(want)) => Some(want),
        // The helper shares the parent's clock: it never outlives it.
        (Some(left), None) => Some(left.max(1)),
        (None, want) => want,
    };
    Ok(BudgetRecord {
        handle: handle.to_string(),
        project: project.to_string(),
        parent: Some(parent.handle.clone()),
        parent_project: (parent.project != project).then(|| parent.project.clone()),
        depth,
        turns,
        minutes,
        first_turns: turns,
        first_minutes: minutes,
        started_ms: now_ms,
        source: Source::Share,
        stopped: false,
    })
}

/// Forgets the budget of a run that never got created (its start failed after admission), which
/// also gives its share back to the parent.
pub fn discard(project: &str, handle: &str) {
    if operations::turns_for_handle(project, handle).map(|turns| turns.is_empty()).unwrap_or(true) {
        store::remove(project, handle);
    }
}
