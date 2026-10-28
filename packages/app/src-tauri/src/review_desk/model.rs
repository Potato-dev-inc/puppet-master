//! The Review Desk data model (SPEC-n33 section 3.2), the part the risk table needs: the limits,
//! `ItemKey`, the facts, the flags and levels, the reasons and `Item`. Marks, the `Clean` hook and the
//! error codes come with the rest of the model.

use serde::{Deserialize, Serialize};

pub const ITEMS_CAP: usize = 200;                  // the list shows at most this many items
pub const BIG_FILES: u32 = 20;                     // "Big change" at >= 20 files ...
pub const BIG_LINES: u32 = 400;                    // ... or >= 400 changed lines (added + deleted)
pub const NAMES_SHOWN: usize = 3;                  // file names in one reason sentence

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum Source { Copy, Mission, Night, Issue, Folder }

/// `copy:<handle>` | `night:<task id>` | `issue:<run id>` | `folder:<pane id>:<first turn>-<last turn>`.
/// Parsed strictly; the only way an action names an item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemKey(String);

impl ItemKey {
    /// Handles and pane ids: `[A-Za-z0-9._-]`, 1 to 80 characters. Night: `nt-` + 8 hex. Issue: `ir-` + 12 hex.
    /// Turns: decimal `u32`, first <= last.
    pub fn parse(s: &str) -> Option<ItemKey> {
        let ok = if let Some(handle) = s.strip_prefix("copy:") {
            plain_name(handle)
        } else if let Some(id) = s.strip_prefix("night:") {
            hex_id(id, "nt-", 8)
        } else if let Some(id) = s.strip_prefix("issue:") {
            hex_id(id, "ir-", 12)
        } else if let Some(rest) = s.strip_prefix("folder:") {
            folder_rest(rest)
        } else {
            false
        };
        ok.then(|| ItemKey(s.to_string()))
    }

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

fn plain_name(s: &str) -> bool {
    (1..=80).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn hex_id(id: &str, prefix: &str, digits: usize) -> bool {
    id.strip_prefix(prefix).is_some_and(|hex| hex.len() == digits && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn folder_rest(rest: &str) -> bool {
    let Some((pane, turns)) = rest.split_once(':') else { return false };
    let Some((first, last)) = turns.split_once('-') else { return false };
    let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match (number(first).then(|| first.parse::<u32>()), number(last).then(|| last.parse::<u32>())) {
        (Some(Ok(a)), Some(Ok(b))) => plain_name(pane) && a <= b,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRow { pub path: String, pub status: char /* 'A' 'M' 'D' */, pub added: u32, pub deleted: u32, pub binary: bool }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckFacts { pub passed: u32, pub total: u32, pub failed: Vec<String> /* check names, <= 5 */ }

/// Every recorded fact the order is made from. No opinions in here.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Facts {
    pub files: Option<Vec<FileRow>>,        // None = Puppet Master could not see the changes
    pub files_total: u32, pub added: u32, pub deleted: u32,
    pub weakened: Vec<String>,              // tamper blocking sentences (missions, Night Shift)
    pub checks: Option<CheckFacts>,         // None = no checks were run
    pub secret_files: Vec<String>, pub ci_files: Vec<String>,
    pub secret_lines: Vec<(String, u32)>, pub home_lines: Vec<(String, u32)>,
    pub lines_scanned: bool,                // false: patch too big / not read (then only names were checked)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum Flag { TestsWeakened, Sensitive, ChecksFailed, TestsChanged, PackagesChanged, Big, Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum Level { High, Check, Low }          // words: "High risk" / "Check first" / "Low risk"

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reason { pub flag: Flag, pub sentence: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Risk { pub tier: u8 /* 1..=7 */, pub level: Level, pub reasons: Vec<Reason> /* tier order */ }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum Keep {
    Merge { project: String, handle: String, into: String },
    NightKeep { task_id: String, into: String },
    IssuePush { run_id: String, reasons: Vec<String> },
    Mark { pane_id: String, through_ms: u64 },
    No { why: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum Throw {
    Discard { project: String, handle: String },
    NightThrow { task_id: String },
    IssueThrow { run_id: String },
    Restore { pane_id: String, checkpoint_id: String, files: u32 },
    No { why: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group { pub root_pane_id: String, pub label: String /* "<root title> and helpers" */ }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub key: ItemKey,
    pub source: Source,
    pub fingerprint: String,       // sha256 hex[..12] of (key, facts); see risk::fingerprint
    pub title: String,             // redacted, first line, control chars removed, <= TITLE_MAX
    pub agent_label: String,       // from the registry (crews::live::label_of), never typed
    pub account: Option<String>,
    pub where_label: String,       // "Separate copy of main" | "Mission" | "Night Shift" | "Issue #12" | "In your folder"
    pub project: String,           // local only; never sent to the phone
    pub pane_id: Option<String>,   // a live pane, else None
    pub finished_at_ms: u64,
    pub turns: u32,
    pub group: Option<Group>,
    pub facts: Facts,
    pub risk: Risk,
    pub keep: Keep,
    pub throw: Throw,
    pub fix: Result<String /* pane id */, String /* why not */>,
}
