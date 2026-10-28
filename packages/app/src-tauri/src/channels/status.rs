//! Text Your Agents: what the Settings page shows (`TelegramStatus`). It never holds the bot token
//! (only the bot's `@name`), and the phone gets none of it: the phone's view is `config::PhoneView`.

use super::config::TelegramConfig;
use super::telegram::poller::{LinkError, LinkInfo, LinkState};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedView {
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub paired_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingView {
    pub username: Option<String>,
    pub first_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    pub code: String,
    pub expires_at_ms: u64,
    pub waiting_for: Option<WaitingView>,
}

/// The shape `src/lib/telegram-api.ts` expects (`TelegramStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStatus {
    pub store_available: bool,
    pub store_label: String,
    pub token_set: bool,
    pub bot_username: Option<String>,
    pub enabled: bool,
    /// `off`, `connecting`, `on` or `error`.
    pub state: &'static str,
    /// `token_rejected`, `conflict`, `webhook_set`, `blocked` or `network`.
    pub error: Option<&'static str>,
    pub paired: Option<PairedView>,
    pub pairing: Option<PairingView>,
    pub caps: Vec<&'static str>,
    pub notify_runs: bool,
    pub short_mode: bool,
    pub last_poll_at_ms: Option<u64>,
    pub strangers_last_hour: u32,
}

/// What the status line says, from the poller's link and whether a poller is running.
pub fn link_state(cfg: &TelegramConfig, alive: bool, link: &LinkInfo) -> (&'static str, Option<&'static str>) {
    if link.state == LinkState::Error {
        return ("error", link.error.map(error_name));
    }
    if !alive || !cfg.enabled {
        return ("off", None);
    }
    match link.state {
        LinkState::On => ("on", None),
        _ => ("connecting", None),
    }
}

fn error_name(err: LinkError) -> &'static str {
    match err {
        LinkError::TokenRejected => "token_rejected",
        LinkError::Conflict => "conflict",
        LinkError::Blocked => "blocked",
    }
}

pub fn paired_view(cfg: &TelegramConfig) -> Option<PairedView> {
    cfg.paired.as_ref().map(|p| PairedView {
        username: p.username.clone(),
        first_name: p.first_name.clone(),
        paired_at_ms: p.paired_at_ms,
    })
}

pub fn cap_names(cfg: &TelegramConfig) -> Vec<&'static str> {
    cfg.caps.list().into_iter().map(|c| c.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(state: LinkState, error: Option<LinkError>) -> LinkInfo {
        LinkInfo { state, error, last_poll_ms: None }
    }

    fn on() -> TelegramConfig {
        TelegramConfig { enabled: true, ..TelegramConfig::default() }
    }

    #[test]
    fn a_stopped_poller_is_off() {
        assert_eq!(link_state(&on(), false, &link(LinkState::On, None)), ("off", None));
        assert_eq!(link_state(&TelegramConfig::default(), true, &link(LinkState::On, None)), ("off", None));
    }

    #[test]
    fn a_running_poller_is_connecting_then_on() {
        assert_eq!(link_state(&on(), true, &link(LinkState::Connecting, None)), ("connecting", None));
        assert_eq!(link_state(&on(), true, &link(LinkState::On, None)), ("on", None));
    }

    #[test]
    fn a_refusal_from_telegram_is_an_error_with_its_name() {
        let cases = [
            (LinkError::TokenRejected, "token_rejected"),
            (LinkError::Conflict, "conflict"),
            (LinkError::Blocked, "blocked"),
        ];
        for (err, name) in cases {
            assert_eq!(link_state(&on(), false, &link(LinkState::Error, Some(err))), ("error", Some(name)));
        }
    }

    #[test]
    fn the_status_json_matches_the_typescript_names() {
        let status = TelegramStatus {
            store_available: true,
            store_label: "Windows Credential Manager".into(),
            token_set: true,
            bot_username: Some("anna_pm_bot".into()),
            enabled: false,
            state: "off",
            error: None,
            paired: None,
            pairing: Some(PairingView { code: "PM7K2QX9".into(), expires_at_ms: 5, waiting_for: None }),
            caps: vec!["watch"],
            notify_runs: false,
            short_mode: false,
            last_poll_at_ms: None,
            strangers_last_hour: 0,
        };
        let value = serde_json::to_value(&status).unwrap();
        for key in [
            "storeAvailable", "storeLabel", "tokenSet", "botUsername", "enabled", "state", "error", "paired",
            "pairing", "caps", "notifyRuns", "shortMode", "lastPollAtMs", "strangersLastHour",
        ] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert_eq!(value["pairing"]["expiresAtMs"], 5);
        assert!(value["pairing"].get("waitingFor").is_some());
    }
}
