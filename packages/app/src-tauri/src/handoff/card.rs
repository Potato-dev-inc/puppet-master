//! The Inbox card for a usage limit, and what answering it does.
//!
//! Detect-only (`DESIGN-RELAY.md` build step 2): the card offers two things, "Remind me at 14:05"
//! and "Stop here", and says the hand-off to another agent is coming later. Neither choice presses
//! a key in the pane: Puppet Master never answers the vendor's own menu and never presses what the
//! agent itself would use to continue (its "esc to cancel" wait, its "press enter to continue").
//!
//! The card is a question Puppet Master raised (`Question.limit`), like a consent card: it arrives
//! through the same list and the same answer path, and plain loopback HTTP may not answer it
//! (`questions::routes`).

use super::detect::LimitHit;
use super::reset::{when_text, Clock};
use crate::questions::{Choice, LimitInfo, NewQuestion, Question, QuestionStore};

pub const KIND_USAGE_LIMIT: &str = "usage_limit";
pub const KIND_RESET_REMINDER: &str = "reset_reminder";
pub const CHOICE_REMIND: &str = "remind";
pub const CHOICE_STOP: &str = "stop";
pub const CHOICE_OK: &str = "ok";
/// Without a printed reset, "remind me" means this long from now.
pub const REMIND_FALLBACK_MS: u64 = 60 * 60 * 1000;
/// How long the card stays in the Inbox. A limit can last hours; the card goes away when the limit
/// does (`watch::Cleared`).
pub const CARD_TIMEOUT_S: u64 = 12 * 3600;

fn choice(id: &str, label: impl Into<String>) -> Choice {
    Choice { id: id.into(), label: label.into() }
}

/// The card for `hit` on a pane of `agent_label`. `has_run`: an agent run (operation) is bound to
/// the pane, so "Stop here" has something to stop.
pub fn limit_card(hit: &LimitHit, agent_label: &str, pane_id: &str, has_run: bool, clock: &dyn Clock) -> NewQuestion {
    let reset_ms = hit.reset_at_ms.and_then(|ms| i64::try_from(ms).ok());
    let at = reset_ms.map(|ms| when_text(ms, clock));
    let question = match &at {
        Some(at) => format!("{agent_label} ran out of usage. It can continue at {at}."),
        None => format!("{agent_label} ran out of usage. It didn't say when it can continue."),
    };
    let mut context = String::new();
    if hit.self_resume_armed {
        context.push_str(&format!("{agent_label} is set to continue on its own. "));
    }
    context.push_str("Handing the work to another agent is coming in a later update.");
    let remind = match &at {
        Some(at) => format!("Remind me at {at}"),
        None => "Remind me in 1 hour".to_string(),
    };
    let stop = if has_run { "Stop here" } else { "Dismiss" };
    NewQuestion {
        asker: None,
        agent: Some(agent_label.to_string()),
        pane_id: Some(pane_id.to_string()),
        question,
        choices: vec![choice(CHOICE_REMIND, remind), choice(CHOICE_STOP, stop)],
        allow_free_text: Some(false),
        context: Some(context),
        timeout_s: Some(CARD_TIMEOUT_S),
        urgency: Some("high".into()),
        limit: Some(LimitInfo {
            kind: KIND_USAGE_LIMIT.into(),
            agent_label: agent_label.to_string(),
            window: hit.window.clone(),
            reset_at_ms: hit.reset_at_ms,
            reset_text: hit.reset_text.clone(),
            self_resume_armed: hit.self_resume_armed,
            has_run,
        }),
        consent: None,
    }
}

/// The card a reminder raises once the reset time has come.
pub fn reminder_card(agent_label: &str, pane_id: &str, window: Option<String>) -> NewQuestion {
    NewQuestion {
        asker: None,
        agent: Some(agent_label.to_string()),
        pane_id: Some(pane_id.to_string()),
        question: format!("{agent_label}'s usage limit should have reset by now."),
        choices: vec![choice(CHOICE_OK, "Got it")],
        allow_free_text: Some(false),
        context: Some("You asked to be reminded. Open its terminal to carry on.".into()),
        timeout_s: Some(CARD_TIMEOUT_S),
        urgency: Some("normal".into()),
        limit: Some(LimitInfo {
            kind: KIND_RESET_REMINDER.into(),
            agent_label: agent_label.to_string(),
            window,
            reset_at_ms: None,
            reset_text: None,
            self_resume_armed: false,
            has_run: false,
        }),
        consent: None,
    }
}

/// Raise `card` in `store` (and tell the phone and the windows in the live app).
pub fn raise(store: &QuestionStore, card: NewQuestion, now: u64) -> Option<Question> {
    let created = store.create(card, now).ok()?;
    crate::push::notify_question(&created);
    // Not in unit tests: reaching the app handle's event code links Tauri's window layer into the
    // test binary (see docs/REPO_MAP.md, "Tests that touch Tauri window APIs").
    #[cfg(not(test))]
    if let Some(app) = crate::bridge::live_app() {
        crate::questions::routes::publish_changed(&app);
    }
    Some(created)
}

/// What a person's answer to a limit card asks for. Both are bookkeeping inside Puppet Master;
/// neither touches the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Raise a reminder card at `due_ms`.
    Remind { pane_id: String, agent_label: String, window: Option<String>, due_ms: u64 },
    /// End the run bound to the pane as "stopped at a usage limit".
    StopRun { pane_id: String },
    /// Nothing to do (dismissed, got it, a card that is not a limit card).
    Nothing,
}

/// Read a resolved question: only an answer counts, a dismissal is "never mind".
pub fn decision(question: &Question, now: u64) -> Decision {
    let Some(info) = &question.limit else { return Decision::Nothing };
    let Some(answer) = question.answer.as_ref().filter(|_| question.state == crate::questions::QuestionState::Answered)
    else {
        return Decision::Nothing;
    };
    let Some(pane_id) = question.pane_id.clone() else { return Decision::Nothing };
    match (info.kind.as_str(), answer.choice_id.as_deref()) {
        (KIND_USAGE_LIMIT, Some(CHOICE_REMIND)) => Decision::Remind {
            pane_id,
            agent_label: info.agent_label.clone(),
            window: info.window.clone(),
            due_ms: info.reset_at_ms.filter(|at| *at > now).unwrap_or(now + REMIND_FALLBACK_MS),
        },
        (KIND_USAGE_LIMIT, Some(CHOICE_STOP)) if info.has_run => Decision::StopRun { pane_id },
        _ => Decision::Nothing,
    }
}
