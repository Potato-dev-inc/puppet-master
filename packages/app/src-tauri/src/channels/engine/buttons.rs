//! Button taps. The button data is only `g:<token>`: it means nothing until `Grants` finds it, once,
//! inside its time. Then the live attention list is read again and `policy` decides again, so a
//! card that changed (or got riskier) since the message was sent is never answered from here.
//!
//! "Allow once…" is two taps: the first one (`ConfirmAllow`) edits the message into a question with
//! `Yes, allow once` and `Cancel`; only the token behind `Yes` can make the answer.

use super::{Effect, Engine, Out, NOT_ALLOWED};
use crate::attention::answer::{Reply, Request};
use crate::attention::caps::DeviceCaps;
use crate::attention::model::{Item, Kind as ItemKind, State};
use crate::channels::config::TelegramConfig;
use crate::channels::grants::{GrantAction, GrantError, CONFIRM_TTL_MS};
use crate::channels::host::Host;
use crate::channels::policy::{self, Answerable};
use crate::channels::render::{self, Outgoing};
use crate::channels::telegram::api::InCallback;
use crate::mobile_pairing::Cap;
use crate::push::triggers::shown_in_full;
use crate::questions::QuestionState;
use serde_json::Value;

const EXPIRED: &str = "This button expired.";
const CHANGED: &str = "This changed. Open Puppet Master.";
const ALREADY_PC: &str = "Already answered on your PC";
const OPEN_TO_ANSWER: &str = "Open Puppet Master to answer.";
const SLOW_DOWN: &str = "Slow down.";
const CANCELLED: &str = "Cancelled. Nothing was allowed.";
/// How many confirm tokens are remembered.
const MAX_CONFIRM_TOKENS: usize = 64;
/// A toast under the button is at most 200 characters in Telegram.
const TOAST_CHARS: usize = 200;

impl Engine {
    pub(super) fn on_callback(&self, host: &dyn Host, cfg: &TelegramConfig, now: u64, cb: &InCallback, out: &mut Out) {
        let Some(paired) = &cfg.paired else { return };
        if !cfg.enabled {
            return;
        }
        // Only the paired person, tapping a button in the paired chat. Anyone else is a stranger.
        if cb.from_id != paired.user_id || cb.chat_id != Some(paired.chat_id) {
            return self.stranger(host, now);
        }
        if !self.limits.lock().answers.take(now) {
            return out.effects.push(toast(cb, SLOW_DOWN));
        }
        let Some(token) = cb.data.as_deref().and_then(|d| d.strip_prefix("g:")) else {
            return out.effects.push(toast(cb, EXPIRED));
        };
        match self.grants.redeem(token, now) {
            // Never issued, already used, or dropped: nothing to edit, the toast is enough.
            Err(GrantError::Unknown) => out.effects.push(toast(cb, EXPIRED)),
            Err(GrantError::Expired) => out.effects.extend(receipt(cb, EXPIRED)),
            Ok(action) => {
                let effects = self.run_grant(host, cfg, now, cb, token, action);
                out.effects.extend(effects);
            }
        }
    }

    fn run_grant(&self, host: &dyn Host, cfg: &TelegramConfig, now: u64, cb: &InCallback, token: &str, action: GrantAction) -> Vec<Effect> {
        if !cfg.caps.has(Cap::Answer) {
            return receipt(cb, NOT_ALLOWED);
        }
        match action {
            GrantAction::Cancel { .. } => receipt(cb, CANCELLED),
            GrantAction::ConfirmAllow { item_id, prompt_id, occurrence, fingerprint } => {
                self.confirm_step(host, now, cb, &item_id, &prompt_id, occurrence, &fingerprint)
            }
            GrantAction::Answer { item_id, choice, prompt_id, occurrence, fingerprint } => {
                let request = Request {
                    choice,
                    text: None,
                    prompt_id,
                    occurrence,
                    fingerprint,
                    answer_key: Some(format!("tg-{token}")),
                };
                self.answer_step(host, cfg, cb, token, &item_id, request)
            }
        }
    }

    /// First tap on "Allow once…": show what would be allowed and ask again.
    #[allow(clippy::too_many_arguments)]
    fn confirm_step(&self, host: &dyn Host, now: u64, cb: &InCallback, item_id: &str, prompt_id: &str, occurrence: u32, fingerprint: &str) -> Vec<Effect> {
        let snapshot = host.snapshot();
        let Some(item) = snapshot.items.iter().find(|i| i.id == item_id && i.state == State::Open) else {
            return receipt(cb, CHANGED);
        };
        match allowed_for(item) {
            Allowed::Confirm => {}
            Allowed::Refused(sentence) => return receipt(cb, sentence),
            Allowed::Choices | Allowed::NoButtons => return receipt(cb, CHANGED),
        }
        let same = item.prompt.as_ref().is_some_and(|p| p.prompt_id == prompt_id && p.occurrence == occurrence && p.fingerprint == fingerprint);
        if !same {
            return receipt(cb, CHANGED);
        }
        // What the person is asked to allow must be shown whole and untouched by redaction.
        let command = item.prompt.as_ref().and_then(|p| p.summary.as_deref()).or(item.sentence.as_deref()).unwrap_or(&item.title);
        if !shown_in_full(command) || render::changed_by_redaction(command) {
            return receipt(cb, OPEN_TO_ANSWER);
        }
        let Some(message_id) = cb.message_id else { return vec![toast(cb, EXPIRED)] };
        let folder = item.pane.as_ref().and_then(|p| p.project.as_deref()).map(last_folder).filter(|f| !f.is_empty());
        let question = match folder {
            Some(folder) => format!("Allow once? {command} · {folder}"),
            None => format!("Allow once? {command}"),
        };
        let yes = self.grants.issue(
            GrantAction::Answer {
                item_id: item.id.clone(),
                choice: Some("allow_once".to_string()),
                prompt_id: Some(prompt_id.to_string()),
                occurrence: Some(occurrence),
                fingerprint: Some(fingerprint.to_string()),
            },
            now,
            CONFIRM_TTL_MS,
        );
        let cancel = self.grants.issue(GrantAction::Cancel { item_id: item.id.clone() }, now, CONFIRM_TTL_MS);
        let (Some(yes_button), Some(cancel_button)) = (render::button("Yes, allow once", &format!("g:{yes}")), render::button("Cancel", &format!("g:{cancel}")))
        else {
            return receipt(cb, CHANGED);
        };
        {
            let mut misc = self.misc.lock();
            misc.confirm_tokens.push_back(yes);
            while misc.confirm_tokens.len() > MAX_CONFIRM_TOKENS {
                misc.confirm_tokens.pop_front();
            }
        }
        let out = Outgoing { text: render::clean(&question, render::MAX_OUT_CHARS), keyboard: vec![vec![yes_button, cancel_button]] };
        vec![Effect::Edit { message_id, out }, Effect::AnswerCallback { id: cb.id.clone(), text: None }]
    }

    /// The tap that answers: re-check the card against the policy, then use the shared answer path.
    fn answer_step(&self, host: &dyn Host, cfg: &TelegramConfig, cb: &InCallback, token: &str, item_id: &str, request: Request) -> Vec<Effect> {
        let snapshot = host.snapshot();
        let Some(item) = snapshot.items.iter().find(|i| i.id == item_id) else {
            return receipt(cb, EXPIRED);
        };
        match allowed_for(item) {
            Allowed::Refused(sentence) => return receipt(cb, sentence),
            Allowed::NoButtons => return receipt(cb, OPEN_TO_ANSWER),
            Allowed::Choices => {
                if !self.question_choice_ok(host, item, request.choice.as_deref()) {
                    return receipt(cb, EXPIRED);
                }
            }
            Allowed::Confirm => {
                let choice = request.choice.as_deref();
                let complete = request.prompt_id.is_some() && request.occurrence.is_some() && request.fingerprint.is_some();
                if !complete || !matches!(choice, Some("allow_once") | Some("deny")) {
                    return receipt(cb, EXPIRED);
                }
                // Allow once only after the confirm tap: the token must be one we made for that.
                if choice == Some("allow_once") && !self.take_confirm_token(token) {
                    return receipt(cb, EXPIRED);
                }
            }
        }
        let caps = DeviceCaps::from_caps(cfg.caps.with(Cap::Control, false));
        let reply = host.answer(&item.id, &request, caps);
        receipt(cb, &receipt_text(&reply))
    }

    /// A question is answered only while it is pending, is a plain `ask_user`, and offers the choice.
    fn question_choice_ok(&self, host: &dyn Host, item: &Item, choice: Option<&str>) -> bool {
        let Some(choice) = choice else { return false };
        let Some((_, subject)) = item.key.split_once(':') else { return false };
        let Some(question) = host.question(subject) else { return false };
        question.state == QuestionState::Pending
            && question.consent.is_none()
            && question.kind.is_none()
            && question.limit.is_none()
            && question.choices.iter().any(|c| c.id == choice)
    }

    fn take_confirm_token(&self, token: &str) -> bool {
        let mut misc = self.misc.lock();
        match misc.confirm_tokens.iter().position(|t| t == token) {
            Some(pos) => {
                misc.confirm_tokens.remove(pos);
                true
            }
            None => false,
        }
    }
}

/// What the chat may do with one card, from `policy`, with the kind checked once more.
enum Allowed {
    /// A question: its own choices.
    Choices,
    /// A low-risk approval: Allow once (after a confirm) or Deny.
    Confirm,
    /// No buttons for this card; the sentence says where to go.
    Refused(&'static str),
    /// The kind has nothing to answer.
    NoButtons,
}

fn allowed_for(item: &Item) -> Allowed {
    match policy::answerable(item.kind, item.risk.as_ref().map(|r| r.level)) {
        Answerable::Choices if item.kind == ItemKind::Question => Allowed::Choices,
        Answerable::AllowOnceWithConfirm if item.kind == ItemKind::Approval => Allowed::Confirm,
        Answerable::Never(sentence) => Allowed::Refused(sentence),
        Answerable::Choices | Answerable::AllowOnceWithConfirm => Allowed::NoButtons,
    }
}

/// The last folder name of a path-like project. Never a full path.
fn last_folder(project: &str) -> String {
    project.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

/// The words under a tapped button, from what the shared answer path said.
fn receipt_text(reply: &Reply) -> String {
    let (status, body) = reply;
    let text = |key: &str| body.get(key).and_then(Value::as_str);
    match *status {
        200 => match text("outcome") {
            Some("answered") => text("choice_label").filter(|l| !l.trim().is_empty()).unwrap_or("Answered").to_string(),
            Some("already_answered") => match text("by") {
                Some("you") => text("choice_label").filter(|l| !l.trim().is_empty()).unwrap_or("Already answered").to_string(),
                Some("phone") => "Already answered on a phone".to_string(),
                _ => ALREADY_PC.to_string(),
            },
            Some("gone") => EXPIRED.to_string(),
            Some("failed") => "Could not send that. Open Puppet Master.".to_string(),
            _ => CHANGED.to_string(),
        },
        403 => match text("error") {
            Some("not_from_chat") => OPEN_TO_ANSWER.to_string(),
            _ => NOT_ALLOWED.to_string(),
        },
        _ => "That did not work. Open Puppet Master.".to_string(),
    }
}

/// Stops the spinner with a short note, changes nothing else.
fn toast(cb: &InCallback, text: &str) -> Effect {
    Effect::AnswerCallback { id: cb.id.clone(), text: Some(render::clean(text, TOAST_CHARS)) }
}

/// The message becomes a receipt with no buttons, and the spinner stops.
fn receipt(cb: &InCallback, text: &str) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(message_id) = cb.message_id {
        effects.push(Effect::Edit { message_id, out: render::plain(text) });
    }
    effects.push(toast(cb, text));
    effects
}
