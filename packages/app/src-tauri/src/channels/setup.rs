//! Text Your Agents: the actions of the person on the PC (Settings > Phone > Telegram) and the one
//! action of a paired phone (switch it off). Only these, `/stop` in the chat and the config file
//! write the settings; the bridge has no route that turns Telegram on, pairs, or changes caps.
//!
//! Nothing here returns or logs the bot token. Errors are plain sentences from this file or from
//! `token`, never the text of a failed web call.

use super::config::{self, TelegramConfig};
use super::engine::{to_wire, Effect, COMMAND_MENU};
use super::runtime::{Runtime, ShownCode};
use super::status::{cap_names, link_state, paired_view, PairingView, TelegramStatus, WaitingView};
use super::telegram::api::ApiError;
use super::token;
use crate::audit::{Kind as AuditKind, Who};
use crate::mobile_pairing::{Cap, Caps};
use crate::redact::known;
use serde_json::json;

/// The error a request for `control` gets (spec 3.5).
pub const CONTROL_NOT_ALLOWED: &str = "TELEGRAM_CONTROL_NOT_ALLOWED";
const NOT_ACCEPTED: &str = "Telegram did not accept this token.";
const UNREACHABLE: &str = "Could not reach Telegram. Check your internet and try again.";
const HAS_WEBHOOK: &str = "This bot already sends its messages to a website. Make a new bot for Puppet Master.";
const NEED_TOKEN: &str = "Check your bot token first.";
const ALREADY_PAIRED: &str = "A chat is already connected. Disconnect first to connect another.";
const NEED_PAIRED: &str = "Connect your chat first.";

impl Runtime {
    /// What the Settings page shows.
    pub fn status(&self) -> TelegramStatus {
        let cfg = self.deps.host.config();
        let now = self.deps.host.now_ms();
        let link = *self.link.lock().unwrap_or_else(|e| e.into_inner());
        let (state, error) = link_state(&cfg, self.alive(), &link);
        let waiting = self.engine.pairing_waiting();
        let pairing = match (&cfg.paired, self.code.lock().as_ref()) {
            (None, Some(shown)) if now < shown.expires_ms || waiting.is_some() => Some(PairingView {
                code: shown.code.clone(),
                expires_at_ms: shown.expires_ms,
                waiting_for: waiting.map(|w| WaitingView { username: w.username, first_name: w.first_name }),
            }),
            _ => None,
        };
        TelegramStatus {
            store_available: self.deps.secrets.available(),
            store_label: self.deps.secrets.label().to_string(),
            token_set: self.read_token().is_some(),
            bot_username: cfg.bot_username.clone(),
            enabled: cfg.enabled,
            state,
            error,
            paired: paired_view(&cfg),
            pairing,
            caps: cap_names(&cfg),
            notify_runs: cfg.notify_runs,
            short_mode: cfg.short_mode,
            last_poll_at_ms: link.last_poll_ms,
            strangers_last_hour: self.engine.strangers_last_hour(now),
        }
    }

    /// "Check": looks at the pasted token with Telegram, and keeps it in the credential store only if
    /// it is a bot that sends no messages to a website. A different bot than before clears the pairing.
    pub fn set_token(&self, raw: &str) -> Result<TelegramStatus, String> {
        if !self.deps.secrets.available() {
            return Err(token::NO_STORE.into());
        }
        let trimmed = raw.trim();
        if !token::looks_like_token(trimmed) {
            return Err(token::BAD_FORMAT.into());
        }
        // Masked before the first web call, so nothing after this line can show it.
        known::register(trimmed);
        let api = (self.deps.api_for)(trimmed);
        let me = api.get_me().map_err(explain)?;
        // Never `deleteWebhook` on someone's bot (D-2).
        if api.webhook_url().map_err(explain)?.is_some() {
            return Err(HAS_WEBHOOK.into());
        }
        token::save_with(&*self.deps.secrets, self.deps.profile.as_deref(), trimmed)?;
        let mut cfg = self.deps.host.config();
        let other_bot = cfg.bot_id.is_some_and(|id| id != me.id);
        if other_bot {
            let was_paired = cfg.paired.is_some();
            self.halt();
            self.discard_waiting_chat();
            cfg = TelegramConfig::default();
            if was_paired {
                self.audit_desktop(AuditKind::ChannelUnpaired, "Bot changed, chat forgotten", json!({ "reason": "new_bot" }));
            }
        }
        cfg.bot_id = Some(me.id);
        cfg.bot_username = Some(me.username);
        self.deps.host.save_config(&cfg)?;
        self.finish()
    }

    /// "Make a code": a one-time code for `/start`, and the poller in pairing mode.
    pub fn pair_start(&self) -> Result<TelegramStatus, String> {
        let cfg = self.deps.host.config();
        if cfg.paired.is_some() {
            return Err(ALREADY_PAIRED.into());
        }
        if cfg.bot_username.is_none() || self.read_token().is_none() {
            return Err(NEED_TOKEN.into());
        }
        let (code, expires_ms) = self.engine.new_pairing_code(self.deps.host.now_ms());
        *self.code.lock() = Some(ShownCode { code, expires_ms });
        self.restart();
        Ok(self.status())
    }

    /// "Yes, that's me" or "Not me". Yes saves the chat as paired (the channel stays off until it is
    /// turned on), tells the chat it is connected and sets the bot's command menu. The pairing poller
    /// ends; turning the channel on starts a fresh one.
    pub fn pair_decide(&self, accept: bool) -> Result<TelegramStatus, String> {
        let effects = self.engine.decide_pairing(&*self.deps.host, accept);
        *self.code.lock() = None;
        self.say_now(effects, accept);
        self.finish()
    }

    /// The channel on or off. On needs a token and a paired chat; the time it is turned on is saved,
    /// and nothing sent before that is ever acted on.
    pub fn set_enabled(&self, on: bool) -> Result<TelegramStatus, String> {
        let mut cfg = self.deps.host.config();
        if on {
            if cfg.paired.is_none() {
                return Err(NEED_PAIRED.into());
            }
            if self.read_token().is_none() {
                return Err(NEED_TOKEN.into());
            }
            cfg.enabled = true;
            cfg.enabled_at_ms = Some(self.deps.host.now_ms());
            self.deps.host.save_config(&cfg)?;
            self.audit_desktop(AuditKind::ChannelOn, "Telegram turned on", json!({ "by": "desktop" }));
            self.restart();
        } else {
            cfg.enabled = false;
            self.deps.host.save_config(&cfg)?;
            self.audit_desktop(AuditKind::ChannelOff, "Telegram turned off", json!({ "by": "desktop" }));
            self.stop();
        }
        self.finish()
    }

    /// What the chat may do and how it reports. `control` is refused, and unknown names with it.
    pub fn set_options(&self, caps: &[String], notify_runs: bool, short_mode: bool) -> Result<TelegramStatus, String> {
        if caps.iter().any(|c| c.trim() == Cap::Control.as_str()) {
            return Err(CONTROL_NOT_ALLOWED.into());
        }
        let caps = Caps::parse_names(caps)?;
        let mut cfg = self.deps.host.config();
        cfg.caps = caps;
        cfg.notify_runs = notify_runs;
        cfg.short_mode = short_mode;
        self.deps.host.save_config(&cfg)?;
        self.finish()
    }

    /// "Disconnect": forgets the token and the chat, and turns everything off.
    pub fn forget(&self) -> Result<TelegramStatus, String> {
        let was_paired = self.deps.host.config().paired.is_some();
        self.stop();
        self.discard_waiting_chat();
        *self.code.lock() = None;
        token::forget_with(&*self.deps.secrets, self.deps.profile.as_deref())?;
        self.deps.host.save_config(&TelegramConfig::default())?;
        if was_paired {
            self.audit_desktop(AuditKind::ChannelUnpaired, "Telegram disconnected", json!({ "by": "desktop" }));
        }
        self.finish()
    }

    /// The paired phone's "Turn off Telegram". It can only switch the channel off; there is no way
    /// to turn it on, pair, or change what the chat may do from here. Returns whether it was on.
    pub fn turn_off_from_phone(&self, device_id: &str) -> Result<bool, String> {
        let mut cfg = self.deps.host.config();
        let was_on = cfg.enabled;
        if was_on {
            cfg.enabled = false;
            self.deps.host.save_config(&cfg)?;
            (self.deps.audit)(
                AuditKind::ChannelOff,
                Who::Device(Some(device_id.to_string())),
                "Telegram turned off from a phone",
                json!({ "by": "phone" }),
            );
        }
        self.stop();
        self.deps.host.changed();
        Ok(was_on)
    }

    /// `{on, connected, botUsername}`: all the phone may learn.
    pub fn phone_view(&self) -> config::PhoneView {
        let cfg = self.deps.host.config();
        let link = *self.link.lock().unwrap_or_else(|e| e.into_inner());
        let connected = self.alive() && link.state == super::telegram::poller::LinkState::On;
        config::phone_view(&cfg, connected)
    }

    /// After an action: the poller follows the settings, the page is told, and it gets the new status.
    fn finish(&self) -> Result<TelegramStatus, String> {
        self.apply();
        self.deps.host.changed();
        Ok(self.status())
    }

    /// A chat that sent the right code and waits for the PC's click is let go when the bot or the
    /// pairing is reset, so an old waiting chat can never be paired by a later click.
    fn discard_waiting_chat(&self) {
        if self.engine.pairing_waiting().is_some() {
            let _ = self.engine.decide_pairing(&*self.deps.host, false);
        }
    }

    fn audit_desktop(&self, kind: AuditKind, summary: &str, detail: serde_json::Value) {
        (self.deps.audit)(kind, Who::Desktop, summary, detail);
    }

    /// Sends the pairing replies right away (the poller is about to end, so its queue would not be
    /// read), and sets the command menu after a yes. A failure here only means the message is lost.
    fn say_now(&self, effects: Vec<Effect>, accepted: bool) {
        let Some(token) = self.read_token() else { return };
        let api = (self.deps.api_for)(&token);
        let chat_id = self.deps.host.config().paired.map(|p| p.chat_id);
        for effect in effects {
            if let (Effect::Send { out, .. }, Some(chat)) = (&effect, chat_id) {
                let _ = api.send(chat, &to_wire(out), None);
            }
        }
        if accepted && chat_id.is_some() {
            let _ = api.set_commands(&COMMAND_MENU);
        }
    }
}

/// A plain sentence for a refused call. `ApiError` carries no URL or token.
fn explain(err: ApiError) -> String {
    match err {
        ApiError::Unauthorized => NOT_ACCEPTED.into(),
        ApiError::Network => UNREACHABLE.into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
