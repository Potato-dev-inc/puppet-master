//! Text Your Agents: the saved Telegram settings (`telegram.json`), their defaults and the phone view.
//!
//! The file lives in the app data folder and only Puppet Master's own commands write it. It is not in
//! `settings.json`, so the settings mirror and the bridge can never patch it. `Control` is never kept:
//! it is stripped on load and on save, so a chat can never be given the power to start or stop agents.

use super::tag_config::{self, TagBotConfig};
use crate::mobile_pairing::{Cap, Caps};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "telegram.json";

/// The one private chat the bot talks to. Set by pairing, and only by the person on this PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedChat {
    pub chat_id: i64,
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub paired_at_ms: u64,
}

/// Everything the Telegram channel keeps between runs. Missing fields take the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TelegramConfig {
    /// The person's switch. Off by default, and off again after `/stop`.
    pub enabled: bool,
    /// Set each time the channel is turned on. Older messages are never acted on.
    pub enabled_at_ms: Option<u64>,
    /// The bot's `@name` from `getMe`, for the UI.
    pub bot_username: Option<String>,
    /// The bot's id. A token change to another bot clears `paired`.
    pub bot_id: Option<i64>,
    pub paired: Option<PairedChat>,
    /// What the chat may do. Never `Control` (see [`sanitize`]).
    pub caps: Caps,
    /// Also send "run finished" and "run failed" messages.
    pub notify_runs: bool,
    /// Send "X needs you" only, with no agent text.
    pub short_mode: bool,
    /// The last update id Telegram handed us, so a restart does not replay messages.
    pub last_update_id: Option<i64>,
    /// Tag Your Agent: the bot answering in approved groups. Off by default (see `tag_config`).
    pub tag_bot: TagBotConfig,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            enabled_at_ms: None,
            bot_username: None,
            bot_id: None,
            paired: None,
            caps: default_caps(),
            notify_runs: false,
            short_mode: false,
            last_update_id: None,
            tag_bot: TagBotConfig::default(),
        }
    }
}

/// Watch, answer and type. `Control` is never part of the default.
pub fn default_caps() -> Caps {
    Caps::watch_and_answer().with(Cap::Type, true)
}

/// Remove what this channel may never hold. Used on load and on save.
pub fn sanitize(mut config: TelegramConfig) -> TelegramConfig {
    config.caps = config.caps.with(Cap::Control, false);
    config.tag_bot = tag_config::sanitize(config.tag_bot);
    config
}

/// The file's path in this profile's app data folder.
pub fn path() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE)
}

/// Read a saved file. A missing, unreadable or half-written file means the defaults (channel off).
pub fn parse(text: &str) -> TelegramConfig {
    serde_json::from_str::<TelegramConfig>(text).map(sanitize).unwrap_or_default()
}

pub fn load_from(path: &Path) -> TelegramConfig {
    std::fs::read_to_string(path).map(|text| parse(&text)).unwrap_or_default()
}

/// Write the file atomically with private permissions. `Control` is removed before it is written.
pub fn save_to(path: &Path, config: &TelegramConfig) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&sanitize(config.clone())).map_err(|err| err.to_string())?;
    crate::secure_file::write_private(path, &text)
}

pub fn load() -> TelegramConfig {
    load_from(&path())
}

pub fn save(config: &TelegramConfig) -> Result<(), String> {
    save_to(&path(), config)
}

/// The only thing the phone may learn: is the channel on, is it connected, and the bot's `@name`.
/// No chat id, no names, no capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    pub on: bool,
    pub connected: bool,
    pub bot_username: Option<String>,
}

pub fn phone_view(config: &TelegramConfig, connected: bool) -> PhoneView {
    PhoneView {
        on: config.enabled,
        connected: config.enabled && connected,
        bot_username: config.bot_username.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-tg-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn paired() -> PairedChat {
        PairedChat {
            chat_id: 424_242_424,
            user_id: 1001,
            username: Some("anna_k".into()),
            first_name: Some("Anna".into()),
            paired_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn defaults_are_off_with_watch_answer_and_type() {
        let config = TelegramConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.enabled_at_ms, None);
        assert_eq!(config.paired, None);
        assert!(!config.notify_runs);
        assert!(!config.short_mode);
        assert_eq!(config.last_update_id, None);
        assert_eq!(config.caps.list(), vec![Cap::Watch, Cap::Answer, Cap::Type]);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = temp_path("telegram.json");
        assert_eq!(load_from(&path), TelegramConfig::default());
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        assert_eq!(parse("{not json"), TelegramConfig::default());
        assert_eq!(parse(""), TelegramConfig::default());
        assert_eq!(parse("[1, 2]"), TelegramConfig::default());
        let path = temp_path("telegram.json");
        std::fs::write(&path, "{\"enabled\": tru").unwrap();
        assert_eq!(load_from(&path), TelegramConfig::default());
    }

    #[test]
    fn missing_fields_take_the_defaults() {
        let config = parse(r#"{"enabled": true, "shortMode": true}"#);
        assert!(config.enabled);
        assert!(config.short_mode);
        assert_eq!(config.caps, default_caps());
        assert_eq!(config.paired, None);
    }

    #[test]
    fn control_is_stripped_on_load() {
        let config = parse(r#"{"caps": ["watch", "control", "answer"]}"#);
        assert_eq!(config.caps.list(), vec![Cap::Watch, Cap::Answer]);
        assert!(!config.caps.has(Cap::Control));
    }

    #[test]
    fn control_is_stripped_on_save() {
        let path = temp_path("telegram.json");
        let config = TelegramConfig { caps: Caps::all(), ..TelegramConfig::default() };
        save_to(&path, &config).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("control"), "control must never reach the file: {raw}");
        assert_eq!(load_from(&path).caps.list(), vec![Cap::Watch, Cap::Answer, Cap::Type]);
    }

    #[test]
    fn unknown_cap_names_are_ignored() {
        let config = parse(r#"{"caps": ["watch", "fly", "type"]}"#);
        assert_eq!(config.caps.list(), vec![Cap::Watch, Cap::Type]);
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp_path("telegram.json");
        let config = TelegramConfig {
            enabled: true,
            enabled_at_ms: Some(1_700_000_123_000),
            bot_username: Some("anna_pm_bot".into()),
            bot_id: Some(123_456_789),
            paired: Some(paired()),
            caps: Caps::watch_and_answer(),
            notify_runs: true,
            short_mode: true,
            last_update_id: Some(77),
            tag_bot: TagBotConfig::default(),
        };
        save_to(&path, &config).unwrap();
        assert_eq!(load_from(&path), config);
    }

    #[test]
    fn the_group_bot_is_off_in_an_old_file_and_survives_a_round_trip() {
        let old = parse(r#"{"enabled": true, "shortMode": true}"#);
        assert!(!old.tag_bot.enabled);
        assert!(old.tag_bot.groups.is_empty());
        let path = temp_path("telegram.json");
        let mut config = TelegramConfig::default();
        config.tag_bot.enabled = true;
        config.tag_bot.daily_cap = 12;
        config.tag_bot.groups.push(tag_config::TagGroup {
            chat_id: -100_555_000_001,
            title: "Test Crew".into(),
            who: tag_config::GroupWho::Anyone,
            agent: Some("test-agent".into()),
            folder: None,
            style: "brief".into(),
        });
        save_to(&path, &config).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"tagBot\"") && raw.contains("\"anyone\""), "{raw}");
        assert_eq!(load_from(&path), config);
    }

    #[test]
    fn a_hand_edited_file_cannot_exceed_the_group_limits() {
        let config = parse(r#"{"tagBot": {"enabled": true, "dailyCap": 0, "perPersonCap": 100000,
            "groups": [{"chatId": -5, "title": "A", "style": "STYLE_PLACEHOLDER"}]}}"#.replace("STYLE_PLACEHOLDER", &"z".repeat(900)).as_str());
        assert_eq!(config.tag_bot.daily_cap, 1);
        assert!(config.tag_bot.per_person_cap <= 100);
        assert_eq!(config.tag_bot.groups[0].style.chars().count(), 200);
    }

    #[test]
    fn phone_view_has_no_chat_id_or_names() {
        let config = TelegramConfig {
            enabled: true,
            bot_username: Some("anna_pm_bot".into()),
            paired: Some(paired()),
            ..TelegramConfig::default()
        };
        let json = serde_json::to_value(phone_view(&config, true)).unwrap();
        assert_eq!(json, serde_json::json!({"on": true, "connected": true, "botUsername": "anna_pm_bot"}));
        let text = json.to_string();
        for secret in ["424242424", "1001", "Anna", "anna_k"] {
            assert!(!text.contains(secret), "phone view leaked {secret}: {text}");
        }
    }

    #[test]
    fn phone_view_is_not_connected_when_off() {
        let config = TelegramConfig::default();
        let view = phone_view(&config, true);
        assert!(!view.on);
        assert!(!view.connected);
        assert_eq!(view.bot_username, None);
    }
}
