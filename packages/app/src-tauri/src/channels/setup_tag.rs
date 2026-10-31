//! Tag Your Agent: the person's actions on the PC (Settings > Phone > Telegram > Tag Your Agent in groups).
//!
//! Only the app's own window calls these (`commands/channels.rs`); the bridge has no route for them and a
//! chat cannot reach them. A group becomes approved only here, by the person. Every action reads the saved
//! settings fresh, changes one thing and saves; the poller keeps running as it is (the engine reads the
//! settings on every batch).

use super::config::TelegramConfig;
use super::host::AgentOption;
use super::runtime::Runtime;
use super::tag_config::{folder_is_allowed, GroupWho, PendingGroup, TagGroup, MAX_GROUPS};
use super::tag_log::GroupAnswerRecord;
use crate::audit::Kind as AuditKind;
use serde::{Deserialize, Serialize};
use serde_json::json;

const NEED_PAIRED: &str = "Connect your chat first.";
const NOT_WAITING: &str = "That group is not waiting for approval.";
const NOT_APPROVED: &str = "That group is not approved.";
const TOO_MANY: &str = "You can approve 20 groups. Remove one first.";
const BAD_AGENT: &str = "That agent can't answer questions on this computer.";
const BAD_FOLDER: &str = "Pick a project folder that exists. Your home folder and drive roots are not allowed.";

/// What the Settings page shows for Tag Your Agent. No bot token, no user ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagBotView {
    /// A chat is paired (the group bot needs it: "only me" means that person).
    pub paired: bool,
    pub bot_username: Option<String>,
    pub enabled: bool,
    pub daily_cap: u32,
    pub per_person_cap: u32,
    pub groups: Vec<TagGroup>,
    pub pending: Vec<PendingGroup>,
    /// The agents a group can choose from (installed ones that can answer a read-only question).
    pub agents: Vec<AgentOption>,
    /// The last answers, newest first.
    pub recent: Vec<GroupAnswerRecord>,
}

/// The fields of an approved group the person can change.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupUpdate {
    pub who: GroupWho,
    pub agent: Option<String>,
    pub folder: Option<String>,
    pub style: String,
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Runtime {
    /// What the page shows.
    pub fn tag_view(&self) -> TagBotView {
        let cfg = self.deps.host.config();
        let mut recent = self.deps.host.recent_group_answers();
        recent.reverse();
        TagBotView {
            paired: cfg.paired.is_some(),
            bot_username: cfg.bot_username.clone(),
            enabled: cfg.tag_bot.enabled,
            daily_cap: cfg.tag_bot.daily_cap,
            per_person_cap: cfg.tag_bot.per_person_cap,
            groups: cfg.tag_bot.groups,
            pending: cfg.tag_bot.pending,
            agents: self.deps.host.tag_agents(),
            recent,
        }
    }

    /// Loads the settings, lets `f` change them, saves, tells the page and returns the new view.
    fn tag_change(&self, f: impl FnOnce(&mut TelegramConfig) -> Result<(), String>) -> Result<TagBotView, String> {
        let mut cfg = self.deps.host.config();
        f(&mut cfg)?;
        // Every limit is enforced here too, so the page shows what the file will hold.
        let cfg = super::config::sanitize(cfg);
        self.deps.host.save_config(&cfg)?;
        self.deps.host.changed();
        Ok(self.tag_view())
    }

    /// The feature switch. On needs a paired chat (the group bot is the same bot, and "only me" is that person).
    pub fn tag_set_enabled(&self, on: bool) -> Result<TagBotView, String> {
        let view = self.tag_change(|cfg| {
            if on && cfg.paired.is_none() {
                return Err(NEED_PAIRED.into());
            }
            cfg.tag_bot.enabled = on;
            Ok(())
        })?;
        (self.deps.audit)(
            AuditKind::Setting,
            crate::audit::Who::Desktop,
            if on { "Tag Your Agent turned on" } else { "Tag Your Agent turned off" },
            json!({ "setting": "tag_bot", "enabled": on }),
        );
        Ok(view)
    }

    /// Answers a day for the whole bot, and an hour for one person. Out-of-range numbers are brought in range
    /// when the file is saved.
    pub fn tag_set_caps(&self, daily_cap: u32, per_person_cap: u32) -> Result<TagBotView, String> {
        self.tag_change(|cfg| {
            cfg.tag_bot.daily_cap = daily_cap;
            cfg.tag_bot.per_person_cap = per_person_cap;
            Ok(())
        })
    }

    /// "Approve": the group moves from pending to approved, with the safe defaults (only me, any agent,
    /// no folder, no style note).
    pub fn tag_approve(&self, chat_id: i64) -> Result<TagBotView, String> {
        let view = self.tag_change(|cfg| {
            if cfg.tag_bot.group(chat_id).is_some() {
                return Ok(());
            }
            let at = cfg.tag_bot.pending.iter().position(|p| p.chat_id == chat_id).ok_or(NOT_WAITING)?;
            if cfg.tag_bot.groups.len() >= MAX_GROUPS {
                return Err(TOO_MANY.into());
            }
            let pending = cfg.tag_bot.pending.remove(at);
            cfg.tag_bot.groups.push(TagGroup { chat_id, title: pending.title, ..TagGroup::default() });
            Ok(())
        })?;
        (self.deps.audit)(
            AuditKind::Setting,
            crate::audit::Who::Desktop,
            "A group was approved for Tag Your Agent",
            json!({ "setting": "tag_bot", "event": "approved" }),
        );
        Ok(view)
    }

    /// "Ignore": the group is forgotten. If it tags the bot again it comes back as pending.
    pub fn tag_ignore(&self, chat_id: i64) -> Result<TagBotView, String> {
        self.tag_change(|cfg| {
            cfg.tag_bot.pending.retain(|p| p.chat_id != chat_id);
            Ok(())
        })
    }

    /// Who may tag, which agent answers, which folder it looks at, and the style note.
    pub fn tag_update_group(&self, chat_id: i64, update: GroupUpdate) -> Result<TagBotView, String> {
        let agents: Vec<String> = self.deps.host.tag_agents().into_iter().map(|a| a.id).collect();
        let agent = blank_to_none(update.agent);
        let folder = blank_to_none(update.folder);
        let home = crate::project_path::home_dir().map(|h| h.to_string_lossy().into_owned());
        self.tag_change(|cfg| {
            let group = cfg.tag_bot.groups.iter_mut().find(|g| g.chat_id == chat_id).ok_or(NOT_APPROVED)?;
            // An agent the group already uses stays valid even if it is not installed right now.
            if let Some(id) = &agent {
                if !agents.iter().any(|a| a.eq_ignore_ascii_case(id)) && group.agent.as_ref() != Some(id) {
                    return Err(BAD_AGENT.into());
                }
            }
            if let Some(path) = &folder {
                if !folder_is_allowed(path, home.as_deref()) || !std::path::Path::new(path).is_dir() {
                    return Err(BAD_FOLDER.into());
                }
            }
            group.who = update.who;
            group.agent = agent;
            group.folder = folder;
            group.style = update.style;
            Ok(())
        })
    }

    /// "Remove": the group is no longer approved. The bot goes quiet there at once.
    pub fn tag_remove(&self, chat_id: i64) -> Result<TagBotView, String> {
        self.tag_change(|cfg| {
            cfg.tag_bot.groups.retain(|g| g.chat_id != chat_id);
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "setup_tag_tests.rs"]
mod tests;
