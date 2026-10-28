//! Companion data types (spec section 2.1) and the rules a saved profile must pass.
//!
//! Field names and casing are the wire format: the phone and the desktop UI read the camelCase JSON
//! these types produce. Nothing in this file names an agent.

use serde::{Deserialize, Serialize};

pub const MAX_COMPANIONS: usize = 3;
pub const SOUL_CAP: usize = 1_500;
pub const USER_CAP: usize = 1_400;
pub const MEMORY_CAP: usize = 2_200;
pub const PENDING_LINE_CAP: usize = 300;
pub const FEED_TEXT_CAP: usize = 200;
pub const MAX_CHECKINS: usize = 8;
pub const MIN_EVERY_MINUTES: u32 = 30;
pub const MAX_SCHEDULED_SENDS_PER_DAY: u32 = 24;
pub const NAME_CAP: usize = 24;
pub const CHECKIN_LABEL_CAP: usize = 40;
pub const CHECKIN_TEXT_CAP: usize = 2_000;

pub const AVATAR_IDS: [&str; 8] = ["mochi", "bit", "nib", "tofu", "lumen", "hoot", "sprout", "kit"];

fn yes() -> bool {
    true
}

/// Nap at 04:00 local time unless the person picks another time or turns the nap off (`null`).
fn default_nap() -> Option<HhMm> {
    Some(HhMm { h: 4, m: 0 })
}

/// A failed rule. `code` is the wire error code of section 11; `message` is plain English for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionError {
    pub code: &'static str,
    pub message: String,
}

impl CompanionError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for CompanionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

pub type CompanionResult<T> = Result<T, CompanionError>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompanionProfile {
    /// Slug `[a-z0-9-]{1,24}`, made from the name, unique.
    pub id: String,
    /// 1..=24 chars, no control characters.
    pub name: String,
    /// One of `AVATAR_IDS`.
    pub avatar: String,
    /// Persona preset id: cheerful | calm | coach | plain | custom.
    pub persona: String,
    /// Registry id of the agent; must pass `agents::agent_ok`.
    pub agent_type: String,
    #[serde(default)]
    pub launch_args: Vec<String>,
    /// `crate::accounts` id.
    #[serde(default)]
    pub account: Option<String>,
    /// Absolute home folder.
    pub folder: String,
    #[serde(default = "yes")]
    pub auto_start: bool,
    /// false = pending items are kept automatically (flagged ones still wait).
    #[serde(default = "yes")]
    pub memory_review: bool,
    /// Push the phones that may watch (`watch` cap) when the companion replies. Default on.
    #[serde(default = "yes")]
    pub push_replies: bool,
    /// Let the companion see what the person's other agents did today (`today.md` in its home folder,
    /// written by `today.rs`). Off by default: nothing is written until the person turns it on.
    #[serde(default)]
    pub share_today: bool,
    #[serde(default = "default_nap")]
    pub nap_at: Option<HhMm>,
    #[serde(default)]
    pub checkins: Vec<CheckIn>,
    pub created_ms: i64,
    #[serde(default)]
    pub archived: bool,
}

/// Local time of day, 0..=23 and 0..=59.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HhMm {
    pub h: u8,
    pub m: u8,
}

impl HhMm {
    pub fn is_valid(self) -> bool {
        self.h <= 23 && self.m <= 59
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckIn {
    /// 8 hex characters.
    pub id: String,
    /// 1..=40 chars, e.g. "Morning brief".
    pub label: String,
    /// 1..=2000 chars: what to ask.
    pub text: String,
    pub when: When,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum When {
    Daily { at: HhMm },
    /// Monday to Friday.
    Weekdays { at: HhMm },
    /// `day`: 0 = Monday .. 6 = Sunday.
    Weekly { day: u8, at: HhMm },
    /// At least `MIN_EVERY_MINUTES`.
    Every { minutes: u32 },
    /// Wire field: `atMs`.
    Once { at_ms: i64 },
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompanionState {
    Starting,
    Idle,
    Working,
    NeedsYou,
    Sleeping,
    Stopped,
}

/// What the UI and the phone read. Phone copies drop `folder` (no folders leave for the phone).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompanionView {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub persona: String,
    pub agent_type: String,
    pub agent_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Last path segment only.
    pub folder_name: String,
    pub state: CompanionState,
    pub state_line: String,
    /// `companion-<id>` while a pane exists.
    pub pane_id: Option<String>,
    /// The attention item that blocks it.
    pub needs: Option<NeedsRef>,
    pub pending_memory: u32,
    pub next_checkin: Option<NextCheckIn>,
    /// Desktop only: the full check-in list, for the editor. The phone copy has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkins: Option<Vec<CheckIn>>,
    /// Restarts in the last 15 minutes.
    pub restarts: u32,
    pub auto_start: bool,
    /// Whether a reply pushes the phones (`CompanionProfile::push_replies`).
    pub push_replies: bool,
    /// Whether the companion sees the day's turns (`CompanionProfile::share_today`).
    pub share_today: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NeedsRef {
    pub item_id: String,
    pub title: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NextCheckIn {
    pub id: String,
    pub label: String,
    pub at_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PendingKind {
    Note,
    User,
    Forget,
}

impl PendingKind {
    pub fn word(self) -> &'static str {
        match self {
            PendingKind::Note => "note",
            PendingKind::User => "user",
            PendingKind::Forget => "forget",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    /// First 12 hex characters of sha256(kind + text).
    pub id: String,
    pub kind: PendingKind,
    /// Redacted, at most `PENDING_LINE_CAP` chars.
    pub text: String,
    /// "Has a link" | "Sounds like an instruction" | None. Flags never block.
    pub flagged: Option<String>,
    /// For `forget`: the line it would remove (None = no match).
    pub target_line: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedKind {
    Reply,
    Checkin,
    Missed,
    Woke,
    Slept,
    Napped,
    Restarted,
    NeedsYou,
    Memory,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub id: String,
    pub at_ms: i64,
    pub kind: FeedKind,
    /// At most `FEED_TEXT_CAP` chars, redacted, no folders.
    pub text: String,
}

impl FeedItem {
    /// A new feed line: a fresh id, the text redacted and cut to `FEED_TEXT_CAP` characters.
    pub fn new(kind: FeedKind, at_ms: i64, text: &str) -> Self {
        let clean = crate::redact::text(text).into_inner();
        Self {
            id: new_hex_id(12),
            at_ms,
            kind,
            text: cut_chars(clean.trim(), FEED_TEXT_CAP),
        }
    }
}

/// The three memory files a companion keeps, with their caps.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFile {
    Soul,
    User,
    Memory,
}

impl MemoryFile {
    pub const ALL: [MemoryFile; 3] = [MemoryFile::Soul, MemoryFile::User, MemoryFile::Memory];

    pub fn file_name(self) -> &'static str {
        match self {
            MemoryFile::Soul => "SOUL.md",
            MemoryFile::User => "USER.md",
            MemoryFile::Memory => "MEMORY.md",
        }
    }

    pub fn cap(self) -> usize {
        match self {
            MemoryFile::Soul => SOUL_CAP,
            MemoryFile::User => USER_CAP,
            MemoryFile::Memory => MEMORY_CAP,
        }
    }
}

/// Characters, not bytes: the caps are about what the agent reads.
pub fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// The first `cap` characters of `text`.
pub fn cut_chars(text: &str, cap: usize) -> String {
    text.chars().take(cap).collect()
}

/// `len` random lowercase hex characters (UUID v4 based).
pub fn new_hex_id(len: usize) -> String {
    let mut s = uuid::Uuid::new_v4().simple().to_string();
    s.truncate(len);
    s
}

/// A slug id for a companion: lower-case letters and digits, single dashes, at most 24 chars.
/// Falls back to "companion" when the name has no letter or digit.
pub fn slug_from_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    let base = cut_chars(trimmed, NAME_CAP);
    let base = base.trim_end_matches('-');
    if base.is_empty() { "companion".to_string() } else { base.to_string() }
}

/// `slug_from_name`, then a `-2`, `-3`, ... suffix until the id is not in `taken`.
pub fn unique_slug(name: &str, taken: &[&str]) -> String {
    let base = slug_from_name(name);
    if !taken.contains(&base.as_str()) {
        return base;
    }
    for n in 2.. {
        let suffix = format!("-{n}");
        let stem = cut_chars(&base, NAME_CAP - suffix.len());
        let candidate = format!("{}{}", stem.trim_end_matches('-'), suffix);
        if !taken.contains(&candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("an unbounded counter always finds a free id")
}

/// `[a-z0-9-]{1,24}`, no leading or trailing dash.
pub fn valid_id(id: &str) -> bool {
    let n = char_len(id);
    (1..=NAME_CAP).contains(&n)
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

/// 1..=24 chars, no control characters, no leading or trailing space.
pub fn valid_name(name: &str) -> bool {
    let n = char_len(name);
    (1..=NAME_CAP).contains(&n) && name == name.trim() && !name.chars().any(char::is_control)
}

/// Checks everything a saved profile must satisfy, except the agent (`agents::agent_ok`) and the
/// folder on disk (`files::ensure_home`).
pub fn validate_profile(profile: &CompanionProfile) -> CompanionResult<()> {
    if !valid_id(&profile.id) {
        return Err(CompanionError::new("COMPANION_NAME_INVALID", "The companion id is not valid."));
    }
    if !valid_name(&profile.name) {
        return Err(CompanionError::new(
            "COMPANION_NAME_INVALID",
            "The name must be 1 to 24 letters, with no extra spaces.",
        ));
    }
    if !AVATAR_IDS.contains(&profile.avatar.as_str()) {
        return Err(CompanionError::new("COMPANION_AVATAR_INVALID", "Pick one of the faces in the list."));
    }
    if !crate::companion::personas::is_persona(&profile.persona) {
        return Err(CompanionError::new("COMPANION_PERSONA_INVALID", "Pick one of the ways to talk."));
    }
    if profile.agent_type.trim().is_empty() {
        return Err(CompanionError::new(
            "COMPANION_AGENT_UNAVAILABLE",
            "Chat can't send messages to this agent yet.",
        ));
    }
    if let Some(at) = profile.nap_at {
        if !at.is_valid() {
            return Err(CompanionError::new("COMPANION_CHECKIN_INVALID", "The nap time is not a real time of day."));
        }
    }
    if profile.checkins.len() > MAX_CHECKINS {
        return Err(CompanionError::new(
            "COMPANION_CHECKIN_INVALID",
            format!("A companion can have at most {MAX_CHECKINS} check-ins."),
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for checkin in &profile.checkins {
        validate_checkin(checkin)?;
        if seen.contains(&checkin.id.as_str()) {
            return Err(CompanionError::new("COMPANION_CHECKIN_INVALID", "Two check-ins have the same id."));
        }
        seen.push(&checkin.id);
    }
    Ok(())
}

/// One check-in: an 8-hex id, a label and text of the allowed length, and a valid `When`.
pub fn validate_checkin(checkin: &CheckIn) -> CompanionResult<()> {
    let bad = |message: &str| CompanionError::new("COMPANION_CHECKIN_INVALID", message.to_string());
    if checkin.id.len() != 8 || !checkin.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("The check-in id is not valid."));
    }
    let label = char_len(&checkin.label);
    if !(1..=CHECKIN_LABEL_CAP).contains(&label) || checkin.label.chars().any(char::is_control) {
        return Err(bad("A check-in label must be 1 to 40 letters."));
    }
    let text = char_len(&checkin.text);
    if !(1..=CHECKIN_TEXT_CAP).contains(&text) {
        return Err(bad("A check-in text must be 1 to 2,000 letters."));
    }
    match &checkin.when {
        When::Daily { at } | When::Weekdays { at } if !at.is_valid() => Err(bad("The check-in time is not valid.")),
        When::Weekly { at, .. } if !at.is_valid() => Err(bad("The check-in time is not valid.")),
        When::Weekly { day, .. } if *day > 6 => Err(bad("The check-in day must be 0 (Monday) to 6 (Sunday).")),
        When::Every { minutes } if *minutes < MIN_EVERY_MINUTES => {
            Err(bad("A repeat check-in must be at least every 30 minutes."))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_lowercase_dashed_and_capped() {
        assert_eq!(slug_from_name("Mochi the Cat!"), "mochi-the-cat");
        assert_eq!(slug_from_name("  Ren  "), "ren");
        assert_eq!(slug_from_name("日本語"), "companion");
        assert_eq!(slug_from_name("a".repeat(40).as_str()), "a".repeat(24));
        let long = format!("{}-b", "a".repeat(23));
        assert_eq!(slug_from_name(&long), "a".repeat(23));
    }

    #[test]
    fn unique_slug_adds_numbers_until_free() {
        assert_eq!(unique_slug("Bit", &[]), "bit");
        assert_eq!(unique_slug("Bit", &["bit"]), "bit-2");
        assert_eq!(unique_slug("Bit", &["bit", "bit-2"]), "bit-3");
        let base = "a".repeat(24);
        let id = unique_slug(&base, &[base.as_str()]);
        assert!(valid_id(&id), "{id}");
        assert!(id.ends_with("-2"));
    }

    #[test]
    fn ids_and_names_follow_the_rules() {
        assert!(valid_id("mochi-2"));
        assert!(!valid_id(""));
        assert!(!valid_id("Mochi"));
        assert!(!valid_id("-mochi"));
        assert!(!valid_id("a".repeat(25).as_str()));
        assert!(valid_name("Mochi"));
        assert!(!valid_name(" Mochi"));
        assert!(!valid_name("Mo\nchi"));
        assert!(!valid_name(""));
        assert!(valid_name(&"n".repeat(24)));
        assert!(!valid_name(&"n".repeat(25)));
    }

    #[test]
    fn caps_count_characters() {
        assert_eq!(char_len("ééé"), 3);
        assert_eq!(cut_chars("ééééé", 2), "éé");
        let item = FeedItem::new(FeedKind::Reply, 1, &"x".repeat(500));
        assert_eq!(char_len(&item.text), FEED_TEXT_CAP);
        assert_eq!(item.id.len(), 12);
    }

    #[test]
    fn when_variants_use_camel_case_fields_on_the_wire() {
        let once = When::Once { at_ms: 42 };
        let json = serde_json::to_value(&once).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "once", "atMs": 42 }));
        let back: When = serde_json::from_value(json).unwrap();
        assert_eq!(back, once);

        let weekly = When::Weekly { day: 2, at: HhMm { h: 9, m: 30 } };
        let json = serde_json::to_value(&weekly).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "weekly", "day": 2, "at": { "h": 9, "m": 30 } }));
        assert_eq!(serde_json::from_value::<When>(json).unwrap(), weekly);
    }

    #[test]
    fn checkin_rules() {
        let at = HhMm { h: 8, m: 0 };
        let ok = |when: When| CheckIn { id: "0a1b2c3d".into(), label: "Brief".into(), text: "Hi".into(), when, enabled: true };
        assert!(validate_checkin(&ok(When::Daily { at })).is_ok());
        assert!(validate_checkin(&ok(When::Every { minutes: 29 })).is_err());
        assert!(validate_checkin(&ok(When::Every { minutes: 30 })).is_ok());
        assert!(validate_checkin(&ok(When::Weekly { day: 7, at })).is_err());
        assert!(validate_checkin(&ok(When::Daily { at: HhMm { h: 24, m: 0 } })).is_err());
        let mut bad_id = ok(When::Daily { at });
        bad_id.id = "xyz".into();
        assert!(validate_checkin(&bad_id).is_err());
    }
}
