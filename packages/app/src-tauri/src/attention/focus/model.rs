//! Focus data model: the limits, the types that go to disk, and the three tiers. Pure data. No clock reads here;
//! callers pass `now_ms` in, so every rule can be tested with a fixed time.
//!
//! The names are the ones in SPEC n33 section 3.3. Words the person sees are "Focus" and "End Focus", never
//! "mute", "DND" or "silence".

use super::summary::FocusSummary;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MINUTES: u32 = 45;
pub const MIN_MINUTES: u32 = 5;
pub const MAX_MINUTES: u32 = 240;
pub const PRESETS: [u32; 3] = [25, 45, 90];
pub const EXTEND_MINUTES: u32 = 15;
pub const NUDGE_AFTER_MS: u64 = 20 * 60_000;
pub const TICK_MS: u64 = 5_000;
/// The list of held things; the counts stay exact beyond it.
pub const HELD_LIST_CAP: usize = 100;
pub const WAITERS_CAP: usize = 200;
/// Same idea as `companion::today::SENTENCE_CAP`, shorter.
pub const SENTENCE_CAP: usize = 120;
pub const FILE_VERSION: u8 = 1;
/// One minute in milliseconds.
pub const MINUTE_MS: u64 = 60_000;

/// Stable error codes (SPEC 3.3) with the one-line message the person sees.
pub const CODE_BAD_MINUTES: &str = "FOCUS_BAD_MINUTES";
pub const CODE_NOT_ON: &str = "FOCUS_NOT_ON";
pub const CODE_PERSON_ONLY: &str = "FOCUS_PERSON_ONLY";

/// Who pressed. `Local` (a plain program on this computer) may only end Focus, never start it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum By {
    Desktop,
    Tray,
    Phone,
    Local,
}

/// Where a pinged thing goes while Focus is on. The three tiers are decided in `policy.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Not sent now. Counted and listed in the summary.
    Hold,
    /// Sent, but quietly (no buzz). The card still waits in the Inbox.
    Quiet,
    /// Sent at full strength, as today.
    Through,
}

/// What kind of held thing it was. Counts only, plus a short redacted sentence in `Held`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    RunDone,
    RunFailed,
    Companion,
    Review,
    Night,
}

/// One Focus session. `ends_at_ms` never passes `started_at_ms + MAX_MINUTES`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// `fc-` plus 12 lowercase hex characters.
    pub id: String,
    pub started_at_ms: u64,
    pub ends_at_ms: u64,
    /// The length asked for at the last start or change. The `+15` extensions are not counted here.
    pub minutes: u32,
    pub extended: u8,
    pub by: By,
}

impl Session {
    /// A new session from `now_ms` for `minutes`, with the end clamped to the 240 minute cap.
    pub fn new(now_ms: u64, minutes: u32, by: By, id: String) -> Self {
        let ends = now_ms.saturating_add(u64::from(minutes).saturating_mul(MINUTE_MS));
        Self { id, started_at_ms: now_ms, ends_at_ms: clamp_end(now_ms, ends), minutes, extended: 0, by }
    }
}

/// The end time, never later than `started_ms` plus the 240 minute cap.
pub fn clamp_end(started_ms: u64, ends_ms: u64) -> u64 {
    ends_ms.min(started_ms.saturating_add(u64::from(MAX_MINUTES) * MINUTE_MS))
}

/// The length check every start and change goes through. `Err` carries `FOCUS_BAD_MINUTES`.
pub fn validate_minutes(minutes: u32) -> Result<u32, FocusError> {
    if (MIN_MINUTES..=MAX_MINUTES).contains(&minutes) {
        Ok(minutes)
    } else {
        Err(FocusError::bad_minutes())
    }
}

/// One held thing. `sentence` is redacted and capped; there is no pane id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub at_ms: u64,
    pub class: Class,
    pub agent: String,
    pub sentence: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldCounts {
    pub run_done: u32,
    pub run_failed: u32,
    pub companion: u32,
    pub review: u32,
    pub night: u32,
}

impl HeldCounts {
    pub fn total(&self) -> u32 {
        self.run_done
            .saturating_add(self.run_failed)
            .saturating_add(self.companion)
            .saturating_add(self.review)
            .saturating_add(self.night)
    }

    pub fn add(&mut self, class: Class) {
        let slot = match class {
            Class::RunDone => &mut self.run_done,
            Class::RunFailed => &mut self.run_failed,
            Class::Companion => &mut self.companion,
            Class::Review => &mut self.review,
            Class::Night => &mut self.night,
        };
        *slot = slot.saturating_add(1);
    }
}

/// A blocking card that appeared during the session. The key is never shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiter {
    pub key: String,
    /// `Quiet` or `Through`.
    pub tier: Tier,
    pub first_seen_ms: u64,
    pub nudged: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub held: Vec<Held>,
    pub counts: HeldCounts,
    pub waiters: Vec<Waiter>,
}

/// The two settings Focus keeps. Stored in the same file as the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_minutes")]
    pub default_minutes: u32,
    #[serde(default = "default_quiet_blocking")]
    pub quiet_blocking: bool,
}

fn default_minutes() -> u32 {
    DEFAULT_MINUTES
}

fn default_quiet_blocking() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self { default_minutes: DEFAULT_MINUTES, quiet_blocking: true }
    }
}

/// The whole saved file (`<app data>/focus/state.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub v: u8,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub session: Option<Session>,
    #[serde(default)]
    pub ledger: Ledger,
    /// Kept until "Got it" or the next Focus.
    #[serde(default)]
    pub last_summary: Option<FocusSummary>,
}

impl Default for State {
    fn default() -> Self {
        Self { v: FILE_VERSION, settings: Settings::default(), session: None, ledger: Ledger::default(), last_summary: None }
    }
}

/// A Focus error: a stable code the phone and the desktop can match, and a plain message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusError {
    pub code: &'static str,
    pub message: String,
}

impl FocusError {
    pub fn bad_minutes() -> Self {
        Self { code: CODE_BAD_MINUTES, message: "Pick between 5 and 240 minutes.".into() }
    }

    pub fn not_on() -> Self {
        Self { code: CODE_NOT_ON, message: "Focus is not on.".into() }
    }

    pub fn person_only() -> Self {
        Self { code: CODE_PERSON_ONLY, message: "Only you can start or change Focus.".into() }
    }
}

#[cfg(test)]
pub(crate) fn sample_summary() -> FocusSummary {
    use super::summary::{Counts, EndedBy};
    FocusSummary {
        session_id: "fc-0123456789ab".into(),
        started_at_ms: 1_000,
        ended_at_ms: 2_000,
        minutes: 25,
        ended_by: EndedBy::Timer,
        counts: Counts { turns_done: 4, agents: 2, failed: 1, asked: 2, urgent: 0, still_waiting: 1, held: HeldCounts::default() },
        line: "While you focused (25 min): 4 turns done, 1 failed, 2 asked for you. 1 still waits.".into(),
    }
}
