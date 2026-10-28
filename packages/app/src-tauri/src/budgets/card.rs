//! The pause card: an Inbox card that says an agent used its budget and offers "Add 10 turns /
//! Stop here". It rides on the existing questions plumbing (the Inbox list, the paired phone, the
//! push), as a question of kind `budget` that only Puppet Master can raise.

use super::accounting::{Reason, Standing};
use super::adjust;
use super::model::BudgetRecord;
use crate::operations;
use crate::questions::{self, Choice, NewQuestion, Question, QuestionState};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

pub const KIND: &str = "budget";
pub const CHOICE_ADD_TURNS: &str = "add_turns";
pub const CHOICE_ADD_MINUTES: &str = "add_minutes";
pub const CHOICE_STOP: &str = "stop";

/// Question id -> the run it pauses. Cards do not outlive the app (like every question), and a
/// paused run raises a fresh one on its next attempt, so this needs no persistence.
static CARDS: Lazy<Mutex<HashMap<String, (String, String)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn agent_label(project: &str, handle: &str) -> String {
    let agent_type = operations::latest_agent_run(project, handle).map(|turn| turn.agent_type).unwrap_or_default();
    crate::tui::descriptors()
        .into_iter()
        .find(|descriptor| descriptor.id == agent_type)
        .map(|descriptor| descriptor.label)
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "The agent".to_string())
}

fn plural(n: u32, unit: &str) -> String {
    if n == 1 {
        format!("{n} {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// "Claude Code used its 20 turns." / "Claude Code used its 60 minutes." / "... its budget."
pub fn headline(agent: &str, record: &BudgetRecord, reason: Reason) -> String {
    match (reason, record.turns, record.minutes) {
        (Reason::Turns, Some(turns), _) => format!("{agent} used its {}.", plural(turns, "turn")),
        (Reason::Time, _, Some(minutes)) => format!("{agent} used its {}.", plural(minutes, "minute")),
        _ => format!("{agent} used its budget."),
    }
}

fn choices(record: &BudgetRecord) -> Vec<Choice> {
    let mut out = Vec::new();
    if let Some(step) = adjust::step_turns(record).filter(|_| record.turns.is_some()) {
        out.push(Choice { id: CHOICE_ADD_TURNS.into(), label: format!("Add {}", plural(step, "turn")) });
    }
    if let Some(step) = adjust::step_minutes(record).filter(|_| record.minutes.is_some()) {
        out.push(Choice { id: CHOICE_ADD_MINUTES.into(), label: format!("Add {step} min") });
    }
    out.push(Choice { id: CHOICE_STOP.into(), label: "Stop here".into() });
    out
}

/// Whether a card for this run is still waiting for the person.
fn pending_card(project: &str, handle: &str, now: u64) -> bool {
    let ids: Vec<String> = CARDS
        .lock()
        .iter()
        .filter(|(_, run)| run.0 == project && run.1 == handle)
        .map(|(id, _)| id.clone())
        .collect();
    ids.iter().any(|id| questions::store().get(id, now).is_some_and(|q| q.state == QuestionState::Pending))
}

/// Puts the pause card in the Inbox, once: a run that is already waiting on a card does not get
/// a second one however often the agent asks again. Returns the new question when one was made.
pub fn raise(record: &BudgetRecord, standing: &Standing, reason: Reason) -> Option<Question> {
    if reason == Reason::Stopped {
        return None;
    }
    // A quiet mission (Night Shift) raises no pause card: its night report says what happened.
    if crate::missions::is_quiet(&record.project, &record.handle) {
        return None;
    }
    let now = questions::now_ms();
    if pending_card(&record.project, &record.handle, now) {
        return None;
    }
    let agent = agent_label(&record.project, &record.handle);
    let used = match reason {
        Reason::Time => format!("{} of {} min", standing.elapsed_ms / 60_000, standing.minutes_limit.unwrap_or(0)),
        _ => {
            let limit = standing.turns_limit.unwrap_or(standing.committed());
            format!("{} of {}", standing.committed(), plural(limit, "turn"))
        }
    };
    let pane_id = operations::latest_agent_run(&record.project, &record.handle).ok().and_then(|turn| turn.pane_id);
    let created = questions::store()
        .create(
            NewQuestion {
                asker: Some("puppet-master".into()),
                agent: Some(agent.clone()),
                pane_id,
                question: headline(&agent, record, reason),
                choices: choices(record),
                allow_free_text: Some(false),
                context: Some(format!("{used}. It's paused after its last turn. Nothing was lost.")),
                timeout_s: Some(questions::MAX_BUDGET_TIMEOUT_S),
                urgency: Some("normal".into()),
                kind: Some(KIND.into()),
                limit: None,
                consent: None,
            },
            now,
        )
        .ok()?;
    CARDS.lock().insert(created.id.clone(), (record.project.clone(), record.handle.clone()));
    crate::push::notify_question(&created);
    crate::attention::on_changed();
    Some(created)
}

/// What the person's answer did, for the caller that wants to tell them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    AddedTurns(u32),
    AddedMinutes(u32),
    Stopped,
}

/// Applies the person's choice on a pause card. Called by every route that can answer a
/// question (desktop and phone); a question that is not a pause card does nothing.
pub fn on_answered(question: &Question) -> Option<Outcome> {
    if question.kind.as_deref() != Some(KIND) || question.state != QuestionState::Answered {
        return None;
    }
    let (project, handle) = CARDS.lock().remove(&question.id)?;
    let record = super::store::get(&project, &handle)?;
    let choice = question.answer.as_ref()?.choice_id.clone()?;
    match choice.as_str() {
        CHOICE_ADD_TURNS => {
            let step = adjust::step_turns(&record)?;
            adjust::add_turns(&project, &handle, step).ok()?;
            Some(Outcome::AddedTurns(step))
        }
        CHOICE_ADD_MINUTES => {
            let step = adjust::step_minutes(&record)?;
            adjust::add_minutes(&project, &handle, step).ok()?;
            Some(Outcome::AddedMinutes(step))
        }
        CHOICE_STOP => {
            adjust::stop(&project, &handle).ok()?;
            Some(Outcome::Stopped)
        }
        _ => None,
    }
}

/// The run a card pauses (for tests and the UI).
pub fn run_of_card(question_id: &str) -> Option<(String, String)> {
    CARDS.lock().get(question_id).cloned()
}
