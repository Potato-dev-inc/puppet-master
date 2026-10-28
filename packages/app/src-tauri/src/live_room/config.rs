//! Live Room settings: the relay address, the default room length and the default viewer limit.
//!
//! The file is `live_room.json` in the app data folder. Only Puppet Master's own commands write it, through
//! `secure_file::write_private`, like the Telegram settings. It is not in `settings.json`, so the settings
//! mirror and the bridge never touch it. The room key and the read token never go in this file.

use super::{MINUTES, VIEWERS};
use crate::team::transport::relay::normalize_relay_url;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "live_room.json";
/// Room length picked when the person has not chosen one.
const DEFAULT_MINUTES: u32 = 60;
/// Viewer limit picked when the person has not chosen one.
const DEFAULT_VIEWERS: u32 = 10;

/// What the person set for Live Rooms. Missing fields take the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveRoomConfig {
    /// The relay address (`https://...`, or `http://` on this computer). Empty means: use the team relay.
    pub relay: String,
    /// Room length in minutes. One of [`MINUTES`].
    pub minutes: u32,
    /// Most viewers at once. One of [`VIEWERS`].
    pub max_viewers: u32,
}

impl Default for LiveRoomConfig {
    fn default() -> Self {
        Self {
            relay: String::new(),
            minutes: DEFAULT_MINUTES,
            max_viewers: DEFAULT_VIEWERS,
        }
    }
}

/// The allowed value nearest to `value`. A tie picks the smaller one.
fn snap(value: u32, allowed: &[u32]) -> u32 {
    allowed
        .iter()
        .copied()
        .min_by_key(|a| (a.abs_diff(value), *a))
        .unwrap_or(value)
}

/// Empty is allowed (no Live Room relay). Anything else must pass the team relay's address rules.
fn checked_relay(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        return Ok(String::new());
    }
    normalize_relay_url(raw)
}

/// The file's path in the app data folder `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Read a saved file. A missing, unreadable or half-written file means the defaults.
/// A relay the rules refuse (for example `http://` to another computer) is dropped, and the numbers snap
/// to the allowed lists.
pub fn parse(text: &str) -> LiveRoomConfig {
    let saved = serde_json::from_str::<LiveRoomConfig>(text).unwrap_or_default();
    LiveRoomConfig {
        relay: checked_relay(&saved.relay).unwrap_or_default(),
        minutes: snap(saved.minutes, &MINUTES),
        max_viewers: snap(saved.max_viewers, &VIEWERS),
    }
}

pub fn load_from(path: &Path) -> LiveRoomConfig {
    std::fs::read_to_string(path).map(|text| parse(&text)).unwrap_or_default()
}

/// Write the file atomically with private permissions. Refuses a relay address the rules do not allow,
/// and writes nothing in that case. The numbers snap to the allowed lists before they are written.
pub fn save_to(path: &Path, config: &LiveRoomConfig) -> Result<(), String> {
    let clean = LiveRoomConfig {
        relay: checked_relay(&config.relay)?,
        minutes: snap(config.minutes, &MINUTES),
        max_viewers: snap(config.max_viewers, &VIEWERS),
    };
    let text = serde_json::to_string_pretty(&clean).map_err(|err| err.to_string())?;
    crate::secure_file::write_private(path, &text)
}

/// Load from the app data folder `dir`.
pub fn load(dir: &Path) -> LiveRoomConfig {
    load_from(&path(dir))
}

/// Save into the app data folder `dir`.
pub fn save(dir: &Path, config: &LiveRoomConfig) -> Result<(), String> {
    save_to(&path(dir), config)
}

/// The relay a room should use: the Live Room relay when it is set and valid, else the team relay when
/// that is set and valid, else `None` (the sheet then links to settings).
pub fn effective_relay(config: &LiveRoomConfig, team_relay: &str) -> Option<String> {
    let own = checked_relay(&config.relay).ok().filter(|url| !url.is_empty());
    own.or_else(|| checked_relay(team_relay).ok().filter(|url| !url.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-lr-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_are_empty_relay_one_hour_ten_viewers() {
        let cfg = LiveRoomConfig::default();
        assert_eq!(cfg.relay, "");
        assert_eq!(cfg.minutes, 60);
        assert_eq!(cfg.max_viewers, 10);
        assert_eq!(parse(""), cfg, "empty text means the defaults");
    }

    #[test]
    fn round_trip_through_the_folder() {
        let dir = temp_dir();
        let cfg = LiveRoomConfig {
            relay: "https://relay.example.com".into(),
            minutes: 240,
            max_viewers: 25,
        };
        save(&dir, &cfg).unwrap();
        assert_eq!(load(&dir), cfg);
        assert!(path(&dir).ends_with("live_room.json"));
    }

    #[test]
    fn saved_file_has_only_the_three_settings() {
        let dir = temp_dir();
        save(&dir, &LiveRoomConfig::default()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path(&dir)).unwrap()).unwrap();
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["maxViewers", "minutes", "relay"]);
    }

    #[test]
    fn bad_file_means_defaults() {
        let dir = temp_dir();
        std::fs::write(path(&dir), "{ not json").unwrap();
        assert_eq!(load(&dir), LiveRoomConfig::default());
        std::fs::write(path(&dir), r#"{"minutes": "sixty"}"#).unwrap();
        assert_eq!(load(&dir), LiveRoomConfig::default(), "a wrong type falls back to all defaults");
        assert_eq!(load(&temp_dir()), LiveRoomConfig::default(), "a missing file is the defaults");
    }

    #[test]
    fn missing_fields_take_the_defaults() {
        let cfg = parse(r#"{"relay": "https://relay.example.com"}"#);
        assert_eq!(cfg.relay, "https://relay.example.com");
        assert_eq!(cfg.minutes, 60);
        assert_eq!(cfg.max_viewers, 10);
    }

    #[test]
    fn unknown_minutes_and_viewers_snap_to_the_nearest_allowed() {
        let cfg = parse(r#"{"minutes": 50, "maxViewers": 7}"#);
        assert_eq!(cfg.minutes, 60, "50 is nearest to 60");
        assert_eq!(cfg.max_viewers, 5, "7 is nearest to 5");
        assert_eq!(parse(r#"{"minutes": 45}"#).minutes, 30, "a tie picks the smaller value");
        assert_eq!(parse(r#"{"minutes": 0}"#).minutes, 30);
        assert_eq!(parse(r#"{"minutes": 9999}"#).minutes, 480);
        assert_eq!(parse(r#"{"maxViewers": 0}"#).max_viewers, 5);
        assert_eq!(parse(r#"{"maxViewers": 999}"#).max_viewers, 50);
    }

    #[test]
    fn save_snaps_the_numbers_too() {
        let dir = temp_dir();
        let cfg = LiveRoomConfig { relay: String::new(), minutes: 1000, max_viewers: 12 };
        save(&dir, &cfg).unwrap();
        let back = load(&dir);
        assert_eq!(back.minutes, 480);
        assert_eq!(back.max_viewers, 10);
    }

    #[test]
    fn save_trims_the_relay_address() {
        let dir = temp_dir();
        let cfg = LiveRoomConfig { relay: "https://relay.example.com/".into(), ..LiveRoomConfig::default() };
        save(&dir, &cfg).unwrap();
        assert_eq!(load(&dir).relay, "https://relay.example.com");
    }

    #[test]
    fn plain_http_to_another_computer_is_refused_and_not_written() {
        let dir = temp_dir();
        let cfg = LiveRoomConfig { relay: "http://relay.example.com".into(), ..LiveRoomConfig::default() };
        assert!(save(&dir, &cfg).is_err());
        assert!(!path(&dir).exists(), "a refused save writes nothing");
        assert!(checked_relay("http://relay.example.com").is_err());
    }

    #[test]
    fn a_saved_remote_http_relay_is_dropped_on_load() {
        let dir = temp_dir();
        std::fs::write(path(&dir), r#"{"relay": "http://relay.example.com", "minutes": 120}"#).unwrap();
        let cfg = load(&dir);
        assert_eq!(cfg.relay, "");
        assert_eq!(cfg.minutes, 120, "the other settings are kept");
    }

    #[test]
    fn loopback_http_is_allowed() {
        let dir = temp_dir();
        let cfg = LiveRoomConfig { relay: "http://127.0.0.1:8787".into(), ..LiveRoomConfig::default() };
        save(&dir, &cfg).unwrap();
        assert_eq!(load(&dir).relay, "http://127.0.0.1:8787");
    }

    #[test]
    fn empty_relay_is_allowed() {
        let dir = temp_dir();
        save(&dir, &LiveRoomConfig { relay: "   ".into(), ..LiveRoomConfig::default() }).unwrap();
        assert_eq!(load(&dir).relay, "");
    }

    #[test]
    fn effective_relay_prefers_the_live_room_relay() {
        let cfg = LiveRoomConfig { relay: "https://live.example.com".into(), ..LiveRoomConfig::default() };
        assert_eq!(effective_relay(&cfg, "https://team.example.com").as_deref(), Some("https://live.example.com"));
    }

    #[test]
    fn effective_relay_falls_back_to_the_team_relay() {
        let cfg = LiveRoomConfig::default();
        assert_eq!(effective_relay(&cfg, "https://team.example.com").as_deref(), Some("https://team.example.com"));
    }

    #[test]
    fn effective_relay_skips_an_invalid_live_room_relay() {
        let cfg = LiveRoomConfig { relay: "http://live.example.com".into(), ..LiveRoomConfig::default() };
        assert_eq!(effective_relay(&cfg, "https://team.example.com").as_deref(), Some("https://team.example.com"));
    }

    #[test]
    fn effective_relay_is_none_when_both_are_empty_or_invalid() {
        let cfg = LiveRoomConfig::default();
        assert_eq!(effective_relay(&cfg, ""), None);
        assert_eq!(effective_relay(&cfg, "relay.example.com"), None, "no scheme is invalid");
        let bad = LiveRoomConfig { relay: "http://live.example.com".into(), ..LiveRoomConfig::default() };
        assert_eq!(effective_relay(&bad, "http://team.example.com"), None);
    }
}
