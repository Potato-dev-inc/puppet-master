//! What the desktop and the phone read about Focus (SPEC n33 3.5 `FocusView`, `PhoneView`). Built from the saved
//! state and a clock reading that the caller passes in, so tests fix the time.
//!
//! The phone view is redacted: counts, the end time and one summary sentence. It has no pane id, no agent name and
//! no held sentence. The field names are the ones `src/lib/focus.ts` reads (camelCase at the top level; the records
//! nested inside keep their Rust field names).

use super::model::{HeldCounts, Settings, State};
use super::summary::{waiting_counts, EndedBy, FocusSummary};
use serde::Serialize;

/// What the desktop shows (`focus_state` and the `focus://changed` payload).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusView {
    pub on: bool,
    pub session_id: Option<String>,
    pub started_at_ms: Option<u64>,
    pub ends_at_ms: Option<u64>,
    /// The length of the session that is on, or the default length when it is off.
    pub minutes: u32,
    pub extended: u8,
    pub held: HeldCounts,
    pub held_total: u32,
    pub waiting_quiet: u32,
    pub waiting_urgent: u32,
    pub settings: Settings,
    pub last_summary: Option<FocusSummary>,
}

/// What the phone reads from `GET /focus`. Counts only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    pub on: bool,
    pub ends_at_ms: Option<u64>,
    pub minutes: u32,
    pub held_total: u32,
    /// Cards that wait for the person during this Focus (quiet and urgent together).
    pub waiting: u32,
    pub last: Option<PhoneSummary>,
}

/// The phone's end line: when Focus ended, the one sentence (counts only) and how it ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhoneSummary {
    pub ended_at_ms: u64,
    pub line: String,
    pub ended_by: EndedBy,
}

/// Both views, read from one state at one time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub desktop: FocusView,
    pub phone: PhoneView,
}

impl View {
    /// The views for `state` at `now_ms`. Focus counts as on only while `now_ms` is before the session's end, so a
    /// session that has run out but not yet been ended reads as off (fails open). While off, nothing is held.
    pub fn from(state: &State, now_ms: u64) -> Self {
        let live = state.session.as_ref().filter(|s| now_ms < s.ends_at_ms);
        let (held, waiting_quiet, waiting_urgent) = match live {
            Some(_) => {
                let (quiet, urgent) = waiting_counts(&state.ledger.waiters);
                (state.ledger.counts.clone(), quiet, urgent)
            }
            None => (HeldCounts::default(), 0, 0),
        };
        let held_total = held.total();
        let minutes = live.map_or(state.settings.default_minutes, |s| s.minutes);
        let last = state.last_summary.as_ref();

        let desktop = FocusView {
            on: live.is_some(),
            session_id: live.map(|s| s.id.clone()),
            started_at_ms: live.map(|s| s.started_at_ms),
            ends_at_ms: live.map(|s| s.ends_at_ms),
            minutes,
            extended: live.map_or(0, |s| s.extended),
            held,
            held_total,
            waiting_quiet,
            waiting_urgent,
            settings: state.settings.clone(),
            last_summary: state.last_summary.clone(),
        };
        let phone = PhoneView {
            on: live.is_some(),
            ends_at_ms: live.map(|s| s.ends_at_ms),
            minutes,
            held_total,
            waiting: waiting_quiet + waiting_urgent,
            last: last.map(|s| PhoneSummary { ended_at_ms: s.ended_at_ms, line: s.line.clone(), ended_by: s.ended_by }),
        };
        Self { desktop, phone }
    }
}
