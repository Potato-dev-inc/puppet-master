//! The commands of the paired person: what each one does once the allow-list, the stale rule, the
//! rate limit and the capability check have passed (see `engine.rs`).

use super::{
    companion_named, plural, prepare_text, say, say_lines, waiting, Effect, Engine, Out, MAX_PANES_LISTED, STATUS_LINES, STOPPED, TOO_LONG,
    UNKNOWN_COMMAND,
};
use crate::audit::Kind as AuditKind;
use crate::channels::commands::Command;
use crate::channels::config::TelegramConfig;
use crate::channels::host::{CompanionLine, Host};
use crate::channels::refs::Target;
use crate::channels::render;
use crate::channels::telegram::api::InMessage;
use crate::mobile_pairing::Cap;
use serde_json::json;

const PANE_GONE: &str = "That agent is gone. Send /panes to see the list.";
const NO_LIST: &str = "Send /panes first, then /to 1 your text.";
const NO_COMPANION: &str = "You have no companion. Use /panes and /to.";
const WHICH_REPLY: &str = "I can't tell what that message was about. Use /panes and /to.";
const HELP: [&str; 9] = [
    "What I can do:",
    "/status - what needs you",
    "/panes - list your agents",
    "/to 2 text - send text to agent 2",
    "/c Name text - talk to a companion",
    "Plain text goes to your companion. Reply to one of my messages to answer that agent.",
    "/stop - turn me off",
    "Buttons on a question or a low-risk approval answer it. Anything riskier waits for you on your PC.",
    "When your PC sleeps or Puppet Master is closed, I can't answer.",
];

impl Engine {
    pub(super) fn run_command(&self, host: &dyn Host, cfg: &TelegramConfig, now: u64, m: &InMessage, cmd: Command, out: &mut Out) {
        match cmd {
            Command::Start(_) => out.effects.push(say("You are already connected. Send /help.")),
            Command::Help => out.effects.push(say_lines(&HELP.map(String::from))),
            Command::Status => out.effects.push(self.status(host, cfg)),
            Command::Panes => out.effects.push(self.panes(host, now)),
            Command::To { n, text } => {
                let pane = self.refs.lock().pane_at(n, now);
                match pane {
                    Some(id) => self.to_pane(host, now, &id, &text, out),
                    None => out.effects.push(say(NO_LIST)),
                }
            }
            Command::Companion { name, text } => {
                let list = host.companions();
                match companion_named(&list, &name) {
                    Some(c) => self.to_companion(host, now, c, &text, out),
                    None => out.effects.push(say("No companion has that name. Send a plain message to talk to your companion.")),
                }
            }
            Command::Text(text) => self.plain_text(host, now, m.reply_to, &text, out),
            Command::TooLong => out.effects.push(say(TOO_LONG)),
            Command::Stop => self.stop(host, cfg, out),
            Command::Unknown => out.effects.push(say(UNKNOWN_COMMAND)),
        }
    }

    /// `/stop`: off, saved, audited, and the poller ends. The person turns it on again on the PC.
    fn stop(&self, host: &dyn Host, cfg: &TelegramConfig, out: &mut Out) {
        let mut next = cfg.clone();
        next.enabled = false;
        if let Err(err) = host.save_config(&next) {
            tracing::warn!(error = %err, "could not save the Telegram off switch");
            host.audit(AuditKind::ChannelError, "Could not save the off switch", json!({ "step": "stop_save" }));
        }
        host.audit(AuditKind::ChannelOff, "Telegram turned off from the chat", json!({ "by": "telegram" }));
        host.changed();
        out.effects.push(say(STOPPED));
        out.effects.push(Effect::Stop);
        out.stop = true;
    }

    /// "2 need you · 1 ready · 4 agents running", then the titles of what waits (only when the chat
    /// may watch and short mode is off).
    fn status(&self, host: &dyn Host, cfg: &TelegramConfig) -> Effect {
        let snapshot = host.snapshot();
        let running = host.panes().iter().filter(|p| p.status == "working").count();
        let blocking = snapshot.counts.blocking as usize;
        let need = if blocking == 1 { "1 needs you".to_string() } else { format!("{blocking} need you") };
        let mut lines = vec![format!("{need} · {} ready · {} running", snapshot.counts.ready, plural(running, "agent", "agents"))];
        if cfg.caps.has(Cap::Watch) && !cfg.short_mode {
            for item in waiting(&snapshot).take(STATUS_LINES) {
                lines.push(match &item.pane {
                    Some(pane) => format!("- {}: {}", pane.heading, item.title),
                    None => format!("- {}", item.title),
                });
            }
        }
        say_lines(&lines)
    }

    /// The numbered list. The numbering is remembered for 30 minutes, for `/to N`.
    fn panes(&self, host: &dyn Host, now: u64) -> Effect {
        let panes: Vec<_> = host.panes().into_iter().take(MAX_PANES_LISTED).collect();
        self.refs.lock().set_list(panes.iter().map(|p| p.id.clone()).collect(), now);
        if panes.is_empty() {
            return say("No agents are open.");
        }
        let lines: Vec<String> = panes
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{} {} · {} · {}", i + 1, p.heading, p.agent_label, p.status))
            .collect();
        say_lines(&lines)
    }

    /// Plain text: to whatever the message replies to, else to the one companion there is.
    fn plain_text(&self, host: &dyn Host, now: u64, reply_to: Option<i64>, text: &str, out: &mut Out) {
        if let Some(message_id) = reply_to {
            let target = self.refs.lock().target_of(message_id, now);
            match target {
                Some(Target::Pane(id)) => self.to_pane(host, now, &id, text, out),
                Some(Target::Companion(id)) => {
                    let list = host.companions();
                    match list.iter().find(|c| c.id == id) {
                        Some(c) => self.to_companion(host, now, c, text, out),
                        None => out.effects.push(say("That companion is gone. Send /panes to see your agents.")),
                    }
                }
                None => out.effects.push(say(WHICH_REPLY)),
            }
            return;
        }
        let list = host.companions();
        match list.as_slice() {
            [] => out.effects.push(say(NO_COMPANION)),
            [one] => self.to_companion(host, now, one, text, out),
            [first, ..] => out.effects.push(say(&format!("Which one? /c {} your message", first.name))),
        }
    }

    /// Text into a pane's message box. The pane must still exist; the box is filled only when the
    /// agent is ready (the outbox decides), so the reply says "when the agent is ready".
    fn to_pane(&self, host: &dyn Host, now: u64, pane_id: &str, raw: &str, out: &mut Out) {
        let Some(pane) = host.panes().into_iter().find(|p| p.id == pane_id) else {
            out.effects.push(say(PANE_GONE));
            return;
        };
        let text = match prepare_text(raw) {
            Ok(t) => t,
            Err(sentence) => return out.effects.push(say(sentence)),
        };
        match host.send_to_pane(&pane.id, &text) {
            Ok(()) => {
                let target = Target::Pane(pane.id.clone());
                self.refs.lock().open_window(&target, now);
                host.audit(AuditKind::ChannelMessage, "Telegram message to a pane", json!({ "target": "pane", "chars": text.chars().count() }));
                out.effects.push(sent(&format!("Sent to {}. It goes in when the agent is ready.", pane.heading), target));
            }
            Err(reason) => out.effects.push(say(&format!("Not sent: {reason}"))),
        }
    }

    /// Text to a companion. A sleeping one is not woken from the chat (D-6).
    fn to_companion(&self, host: &dyn Host, now: u64, c: &CompanionLine, raw: &str, out: &mut Out) {
        if !c.awake {
            out.effects.push(say(&format!("{} is asleep. Wake it in Puppet Master.", c.name)));
            return;
        }
        let text = match prepare_text(raw) {
            Ok(t) => t,
            Err(sentence) => return out.effects.push(say(sentence)),
        };
        match host.send_to_pane(&c.pane_id, &text) {
            Ok(()) => {
                let target = Target::Companion(c.id.clone());
                self.refs.lock().open_window(&target, now);
                host.audit(AuditKind::ChannelMessage, "Telegram message to a companion", json!({ "target": "companion", "chars": text.chars().count() }));
                out.effects.push(sent(&format!("Sent to {}.", c.name), target));
            }
            Err(reason) => out.effects.push(say(&format!("Not sent: {reason}"))),
        }
    }
}

/// A reply the later "reply to this message" can find its target from.
fn sent(text: &str, target: Target) -> Effect {
    Effect::Send { out: render::plain(text), reply_to: None, remember: Some(target) }
}
