//! APR-1/4/6: the approval store (`<app data>/agent-approvals.json`).
//!
//! Same persistence contract as `standby/config.rs`: a missing or unreadable file means "nothing
//! approved" (never a panic, never an error to the caller), and saves go through a temp file and a
//! rename. A record binds a file path to the fingerprint the user approved; when the fingerprint
//! the loader computes no longer matches, the definition is `approval_stale`. The record can be
//! removed to revoke it (APR-6). Nothing here is reachable from the MCP surface (APR-7); the HTTP
//! route that writes records arrives with T19.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::standby::key::canonical_project;

pub const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub source: String,
    /// Absolute path of the definition file as it was approved.
    pub path: String,
    pub fingerprint: String,
    /// Absolute resolved binary path at approval time (pinned at spawn by a later phase).
    pub binary: String,
    /// RFC 3339, UTC.
    pub approved_at: String,
    /// The headless / visible argv the user saw when approving (APR-3), kept so a later change
    /// can be shown as a diff against what was approved (P5-4). Absent in older records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headless_argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tui_argv: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalState {
    /// No record for this file.
    Unapproved,
    /// A record exists and its fingerprint equals the current one.
    Approved,
    /// A record exists but the definition's fingerprint has changed since.
    Stale,
}

impl ApprovalState {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalState::Unapproved => "unapproved",
            ApprovalState::Approved => "approved",
            ApprovalState::Stale => "approval_stale",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct StoreFile {
    version: u32,
    approvals: Vec<Approval>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Approvals {
    approvals: Vec<Approval>,
}

pub fn default_path() -> PathBuf {
    crate::app_paths::app_data_dir().join("agent-approvals.json")
}

fn key(path: &str) -> String {
    canonical_project(path)
}

impl Approvals {
    pub fn records(&self) -> &[Approval] {
        &self.approvals
    }

    /// Missing, unreadable, malformed or wrong-version file: empty set (everything unapproved).
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else { return Self::default() };
        match serde_json::from_str::<StoreFile>(&text) {
            Ok(file) if file.version == FILE_VERSION => Self { approvals: file.approvals },
            Ok(file) => {
                tracing::warn!(version = file.version, "agent approvals file has an unsupported version; treating nothing as approved");
                Self::default()
            }
            Err(error) => {
                tracing::warn!(%error, "agent approvals file unreadable; treating nothing as approved");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let file = StoreFile { version: FILE_VERSION, approvals: self.approvals.clone() };
        let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|error| error.to_string())?;
        std::fs::rename(&tmp, path).map_err(|error| error.to_string())
    }

    pub fn get(&self, path: &Path) -> Option<&Approval> {
        let wanted = key(&path.to_string_lossy());
        self.approvals.iter().find(|a| key(&a.path) == wanted)
    }

    pub fn state(&self, path: &Path, fingerprint: &str) -> ApprovalState {
        match self.get(path) {
            None => ApprovalState::Unapproved,
            Some(record) if record.fingerprint == fingerprint => ApprovalState::Approved,
            Some(_) => ApprovalState::Stale,
        }
    }

    /// Record (or replace the record of the same file) an approval.
    pub fn approve(&mut self, approval: Approval) {
        let wanted = key(&approval.path);
        self.approvals.retain(|a| key(&a.path) != wanted);
        self.approvals.push(approval);
    }

    /// Remove every record of agent `id`; true when something was removed.
    pub fn revoke(&mut self, id: &str) -> bool {
        let before = self.approvals.len();
        self.approvals.retain(|a| a.id != id);
        self.approvals.len() != before
    }
}

/// Now as RFC 3339 UTC (`2026-10-09T12:00:00Z`).
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    rfc3339_from_unix(secs)
}

fn rfc3339_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod time_tests {
    use super::rfc3339_from_unix;

    #[test]
    fn formats_known_instants() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_from_unix(1_760_000_000), "2025-10-09T08:53:20Z");
    }
}
