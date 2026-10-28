//! Text Your Agents: what the chat hears about. `Notifier` decides, for one thing that happened (a
//! card, a question, a run end, a companion reply, a pane's turn end), whether the chat gets a
//! message, what it says and which buttons come with it.
//!
//! Rules (spec 3.3 "Outbound notifications", D-8, D-9):
//! - nothing goes out unless the chat is on and paired;
//! - "needs you" kinds (`Kind::is_urgent`) go out unless the 30 s gate or the outbound bucket holds
//!   them back. Those are counted, and one "N more things need you" note follows (`flush_held`);
//! - a run end goes out only with `notify_runs`, or when the pane is in a conversation window;
//! - a companion's reply goes out only inside that companion's window;
//! - a pane's turn end goes out only inside that pane's window;
//! - short mode, or no `watch` cap: no agent text (`View::Hidden`) and no buttons;
//! - buttons only where `policy` allows them, with the `answer` cap, and only one-tap grants from `grants`.
//!
//! The output is a `Note`: text, buttons, and the target a reply to that message means. The engine
//! turns it into a send and records the new message id against `remember`.

use crate::attention::model::{Kind as Item, Level};
use crate::channels::config::TelegramConfig;
use crate::channels::grants::{GrantAction, Grants, ANSWER_TTL_MS};
use crate::channels::policy::{answerable, Answerable};
use crate::channels::rate::Bucket;
use crate::channels::refs::{Refs, Target};
use crate::channels::render::{self, button, clean_multiline, Button, Outgoing, MAX_OUT_CHARS};
use crate::mobile_pairing::Cap;
use crate::push::hub::{Candidate, ItemRef};
use crate::push::triggers::{
    copy_for_view, shown_in_full, truncate_chars, Gate, Kind, PromptInfo, Verdict, View,
    RATE_LIMIT_MS,
};
use crate::questions::{Question, MAX_CHOICES};
use crate::redact::Redacted;
use parking_lot::Mutex;

/// The question text the chat shows (spec 3.3).
const MAX_QUESTION_CHARS: usize = 600;
/// Shown under a card the chat cannot answer. The same words as `policy` for an approval that is not
/// low risk; a test keeps them equal.
const OPEN_TO_ANSWER: &str = "Open Puppet Master to answer.";

/// One message for the chat. The engine turns it into a send and, after the send, records the new
/// message id against `remember` (so a reply to that message means that pane or companion).
#[derive(Debug, Clone)]
pub struct Note {
    pub out: Outgoing,
    pub remember: Option<Target>,
}

impl Note {
    /// The send the poller performs: to the paired chat, linked to `remember` once Telegram gives the
    /// message an id. TODO(TG-12): the engine has no notify entry point yet; the live wiring (hub events
    /// to `Notifier::offer`) pushes this effect into the poller's `Running::outbox`.
    pub fn into_effect(self) -> crate::channels::engine::Effect {
        crate::channels::engine::Effect::Send { out: self.out, reply_to: None, remember: self.remember }
    }
}

/// What the notifier reads and changes outside itself: the windows and message links, the button
/// grants, and the outbound bucket the engine shares (20 messages a minute in all).
pub struct Links<'a> {
    pub refs: &'a Refs,
    pub grants: &'a Grants,
    pub outbound: &'a mut Bucket,
}

/// Urgent messages held back by the gate or the bucket, waiting for one summary note.
#[derive(Debug, Clone, Copy)]
struct Held {
    count: u32,
    since_ms: u64,
}

/// The notifier's own memory: the 30 s gate per key and duplicate prompts (`triggers::Gate`), and the
/// held-back count. Lost on restart, like the rest of the chat state.
#[derive(Default)]
pub struct Notifier {
    gate: Mutex<Gate>,
    held: Mutex<Option<Held>>,
}

impl Notifier {
    /// A candidate that happened on the desktop. `question` is the `ask_user` question behind a
    /// `Kind::Question` candidate (its choices become buttons); `None` for everything else.
    pub fn offer(
        &self,
        c: &Candidate,
        question: Option<&Question>,
        cfg: &TelegramConfig,
        mut links: Links<'_>,
        now: u64,
    ) -> Option<Note> {
        if !sending(cfg) {
            return None;
        }
        match c.kind {
            // A companion's reply has its own entry point (`companion_reply`).
            Kind::CompanionReply => return None,
            Kind::RunFinished | Kind::RunFailed => {
                let window = c
                    .pane_id
                    .as_deref()
                    .is_some_and(|pane| links.refs.in_window(&pane_target(pane), now));
                if !cfg.notify_runs && !window {
                    return None;
                }
            }
            _ => {}
        }
        let prompt_id = c.prompt.as_ref().map(|p| p.prompt_id.as_str());
        let verdict = self.gate.lock().admit(now, &c.key, c.kind, prompt_id);
        match verdict {
            Verdict::Send => {}
            Verdict::RateLimited => {
                if c.kind.is_urgent() {
                    self.hold(now);
                }
                return None;
            }
            Verdict::Duplicate | Verdict::Resolved => return None,
        }
        if !links.outbound.take(now) {
            if c.kind.is_urgent() {
                self.hold(now);
            }
            return None;
        }
        let view = view_for(cfg);
        let (lines, keyboard) = self.body(c, question, cfg, view, &links, now);
        Some(Note {
            out: Outgoing {
                text: compose(&lines),
                keyboard,
            },
            remember: c.pane_id.clone().map(Target::Pane),
        })
    }

    /// A companion's reply. Goes out only inside the window of that companion.
    pub fn companion_reply(
        &self,
        companion_id: &str,
        name: &str,
        text: &str,
        cfg: &TelegramConfig,
        mut links: Links<'_>,
        now: u64,
    ) -> Option<Note> {
        if !sending(cfg) || !cfg.caps.has(Cap::Watch) || text.trim().is_empty() {
            return None;
        }
        let target = Target::Companion(companion_id.to_string());
        if !links.refs.in_window(&target, now) {
            return None;
        }
        let lines = if cfg.short_mode {
            vec![format!("{name} replied. Open Puppet Master to read it.")]
        } else {
            vec![format!("{name} replied:"), text.to_string()]
        };
        if !links.outbound.take(now) {
            return None;
        }
        Some(Note {
            out: Outgoing {
                text: compose(&lines),
                keyboard: Vec::new(),
            },
            remember: Some(target),
        })
    }

    /// A pane's turn ended, and the pane is in its conversation window. `sentence` is the turn's
    /// one-line summary (`turn_summary`), `label` the agent's name, `heading` what the sidebar calls it.
    pub fn turn_finished(
        &self,
        pane_id: &str,
        label: &str,
        heading: Option<&str>,
        sentence: &str,
        cfg: &TelegramConfig,
        mut links: Links<'_>,
        now: u64,
    ) -> Option<Note> {
        if !sending(cfg) || !cfg.caps.has(Cap::Watch) {
            return None;
        }
        let target = pane_target(pane_id);
        if !links.refs.in_window(&target, now) {
            return None;
        }
        if self
            .gate
            .lock()
            .admit(now, pane_id, Kind::RunFinished, None)
            != Verdict::Send
        {
            return None;
        }
        if !links.outbound.take(now) {
            return None;
        }
        let line = if cfg.short_mode {
            format!("Done · {label}")
        } else {
            let who = heading
                .map(str::trim)
                .filter(|h| !h.is_empty())
                .unwrap_or(label);
            let sentence = sentence.trim();
            if sentence.is_empty() {
                format!("Done · {who}")
            } else {
                format!("Done · {who}: {sentence}")
            }
        };
        Some(Note {
            out: Outgoing {
                text: compose(&[line]),
                keyboard: Vec::new(),
            },
            remember: Some(target),
        })
    }

    /// The one summary for urgent messages held back. Sent once the first held message is older than
    /// the 30 s window and the outbound bucket has room. `None` while nothing is held.
    pub fn flush_held(&self, outbound: &mut Bucket, now: u64) -> Option<Note> {
        let mut held = self.held.lock();
        let pending = (*held)?;
        if now.saturating_sub(pending.since_ms) < RATE_LIMIT_MS || !outbound.take(now) {
            return None;
        }
        *held = None;
        let text = if pending.count == 1 {
            "1 more thing needs you. Send /status to see them.".to_string()
        } else {
            format!(
                "{} more things need you. Send /status to see them.",
                pending.count
            )
        };
        Some(Note {
            out: Outgoing {
                text: compose(&[text]),
                keyboard: Vec::new(),
            },
            remember: None,
        })
    }

    fn hold(&self, now: u64) {
        let mut held = self.held.lock();
        let since_ms = held.map(|h| h.since_ms).unwrap_or(now);
        let count = held.map(|h| h.count).unwrap_or(0).saturating_add(1);
        *held = Some(Held { count, since_ms });
    }

    /// The lines of a "needs you" message and its buttons.
    fn body(
        &self,
        c: &Candidate,
        question: Option<&Question>,
        cfg: &TelegramConfig,
        view: View,
        links: &Links<'_>,
        now: u64,
    ) -> (Vec<String>, Vec<Vec<Button>>) {
        let label = c.agent_label.as_str();
        let heading = c.heading.as_deref();
        let can_answer = view == View::Full && cfg.caps.has(Cap::Answer);
        match c.kind {
            Kind::WaitingInput => {
                let words = copy_for_view(
                    c.kind,
                    label,
                    heading,
                    c.prompt.as_ref(),
                    c.detail.as_deref(),
                    view,
                );
                let risk = c
                    .prompt
                    .as_ref()
                    .and_then(|p| p.risk.as_ref())
                    .map(|r| Level::from(r.level));
                let buttons = match (c.prompt.as_ref(), c.item.as_ref()) {
                    (Some(p), Some(item))
                        if can_answer
                            && answerable(Item::Approval, risk)
                                == Answerable::AllowOnceWithConfirm
                            && approval_safe(p) =>
                    {
                        approval_buttons(p, item, links.grants, now)
                    }
                    _ => None,
                };
                match buttons {
                    Some(row) => (vec![words.title, words.body], vec![row]),
                    None => (
                        vec![words.title, words.body, OPEN_TO_ANSWER.to_string()],
                        Vec::new(),
                    ),
                }
            }
            Kind::Question => {
                let words = copy_for_view(
                    c.kind,
                    label,
                    heading,
                    c.prompt.as_ref(),
                    c.detail.as_deref(),
                    view,
                );
                let mut lines = vec![words.title];
                match c.prompt.as_ref() {
                    Some(p) if view == View::Full && !p.text.trim().is_empty() => {
                        lines.push(format!("Question from {label}"));
                        lines.push(truncate_chars(&p.text, MAX_QUESTION_CHARS));
                    }
                    _ => lines.push(words.body),
                }
                let rows = if can_answer {
                    question_rows(question, c.item.as_ref(), links.grants, now)
                } else {
                    Vec::new()
                };
                if rows.is_empty() {
                    lines.push(OPEN_TO_ANSWER.to_string());
                }
                (lines, rows)
            }
            Kind::TrustGate | Kind::Consent | Kind::Budget | Kind::Limit | Kind::Error => {
                let words = copy_for_view(
                    c.kind,
                    label,
                    heading,
                    c.prompt.as_ref(),
                    c.detail.as_deref(),
                    view,
                );
                let mut lines = vec![words.title, words.body];
                if let Some(refusal) = refusal(c.kind) {
                    lines.push(refusal.to_string());
                }
                (lines, Vec::new())
            }
            Kind::RunFinished | Kind::RunFailed => {
                let words = copy_for_view(
                    c.kind,
                    label,
                    heading,
                    c.prompt.as_ref(),
                    c.detail.as_deref(),
                    view,
                );
                (vec![words.title, words.body], Vec::new())
            }
            // `companion_reply` builds these messages.
            Kind::CompanionReply => (Vec::new(), Vec::new()),
        }
    }
}

/// Sends happen only with the chat on and paired.
fn sending(cfg: &TelegramConfig) -> bool {
    cfg.enabled && cfg.paired.is_some()
}

/// Short mode and a missing `watch` cap both hide what the agent wrote.
fn view_for(cfg: &TelegramConfig) -> View {
    if cfg.short_mode || !cfg.caps.has(Cap::Watch) {
        View::Hidden
    } else {
        View::Full
    }
}

fn pane_target(pane_id: &str) -> Target {
    Target::Pane(pane_id.to_string())
}

/// An approval gets buttons only when it is a calm, low-risk command shown in full, and cleaning did
/// not change it (a secret or a path would have been removed from what the person is asked to allow).
fn approval_safe(p: &PromptInfo) -> bool {
    p.actionable && shown_in_full(&p.text) && !render::changed_by_redaction(&p.text)
}

/// **Allow once…** (opens the confirm step, which the engine finishes) and **Deny**, one row.
fn approval_buttons(
    p: &PromptInfo,
    item: &ItemRef,
    grants: &Grants,
    now: u64,
) -> Option<Vec<Button>> {
    let allow = grants.issue(
        GrantAction::ConfirmAllow {
            item_id: item.id.clone(),
            prompt_id: p.prompt_id.clone(),
            occurrence: item.occurrence,
            fingerprint: item.fingerprint.clone(),
        },
        now,
        ANSWER_TTL_MS,
    );
    let deny = grants.issue(
        GrantAction::Answer {
            item_id: item.id.clone(),
            choice: Some("deny".to_string()),
            prompt_id: Some(p.prompt_id.clone()),
            occurrence: Some(item.occurrence),
            fingerprint: Some(item.fingerprint.clone()),
        },
        now,
        ANSWER_TTL_MS,
    );
    Some(vec![
        button("Allow once…", &format!("g:{allow}"))?,
        button("Deny", &format!("g:{deny}"))?,
    ])
}

/// One row per choice, at most `MAX_CHOICES`. Empty when the question or its card is missing.
fn question_rows(
    question: Option<&Question>,
    item: Option<&ItemRef>,
    grants: &Grants,
    now: u64,
) -> Vec<Vec<Button>> {
    let (Some(question), Some(item)) = (question, item) else {
        return Vec::new();
    };
    question
        .choices
        .iter()
        .filter(|choice| !choice.label.trim().is_empty())
        .take(MAX_CHOICES)
        .filter_map(|choice| {
            let token = grants.issue(
                GrantAction::Answer {
                    item_id: item.id.clone(),
                    choice: Some(choice.id.clone()),
                    prompt_id: None,
                    occurrence: None,
                    fingerprint: None,
                },
                now,
                ANSWER_TTL_MS,
            );
            button(&choice.label, &format!("g:{token}")).map(|b| vec![b])
        })
        .collect()
}

/// The one sentence the chat shows under a card it cannot answer (the `policy` wording).
fn refusal(kind: Kind) -> Option<&'static str> {
    let item = match kind {
        Kind::TrustGate => Item::TrustGate,
        Kind::Consent => Item::Consent,
        Kind::Budget => Item::Budget,
        Kind::Limit => Item::Limit,
        Kind::Error => Item::Error,
        _ => return None,
    };
    match answerable(item, None) {
        Answerable::Never(text) => Some(text),
        Answerable::Choices | Answerable::AllowOnceWithConfirm => None,
    }
}

/// Joins the lines of a message with line breaks, then cleans the WHOLE text once
/// (`render::clean_multiline`: redaction, e-mail masking, control and bidi characters, then the cap).
/// Redacting line by line would miss a secret that spans lines, such as a PEM private key.
fn compose(lines: &[String]) -> Redacted<String> {
    let joined = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    clean_multiline(&joined, MAX_OUT_CHARS)
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
