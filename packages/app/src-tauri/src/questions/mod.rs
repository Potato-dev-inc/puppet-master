//! Questions an agent asks THE HUMAN through the `ask_user` MCP tool.
//!
//! A question is a pending record in an in-memory store. The asking tool call blocks (the MCP
//! shim long-polls `POST /questions/ask`, like `wait_agents`) until a person answers it on the
//! desktop or the paired phone, dismisses it, or its timeout passes. Only the first answer wins.
//!
//! Persistence: questions survive an MCP/bridge reconnect while the app keeps running. They do
//! NOT survive an app restart (the shim then sees `QUESTION_NOT_FOUND` and reports the question
//! as cancelled). That is deliberate: an answer typed hours later for an agent that is gone is
//! worse than asking again.

pub mod routes;
#[cfg(test)]
mod tests;

use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_TIMEOUT_S: u64 = 600;
pub const MAX_TIMEOUT_S: u64 = 3600;
/// A pause card waits as long as the run does: the run stays paused until someone decides.
pub const MAX_BUDGET_TIMEOUT_S: u64 = 24 * 3600;
/// Kinds of the cards about a GitHub issue (`issue_intake`): start work on an issue, open a draft PR.
pub const ISSUE_KIND_START: &str = "issue_start";
pub const ISSUE_KIND_PUSH: &str = "issue_push";
/// Kinds of the cards about a failed CI check on a PR Puppet Master opened (`ci_fix`): ask to fix it, ask to push
/// the fix. Only the person answers them (see `PERSON_ONLY_KINDS`).
pub const CI_KIND_FIX: &str = "ci_fix";
pub const CI_KIND_PUSH: &str = "ci_push";
/// Cards only the person can answer: never an agent, never a companion, never a chat channel. A plain
/// loopback request may not answer or dismiss them (`routes.rs`), and a chat answer is refused in
/// `attention::answer`. Answered from the desktop window (Tauri command) or a paired phone only.
pub const PERSON_ONLY_KINDS: &[&str] = &["budget", ISSUE_KIND_START, ISSUE_KIND_PUSH, CI_KIND_FIX, CI_KIND_PUSH];
/// A usage-limit card (raised by Puppet Master, never through `ask_user`) outlives an hour: a limit
/// can last a working day.
pub const MAX_LIMIT_TIMEOUT_S: u64 = 24 * 3600;
pub const MAX_CHOICES: usize = 6;
pub const MAX_QUESTION_CHARS: usize = 2000;
pub const MAX_CONTEXT_CHARS: usize = 1000;
pub const MAX_LABEL_CHARS: usize = 100;
pub const MAX_ANSWER_TEXT_CHARS: usize = 4000;
pub const MAX_PENDING: usize = 50;
/// Resolved questions stay readable (idempotent re-answers, late shim polls) for this long.
const RESOLVED_KEEP_MS: u64 = 10 * 60 * 1000;
const RESOLVED_KEEP_MAX: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Pending,
    Answered,
    /// Dismissed by the person, or cancelled by the asking agent.
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Answer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// `desktop` or `phone`.
    pub answered_by: String,
}

/// What a consent card is about. Present only on questions Puppet Master itself raised for an agent
/// that wants to control something it did not start (`agent_authority`); an agent cannot set it
/// through `ask_user`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsentInfo {
    /// `take_over` or `answer_prompt`.
    pub action: String,
    /// Who is asking, in the words the card uses ("Claude Code", or "An agent").
    pub requester: String,
    /// What it wants to control or answer in, in the words the card uses ("Codex").
    pub target: String,
    /// The pane it is about, when there is one: the card shows the pane's own name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    /// For `answer_prompt`: the plain risk level of the prompt (`medium` or `high`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

/// What a usage-limit card is about (`handoff::card`). Present only on cards Puppet Master itself
/// raised when an agent ran out of usage; an agent cannot set it through `ask_user`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LimitInfo {
    /// `usage_limit` (the agent ran out) or `reset_reminder` (a reminder the person asked for).
    pub kind: String,
    /// The agent's name in the words the card uses ("Claude Code").
    pub agent_label: String,
    /// What ran out ("session limit", "weekly limit"), when the screen said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// When it can continue (epoch ms), when the screen printed a time this machine could read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at_ms: Option<u64>,
    /// The time as the screen printed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_text: Option<String>,
    /// The agent shows it will carry on by itself at the reset.
    #[serde(default)]
    pub self_resume_armed: bool,
    /// A run is bound to the pane, so "Stop here" has something to stop.
    #[serde(default)]
    pub has_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    /// MCP connection (session) that asked, when the call carried one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asker: Option<String>,
    /// Agent label as the agent gave it; the UI falls back to the pane's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    pub question: String,
    pub choices: Vec<Choice>,
    pub allow_free_text: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// `normal` or `high`.
    pub urgency: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub state: QuestionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// Set when this is a consent card, not a question an agent asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent: Option<ConsentInfo>,
    /// What raised it when it was not an agent's `ask_user`: `budget` for the pause card of a
    /// run that used its budget. The Inbox renders those with their own wording.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Set when this is a usage-limit card (or a reminder for one), not a question an agent asked.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<LimitInfo>,
    /// The Red to Green fix (`cf-` + 12 hex) a `ci_fix` or `ci_push` card is about. The card's
    /// `Show log` reads the stored snapshot by it. Only an id: it never carries log text, and it is
    /// set only on those two kinds when the id has the exact shape.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fix_id: Option<String>,
    #[serde(skip)]
    resolved_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct NewQuestion {
    pub asker: Option<String>,
    pub agent: Option<String>,
    pub pane_id: Option<String>,
    pub question: String,
    pub choices: Vec<Choice>,
    pub allow_free_text: Option<bool>,
    pub context: Option<String>,
    pub timeout_s: Option<u64>,
    pub urgency: Option<String>,
    /// Only `agent_authority` sets this.
    pub consent: Option<ConsentInfo>,
    pub kind: Option<String>,
    /// Only `handoff` sets this.
    pub limit: Option<LimitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionError {
    pub code: &'static str,
    pub message: String,
}

impl QuestionError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    /// HTTP status for the bridge routes.
    pub fn status(&self) -> u16 {
        match self.code {
            "QUESTION_NOT_FOUND" => 404,
            "TOO_MANY_QUESTIONS" => 429,
            _ => 400,
        }
    }
}

/// Result of an answer or dismiss attempt. `accepted` is false when the question had already
/// been resolved: the stored resolution is returned unchanged (first answer wins).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Resolution {
    pub accepted: bool,
    pub question: Question,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn trimmed(text: &str) -> String {
    text.trim().to_string()
}

#[derive(Default)]
struct Inner {
    items: Vec<Question>,
    /// Bumped on every change.
    revision: u64,
}

/// The store: a list behind a mutex plus a condvar that wakes blocked `ask_user` calls.
pub struct QuestionStore {
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl Default for QuestionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl QuestionStore {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()), changed: Condvar::new() }
    }

    pub fn create(&self, new: NewQuestion, now: u64) -> Result<Question, QuestionError> {
        self.create_as(new, None, now)
    }

    /// Like `create`, with an id the caller picked (a UUID not in use). For a caller that must store the
    /// card's id before the card can be seen. `None` picks a fresh id.
    pub fn create_as(&self, new: NewQuestion, chosen_id: Option<&str>, now: u64) -> Result<Question, QuestionError> {
        self.create_for_fix(new, chosen_id, None, now)
    }

    /// Like `create_as`, for a Red to Green card: `fix_id` (`cf-` + 12 lowercase hex) names the fix the card is
    /// about so its `Show log` can read the stored snapshot. It is kept only on the `ci_fix` and `ci_push`
    /// kinds and only in that exact shape, so it can never carry text.
    pub fn create_for_fix(
        &self,
        new: NewQuestion,
        chosen_id: Option<&str>,
        fix_id: Option<&str>,
        now: u64,
    ) -> Result<Question, QuestionError> {
        let text = trimmed(&new.question);
        if text.is_empty() {
            return Err(QuestionError::new("INVALID_ARGUMENT", "question must not be empty"));
        }
        if text.chars().count() > MAX_QUESTION_CHARS {
            return Err(QuestionError::new(
                "INVALID_ARGUMENT",
                format!("question is longer than {MAX_QUESTION_CHARS} characters"),
            ));
        }
        if new.choices.len() > MAX_CHOICES {
            return Err(QuestionError::new("INVALID_ARGUMENT", format!("at most {MAX_CHOICES} choices")));
        }
        let mut choices: Vec<Choice> = Vec::with_capacity(new.choices.len());
        for choice in &new.choices {
            let id = trimmed(&choice.id);
            let label = trimmed(&choice.label);
            if id.is_empty() || label.is_empty() {
                return Err(QuestionError::new("INVALID_ARGUMENT", "every choice needs an id and a label"));
            }
            if label.chars().count() > MAX_LABEL_CHARS || id.chars().count() > MAX_LABEL_CHARS {
                return Err(QuestionError::new(
                    "INVALID_ARGUMENT",
                    format!("choice ids and labels are limited to {MAX_LABEL_CHARS} characters"),
                ));
            }
            if choices.iter().any(|c| c.id == id) {
                return Err(QuestionError::new("INVALID_ARGUMENT", format!("duplicate choice id: {id}")));
            }
            choices.push(Choice { id, label });
        }
        let allow_free_text = new.allow_free_text.unwrap_or(choices.is_empty());
        if choices.is_empty() && !allow_free_text {
            return Err(QuestionError::new(
                "INVALID_ARGUMENT",
                "a question without choices must allow free text",
            ));
        }
        let context = new.context.as_deref().map(trimmed).filter(|c| !c.is_empty());
        if context.as_ref().is_some_and(|c| c.chars().count() > MAX_CONTEXT_CHARS) {
            return Err(QuestionError::new(
                "INVALID_ARGUMENT",
                format!("context is longer than {MAX_CONTEXT_CHARS} characters"),
            ));
        }
        let max_timeout = if new.kind.as_deref().is_some_and(|k| PERSON_ONLY_KINDS.contains(&k)) {
            MAX_BUDGET_TIMEOUT_S
        } else if new.limit.is_some() {
            MAX_LIMIT_TIMEOUT_S
        } else {
            MAX_TIMEOUT_S
        };
        let timeout_s = new.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S);
        if timeout_s == 0 || timeout_s > max_timeout {
            return Err(QuestionError::new(
                "INVALID_ARGUMENT",
                format!("timeout_s must be between 1 and {max_timeout}"),
            ));
        }
        let urgency = match new.urgency.as_deref() {
            None | Some("normal") => "normal",
            Some("high") => "high",
            Some(other) => {
                return Err(QuestionError::new(
                    "INVALID_ARGUMENT",
                    format!("urgency must be normal or high, got {other}"),
                ))
            }
        }
        .to_string();

        let mut inner = self.inner.lock();
        Self::expire_locked(&mut inner, now);
        if inner.items.iter().filter(|q| q.state == QuestionState::Pending).count() >= MAX_PENDING {
            return Err(QuestionError::new(
                "TOO_MANY_QUESTIONS",
                "too many unanswered questions; wait for the person to answer some",
            ));
        }
        let id = match chosen_id {
            None => uuid::Uuid::new_v4().to_string(),
            Some(chosen) => {
                let chosen = uuid::Uuid::parse_str(chosen)
                    .map_err(|_| QuestionError::new("INVALID_ARGUMENT", "id must be a UUID"))?
                    .to_string();
                if inner.items.iter().any(|q| q.id == chosen) {
                    return Err(QuestionError::new("INVALID_ARGUMENT", "that id is already in use"));
                }
                chosen
            }
        };
        let new_kind_is_ci = matches!(new.kind.as_deref(), Some(CI_KIND_FIX | CI_KIND_PUSH));
        let question = Question {
            id,
            asker: new.asker,
            agent: new.agent.map(|a| trimmed(&a)).filter(|a| !a.is_empty()),
            pane_id: new.pane_id.map(|p| trimmed(&p)).filter(|p| !p.is_empty()),
            question: text,
            choices,
            allow_free_text,
            context,
            urgency,
            created_at_ms: now,
            expires_at_ms: now + timeout_s * 1000,
            state: QuestionState::Pending,
            answer: None,
            consent: new.consent,
            kind: new.kind.map(|k| trimmed(&k)).filter(|k| !k.is_empty()),
            limit: new.limit,
            fix_id: fix_id
                .filter(|id| new_kind_is_ci && crate::ci_fix::model::valid_fix_id(id))
                .map(str::to_string),
            resolved_at_ms: None,
        };
        inner.items.push(question.clone());
        self.bump(&mut inner);
        Ok(question)
    }

    /// Pending questions, oldest first, after expiring overdue ones.
    pub fn list_pending(&self, now: u64) -> Vec<Question> {
        let mut inner = self.inner.lock();
        if Self::expire_locked(&mut inner, now) {
            self.bump(&mut inner);
        }
        inner.items.iter().filter(|q| q.state == QuestionState::Pending).cloned().collect()
    }

    /// Any question, resolved or not (resolved ones are kept for a few minutes).
    pub fn get(&self, id: &str, now: u64) -> Option<Question> {
        let mut inner = self.inner.lock();
        if Self::expire_locked(&mut inner, now) {
            self.bump(&mut inner);
        }
        inner.items.iter().find(|q| q.id == id).cloned()
    }

    pub fn revision(&self) -> u64 {
        self.inner.lock().revision
    }

    /// Answer with a choice id or free text (exactly one). The first answer wins; later ones
    /// get the stored resolution back with `accepted: false`.
    pub fn answer(
        &self,
        id: &str,
        choice_id: Option<&str>,
        text: Option<&str>,
        answered_by: &str,
        now: u64,
    ) -> Result<Resolution, QuestionError> {
        let result = {
            let mut inner = self.inner.lock();
            let expired = Self::expire_locked(&mut inner, now);
            let result = Self::answer_locked(&mut inner, id, choice_id, text, answered_by, now);
            if expired || matches!(&result, Ok(r) if r.accepted) {
                self.bump(&mut inner);
            }
            result
        };
        // After the lock is released: the observer may take other locks and write the audit log.
        if let Ok(resolution) = &result {
            if resolution.accepted {
                crate::agent_authority::consent::on_resolved(&resolution.question);
                // A pause card for a run that used its budget: apply "Add turns" / "Stop here".
                crate::budgets::answer_applied(&resolution.question);
                crate::handoff::on_resolved(&resolution.question);
                // A card about a GitHub issue: start the run, open the draft PR, throw the copy away.
                crate::issue_intake::live::on_resolved(&resolution.question);
                // A card about a failed CI check: start the fix, push it, stop watching.
                crate::ci_fix::live::on_resolved(&resolution.question);
            }
        }
        result
    }

    fn answer_locked(
        inner: &mut Inner,
        id: &str,
        choice_id: Option<&str>,
        text: Option<&str>,
        answered_by: &str,
        now: u64,
    ) -> Result<Resolution, QuestionError> {
        let index = inner
            .items
            .iter()
            .position(|q| q.id == id)
            .ok_or_else(|| QuestionError::new("QUESTION_NOT_FOUND", format!("no question {id}")))?;
        let q = &mut inner.items[index];
        if q.state != QuestionState::Pending {
            return Ok(Resolution { accepted: false, question: q.clone() });
        }
        let answer = match (choice_id, text) {
            (Some(_), Some(_)) => {
                return Err(QuestionError::new("INVALID_ARGUMENT", "send either choice_id or text, not both"))
            }
            (Some(choice), None) => {
                if !q.choices.iter().any(|c| c.id == choice) {
                    return Err(QuestionError::new("INVALID_CHOICE", format!("unknown choice id: {choice}")));
                }
                Answer { choice_id: Some(choice.to_string()), text: None, answered_by: answered_by.into() }
            }
            (None, Some(free)) => {
                let free = trimmed(free);
                if !q.allow_free_text {
                    return Err(QuestionError::new(
                        "FREE_TEXT_NOT_ALLOWED",
                        "this question only takes one of its choices",
                    ));
                }
                if free.is_empty() {
                    return Err(QuestionError::new("INVALID_ARGUMENT", "answer text must not be empty"));
                }
                if free.chars().count() > MAX_ANSWER_TEXT_CHARS {
                    return Err(QuestionError::new(
                        "ANSWER_TOO_LONG",
                        format!("answer is longer than {MAX_ANSWER_TEXT_CHARS} characters"),
                    ));
                }
                Answer { choice_id: None, text: Some(free), answered_by: answered_by.into() }
            }
            (None, None) => return Err(QuestionError::new("INVALID_ARGUMENT", "send choice_id or text")),
        };
        q.state = QuestionState::Answered;
        q.answer = Some(answer);
        q.resolved_at_ms = Some(now);
        Ok(Resolution { accepted: true, question: q.clone() })
    }

    /// Dismiss (person) or cancel (asking agent). Idempotent like `answer`.
    pub fn dismiss(&self, id: &str, now: u64) -> Result<Resolution, QuestionError> {
        let resolution = {
            let mut inner = self.inner.lock();
            let expired = Self::expire_locked(&mut inner, now);
            let Some(q) = inner.items.iter_mut().find(|q| q.id == id) else {
                if expired {
                    self.bump(&mut inner);
                }
                return Err(QuestionError::new("QUESTION_NOT_FOUND", format!("no question {id}")));
            };
            let accepted = q.state == QuestionState::Pending;
            if accepted {
                q.state = QuestionState::Cancelled;
                q.resolved_at_ms = Some(now);
            }
            let question = q.clone();
            if accepted || expired {
                self.bump(&mut inner);
            }
            Resolution { accepted, question }
        };
        if resolution.accepted {
            crate::agent_authority::consent::on_resolved(&resolution.question);
            // Dismissing an issue card says "not now" (the routes let only the person do it).
            crate::issue_intake::live::on_resolved(&resolution.question);
            // Dismissing a CI card says "not now" / "don't push".
            crate::ci_fix::live::on_resolved(&resolution.question);
        }
        Ok(resolution)
    }

    /// Block until the question leaves `Pending` or `wait` passes. Returns its latest state,
    /// or `None` when the id is unknown.
    pub fn wait_resolved(&self, id: &str, wait: Duration) -> Option<Question> {
        let deadline = std::time::Instant::now() + wait;
        let mut inner = self.inner.lock();
        loop {
            let now = now_ms();
            if Self::expire_locked(&mut inner, now) {
                self.bump(&mut inner);
            }
            let q = inner.items.iter().find(|q| q.id == id).cloned()?;
            if q.state != QuestionState::Pending {
                return Some(q);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Some(q);
            }
            // Wake for a change, or when this question's own expiry passes.
            let until_expiry = Duration::from_millis(q.expires_at_ms.saturating_sub(now).max(1));
            self.changed.wait_for(&mut inner, remaining.min(until_expiry));
        }
    }

    /// Marks overdue pending questions timed out and drops stale resolved ones. True when a
    /// question changed state.
    fn expire_locked(inner: &mut Inner, now: u64) -> bool {
        let mut changed = false;
        for q in inner.items.iter_mut() {
            if q.state == QuestionState::Pending && q.expires_at_ms <= now {
                q.state = QuestionState::TimedOut;
                q.resolved_at_ms = Some(now);
                changed = true;
            }
        }
        inner
            .items
            .retain(|q| q.resolved_at_ms.map_or(true, |t| now.saturating_sub(t) < RESOLVED_KEEP_MS));
        let resolved = inner.items.iter().filter(|q| q.state != QuestionState::Pending).count();
        if resolved > RESOLVED_KEEP_MAX {
            let mut drop = resolved - RESOLVED_KEEP_MAX;
            inner.items.retain(|q| {
                if drop > 0 && q.state != QuestionState::Pending {
                    drop -= 1;
                    false
                } else {
                    true
                }
            });
        }
        changed
    }

    fn bump(&self, inner: &mut Inner) {
        inner.revision += 1;
        self.changed.notify_all();
    }
}

static STORE: Lazy<QuestionStore> = Lazy::new(QuestionStore::new);

/// The process-wide store used by the bridge routes, Tauri commands and the MCP tool.
pub fn store() -> &'static QuestionStore {
    &STORE
}

/// The MCP tool result for a (possibly unresolved) question.
pub fn tool_result(question: &Question) -> serde_json::Value {
    use serde_json::json;
    match question.state {
        QuestionState::Pending => json!({ "resolved": false, "question_id": question.id }),
        QuestionState::Answered => {
            let answer = question.answer.clone().unwrap_or(Answer {
                choice_id: None,
                text: None,
                answered_by: "desktop".into(),
            });
            let mut out = json!({
                "resolved": true,
                "question_id": question.id,
                "answered": true,
                "answered_by": answer.answered_by,
            });
            if let Some(choice) = answer.choice_id {
                out["choice_id"] = json!(choice);
            }
            if let Some(text) = answer.text {
                out["text"] = json!(text);
            }
            out
        }
        QuestionState::Cancelled => {
            json!({ "resolved": true, "question_id": question.id, "answered": false, "reason": "cancelled" })
        }
        QuestionState::TimedOut => {
            json!({ "resolved": true, "question_id": question.id, "answered": false, "reason": "timeout" })
        }
    }
}
