//! Types and limits of Collision Alert (SPEC-n33 section 3.2).
//!
//! Plain data only: no process, no file and no clock here. Every limit is a named constant, so the
//! tests and the spec point at the same numbers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---- limits (every one is a named const, tested) ----
/// First look after a group has 2+ panes and one of them is running.
pub const FIRST_POLL_MS: u64 = 8_000;
/// Gap between looks while any pane of the checkout is running.
pub const POLL_MS: u64 = 20_000;
pub const POLL_MAX_MS: u64 = 120_000;
/// Next gap is at least this many times the last look's cost, within `POLL_MS..=POLL_MAX_MS`.
pub const BACKOFF_FACTOR: u64 = 20;
pub const TURN_END_DEBOUNCE_MS: u64 = 2_000;
/// A look slower than this is "slow".
pub const SLOW_LOOK_MS: u64 = 3_000;
/// Three slow looks in a row: the checkout is only looked at on turn ends for `TURN_END_ONLY_MS`.
pub const SLOW_LOOKS_TO_PARK: u8 = 3;
pub const TURN_END_ONLY_MS: u64 = 30 * 60_000;
/// One git process is killed after this.
pub const PROBE_TIMEOUT_MS: u64 = 10_000;
pub const REPO_CACHE_MS: u64 = 60_000;
pub const MAX_GROUPS: usize = 4;
/// Oldest first; the rest are ignored.
pub const MAX_PANES_PER_GROUP: usize = 6;
pub const MAX_ALERTS: usize = 6;
pub const MAX_SAMPLE_FILES: usize = 400;
pub const MAX_STAT_FILES: usize = 2_000;
pub const MAX_COMPARE_FILES: usize = 20;
pub const MAX_FILES_SHOWN: usize = 5;
/// Display only.
pub const MAX_PATH_CHARS: usize = 120;
pub const MAX_PATCH_BYTES: usize = 256 * 1024;
pub const MAX_HUNKS_PER_FILE: usize = 500;
/// Touching lines clash (git does not merge adjacent edits).
pub const CLASH_GAP_LINES: u32 = 1;
pub const CREATED_SLACK_MS: u64 = 2_000;
pub const RESOLVE_AFTER_MISSES: u8 = 2;
/// "Smaller change" = at most 70% of the other side's files ...
pub const SMALLER_RATIO_PCT: u32 = 70;
/// ... and at least 2 files fewer.
pub const SMALLER_MIN_GAP: u32 = 2;

/// `(first, regular)` gap in ms between two looks of a checkout.
/// `PM_FAKE_COLLISION_POLL_MS` replaces both, under `cfg(any(test, feature = "dev-fake-tui"))` only.
pub fn poll_ms() -> (u64, u64) {
    #[cfg(any(test, feature = "dev-fake-tui"))]
    {
        if let Some(ms) = override_from(std::env::var("PM_FAKE_COLLISION_POLL_MS").ok().as_deref()) {
            return (ms, ms);
        }
    }
    (FIRST_POLL_MS, POLL_MS)
}

/// Only a positive whole number counts as an override.
#[cfg(any(test, feature = "dev-fake-tui"))]
fn override_from(raw: Option<&str>) -> Option<u64> {
    raw?.trim().parse::<u64>().ok().filter(|ms| *ms > 0)
}

/// Whether the two panes are in separate copies of one repo, or in one folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Copies,
    SameFolder,
}

/// How loud an alert is. `Clash` is the loudest (largest).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    SameFile,
    SharedFolder,
    Clash,
}

/// Running (a turn is in progress) or resting (idle, waiting for input, error, or anything else).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneRun {
    Working,
    Resting,
}

/// One agent pane, as the engine sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneFact {
    pub pane_id: String,
    pub agent_id: String,
    /// The sidebar heading, for example "Name 2". Never agent text.
    pub label: String,
    pub cwd: String,
    pub run: PaneRun,
    pub created_ms: u64,
}

/// The git repo a pane's folder belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoFact {
    /// Comparable path of the common git dir (same for every copy of one repo).
    pub key: String,
    /// The checkout's top folder.
    pub root: String,
    /// True for a worktree copy (its `.git` is a pointer file).
    pub linked: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchKind {
    Added,
    Modified,
    Deleted,
}

/// One changed file of a checkout. `path` is clean and relative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touched {
    pub path: String,
    pub kind: TouchKind,
    /// `None` = deleted.
    pub mtime_ms: Option<u64>,
}

/// One look at a checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub head: String,
    pub files: Vec<Touched>,
    pub truncated: bool,
    pub cost_ms: u64,
}

/// Inclusive old-side line range. An insertion after line n is `{n, n}`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHunks {
    pub spans: Vec<Span>,
    pub binary: bool,
    pub cut: bool,
}

/// Why two files are in an overlap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum How {
    /// Clash: these lines overlap (union of the first overlapping pair).
    Lines(Span),
    /// Clash: could not compare (big change).
    LinesUnknown,
    /// Clash: both made the same new file.
    BothNew,
    /// Clash: one side deleted it. Index 0 or 1 into the pair.
    DeleteVsEdit { deleted_by: usize },
    /// Clash: both changed a file that is not text.
    Binary,
    /// Same file, different places.
    Apart,
    /// Same file; the sides started from different versions, so lines were not compared.
    DifferentStart,
    /// Same folder: changed while both worked.
    Shared,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOverlap {
    pub path: String,
    pub how: How,
}

/// The facts of one side that the advice rules use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideFacts {
    pub pane_id: String,
    pub working: bool,
    pub files: u32,
    pub started_ms: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    AlreadyDone,
    SmallerChange,
    StartedFirst,
}

/// `first` is 0 or 1: the side to finish first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Advice {
    pub first: usize,
    pub why: Why,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    /// `ca-` + 12 lowercase hex characters, see `AlertId::of`.
    pub id: String,
    pub relation: Relation,
    pub level: Level,
    /// Sorted by pane id.
    pub panes: [PaneFact; 2],
    pub sides: [SideFacts; 2],
    /// Clash files first, then by newest change. All of them; the view cuts to `MAX_FILES_SHOWN`.
    pub files: Vec<FileOverlap>,
    /// `None` for `SameFile`.
    pub advice: Option<Advice>,
    pub first_ms: u64,
    pub last_ms: u64,
    pub misses: u8,
    /// Ignored by the person.
    pub hidden: bool,
}

/// `collision.json`. Default: on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: true }
    }
}

/// Alert ids. One alert per pair of panes.
pub struct AlertId;

impl AlertId {
    /// `ca-` plus 12 lowercase hex characters of sha256 over the sorted ids joined by `|`.
    /// The order of `pane_ids` does not change the result.
    pub fn of(pane_ids: &[&str]) -> String {
        let mut ids: Vec<&str> = pane_ids.to_vec();
        ids.sort_unstable();
        let digest = Sha256::digest(ids.join("|").as_bytes());
        let mut out = String::from("ca-");
        for byte in digest.iter().take(6) {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }
}

/// One side of a view. No folder, no process text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideView {
    pub pane_id: String,
    pub label: String,
    pub working: bool,
}

/// One file line of a view. `path` is cut to `MAX_PATH_CHARS`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub text: String,
}

/// What the windows get (camelCase JSON). Built by `view.rs` from an `Alert`. Contains no folder,
/// no pane process and no author text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertView {
    pub id: String,
    /// "clash" | "same_file" | "shared_folder"
    pub level: String,
    /// "copies" | "same_folder"
    pub relation: String,
    pub title: String,
    pub detail: String,
    pub advice: Option<String>,
    pub place: String,
    pub footer: String,
    pub sides: [SideView; 2],
    pub files: Vec<FileView>,
    pub more: u32,
    pub first_ms: u64,
    pub last_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn well_formed_id(id: &str) -> bool {
        id.len() == 15
            && id.starts_with("ca-")
            && id[3..].chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    }

    fn sample_view() -> AlertView {
        AlertView {
            id: "ca-0123456789ab".to_string(),
            level: "clash".to_string(),
            relation: "copies".to_string(),
            title: "fake_a and fake_b both changed src/auth.ts".to_string(),
            detail: "Lines 40-80 overlap.".to_string(),
            advice: None,
            place: "In separate copies.".to_string(),
            footer: "Nothing was paused. You decide.".to_string(),
            sides: [
                SideView { pane_id: "pane-1".to_string(), label: "fake_a".to_string(), working: true },
                SideView { pane_id: "pane-2".to_string(), label: "fake_b".to_string(), working: false },
            ],
            files: vec![FileView { path: "src/auth.ts".to_string(), text: "Lines 40-80".to_string() }],
            more: 0,
            first_ms: 1,
            last_ms: 2,
        }
    }

    #[test]
    fn serde_names_are_snake_case_for_enums() {
        assert_eq!(serde_json::to_string(&Level::Clash).unwrap(), "\"clash\"");
        assert_eq!(serde_json::to_string(&Level::SameFile).unwrap(), "\"same_file\"");
        assert_eq!(serde_json::to_string(&Level::SharedFolder).unwrap(), "\"shared_folder\"");
        assert_eq!(serde_json::to_string(&Relation::Copies).unwrap(), "\"copies\"");
        assert_eq!(serde_json::to_string(&Relation::SameFolder).unwrap(), "\"same_folder\"");
    }

    #[test]
    fn alert_view_uses_camel_case_keys() {
        let json = serde_json::to_value(sample_view()).unwrap();
        for key in ["firstMs", "lastMs", "advice", "more", "footer", "place", "sides", "files", "relation"] {
            assert!(json.get(key).is_some(), "missing key {key}");
        }
        assert!(json.get("first_ms").is_none());
        assert_eq!(json["level"], "clash");
        assert_eq!(json["sides"][0]["paneId"], "pane-1");
        assert_eq!(json["sides"][0]["working"], true);
        assert_eq!(json["files"][0]["path"], "src/auth.ts");
        assert!(json["advice"].is_null());
    }

    #[test]
    fn alert_id_is_order_independent_and_well_formed() {
        let ab = AlertId::of(&["fake_b-pane", "fake_a-pane"]);
        let ba = AlertId::of(&["fake_a-pane", "fake_b-pane"]);
        assert_eq!(ab, ba);
        assert!(well_formed_id(&ab), "bad id {ab}");
        assert_ne!(AlertId::of(&["fake_a-pane", "vendor_x-pane"]), ab);
    }

    #[test]
    fn level_orders_clash_as_the_loudest() {
        assert!(Level::Clash > Level::SharedFolder);
        assert!(Level::SharedFolder > Level::SameFile);
        assert_eq!([Level::SameFile, Level::Clash, Level::SharedFolder].iter().max(), Some(&Level::Clash));
    }

    #[test]
    fn every_limit_is_non_zero_and_ordered() {
        assert!(FIRST_POLL_MS > 0);
        assert!(POLL_MS > 0);
        assert!(POLL_MAX_MS > 0);
        assert!(BACKOFF_FACTOR > 0);
        assert!(TURN_END_DEBOUNCE_MS > 0);
        assert!(SLOW_LOOK_MS > 0);
        assert!(SLOW_LOOKS_TO_PARK > 0);
        assert!(TURN_END_ONLY_MS > 0);
        assert!(PROBE_TIMEOUT_MS > 0);
        assert!(REPO_CACHE_MS > 0);
        assert!(MAX_GROUPS > 0);
        assert!(MAX_PANES_PER_GROUP > 0);
        assert!(MAX_ALERTS > 0);
        assert!(MAX_SAMPLE_FILES > 0);
        assert!(MAX_STAT_FILES > 0);
        assert!(MAX_COMPARE_FILES > 0);
        assert!(MAX_FILES_SHOWN > 0);
        assert!(MAX_PATH_CHARS > 0);
        assert!(MAX_PATCH_BYTES > 0);
        assert!(MAX_HUNKS_PER_FILE > 0);
        assert!(CLASH_GAP_LINES > 0);
        assert!(CREATED_SLACK_MS > 0);
        assert!(RESOLVE_AFTER_MISSES > 0);
        assert!(SMALLER_RATIO_PCT > 0 && SMALLER_RATIO_PCT <= 100);
        assert!(SMALLER_MIN_GAP > 0);
        assert!(POLL_MS <= POLL_MAX_MS);
        assert!(FIRST_POLL_MS <= POLL_MAX_MS);
        assert!(MAX_FILES_SHOWN <= MAX_SAMPLE_FILES);
    }

    #[test]
    fn poll_ms_returns_the_consts_without_an_override() {
        if std::env::var_os("PM_FAKE_COLLISION_POLL_MS").is_none() {
            assert_eq!(poll_ms(), (FIRST_POLL_MS, POLL_MS));
        }
    }

    #[test]
    fn override_accepts_only_positive_whole_numbers() {
        assert_eq!(override_from(Some("250")), Some(250));
        assert_eq!(override_from(Some(" 40 ")), Some(40));
        assert_eq!(override_from(Some("0")), None);
        assert_eq!(override_from(Some("-5")), None);
        assert_eq!(override_from(Some("soon")), None);
        assert_eq!(override_from(None), None);
    }

    #[test]
    fn settings_default_is_on() {
        assert!(Settings::default().enabled);
    }
}
