//! Tag Your Agent: the group path of the engine.
//!
//! A group message reaches this file only when the feature switch (`tagBot.enabled`) is on; with it off
//! `on_message` returns before anything here runs and groups are dropped exactly as before.
//!
//! The order is fixed, and a group message NEVER reaches command parsing, pairing or the approval policy:
//!
//! 1. A bot, or no sender: dropped. The channel must be on and paired.
//! 2. Older than 10 minutes (or from before the channel was turned on): dropped, with no "skipped" note.
//! 3. Not a tag (an `@botname` mention entity, or a reply to one of the bot's own messages): dropped.
//! 4. A group that is not approved on the PC: remembered as pending (id and title only), nothing is sent.
//! 5. Who may tag: `only_me` is the paired Telegram user; `anyone` is anyone, with an hourly cap each.
//! 6. At most two questions are asked at once; then the per-person and the daily cap.
//! 7. The question is handed on as an `Effect::Ask`. The poller runs it on its own thread (an ask takes
//!    seconds to a minute), then `answer_job` turns the result into one threaded reply.
//!
//! The answer comes from the person's own agent through the read-only headless ask (`Host::ask_group`);
//! nothing is written and no pane is touched. Group text is data (`tag_prompt`), the reply is cleaned like
//! every other message (`render`), and a failure is a fixed sentence with no detail.

use super::{Effect, Engine, Out};
use crate::audit::Kind as AuditKind;
use crate::channels::config::TelegramConfig;
use crate::channels::host::{GroupAsk, Host};
use crate::channels::rate::Bucket;
use crate::channels::render::{self, Outgoing};
use crate::channels::tag_config::{GroupWho, PendingGroup, MAX_PENDING, MAX_TITLE_CHARS};
use crate::channels::tag_log::{GroupAnswerRecord, ANSWER_CHARS, QUESTION_CHARS};
use crate::channels::tag_prompt;
use crate::channels::telegram::api::InMessage;
use serde_json::json;
use std::collections::HashMap;

/// What the group is told when the agent could not answer. No detail, no path, no error text.
pub const FAILED: &str = "Sorry, I couldn't answer that right now.";
/// Said once per group per day when the daily cap is used up.
pub const DAY_LIMIT: &str = "I've hit today's limit. Ask me again tomorrow.";
const BUSY: &str = "I'm busy with other questions. Try again in a minute.";
const UNTITLED: &str = "Untitled group";

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;
/// Questions asked at the same time. Each one starts a headless agent, so this is a cost brake.
const MAX_INFLIGHT: usize = 2;
/// Per-person buckets kept; past this the table is cleared rather than grown.
const MAX_PEOPLE: usize = 1000;

/// One question, ready to ask. Made by `on_group_message`, run by the poller.
#[derive(Debug, Clone)]
pub struct GroupJob {
    pub chat_id: i64,
    /// The tagged message: the reply goes under it.
    pub reply_to: i64,
    pub group_title: String,
    /// The asker's display name, cleaned.
    pub asker: String,
    /// The tagged message without the tag, cleaned and cut (for the record).
    pub question: String,
    pub ask: GroupAsk,
}

/// What the group path remembers between messages. In memory only: a restart starts the caps afresh.
pub(super) struct GroupState {
    per_person: HashMap<(i64, i64), (u32, Bucket)>,
    daily: Option<(u32, Bucket)>,
    /// The UTC day each group was last told about the daily limit.
    limit_note_day: HashMap<i64, u64>,
    pending_adds: Bucket,
    busy_note: Bucket,
    inflight: usize,
}

impl GroupState {
    pub(super) fn new() -> Self {
        Self {
            per_person: HashMap::new(),
            daily: None,
            limit_note_day: HashMap::new(),
            // A bot added to many groups cannot make the PC write the settings file all day.
            pending_adds: Bucket::new(10, HOUR_MS),
            busy_note: Bucket::new(1, 60_000),
            inflight: 0,
        }
    }

    /// One token from this person's hourly bucket. The bucket is rebuilt when the cap was changed.
    fn take_person(&mut self, key: (i64, i64), cap: u32, now: u64) -> bool {
        if self.per_person.len() >= MAX_PEOPLE && !self.per_person.contains_key(&key) {
            self.per_person.clear();
        }
        let slot = self.per_person.entry(key).or_insert_with(|| (cap, Bucket::new(cap, HOUR_MS)));
        if slot.0 != cap {
            *slot = (cap, Bucket::new(cap, HOUR_MS));
        }
        slot.1.take(now)
    }

    fn take_daily(&mut self, cap: u32, now: u64) -> bool {
        let slot = self.daily.get_or_insert_with(|| (cap, Bucket::new(cap, DAY_MS)));
        if slot.0 != cap {
            *slot = (cap, Bucket::new(cap, DAY_MS));
        }
        slot.1.take(now)
    }
}

/// Frees a question's place when it ends, however it ends (also on a panic).
struct Inflight<'a>(&'a Engine);

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        let mut state = self.0.group.lock();
        state.inflight = state.inflight.saturating_sub(1);
    }
}

/// A reply in a group, under the message that tagged the bot.
fn reply(chat_id: i64, reply_to: i64, text: &str) -> Effect {
    Effect::Reply { chat_id, reply_to, out: render::plain(text) }
}

/// True when the message tags this bot: an `@botname` mention entity, or a reply to one of the bot's own
/// messages (its id from `getMe`). A name that only looks like a tag in the text does not count.
fn is_tag(cfg: &TelegramConfig, m: &InMessage, bot_username: &str) -> bool {
    let name = format!("@{bot_username}");
    let by_mention = m.mentions.iter().any(|mention| mention.eq_ignore_ascii_case(&name));
    let by_reply = m.reply_to_from_is_bot && cfg.bot_id.is_some() && m.reply_to_from_id == cfg.bot_id;
    by_mention || by_reply
}

/// A display name for the record: first name, else `@username`, else "Someone".
fn asker_name(m: &InMessage) -> String {
    let raw = m.first_name.clone().filter(|n| !n.trim().is_empty()).or_else(|| m.username.as_ref().map(|u| format!("@{u}")));
    render::clean(&raw.unwrap_or_else(|| "Someone".into()), 40).into_inner()
}

impl Engine {
    /// A message from a group. See the module comment for the order.
    pub(super) fn on_group_message(&self, host: &dyn Host, cfg: &TelegramConfig, now: u64, m: &InMessage, out: &mut Out) {
        if !cfg.tag_bot.enabled {
            return;
        }
        let Some(from) = m.from_id else { return };
        if m.from_is_bot || !m.chat_group {
            return;
        }
        let (Some(paired), Some(bot_username)) = (&cfg.paired, cfg.bot_username.as_deref()) else { return };
        if !cfg.enabled || super::is_stale(cfg, m.date_s, now) {
            return;
        }
        let Some(text) = m.text.as_deref().filter(|t| !t.trim().is_empty()) else { return };
        if !is_tag(cfg, m, bot_username) {
            return;
        }
        let Some(group) = cfg.tag_bot.group(m.chat_id) else {
            self.remember_pending(host, m, now);
            return;
        };
        if group.who == GroupWho::OnlyMe && from != paired.user_id {
            return;
        }
        let question = tag_prompt::strip_mention(text, bot_username);
        let replied = m.reply_to_text.as_deref().filter(|t| !t.trim().is_empty());
        if question.is_empty() && replied.is_none() {
            return;
        }
        let reply_to = m.message_id;
        {
            let mut state = self.group.lock();
            if state.inflight >= MAX_INFLIGHT {
                if state.busy_note.take(now) {
                    out.effects.push(reply(m.chat_id, reply_to, BUSY));
                }
                return;
            }
            if group.who == GroupWho::Anyone && !state.take_person((m.chat_id, from), cfg.tag_bot.per_person_cap, now) {
                return;
            }
            if !state.take_daily(cfg.tag_bot.daily_cap, now) {
                let today = now / DAY_MS;
                if state.limit_note_day.insert(m.chat_id, today) != Some(today) {
                    out.effects.push(reply(m.chat_id, reply_to, DAY_LIMIT));
                }
                return;
            }
            state.inflight += 1;
        }
        let job = GroupJob {
            chat_id: m.chat_id,
            reply_to,
            group_title: render::clean(m.chat_title.as_deref().unwrap_or(&group.title), MAX_TITLE_CHARS).into_inner(),
            asker: asker_name(m),
            question: render::clean(&question, QUESTION_CHARS).into_inner(),
            ask: GroupAsk {
                agent: group.agent.clone(),
                folder: group.folder.clone(),
                prompt: tag_prompt::compose(&question, replied, &group.style),
            },
        };
        out.effects.push(Effect::Ask(job));
    }

    /// A group that tagged the bot and is not approved: its id and title are kept so the person can approve
    /// it on the PC. Nothing is sent to the group. Only a new group (or a new title) writes the file.
    fn remember_pending(&self, host: &dyn Host, m: &InMessage, now: u64) {
        let title = m.chat_title.as_deref().map(|t| render::clean(t, MAX_TITLE_CHARS).into_inner()).filter(|t| !t.trim().is_empty());
        let title = title.unwrap_or_else(|| UNTITLED.to_string());
        let mut cfg = host.config();
        if cfg.tag_bot.group(m.chat_id).is_some() {
            return;
        }
        match cfg.tag_bot.pending.iter_mut().find(|p| p.chat_id == m.chat_id) {
            Some(p) if p.title == title => return,
            Some(p) => p.title = title,
            None => {
                if !self.group.lock().pending_adds.take(now) {
                    return;
                }
                cfg.tag_bot.pending.push(PendingGroup { chat_id: m.chat_id, title, seen_at_ms: now });
                if cfg.tag_bot.pending.len() > MAX_PENDING {
                    let extra = cfg.tag_bot.pending.len() - MAX_PENDING;
                    cfg.tag_bot.pending.drain(..extra);
                }
                host.audit(AuditKind::ChannelMessage, "A group asked to use the bot", json!({ "scope": "group", "event": "pending" }));
            }
        }
        match host.save_config(&cfg) {
            Ok(()) => host.changed(),
            Err(err) => tracing::warn!(error = %err, "could not save a pending group"),
        }
    }

    /// Runs one question to its end and returns the reply to send (nothing when the person turned the bot
    /// off or removed the group while it was thinking). Blocks while the agent works, so it runs on its own
    /// thread, never the poll thread.
    pub fn answer_job(&self, host: &dyn Host, job: GroupJob) -> Vec<Effect> {
        let _place = Inflight(self);
        let outcome = host.ask_group(&job.ask);
        let cfg = host.config();
        let still_on = cfg.enabled && cfg.paired.is_some() && cfg.tag_bot.enabled && cfg.tag_bot.group(job.chat_id).is_some();
        if !still_on {
            return Vec::new();
        }
        let answer = outcome.answer.as_deref().map(str::trim).filter(|a| !a.is_empty());
        let agent = if outcome.agent_label.trim().is_empty() { "No agent".to_string() } else { outcome.agent_label.clone() };
        host.record_group_answer(&GroupAnswerRecord {
            at_ms: host.now_ms(),
            group_title: job.group_title.clone(),
            asker: job.asker.clone(),
            question: job.question.clone(),
            answer: answer.map(|a| render::clean(a, ANSWER_CHARS).into_inner()),
            agent,
            ok: answer.is_some(),
        });
        // Counts and labels only: the text is kept in the record above, never in the audit log.
        host.audit(AuditKind::ChannelMessage, "Answered a group question", json!({ "scope": "group", "ok": answer.is_some() }));
        host.changed();
        let text = answer.unwrap_or(FAILED);
        let out: Outgoing = render::plain(text);
        vec![Effect::Reply { chat_id: job.chat_id, reply_to: job.reply_to, out }]
    }

    /// A question that was handed out but will not be run (the poller was replaced): frees its place.
    pub fn abandon_job(&self) {
        drop(Inflight(self));
    }
}

#[cfg(test)]
#[path = "group_tests.rs"]
mod tests;
