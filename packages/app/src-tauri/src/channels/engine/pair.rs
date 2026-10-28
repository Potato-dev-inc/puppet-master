//! The pairing side of the engine: `/start CODE` from a chat nobody has paired yet, and the
//! desktop's yes or no. Pairing needs the right code AND a click on the PC (D-4).

use super::{say, Effect, Engine, Out, STALE_MS};
use crate::audit::Kind as AuditKind;
use crate::channels::commands::{self, Command};
use crate::channels::config::TelegramConfig;
use crate::channels::host::Host;
use crate::channels::pairing::{PairStep, Who};
use crate::channels::render;
use crate::channels::telegram::api::InMessage;
use serde_json::json;

const WRONG_CODE: &str = "That code is not right.";
const CODE_CLOSED: &str = "This code does not work any more. Make a new code in Puppet Master.";
const WAITING: &str = "Waiting for you to confirm on your PC.";
const CONNECTED: &str = "Connected. Telegram can read what this bot sends. Send /help to see what I can do.";

impl Engine {
    /// Makes a fresh pairing code (the desktop's "Make a code"). Returns the code and the time it
    /// stops working.
    pub fn new_pairing_code(&self, now: u64) -> (String, u64) {
        self.misc.lock().closed_note_sent = false;
        self.pairing.lock().new_code(now)
    }

    /// The chat that sent the right code and waits for the PC's click (name only, for the card).
    pub fn pairing_waiting(&self) -> Option<Who> {
        self.pairing.lock().pending().cloned()
    }

    /// The PC's answer to "Is this you?". Yes saves the chat as the paired one (the channel stays
    /// off until the person turns it on) and the bot says it is connected. No burns the code and
    /// says nothing to anyone.
    pub fn decide_pairing(&self, host: &dyn Host, accept: bool) -> Vec<Effect> {
        let now = host.now_ms();
        let paired = self.pairing.lock().decide(accept, now);
        if !accept {
            host.audit(AuditKind::ChannelUnpaired, "Pairing refused on the PC", json!({ "reason": "not_me" }));
            host.changed();
            return Vec::new();
        }
        let Some(paired) = paired else { return Vec::new() };
        let user_id = paired.user_id;
        let mut cfg = host.config();
        cfg.paired = Some(paired);
        if let Err(err) = host.save_config(&cfg) {
            tracing::warn!(error = %err, "could not save the Telegram pairing");
            host.audit(AuditKind::ChannelError, "Could not save the pairing", json!({ "step": "pairing_save" }));
            host.changed();
            return Vec::new();
        }
        host.audit(AuditKind::ChannelPaired, "Telegram chat paired", json!({ "user_id": user_id }));
        host.changed();
        vec![say(CONNECTED)]
    }

    /// A message while nobody is paired: only `/start CODE` in a private chat with the sender
    /// themselves counts. Everyone else gets no reply at all (the bot looks dead).
    pub(super) fn pairing_message(&self, host: &dyn Host, cfg: &TelegramConfig, now: u64, m: &InMessage, from: i64, out: &mut Out) {
        if m.chat_id != from {
            return;
        }
        // An old `/start` (the PC was off) is not read either.
        let fresh = u64::try_from(m.date_s).ok().map(|s| s.saturating_mul(1000)).is_some_and(|at| at > 0 && now.saturating_sub(at) <= STALE_MS);
        if !fresh {
            return;
        }
        let Some(text) = m.text.as_deref() else { return };
        let Command::Start(Some(code)) = commands::parse(text, cfg.bot_username.as_deref()) else { return };
        let who = Who { chat_id: m.chat_id, user_id: from, username: m.username.clone(), first_name: m.first_name.clone() };
        let (step, was_waiting) = {
            let mut pairing = self.pairing.lock();
            // While one chat waits for the PC, nobody else is answered.
            let waiting = pairing.pending().map(|p| (p.chat_id, p.user_id));
            if waiting.is_some_and(|w| w != (who.chat_id, who.user_id)) {
                return;
            }
            let was_waiting = waiting.is_some();
            (pairing.offer(&code, who, now), was_waiting)
        };
        let reply = match step {
            PairStep::NoCode => return,
            PairStep::Wrong { .. } => WRONG_CODE,
            PairStep::Burned | PairStep::Expired => {
                // Said once per code: after that the bot is quiet again.
                let mut misc = self.misc.lock();
                if std::mem::replace(&mut misc.closed_note_sent, true) {
                    return;
                }
                CODE_CLOSED
            }
            PairStep::AwaitingDesktop => {
                host.changed();
                if was_waiting {
                    return;
                }
                WAITING
            }
        };
        out.effects.push(Effect::SendTo { chat_id: m.chat_id, out: render::plain(reply) });
    }
}
