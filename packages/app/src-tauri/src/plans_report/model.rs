//! What "Your plans" returns. Camel case for the page. No folder, no pane id, no chat text.

use serde::Serialize;

/// One turn, cut down to what the report needs (from an operation record, a turn summary, or both).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFact {
    pub agent_id: String,
    /// Account id; `None` = the agent's own login.
    pub account: Option<String>,
    /// Local-time month is decided from this.
    pub started_ms: u64,
    pub busy_ms: u64,
    /// Files changed in the turn, when the summary knew.
    pub files: Option<FilesFact>,
    /// The turn's run was merged from its own worktree.
    pub merged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilesFact {
    pub count: u32,
    pub added: u32,
    pub deleted: u32,
}

/// A usage limit as the log recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitFact {
    pub agent_id: String,
    pub account: Option<String>,
    pub start_ms: u64,
    /// When it ended: the agent carried on, the screen moved on, or the reset time passed.
    /// `None` = never recorded (the app closed, or the pane did).
    pub end_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Files {
    /// Turns whose summary told how many files changed.
    pub known_turns: u32,
    pub changed: u32,
    pub added: u32,
    pub deleted: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub hits: u32,
    pub limited_ms: u64,
}

/// One subscription: an agent under one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRow {
    pub agent_id: String,
    pub agent_label: String,
    /// `None` = the agent's own login ("Default account").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub account_label: String,
    pub turns: u32,
    pub busy_ms: u64,
    pub files: Files,
    /// Runs whose own worktree was merged.
    pub merged: u32,
    pub limits: Limits,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub turns: u32,
    pub busy_ms: u64,
    pub files_changed: u32,
    pub added: u32,
    pub deleted: u32,
    pub merged: u32,
    pub limit_hits: u32,
    pub limited_ms: u64,
}

/// One calendar month in this computer's time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthInfo {
    /// `2026-10`.
    pub id: String,
    /// `October 2026`.
    pub label: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlansReport {
    pub month: MonthInfo,
    /// Months you can switch to, newest first (this month always, then every earlier month with turns).
    pub months: Vec<MonthInfo>,
    pub rows: Vec<PlanRow>,
    pub totals: Totals,
    /// Plain sentences about what is and is not counted ("Counting limits since 12 October 2026.").
    pub notes: Vec<String>,
    /// Day (`2026-10-28`) the limit log began; `None` until a limit has been logged or the log is new.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits_counted_since: Option<String>,
}
