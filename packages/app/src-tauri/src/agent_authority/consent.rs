//! The consent card: a question Puppet Master asks the person on behalf of an agent that wants to
//! control a pane it did not start, or to answer a risky prompt in one.
//!
//! It is stored as an ordinary question (`questions::Question` with `consent` set), so the Inbox,
//! the phone's question list, the "Needs you" count and the push pipeline already know how to show
//! it. This module adds the part a question does not have: what the answer MEANS for the agent's
//! next call.
//!
//! * Allow once: the agent's next matching call goes through, once.
//! * Always for this pane (take-over only): that pane no longer asks, for as long as the app runs.
//! * Deny, or dismissing the card: the agent's calls fail with `CONSENT_DENIED` while the card is
//!   remembered (ten minutes), then it may ask again.
//!
//! Answering a risky prompt on a pane that is not the agent's own is decided each time; there is no
//! "always" for it.

use crate::audit::{self, Kind, Who};
use crate::questions::{self, Choice, ConsentInfo, NewQuestion, Question, QuestionState};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};

pub const CHOICE_ONCE: &str = "allow_once";
pub const CHOICE_ALWAYS: &str = "always";
pub const CHOICE_DENY: &str = "deny";

const ACTION_TAKE_OVER: &str = "take_over";
const ACTION_ANSWER: &str = "answer_prompt";
/// How long the agent's call can stay unanswered before the card expires.
const CARD_TIMEOUT_S: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    TakeOver,
    AnswerPrompt,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TakeOver => ACTION_TAKE_OVER,
            Self::AnswerPrompt => ACTION_ANSWER,
        }
    }
}

/// What the agent is asking for.
#[derive(Debug, Clone)]
pub struct Ask {
    /// The MCP connection that is asking.
    pub session: String,
    /// The pane id, or the run handle when there is no pane. What a grant or a remembered answer is keyed on.
    pub subject: String,
    pub pane_id: Option<String>,
    pub action: Action,
    /// For `answer_prompt`: the prompt's id, so "once" covers one prompt.
    pub prompt_key: String,
    /// Words for the card.
    pub requester: String,
    pub target: String,
    /// For `answer_prompt`: `medium` or `high`, and the plain headline ("Delete a folder").
    pub risk: Option<String>,
    pub risk_headline: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    Once,
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed(Basis),
    /// A card is waiting for the person. The id is the question's.
    Pending(String),
    Denied(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    session: String,
    subject: String,
    action: Action,
    prompt: String,
}

struct Slot {
    question_id: String,
    /// "Allow once" was used up by a call that went through.
    consumed: bool,
}

#[derive(Default)]
struct Ledger {
    slots: HashMap<Key, Slot>,
    /// Panes the person allowed agents to take over without asking again.
    always: HashSet<String>,
}

static LEDGER: Lazy<Mutex<Ledger>> = Lazy::new(|| Mutex::new(Ledger::default()));

fn key_of(ask: &Ask) -> Key {
    Key { session: ask.session.clone(), subject: ask.subject.clone(), action: ask.action, prompt: ask.prompt_key.clone() }
}

/// Has the person said "always for this pane"?
pub fn always_allowed(subject: &str) -> bool {
    LEDGER.lock().always.contains(subject)
}

/// What the call may do now. Creates the card the first time; afterwards reads the person's answer.
pub fn decide(ask: &Ask, now: u64) -> Verdict {
    let key = key_of(ask);
    let mut guard = LEDGER.lock();
    // Plain `&mut Ledger`, so the slot and the grant set can be borrowed side by side.
    let ledger = &mut *guard;
    if ask.action == Action::TakeOver && ledger.always.contains(&ask.subject) {
        return Verdict::Allowed(Basis::Always);
    }
    if let Some(slot) = ledger.slots.get_mut(&key) {
        if let Some(question) = questions::store().get(&slot.question_id, now) {
            match question.state {
                QuestionState::Pending => return Verdict::Pending(question.id),
                QuestionState::Cancelled => return Verdict::Denied(question.id),
                QuestionState::Answered => {
                    let choice = question.answer.as_ref().and_then(|a| a.choice_id.as_deref());
                    match choice {
                        Some(CHOICE_ALWAYS) if ask.action == Action::TakeOver => {
                            ledger.always.insert(ask.subject.clone());
                            return Verdict::Allowed(Basis::Always);
                        }
                        Some(CHOICE_ONCE) | Some(CHOICE_ALWAYS) if !slot.consumed => {
                            slot.consumed = true;
                            return Verdict::Allowed(Basis::Once);
                        }
                        // "Once" already used, or a deny: a used-up permit asks again below.
                        Some(CHOICE_ONCE) | Some(CHOICE_ALWAYS) => {}
                        _ => return Verdict::Denied(question.id),
                    }
                }
                QuestionState::TimedOut => {}
            }
        }
        ledger.slots.remove(&key);
    }
    match raise(ask, now) {
        Some(id) => {
            ledger.slots.insert(key, Slot { question_id: id.clone(), consumed: false });
            Verdict::Pending(id)
        }
        // The store is full of unanswered questions: say "no" rather than let the call through.
        None => Verdict::Denied(String::new()),
    }
}

/// The requester's name inside a sentence: "Allow another agent to ..." rather than "Allow Another agent ...".
fn in_sentence(requester: &str) -> String {
    if requester == super::gate::UNKNOWN_REQUESTER {
        "another agent".to_string()
    } else {
        requester.to_string()
    }
}

fn card_words(ask: &Ask) -> (String, String, Vec<Choice>) {
    let choice = |id: &str, label: &str| Choice { id: id.into(), label: label.into() };
    let who = in_sentence(&ask.requester);
    match ask.action {
        Action::TakeOver => (
            format!("Allow {who} to control {}?", ask.target),
            "It would type into the pane and answer its questions. It did not start this pane.".to_string(),
            vec![choice(CHOICE_ONCE, "Allow once"), choice(CHOICE_ALWAYS, "Always for this pane"), choice(CHOICE_DENY, "Deny")],
        ),
        Action::AnswerPrompt => (
            format!("Allow {who} to answer a question in {}?", ask.target),
            ask.risk_headline
                .clone()
                .map(|headline| format!("{headline}. It did not start this pane."))
                .unwrap_or_else(|| "It did not start this pane.".to_string()),
            vec![choice(CHOICE_ONCE, "Allow once"), choice(CHOICE_DENY, "Deny")],
        ),
    }
}

fn raise(ask: &Ask, now: u64) -> Option<String> {
    let (text, context, choices) = card_words(ask);
    let created = questions::store()
        .create(
            NewQuestion {
                asker: None,
                // The push label: "Claude Code asks". The target is in the text.
                agent: Some(ask.requester.clone()),
                // Not the target pane's id: the pane is not the one waiting, the person is.
                pane_id: None,
                question: text,
                choices,
                allow_free_text: Some(false),
                context: Some(context),
                timeout_s: Some(CARD_TIMEOUT_S),
                urgency: Some("high".into()),
                consent: Some(ConsentInfo {
                    action: ask.action.as_str().into(),
                    requester: ask.requester.clone(),
                    target: ask.target.clone(),
                    pane_id: ask.pane_id.clone(),
                    risk: ask.risk.clone(),
                }),
            },
            now,
        )
        .ok()?;
    // A question's push has no buttons: it opens the app. That is the whole point.
    crate::push::notify_question(&created);
    // Tell the windows to re-read the list now instead of at the next poll. Not in unit tests:
    // reaching the app handle's event code links Tauri's window layer into the test binary, which
    // then fails to start on Windows (see docs/REPO_MAP.md, "Tests that touch Tauri window APIs").
    #[cfg(not(test))]
    if let Some(app) = crate::bridge::live_app() {
        questions::routes::publish_changed(&app);
    }
    Some(created.id)
}

/// Called by the questions store after a person answered or dismissed a card. Writes the audit line
/// and remembers "always".
pub fn on_resolved(question: &Question) {
    let Some(info) = &question.consent else { return };
    let by = question.answer.as_ref().map(|a| a.answered_by.as_str()).unwrap_or("desktop");
    let who = if by == "phone" { Who::Device(None) } else { Who::Desktop };
    let choice = question.answer.as_ref().and_then(|a| a.choice_id.as_deref());
    let decision = match (question.state, choice) {
        (QuestionState::Answered, Some(CHOICE_ONCE)) => "allow_once",
        (QuestionState::Answered, Some(CHOICE_ALWAYS)) => "always",
        (QuestionState::Answered, _) => "deny",
        (QuestionState::Cancelled, _) => "dismissed",
        _ => return,
    };
    if decision == "always" && info.action == ACTION_TAKE_OVER {
        if let Some(pane) = &info.pane_id {
            LEDGER.lock().always.insert(pane.clone());
        }
    }
    let who_asked = in_sentence(&info.requester);
    let (allowed, refused) = if info.action == ACTION_ANSWER {
        (
            format!("to answer a question in {}", info.target),
            format!("{who_asked} answering in {}", info.target),
        )
    } else {
        (format!("to control {}", info.target), format!("{who_asked} controlling {}", info.target))
    };
    let summary = match decision {
        "allow_once" => format!("You allowed {who_asked} {allowed}, once"),
        "always" => format!("You allowed {who_asked} {allowed}, always for this pane"),
        _ => format!("You said no to {refused}"),
    };
    audit::record(
        Kind::Consent,
        who,
        &summary,
        serde_json::json!({
            "action": info.action,
            "decision": decision,
            "pane_id": info.pane_id,
            "risk": info.risk,
            "question_id": question.id,
        }),
    );
}

/// Forget every grant and remembered answer (tests).
#[cfg(test)]
pub fn reset() {
    let mut ledger = LEDGER.lock();
    ledger.slots.clear();
    ledger.always.clear();
}
