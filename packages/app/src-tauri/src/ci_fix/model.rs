//! Data model for "Red to Green" (SPEC n33 section 3.2): the settings, the watched pull requests,
//! the fix tries, the cleaned log snapshot, the push facts and the error codes. Pure data plus the
//! validators. No I/O here (see `store`).
//!
//! Defaults are the safe ones: the feature is off, every try and every push asks first, and the
//! budgets are always set. Every number read from a file is pulled into its range by `clamped`.

use crate::issue_intake::model::{AgentPick, PushFacts};
use serde::{Deserialize, Serialize};

pub const TICK_MS: u64 = 20_000;
pub const POLL_PENDING_S: u64 = 60;
pub const POLL_PR_S: u64 = 300;
pub const BACKOFF_MAX_S: u64 = 900;
pub const HUNG_CHECK_MS: u64 = 30 * 60_000;
pub const BUSY_WAIT_MS: u64 = 30 * 60_000;
pub const WATCH_IDLE_DAYS: u64 = 14;
pub const WATCHES_CAP: usize = 30;
pub const WATCH_RECORDS_CAP: usize = 200;
pub const FIX_RECORDS_CAP: usize = 300;
pub const PUSHED_SHAS_CAP: usize = 20;
pub const JOBS_READ_MAX: usize = 3;
pub const LOG_RAW_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const LOG_LINES_PER_JOB: usize = 200;
pub const LOG_LINE_MAX_CHARS: usize = 400;
pub const LOG_TOTAL_MAX_BYTES: usize = 24 * 1024;
pub const CHECKS_SHOWN_MAX: usize = 10;
pub const TRIES_DEFAULT: u32 = 2;
pub const TRIES_MAX: u32 = 3;
pub const TURNS_DEFAULT: u32 = 15;
pub const TURNS_MAX: u32 = 50;
pub const MINUTES_DEFAULT: u32 = 20;
pub const MINUTES_MIN: u32 = 5;
pub const MINUTES_MAX: u32 = 60;
pub const FIXES_PER_DAY_DEFAULT: u32 = 6;
pub const FIXES_PER_DAY_MAX: u32 = 20;
pub const CARD_TIMEOUT_S: u64 = 24 * 3600;
pub const CARD_KIND_FIX: &str = "ci_fix";
pub const CARD_KIND_PUSH: &str = "ci_push";
pub const CHOICE_FIX: &str = "fix";
pub const CHOICE_NOT_NOW: &str = "not_now";
pub const CHOICE_STOP: &str = "stop_watching";
pub const CHOICE_PUSH: &str = "push";
pub const CHOICE_LOOK: &str = "look";
pub const CHOICE_KEEP: &str = "dont_push";

// Error codes (plain English messages are written where they are raised).
pub const CI_FIX_GH_MISSING: &str = "CI_FIX_GH_MISSING";
pub const CI_FIX_GH_SIGNED_OUT: &str = "CI_FIX_GH_SIGNED_OUT";
pub const CI_FIX_NOT_PM_BRANCH: &str = "CI_FIX_NOT_PM_BRANCH";
pub const CI_FIX_NOT_YOURS: &str = "CI_FIX_NOT_YOURS";
pub const CI_FIX_FORK: &str = "CI_FIX_FORK";
pub const CI_FIX_PR_CLOSED: &str = "CI_FIX_PR_CLOSED";
pub const CI_FIX_HEAD_MOVED: &str = "CI_FIX_HEAD_MOVED";
pub const CI_FIX_COPY_GONE: &str = "CI_FIX_COPY_GONE";
pub const CI_FIX_COPY_BUSY: &str = "CI_FIX_COPY_BUSY";
pub const CI_FIX_AGENT_NOT_READY: &str = "CI_FIX_AGENT_NOT_READY";
pub const CI_FIX_TRIES_USED: &str = "CI_FIX_TRIES_USED";
pub const CI_FIX_DAY_CAP: &str = "CI_FIX_DAY_CAP";
pub const CI_FIX_START_FAILED: &str = "CI_FIX_START_FAILED";
pub const CI_FIX_GIT_CHANGED: &str = "CI_FIX_GIT_CHANGED";
pub const CI_FIX_CI_FILES: &str = "CI_FIX_CI_FILES";
pub const CI_FIX_HISTORY_REWRITTEN: &str = "CI_FIX_HISTORY_REWRITTEN";
pub const CI_FIX_REMOTE_MOVED: &str = "CI_FIX_REMOTE_MOVED";
pub const CI_FIX_PUSH_HELD: &str = "CI_FIX_PUSH_HELD";
pub const CI_FIX_PUSH_FAILED: &str = "CI_FIX_PUSH_FAILED";

/// The Labs settings (`settings.json`). Off by default; the four numbers are always in range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The Labs switch.
    pub enabled: bool,
    /// Tries per pull request (1..=3).
    pub tries_per_pr: u32,
    /// Turns per try, for a new pane (1..=50).
    pub turns_per_try: u32,
    /// Minutes per try, for a new pane (5..=60).
    pub minutes_per_try: u32,
    /// Fixes started in a day, all pull requests (1..=20).
    pub fixes_per_day: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            tries_per_pr: TRIES_DEFAULT,
            turns_per_try: TURNS_DEFAULT,
            minutes_per_try: MINUTES_DEFAULT,
            fixes_per_day: FIXES_PER_DAY_DEFAULT,
        }
    }
}

impl Settings {
    /// The same settings with every number pulled into its range. Nothing else changes.
    pub fn clamped(&self) -> Settings {
        Settings {
            enabled: self.enabled,
            tries_per_pr: self.tries_per_pr.clamp(1, TRIES_MAX),
            turns_per_try: self.turns_per_try.clamp(1, TURNS_MAX),
            minutes_per_try: self.minutes_per_try.clamp(MINUTES_MIN, MINUTES_MAX),
            fixes_per_day: self.fixes_per_day.clamp(1, FIXES_PER_DAY_MAX),
        }
    }
}

/// Where a watched pull request is. Serialized with a `state` tag in snake_case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WatchState {
    Watching,
    /// GitHub disagrees (fork, another author, a head that moved), or a read failed.
    Paused { why: String },
    /// The person stopped it, the cap, the 14 days, or the copy is gone.
    Stopped { why: String },
    Closed,
    Merged,
}

/// One pull request Puppet Master opened from one of its own copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
    /// `cw-` + 8 lowercase hex.
    pub id: String,
    /// The folder (git top level) the copy belongs to.
    pub project: String,
    /// The copy's handle (the run handle for runs Puppet Master started).
    pub handle: String,
    /// `owner/repo`.
    pub owner_repo: String,
    /// `pm/...`.
    pub branch: String,
    /// The PR base at open time.
    pub base: String,
    pub pr_number: u64,
    /// Starts with `https://github.com/<owner_repo>/pull/`.
    pub pr_url: String,
    pub agent: AgentPick,
    pub agent_label: String,
    /// "issue" | "ship".
    pub source: String,
    pub added_at_ms: u64,
    pub last_push_ms: u64,
    /// 40 lowercase hex each, newest last, at most `PUSHED_SHAS_CAP`.
    pub pushed_shas: Vec<String>,
    pub tries_used: u32,
    pub state: WatchState,
}

/// One failed check on the PR head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedCheck {
    /// Cleaned, one line, 80 characters.
    pub name: String,
    /// "failure" | "timed_out" | "startup_failure".
    pub conclusion: String,
    /// Some only for a check run from GitHub Actions.
    pub job_id: Option<u64>,
    /// `https://github.com/...` only, else empty.
    pub url: String,
}

/// Counts of what the log cleaner did. Only counts are kept, never the removed text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogNotes {
    pub lines: u32,
    pub cut_lines: u32,
    pub hidden_secrets: u32,
    pub invisible_chars: u32,
    pub jobs_read: u32,
    pub jobs_without_log: u32,
    pub truncated: bool,
}

/// The cleaned log text the agent gets. Emptied when the fix reaches a finished state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSnapshot {
    /// (check name, cleaned lines).
    pub jobs: Vec<(String, Vec<String>)>,
    pub notes: LogNotes,
    /// sha256 of the JSON body the agent gets, first 8 hex. Kept after the lines are dropped.
    pub sha8: String,
}

/// What the push card shows about a finished fix (on top of the shared push check).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixFacts {
    pub push: PushFacts,
    /// (path, line)
    pub skipped_tests: Vec<(String, u32)>,
    pub deleted_tests: Vec<String>,
    pub ci_run_files: Vec<String>,
    /// (path, line)
    pub web_lines: Vec<(String, u32)>,
    pub remote_moved: bool,
    pub history_rewritten: bool,
    pub wrong_branch: bool,
    pub over_cap: bool,
    pub commits: u32,
}

/// Where one fix try is. Serialized with a `state` tag in snake_case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FixState {
    /// The start card is open.
    Asking { question_id: String },
    /// "Not now" or dismissed. The person can ask again from the row.
    Declined,
    Expired,
    /// Yes, but the copy is busy or another fix runs.
    Queued { why: String, since_ms: u64 },
    Starting,
    /// `route` is "same_pane" or "new_pane".
    Running { route: String, handle: String, operation_id: String, pane_id: Option<String> },
    Checking,
    AskPush { question_id: String, facts: FixFacts },
    Pushing,
    Pushed { sha: String },
    Green { sha: String },
    StillRed { sha: String },
    NoChanges { message: String },
    Held { code: String, message: String },
    /// "Don't push": the commits stay in the copy, nothing is deleted.
    NotPushed,
    Failed { code: String, message: String },
}

/// One fix try on one watched pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixRun {
    /// `cf-` + 12 lowercase hex.
    pub id: String,
    pub watch_id: String,
    /// 1-based, counted when the agent starts.
    pub try_no: u32,
    pub failing_sha: String,
    /// At most `CHECKS_SHOWN_MAX`.
    pub checks: Vec<FailedCheck>,
    pub log: LogSnapshot,
    pub at_ms: u64,
    pub first_failure_ms: u64,
    /// "desktop" | "phone".
    pub approved_by: Option<String>,
    pub git_fingerprint: Option<String>,
    pub state: FixState,
}

/// True while a fix still needs its log text or its copy (not yet finished).
pub fn fix_active(state: &FixState) -> bool {
    matches!(
        state,
        FixState::Asking { .. }
            | FixState::Queued { .. }
            | FixState::Starting
            | FixState::Running { .. }
            | FixState::Checking
            | FixState::AskPush { .. }
            | FixState::Pushing
    )
}

/// `cw-` + 8 lowercase hex characters.
pub fn valid_watch_id(id: &str) -> bool {
    valid_hex_id(id, "cw-", 8)
}

/// `cf-` + 12 lowercase hex characters.
pub fn valid_fix_id(id: &str) -> bool {
    valid_hex_id(id, "cf-", 12)
}

/// A git commit id: exactly 40 lowercase hex characters.
pub fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_hex_id(id: &str, prefix: &str, hex_len: usize) -> bool {
    match id.strip_prefix(prefix) {
        Some(hex) => hex.len() == hex_len && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}
