//! Keep going: the data model (SPEC n32 section 3.2).
//!
//! Plain data and pure helpers only: one record per hand-over, the settings, the offer shape and
//! the target id format. No I/O here (that is `store.rs`). Generic code: no agent names; ids come
//! from the person's settings or the agent registry.

use serde::{Deserialize, Serialize};

/// Most hops in one chain. A hop needs a person's tap each time.
pub const HOP_CAP: u8 = 3;
/// Largest brief written for the new agent.
pub const BRIEF_MAX_BYTES: usize = 6 * 1024;
/// Hand-over records kept (oldest dropped first).
pub const RECORDS_CAP: usize = 200;
/// Targets kept in the settings order.
pub const SETTINGS_ORDER_CAP: usize = 8;
/// Wait after a reset before asking to move back.
pub const COME_BACK_GRACE_MS: u64 = 60_000;
/// Re-check interval when the other side is busy.
pub const COME_BACK_RETRY_MS: u64 = 10 * 60_000;
/// Re-checks before the come-back is dropped.
pub const COME_BACK_TRIES: u8 = 3;
/// How long the old agent gets to exit before nothing new starts.
pub const STOP_WAIT_MS: u64 = 5_000;

pub const ERR_NO_TARGET: &str = "KEEP_GOING_NO_TARGET";
pub const ERR_SAME_VENDOR: &str = "KEEP_GOING_SAME_VENDOR";
pub const ERR_TARGET_LIMITED: &str = "KEEP_GOING_TARGET_LIMITED";
pub const ERR_HOP_CAP: &str = "KEEP_GOING_HOP_CAP";
pub const ERR_ALREADY: &str = "KEEP_GOING_ALREADY";
pub const ERR_NOT_AT_LIMIT: &str = "KEEP_GOING_NOT_AT_LIMIT";
pub const ERR_NOT_HERE: &str = "KEEP_GOING_NOT_HERE";
pub const ERR_BUSY: &str = "KEEP_GOING_BUSY";
pub const ERR_OLD_DID_NOT_STOP: &str = "OLD_AGENT_DID_NOT_STOP";
pub const ERR_START_FAILED: &str = "KEEP_GOING_START_FAILED";

/// Record id: `kg-` + 12 lowercase hex characters. The only text that ever becomes a file name.
pub fn valid_id(id: &str) -> bool {
    match id.strip_prefix("kg-") {
        Some(hex) => hex.len() == 12 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}

/// `agent` or `agent@account` (the same form `backup_agents` uses).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Target {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl Target {
    /// Parse `agent` or `agent@account`. Trims the text. Each part is 1 to 64 characters of
    /// letters, digits, `_`, `.` or `-` (and not `.` or `..`). At most one `@`.
    pub fn parse(text: &str) -> Option<Target> {
        let text = text.trim();
        let (agent, account) = match text.split_once('@') {
            Some((agent, account)) => {
                if account.contains('@') {
                    return None;
                }
                (agent, Some(account))
            }
            None => (text, None),
        };
        if !valid_part(agent) || account.is_some_and(|account| !valid_part(account)) {
            return None;
        }
        Some(Target { agent_id: agent.to_string(), account: account.map(str::to_string) })
    }

    /// `agent` or `agent@account`.
    pub fn key(&self) -> String {
        match &self.account {
            Some(account) => format!("{}@{}", self.agent_id, account),
            None => self.agent_id.clone(),
        }
    }
}

fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 64
        && part != "."
        && part != ".."
        && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    UsageLimit,
    CameBack,
}

/// One side of a hand-over (the old pane or the new one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side {
    /// The new side's pane is filled once `run_agent` returns ("" before).
    pub pane_id: String,
    pub agent_id: String,
    pub account: Option<String>,
    /// The name the person sees, such as "<agent> · <account>" (account only when set). Never an e-mail.
    pub label: String,
    pub handle: Option<String>,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Stopping,
    Starting,
    Running,
    Failed { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "back", rename_all = "snake_case")]
pub enum Back {
    Off,
    AtReset { due_ms: u64, tries: u8 },
    Asked,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefMeta {
    /// Absolute path of the private brief file (never shown outside the desktop window).
    pub file: String,
    pub sha8: String,
    pub bytes: usize,
    pub trimmed: Vec<String>,
    pub turns: u32,
}

/// One hand-over, from the old pane to the new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handover {
    /// `kg-` + 12 hex.
    pub id: String,
    /// Id of the first record of the chain.
    pub chain_id: String,
    /// 1-based.
    pub hop: u8,
    pub at_ms: u64,
    pub reason: Reason,
    pub from: Side,
    pub to: Side,
    /// Run project key (the origin repo for a worktree).
    pub project: String,
    /// Folder the new pane works in. Local only, never sent anywhere.
    pub folder: String,
    pub worktree_handle: Option<String>,
    pub from_reset_at_ms: Option<u64>,
    /// Redacted, at most 1,200 characters.
    pub goal: String,
    pub back: Back,
    pub brief: BriefMeta,
    pub state: State,
}

/// What the person set in Settings > Agents > Keep going.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub order: Vec<Target>,
    #[serde(default = "yes")]
    pub ask_back: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { order: Vec::new(), ask_back: true }
    }
}

fn yes() -> bool {
    true
}

/// The agent that is out of usage (input to `offers::rank`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub agent_id: String,
    pub account: Option<String>,
    pub vendor: Option<String>,
}

/// One agent that could take over (input to `offers::rank`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub target: Target,
    pub label: String,
    /// `limits().vendor()`; None = no `[limits]` table.
    pub vendor: Option<String>,
    /// `companion::agents::agent_ok(...)` mapped to text.
    pub runnable: Result<(), String>,
    /// Some(_) = limit live (engine note or record); the inner value is the reset time.
    pub limited_until: Option<Option<u64>>,
}

/// One row the picker shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub target: Target,
    pub label: String,
    pub ok: bool,
    pub why_not: Option<String>,
}

/// What the limit card offers (`card::limit_card`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepGoingOffer {
    Offer { target: Target, label: String },
    None { why: String },
    NotHere,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(text: &str) -> Option<Target> {
        Target::parse(text)
    }

    #[test]
    fn target_parse_accepts_agent_and_agent_at_account() {
        assert_eq!(target("fake_a"), Some(Target { agent_id: "fake_a".into(), account: None }));
        assert_eq!(
            target("fake_a@work"),
            Some(Target { agent_id: "fake_a".into(), account: Some("work".into()) })
        );
        assert_eq!(target("  fake_a  "), Some(Target { agent_id: "fake_a".into(), account: None }));
        assert!(target(&"a".repeat(64)).is_some(), "64 characters is the largest part");
    }

    #[test]
    fn target_parse_rejects_bad_text() {
        for bad in ["", "   ", "a@b@c", "../x", "x/../y", "@work", "fake_a@", "a b", "..", ".", "a\\b"] {
            assert_eq!(target(bad), None, "{bad:?} must be refused");
        }
        assert_eq!(target(&"a".repeat(65)), None, "65 characters is too long");
        assert_eq!(target(&format!("fake_a@{}", "b".repeat(65))), None);
    }

    #[test]
    fn target_key_round_trips() {
        assert_eq!(target("fake_a").unwrap().key(), "fake_a");
        assert_eq!(target("fake_a@work").unwrap().key(), "fake_a@work");
        let key = target("fake_a@work").unwrap().key();
        assert_eq!(Target::parse(&key), target("fake_a@work"));
    }

    #[test]
    fn valid_id_is_kg_plus_twelve_lowercase_hex() {
        assert!(valid_id("kg-0123456789ab"));
        for bad in ["kg-0123456789AB", "kg-0123456789a", "kg-0123456789abc", "xx-0123456789ab", "kg-../../etc"] {
            assert!(!valid_id(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn state_and_back_tags_are_stable() {
        assert_eq!(serde_json::to_string(&State::Stopping).unwrap(), r#"{"state":"stopping"}"#);
        assert_eq!(serde_json::to_string(&State::Starting).unwrap(), r#"{"state":"starting"}"#);
        assert_eq!(serde_json::to_string(&State::Running).unwrap(), r#"{"state":"running"}"#);
        assert_eq!(
            serde_json::to_string(&State::Failed { code: "C".into(), message: "m".into() }).unwrap(),
            r#"{"state":"failed","code":"C","message":"m"}"#
        );
        assert_eq!(serde_json::to_string(&Back::Off).unwrap(), r#"{"back":"off"}"#);
        assert_eq!(serde_json::to_string(&Back::Asked).unwrap(), r#"{"back":"asked"}"#);
        assert_eq!(serde_json::to_string(&Back::Done).unwrap(), r#"{"back":"done"}"#);
        assert_eq!(
            serde_json::to_string(&Back::AtReset { due_ms: 5, tries: 1 }).unwrap(),
            r#"{"back":"at_reset","due_ms":5,"tries":1}"#
        );
        assert_eq!(serde_json::to_string(&Reason::UsageLimit).unwrap(), r#""usage_limit""#);
        assert_eq!(serde_json::to_string(&Reason::CameBack).unwrap(), r#""came_back""#);
    }

    #[test]
    fn target_json_omits_missing_account() {
        assert_eq!(serde_json::to_string(&target("fake_a").unwrap()).unwrap(), r#"{"agent_id":"fake_a"}"#);
        let back: Target = serde_json::from_str(r#"{"agent_id":"fake_a"}"#).unwrap();
        assert_eq!(back, target("fake_a").unwrap());
    }

    #[test]
    fn settings_default_asks_to_come_back() {
        assert_eq!(Settings::default(), Settings { order: vec![], ask_back: true });
        let parsed: Settings = serde_json::from_str("{}").unwrap();
        assert!(parsed.ask_back, "a file without the switch keeps it on");
        assert!(parsed.order.is_empty());
    }
}
