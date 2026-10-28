//! The Inbox cards for a usage limit and for Keep going, and what answering them asks for.
//!
//! The limit card offers `Continue on <agent>` when another agent can take over (the first choice),
//! then "Remind me" and "Stop here". Neither choice presses a key in the pane: Puppet Master never
//! answers the vendor's own menu and never presses what the agent itself would use to continue
//! (its "esc to cancel" wait, its "press enter to continue"). A `continue:` answer only asks for a
//! hand-over; `keep_going::live` does the work and refuses anything it cannot do.
//!
//! The cards are questions Puppet Master raised (`Question.limit`), like a consent card: they arrive
//! through the same list and the same answer path, and plain loopback HTTP may not answer them
//! (`questions::routes`).

use super::detect::LimitHit;
use super::keep_going::model::{KeepGoingOffer, Target};
use super::reset::{when_text, Clock};
use crate::questions::{Choice, LimitInfo, NewQuestion, Question, QuestionStore};

pub const KIND_USAGE_LIMIT: &str = "usage_limit";
pub const KIND_RESET_REMINDER: &str = "reset_reminder";
/// The agent that ran out can work again: offer to move the task back (Keep going).
pub const KIND_COME_BACK: &str = "come_back";
/// A hand-over did not start: say why, offer to try again.
pub const KIND_KEEP_GOING_FAILED: &str = "keep_going_failed";
pub const CHOICE_REMIND: &str = "remind";
pub const CHOICE_STOP: &str = "stop";
pub const CHOICE_OK: &str = "ok";
/// `continue:<target key>`: take over on that agent. Only on a usage-limit card with an offer.
pub const CHOICE_CONTINUE_PREFIX: &str = "continue:";
/// `retry:<target key>`: try the same hand-over again. Only on a failure card.
pub const CHOICE_RETRY_PREFIX: &str = "retry:";
pub const CHOICE_BACK: &str = "back";
pub const CHOICE_STAY: &str = "stay";
/// Without a printed reset, "remind me" means this long from now.
pub const REMIND_FALLBACK_MS: u64 = 60 * 60 * 1000;
/// How long the card stays in the Inbox. A limit can last hours; the card goes away when the limit
/// does (`watch::Cleared`).
pub const CARD_TIMEOUT_S: u64 = 12 * 3600;

fn choice(id: &str, label: impl Into<String>) -> Choice {
    Choice { id: id.into(), label: label.into() }
}

/// The card for `hit` on a pane of `agent_label`. `has_run`: an agent run (operation) is bound to
/// the pane, so "Stop here" has something to stop. `offer` says whether another agent can take over.
pub fn limit_card(
    hit: &LimitHit,
    agent_label: &str,
    pane_id: &str,
    has_run: bool,
    clock: &dyn Clock,
    offer: &KeepGoingOffer,
) -> NewQuestion {
    let reset_ms = hit.reset_at_ms.and_then(|ms| i64::try_from(ms).ok());
    let at = reset_ms.map(|ms| when_text(ms, clock));
    let question = match &at {
        Some(at) => format!("{agent_label} ran out of usage. It can continue at {at}."),
        None => format!("{agent_label} ran out of usage. It didn't say when it can continue."),
    };
    let mut sentences = Vec::new();
    let mut choices = Vec::new();
    match offer {
        KeepGoingOffer::Offer { target, label } => {
            if hit.self_resume_armed {
                sentences.push(format!("{agent_label} is set to continue on its own. Continuing on {label} stops that."));
            }
            sentences.push(format!(
                "{label} gets a short brief and works in the same folder. {agent_label} in this pane is stopped first."
            ));
            choices.push(choice(&format!("{CHOICE_CONTINUE_PREFIX}{}", target.key()), format!("Continue on {label}")));
        }
        KeepGoingOffer::None { why } => {
            if hit.self_resume_armed {
                sentences.push(format!("{agent_label} is set to continue on its own."));
            }
            sentences.push(format!(
                "No other agent can take over now: {}. Pick one in Settings > Agents > Keep going.",
                why.trim_end_matches('.')
            ));
        }
        KeepGoingOffer::NotHere => {
            if hit.self_resume_armed {
                sentences.push(format!("{agent_label} is set to continue on its own."));
            }
        }
    }
    let context = sentences.join(" ");
    let remind = match &at {
        Some(at) => format!("Remind me at {at}"),
        None => "Remind me in 1 hour".to_string(),
    };
    let stop = if has_run { "Stop here" } else { "Dismiss" };
    choices.push(choice(CHOICE_REMIND, remind));
    choices.push(choice(CHOICE_STOP, stop));
    NewQuestion {
        asker: None,
        agent: Some(agent_label.to_string()),
        pane_id: Some(pane_id.to_string()),
        question,
        choices,
        allow_free_text: Some(false),
        context: (!context.is_empty()).then_some(context),
        timeout_s: Some(CARD_TIMEOUT_S),
        urgency: Some("high".into()),
        kind: None,
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
        kind: None,
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

/// The come-back card, on the NEW pane: the agent that ran out (`back_label`, "Claude Code") can
/// work again. Asks whether to move the task back from the agent working on it now (`now_label`,
/// "Codex"). `goal_short` is the task, already shortened by the caller.
pub fn come_back_card(now_label: &str, back_label: &str, pane_id: &str, goal_short: &str) -> NewQuestion {
    NewQuestion {
        asker: None,
        agent: Some(now_label.to_string()),
        pane_id: Some(pane_id.to_string()),
        question: format!("{back_label} can work again. Move \"{goal_short}\" back from {now_label}?"),
        choices: vec![
            choice(CHOICE_BACK, format!("Move back to {back_label}")),
            choice(CHOICE_STAY, format!("Stay on {now_label}")),
        ],
        allow_free_text: Some(false),
        context: Some("Nothing moves until you choose.".into()),
        timeout_s: Some(CARD_TIMEOUT_S),
        urgency: Some("normal".into()),
        kind: None,
        limit: Some(LimitInfo {
            kind: KIND_COME_BACK.into(),
            agent_label: back_label.to_string(),
            window: None,
            reset_at_ms: None,
            reset_text: None,
            self_resume_armed: false,
            has_run: false,
        }),
        consent: None,
    }
}

/// The card for a hand-over that did not start. `message` is the plain-English reason. `retry` is
/// the target to offer "Try again" for (only when a retry can help).
pub fn keep_going_failed_card(
    from_label: &str,
    to_label: &str,
    pane_id: &str,
    message: &str,
    retry: Option<&Target>,
) -> NewQuestion {
    let mut choices = Vec::new();
    if let Some(target) = retry {
        choices.push(choice(&format!("{CHOICE_RETRY_PREFIX}{}", target.key()), format!("Try {to_label} again")));
    }
    choices.push(choice(CHOICE_OK, "Got it"));
    NewQuestion {
        asker: None,
        agent: Some(from_label.to_string()),
        pane_id: Some(pane_id.to_string()),
        question: message.to_string(),
        choices,
        allow_free_text: Some(false),
        context: None,
        timeout_s: Some(CARD_TIMEOUT_S),
        urgency: Some("high".into()),
        kind: None,
        limit: Some(LimitInfo {
            kind: KIND_KEEP_GOING_FAILED.into(),
            agent_label: from_label.to_string(),
            window: None,
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

/// What a person's answer to a card asks for. `Remind` and `StopRun` are bookkeeping; the Keep
/// going variants start work in `keep_going::live` (through `Env::keep_going`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Raise a reminder card at `due_ms`.
    Remind { pane_id: String, agent_label: String, window: Option<String>, due_ms: u64 },
    /// End the run bound to the pane as "stopped at a usage limit".
    StopRun { pane_id: String },
    /// Hand the task over from the pane to `target` (the limit card's `continue:` choice).
    Continue { pane_id: String, target: Target },
    /// Move the task back to the agent that ran out (the come-back card's `back` choice).
    ComeBack { pane_id: String },
    /// Keep the task where it is (the come-back card's `stay` choice).
    Stay { pane_id: String },
    /// Try the same hand-over again (the failure card's `retry:` choice).
    Retry { pane_id: String, target: Target },
    /// Nothing to do (dismissed, got it, a card that is not one of ours, a choice id that is not
    /// valid).
    Nothing,
}

/// The target in a `<prefix><key>` choice id. Strict: the rest of the id must be exactly a key.
fn target_of(choice: &str, prefix: &str) -> Option<Target> {
    let key = choice.strip_prefix(prefix)?;
    Target::parse(key).filter(|target| target.key() == key)
}

/// Read a resolved question: only an answer counts, a dismissal is "never mind".
pub fn decision(question: &Question, now: u64) -> Decision {
    let Some(info) = &question.limit else { return Decision::Nothing };
    let Some(answer) = question.answer.as_ref().filter(|_| question.state == crate::questions::QuestionState::Answered)
    else {
        return Decision::Nothing;
    };
    let Some(pane_id) = question.pane_id.clone() else { return Decision::Nothing };
    let choice = answer.choice_id.as_deref().unwrap_or_default();
    match (info.kind.as_str(), choice) {
        (KIND_USAGE_LIMIT, CHOICE_REMIND) => Decision::Remind {
            pane_id,
            agent_label: info.agent_label.clone(),
            window: info.window.clone(),
            due_ms: info.reset_at_ms.filter(|at| *at > now).unwrap_or(now + REMIND_FALLBACK_MS),
        },
        (KIND_USAGE_LIMIT, CHOICE_STOP) if info.has_run => Decision::StopRun { pane_id },
        (KIND_USAGE_LIMIT, _) => match target_of(choice, CHOICE_CONTINUE_PREFIX) {
            Some(target) => Decision::Continue { pane_id, target },
            None => Decision::Nothing,
        },
        (KIND_COME_BACK, CHOICE_BACK) => Decision::ComeBack { pane_id },
        (KIND_COME_BACK, CHOICE_STAY) => Decision::Stay { pane_id },
        (KIND_KEEP_GOING_FAILED, _) => match target_of(choice, CHOICE_RETRY_PREFIX) {
            Some(target) => Decision::Retry { pane_id, target },
            None => Decision::Nothing,
        },
        _ => Decision::Nothing,
    }
}
