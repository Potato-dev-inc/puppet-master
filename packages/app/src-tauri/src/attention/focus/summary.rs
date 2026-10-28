//! The end-of-Focus summary. FT-1 defines the types only, so the saved state can hold one. FT-5 fills in
//! `build`, `line`, `body` and `is_empty` (SPEC n33 3.4 D).

use super::model::HeldCounts;
use serde::{Deserialize, Serialize};

/// Why Focus ended: the time ran out, or the person ended it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndedBy {
    Timer,
    Early,
}

/// The numbers in one summary. Counts only: no names, no ids, no sentences.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub turns_done: u32,
    pub agents: u32,
    pub failed: u32,
    pub asked: u32,
    pub urgent: u32,
    pub still_waiting: u32,
    #[serde(default)]
    pub held: HeldCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSummary {
    pub session_id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub minutes: u32,
    pub ended_by: EndedBy,
    pub counts: Counts,
    /// One plain sentence, for example `While you focused (45 min): 4 turns done, 1 failed.`
    pub line: String,
}
