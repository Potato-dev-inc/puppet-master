//! Text Your Agents: the engine. It reads one batch of updates from the bot and decides what is
//! acted on and what goes back. This is the security core of the feature, so the order is fixed:
//!
//! 1. Not a private chat, from a bot, or no sender: dropped.
//! 2. Nobody paired yet: only `/start CODE` is read (`engine/pair.rs`), everything else is dropped
//!    without a reply. Paired but switched off: dropped.
//! 3. A chat id or user id other than the paired ones is a stranger: dropped without a reply,
//!    counted, and audited at most once an hour.
//! 4. A message sent before the channel was turned on, or more than 10 minutes ago, is never acted
//!    on. After the batch, ONE note says how many were skipped.
//! 5. Inbound bucket (20 a minute), then the command is parsed and its capability checked.
//! 6. Button taps (`engine/buttons.rs`) are looked up in `Grants` (single use, short lived); the
//!    button data itself means nothing.
//!
//! Everything sent out goes through `render::clean`, `render::clean_multiline` or `render::plain`. The engine never answers
//! a trust gate, a consent card, a budget or limit, or an approval above low risk: `policy` says no
//! here, and `attention::answer` says no again for `Via::Telegram`.

use super::commands::{self, Command, MAX_IN_CHARS};
use super::config::TelegramConfig;
use super::grants::Grants;
use super::host::{CompanionLine, Host};
use super::pairing::Pairing;
use super::policy;
use super::rate::{Bucket, Limits};
use super::refs::{Refs, Target};
use super::render::{self, Outgoing, MAX_OUT_CHARS};
use super::telegram::api::{InMessage, Update, UpdateKind, WireButton, WireMessage};
use crate::attention::model::{Lane, State};
use crate::audit::Kind as AuditKind;
use crate::push::triggers::{sanitize, truncate_chars};
use crate::redact::{self, Redacted};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::VecDeque;

mod buttons;
mod pair;
mod run;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod engine_tests;

/// A message older than this is never acted on (10 minutes).
pub const STALE_MS: u64 = 600_000;
/// The channel shows a warning when this many strangers wrote to the bot in an hour.
pub const STRANGER_WARN: u32 = 50;
/// `/status` lists at most this many waiting cards.
pub(super) const STATUS_LINES: usize = 8;
/// `/panes` lists at most this many panes.
pub const MAX_PANES_LISTED: usize = 20;
/// One line of a multi-line reply is cut to this many characters (after the whole reply is cleaned).
const LINE_CHARS: usize = 220;
/// The most characters of a reply that are cleaned at once, before the lines are cut (more than any reply holds).
const WHOLE_CHARS: usize = 20_000;

pub const NOT_ALLOWED: &str = "Telegram is not allowed to do that. Change it in Puppet Master on your PC.";
const TOO_MANY: &str = "Too many messages. Wait a minute.";
const UNKNOWN_COMMAND: &str = "I don't know that. Send /help.";
const TOO_LONG: &str = "Too long (1000 characters at most).";
const STOPPED: &str = "Telegram is off. Turn it on again in Puppet Master on your PC.";

/// The bot's command menu, for `setMyCommands` right after pairing.
pub const COMMAND_MENU: [(&str, &str); 6] = [
    ("status", "What needs you"),
    ("panes", "List your agents"),
    ("to", "Send text to an agent: /to 2 text"),
    ("c", "Talk to a companion: /c Name text"),
    ("help", "What I can do"),
    ("stop", "Turn Telegram off"),
];

/// What the poller must do after a batch. Nothing here is sent by the engine itself.
#[derive(Debug, Clone)]
pub enum Effect {
    /// A message to the paired chat. `remember` links the sent message to a pane or companion, so a
    /// reply to it goes to the same place.
    Send { out: Outgoing, reply_to: Option<i64>, remember: Option<Target> },
    /// A message to a chat that is not paired (yet): only the pairing replies use it.
    SendTo { chat_id: i64, out: Outgoing },
    Edit { message_id: i64, out: Outgoing },
    AnswerCallback { id: String, text: Option<Redacted<String>> },
    /// The poller must exit: `/stop`.
    Stop,
}

/// Turns a message into the shape the Telegram client sends.
pub fn to_wire(out: &Outgoing) -> WireMessage {
    WireMessage {
        text: out.text.clone(),
        keyboard: out
            .keyboard
            .iter()
            .map(|row| row.iter().map(|b| WireButton { label: b.label.clone(), data: b.data.clone() }).collect())
            .collect(),
    }
}

/// Small state that has no home in `Limits` or `Refs`.
struct Misc {
    /// "Too many messages" is said at most once a minute.
    slow_note: Bucket,
    /// The "make a new code" reply is sent once per code.
    closed_note_sent: bool,
    /// When strangers wrote, newest last, for the last hour.
    strangers: VecDeque<u64>,
    /// Tokens of the "Yes, allow once" buttons this engine made. A direct allow-once grant (which
    /// skips the confirm step) is refused even if some other code minted it.
    confirm_tokens: VecDeque<String>,
}

pub struct Engine {
    pairing: Mutex<Pairing>,
    grants: Grants,
    limits: Mutex<Limits>,
    refs: Mutex<Refs>,
    misc: Mutex<Misc>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// What one batch produced so far.
pub(super) struct Out {
    pub effects: Vec<Effect>,
    pub stale: u32,
    pub stop: bool,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            pairing: Mutex::new(Pairing::default()),
            grants: Grants::default(),
            limits: Mutex::new(Limits::new()),
            refs: Mutex::new(Refs::default()),
            misc: Mutex::new(Misc {
                slow_note: Bucket::new(1, 60_000),
                closed_note_sent: false,
                strangers: VecDeque::new(),
                confirm_tokens: VecDeque::new(),
            }),
        }
    }

    /// The button tokens. The notifier mints the buttons of its cards here.
    pub fn grants(&self) -> &Grants {
        &self.grants
    }

    /// The pane list, message links and conversation windows.
    pub fn refs(&self) -> &Mutex<Refs> {
        &self.refs
    }

    /// The buckets. The notifier takes the outbound one.
    pub fn limits(&self) -> &Mutex<Limits> {
        &self.limits
    }

    /// How many strangers wrote to the bot in the last hour.
    pub fn strangers_last_hour(&self, now: u64) -> u32 {
        let mut misc = self.misc.lock();
        prune(&mut misc.strangers, now);
        misc.strangers.len() as u32
    }

    /// Handle everything Telegram handed us in one poll. A batch, not one by one, so the note about
    /// skipped messages is a single message. After `/stop` the rest of the batch is not read.
    pub fn on_batch(&self, host: &dyn Host, updates: &[Update]) -> Vec<Effect> {
        let cfg = host.config();
        let now = host.now_ms();
        let mut out = Out { effects: Vec::new(), stale: 0, stop: false };
        for update in updates {
            match &update.kind {
                UpdateKind::Message(m) => self.on_message(host, &cfg, now, m, &mut out),
                UpdateKind::Callback(c) => self.on_callback(host, &cfg, now, c, &mut out),
                UpdateKind::Other => {}
            }
            if out.stop {
                break;
            }
        }
        if out.stale > 0 && !out.stop {
            out.effects.push(say(&stale_note(out.stale)));
        }
        out.effects
    }

    fn on_message(&self, host: &dyn Host, cfg: &TelegramConfig, now: u64, m: &InMessage, out: &mut Out) {
        // Private chats only, and only a person: groups, channels and bots never reach the rules below.
        let Some(from) = m.from_id else { return };
        if !m.chat_private || m.from_is_bot {
            return;
        }
        let Some(paired) = &cfg.paired else {
            return self.pairing_message(host, cfg, now, m, from, out);
        };
        // Second lock: the poller should not even run when the channel is off.
        if !cfg.enabled {
            return;
        }
        if m.chat_id != paired.chat_id || from != paired.user_id {
            return self.stranger(host, now);
        }
        let Some(text) = m.text.as_deref().filter(|t| !t.trim().is_empty()) else { return };
        if is_stale(cfg, m.date_s, now) {
            out.stale += 1;
            return;
        }
        let cmd = commands::parse(text, cfg.bot_username.as_deref());
        // `/stop` is never held back by the rate limit: it is the way out.
        if cmd != Command::Stop && !self.limits.lock().inbound.take(now) {
            if self.misc.lock().slow_note.take(now) {
                out.effects.push(say(TOO_MANY));
            }
            return;
        }
        if let Some(need) = policy::command_need(&cmd) {
            if !cfg.caps.satisfies(need) {
                out.effects.push(say(NOT_ALLOWED));
                return;
            }
        }
        self.run_command(host, cfg, now, m, cmd, out);
    }

    /// A chat or user that is not the paired one. No reply, a count, and one audit line an hour.
    pub(super) fn stranger(&self, host: &dyn Host, now: u64) {
        let count = {
            let mut misc = self.misc.lock();
            prune(&mut misc.strangers, now);
            misc.strangers.push_back(now);
            misc.strangers.len() as u32
        };
        let audit_now = {
            let mut limits = self.limits.lock();
            limits.strangers_last_hour = count;
            limits.stranger_audit.take(now)
        };
        if audit_now {
            host.audit(AuditKind::ChannelStranger, "Someone else wrote to the bot", json!({ "count": count }));
            host.changed();
        } else if count == STRANGER_WARN {
            host.changed();
        }
    }
}

fn prune(times: &mut VecDeque<u64>, now: u64) {
    while times.front().is_some_and(|t| now.saturating_sub(*t) >= 3_600_000) {
        times.pop_front();
    }
}

/// Before the channel was turned on, more than 10 minutes old, or with a date we cannot trust.
/// With no `enabled_at_ms` nothing counts as fresh (fail closed).
fn is_stale(cfg: &TelegramConfig, date_s: i64, now: u64) -> bool {
    let Some(at_ms) = u64::try_from(date_s).ok().map(|s| s.saturating_mul(1000)).filter(|ms| *ms > 0) else {
        return true;
    };
    let Some(enabled_at) = cfg.enabled_at_ms else { return true };
    at_ms < enabled_at || now.saturating_sub(at_ms) > STALE_MS
}

fn stale_note(n: u32) -> String {
    let (count, verb) = if n == 1 { ("1 message came".to_string(), "It was") } else { (format!("{n} messages came"), "They were") };
    format!("{count} while your PC was asleep or Puppet Master was off. {verb} not sent. Send them again if you still want them.")
}

// ---------------------------------------------------------------------------------------------
// Building messages

/// A plain reply to the paired chat.
pub(super) fn say(text: &str) -> Effect {
    Effect::Send { out: render::plain(text), reply_to: None, remember: None }
}

/// A reply of several lines. The WHOLE text is cleaned first (`render::clean_multiline`), so a secret that
/// spans lines, such as a PEM private key in a pane name or a card title, is masked as one block. Only then
/// is each line cut to `LINE_CHARS`, and lines are dropped from the end when the whole is over the message cap.
/// Never clean the lines one by one: a piece of a key would not be seen as a key.
pub(super) fn lines_out(lines: &[String]) -> Outgoing {
    let whole = render::clean_multiline(&lines.join("\n"), WHOLE_CHARS);
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    for line in whole.as_str().split('\n') {
        let line = truncate_chars(line, LINE_CHARS);
        let len = line.chars().count() + 1;
        if used + len > MAX_OUT_CHARS {
            break;
        }
        used += len;
        kept.push(line);
    }
    // Already clean; `strict` again changes nothing and gives back the `Redacted` type.
    Outgoing { text: redact::strict(&kept.join("\n")), keyboard: Vec::new() }
}

pub(super) fn say_lines(lines: &[String]) -> Effect {
    Effect::Send { out: lines_out(lines), reply_to: None, remember: None }
}

/// Text for a pane or companion: bidi and control characters out (line breaks and tabs stay),
/// trimmed, 1..=1000 characters. The error is the sentence to show.
pub(super) fn prepare_text(raw: &str) -> Result<String, &'static str> {
    let kept: String = sanitize(raw).chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    let text = kept.trim();
    if text.is_empty() {
        return Err("There is no text to send.");
    }
    if text.chars().count() > MAX_IN_CHARS {
        return Err(TOO_LONG);
    }
    Ok(text.to_string())
}

/// `/status` and `/panes` word for a number of things.
pub(super) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub(super) fn companion_named<'a>(list: &'a [CompanionLine], name: &str) -> Option<&'a CompanionLine> {
    list.iter().find(|c| c.name.trim().eq_ignore_ascii_case(name.trim()))
}

/// The items that block an agent and are still open.
pub(super) fn waiting(snapshot: &crate::attention::model::Snapshot) -> impl Iterator<Item = &crate::attention::model::Item> {
    snapshot.items.iter().filter(|i| i.state == State::Open && i.lane == Lane::Blocking)
}
