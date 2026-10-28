//! The Review Desk data model (SPEC n33 section 3.2). Plain types only: no I/O, no git, no model.
//!
//! Every fact the order is made from lives in [`Facts`]; the order itself is `risk.rs`. An [`ItemKey`]
//! is the only way an action names an item, so it is parsed strictly here (S10).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Only work that finished in the last 14 days.
pub const WINDOW_MS: u64 = 14 * 24 * 3_600_000;
pub const ITEMS_CAP: usize = 200;
/// "Big change" at >= 20 files ...
pub const BIG_FILES: u32 = 20;
/// ... or >= 400 changed lines (added + deleted).
pub const BIG_LINES: u32 = 400;
/// Patch scan limit; equal to `issue_intake::pushcheck::DIFF_MAX_BYTES`.
pub const PATCH_SCAN_MAX: usize = 256 * 1024;
pub const DETAIL_PATCH_MAX: usize = 512 * 1024;
/// Per-copy facts cache lifetime.
pub const FACTS_TTL_MS: u64 = 60_000;
pub const TITLE_MAX: usize = 120;
/// File names shown in one reason sentence.
pub const NAMES_SHOWN: usize = 3;
pub const MARKS_CAP: usize = 2_000;
/// The person's note in Ask to fix.
pub const NOTE_MAX: usize = 1_000;

/// Longest key the parser accepts (a copy handle is at most 80 characters).
const KEY_MAX: usize = 200;
const TOKEN_MAX: usize = 80;

/// Where a row comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Copy,
    Mission,
    Night,
    Issue,
    Folder,
}

/// `copy:<handle>` | `night:<task id>` | `issue:<run id>` | `folder:<pane id>:<first turn>-<last turn>`
/// (a mission copy is `copy:mission-<id>`). Parsed strictly; the only way an action names an item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemKey(String);

impl ItemKey {
    /// Accepts only the four exact forms. Handles and pane ids are `[A-Za-z0-9._-]{1,80}` (not `.` or `..`),
    /// night task ids are `nt-` + 8 hex, issue run ids are `ir-` + 12 hex, turns are `u32` with first <= last.
    pub fn parse(raw: &str) -> Option<ItemKey> {
        if raw.len() > KEY_MAX || raw.chars().any(char::is_control) {
            return None;
        }
        let ok = if let Some(handle) = raw.strip_prefix("copy:") {
            valid_token(handle)
        } else if let Some(id) = raw.strip_prefix("night:") {
            valid_hex_id(id, "nt-", 8)
        } else if let Some(id) = raw.strip_prefix("issue:") {
            valid_hex_id(id, "ir-", 12)
        } else if let Some(rest) = raw.strip_prefix("folder:") {
            valid_folder(rest)
        } else {
            false
        };
        ok.then(|| ItemKey(raw.to_string()))
    }

    /// `copy` | `night` | `issue` | `folder`. Only for the UI's grouping; never used to act.
    pub fn source_hint(&self) -> &'static str {
        if self.0.starts_with("copy:") {
            "copy"
        } else if self.0.starts_with("night:") {
            "night"
        } else if self.0.starts_with("issue:") {
            "issue"
        } else {
            "folder"
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= TOKEN_MAX
        && raw != "."
        && raw != ".."
        && raw.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_hex_id(raw: &str, prefix: &str, hex_len: usize) -> bool {
    match raw.strip_prefix(prefix) {
        Some(rest) => rest.len() == hex_len && rest.bytes().all(|b| b.is_ascii_hexdigit()),
        None => false,
    }
}

fn valid_folder(rest: &str) -> bool {
    let Some((pane, range)) = rest.split_once(':') else {
        return false;
    };
    let Some((first, last)) = range.split_once('-') else {
        return false;
    };
    if !valid_token(pane) || !all_digits(first) || !all_digits(last) {
        return false;
    }
    match (first.parse::<u32>(), last.parse::<u32>()) {
        (Ok(first), Ok(last)) => first <= last,
        _ => false,
    }
}

fn all_digits(raw: &str) -> bool {
    !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit())
}

/// Turns the redaction and clean-up step into a value the model can take without knowing the live layer.
/// The live layer builds it from `redact::strict_with` + home-to-`~` + [`strip_controls`]; tests pass a fake.
pub struct Clean {
    pub text: Box<dyn Fn(&str) -> String>,
}

impl Clean {
    pub fn apply(&self, raw: &str) -> String {
        (self.text)(raw)
    }
}

/// Removes control characters and bidi overrides (S4). Keeps ordinary spaces and printable text.
pub fn strip_controls(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() && !matches!(*c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRow {
    pub path: String,
    /// `A` added, `M` modified, `D` deleted.
    pub status: char,
    pub added: u32,
    pub deleted: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckFacts {
    pub passed: u32,
    pub total: u32,
    /// Check names, at most 5.
    pub failed: Vec<String>,
}

/// Every recorded fact the order is made from. No opinions in here.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Facts {
    /// `None` = Puppet Master could not see the changes.
    pub files: Option<Vec<FileRow>>,
    pub files_total: u32,
    pub added: u32,
    pub deleted: u32,
    /// Tamper blocking sentences (missions, Night Shift).
    pub weakened: Vec<String>,
    /// `None` = no checks were run.
    pub checks: Option<CheckFacts>,
    pub secret_files: Vec<String>,
    pub ci_files: Vec<String>,
    pub secret_lines: Vec<(String, u32)>,
    pub home_lines: Vec<(String, u32)>,
    /// `false`: patch too big or not read (then only names were checked).
    pub lines_scanned: bool,
}

/// Risk flags, in table order (`risk.rs`). Derived `Ord` follows the variant order, i.e. the tier order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    TestsWeakened,
    Sensitive,
    ChecksFailed,
    TestsChanged,
    PackagesChanged,
    Big,
    Unknown,
}

/// Words: "High risk" / "Check first" / "Low risk".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    High,
    Check,
    Low,
}

/// One plain sentence that says why a row is where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reason {
    pub flag: Flag,
    pub sentence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Risk {
    /// 1..=7, see the table in `risk.rs`.
    pub tier: u8,
    pub level: Level,
    /// Tier order.
    pub reasons: Vec<Reason>,
}

/// What Keep does for an item. `No` carries the plain reason the button is disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Keep {
    Merge { project: String, handle: String, into: String },
    NightKeep { task_id: String, into: String },
    IssuePush { run_id: String, reasons: Vec<String> },
    Mark { pane_id: String, through_ms: u64 },
    No { why: String },
}

/// What Throw away does for an item. `No` carries the plain reason the button is disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Throw {
    Discard { project: String, handle: String },
    NightThrow { task_id: String },
    IssueThrow { run_id: String },
    Restore { pane_id: String, checkpoint_id: String, files: u32 },
    No { why: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub root_pane_id: String,
    /// `<root title> and helpers`.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub key: ItemKey,
    pub source: Source,
    /// sha256 hex[..12] of (key, files rows, weakened, checks, flags); see `risk::fingerprint`.
    pub fingerprint: String,
    /// Redacted, first line, control characters removed, at most `TITLE_MAX`.
    pub title: String,
    /// From the registry (`crews::live::label_of`), never typed.
    pub agent_label: String,
    pub account: Option<String>,
    /// "Separate copy of main" | "Mission" | "Night Shift" | "Issue #12" | "In your folder".
    pub where_label: String,
    /// Local only; never sent to the phone.
    pub project: String,
    /// A live pane, else `None`.
    pub pane_id: Option<String>,
    pub finished_at_ms: u64,
    pub turns: u32,
    pub group: Option<Group>,
    pub facts: Facts,
    pub risk: Risk,
    pub keep: Keep,
    pub throw: Throw,
    /// `Ok(pane id)` or `Err(why not)`.
    pub fix: Result<String, String>,
}

/// `<app data>/review-desk/marks.json`: which turns the person already looked at (folder items).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Marks {
    pub version: u32,
    pub panes: BTreeMap<String, Mark>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub through_ms: u64,
    pub at_ms: u64,
    pub how: How,
    /// Previous `through_ms`, for Undo.
    pub before: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    Kept,
    ThrownAway,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_parse_accepts_each_form() {
        for raw in [
            "copy:h1",
            "copy:mission-abc.1_2",
            "night:nt-0123abcd",
            "issue:ir-0123456789ab",
            "folder:pane-1:0-0",
            "folder:p:5-5",
        ] {
            let key = ItemKey::parse(raw).unwrap_or_else(|| panic!("should accept {raw}"));
            assert_eq!(key.as_str(), raw);
        }
    }

    #[test]
    fn key_parse_refuses_bad_forms() {
        let long = format!("copy:{}", "a".repeat(300));
        for raw in [
            "copy:../x",
            "copy:",
            "copy:.",
            "copy:..",
            "night:nt-1",
            "issue:ir-zz",
            "folder:p:5-2",
            "folder:p:1-x",
            "folder:p:+1-2",
            "folder:p:1",
            "folder:../p:1-2",
            "other:h1",
            "",
            long.as_str(),
        ] {
            assert!(ItemKey::parse(raw).is_none(), "should refuse {raw}");
        }
    }

    #[test]
    fn key_source_hint_and_text() {
        let copy = ItemKey::parse("copy:mission-7").unwrap();
        assert_eq!(copy.source_hint(), "copy");
        assert_eq!(ItemKey::parse("night:nt-0123abcd").unwrap().source_hint(), "night");
        assert_eq!(ItemKey::parse("issue:ir-0123456789ab").unwrap().source_hint(), "issue");
        assert_eq!(ItemKey::parse("folder:p:1-2").unwrap().source_hint(), "folder");
        assert_eq!(copy.as_str(), "copy:mission-7");
    }

    #[test]
    fn serde_tags_are_stable() {
        assert_eq!(
            serde_json::to_value(Keep::Merge { project: "p".into(), handle: "h".into(), into: "main".into() }).unwrap(),
            json!({"kind": "merge", "project": "p", "handle": "h", "into": "main"})
        );
        assert_eq!(
            serde_json::to_value(Keep::NightKeep { task_id: "nt-0123abcd".into(), into: "main".into() }).unwrap(),
            json!({"kind": "night_keep", "task_id": "nt-0123abcd", "into": "main"})
        );
        assert_eq!(
            serde_json::to_value(Keep::IssuePush { run_id: "ir-0123456789ab".into(), reasons: vec!["x".into()] })
                .unwrap(),
            json!({"kind": "issue_push", "run_id": "ir-0123456789ab", "reasons": ["x"]})
        );
        assert_eq!(
            serde_json::to_value(Keep::Mark { pane_id: "p".into(), through_ms: 5 }).unwrap(),
            json!({"kind": "mark", "pane_id": "p", "through_ms": 5})
        );
        assert_eq!(
            serde_json::to_value(Keep::No { why: "held".into() }).unwrap(),
            json!({"kind": "no", "why": "held"})
        );
        assert_eq!(
            serde_json::to_value(Throw::Discard { project: "p".into(), handle: "h".into() }).unwrap(),
            json!({"kind": "discard", "project": "p", "handle": "h"})
        );
        assert_eq!(
            serde_json::to_value(Throw::NightThrow { task_id: "nt-0123abcd".into() }).unwrap(),
            json!({"kind": "night_throw", "task_id": "nt-0123abcd"})
        );
        assert_eq!(
            serde_json::to_value(Throw::IssueThrow { run_id: "ir-0123456789ab".into() }).unwrap(),
            json!({"kind": "issue_throw", "run_id": "ir-0123456789ab"})
        );
        assert_eq!(
            serde_json::to_value(Throw::Restore { pane_id: "p".into(), checkpoint_id: "c1".into(), files: 2 }).unwrap(),
            json!({"kind": "restore", "pane_id": "p", "checkpoint_id": "c1", "files": 2})
        );
        assert_eq!(
            serde_json::to_value(Throw::No { why: "busy".into() }).unwrap(),
            json!({"kind": "no", "why": "busy"})
        );
    }

    #[test]
    fn serde_flag_level_and_source_names_are_stable() {
        assert_eq!(
            serde_json::to_value([
                Flag::TestsWeakened,
                Flag::Sensitive,
                Flag::ChecksFailed,
                Flag::TestsChanged,
                Flag::PackagesChanged,
                Flag::Big,
                Flag::Unknown,
            ])
            .unwrap(),
            json!([
                "tests_weakened",
                "sensitive",
                "checks_failed",
                "tests_changed",
                "packages_changed",
                "big",
                "unknown"
            ])
        );
        assert_eq!(
            serde_json::to_value([Level::High, Level::Check, Level::Low]).unwrap(),
            json!(["high", "check", "low"])
        );
        assert_eq!(
            serde_json::to_value([Source::Copy, Source::Mission, Source::Night, Source::Issue, Source::Folder])
                .unwrap(),
            json!(["copy", "mission", "night", "issue", "folder"])
        );
        assert_eq!(serde_json::to_value([How::Kept, How::ThrownAway]).unwrap(), json!(["kept", "thrown_away"]));
        assert!(Flag::TestsWeakened < Flag::Unknown, "tier order follows the variant order");
    }

    #[test]
    fn clean_uses_the_injected_text_function() {
        let clean = Clean { text: Box::new(|s| format!("[{s}]")) };
        assert_eq!(clean.apply("cart"), "[cart]");
    }

    #[test]
    fn strip_controls_removes_bidi_and_control_chars() {
        assert_eq!(strip_controls("a\u{202E}b\u{7}c\nd e"), "abcd e");
        assert_eq!(strip_controls("tests/cart.test.ts"), "tests/cart.test.ts");
    }
}
