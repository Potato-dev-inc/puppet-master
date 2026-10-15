//! Reading the log back: the tip to continue from, the recent lines for the Settings list, and the
//! chain check.

use super::{entry_hash, Entry, Head, Tip, GENESIS, HEAD_FILE};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Where the check failed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Broken {
    /// The month file, for example `2026-10.jsonl`.
    pub file: String,
    /// 1-based line number inside that file (0 when the problem is the missing tail).
    pub line: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VerifyReport {
    pub ok: bool,
    /// Lines that checked out before the first problem (or all of them when `ok`).
    pub entries: u64,
    pub broken: Option<Broken>,
}

/// The month files in order, oldest first.
fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn read_head(dir: &Path) -> Option<Head> {
    serde_json::from_slice(&std::fs::read(dir.join(HEAD_FILE)).ok()?).ok()
}

/// The newest line's position, or the head file, or the start of a new chain.
pub(super) fn load_tip(dir: &Path) -> Tip {
    let last_line = log_files(dir)
        .last()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string));
    if let Some(entry) = last_line.and_then(|l| serde_json::from_str::<Entry>(&l).ok()) {
        return Tip { seq: entry.seq, hash: entry.hash, ts_ms: entry.ts_ms };
    }
    match read_head(dir) {
        Some(head) => Tip { seq: head.seq, hash: head.hash, ts_ms: 0 },
        None => Tip { seq: 0, hash: GENESIS.to_string(), ts_ms: 0 },
    }
}

/// The newest `limit` readable lines, newest first. A line that does not parse is skipped here;
/// `verify_dir` is what reports it.
pub(super) fn read_recent(dir: &Path, limit: usize) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for path in log_files(dir).into_iter().rev() {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines().rev() {
            if let Ok(entry) = serde_json::from_str::<Entry>(line) {
                out.push(entry);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out
}

fn broken(file: &Path, line: usize, reason: impl Into<String>, entries: u64) -> VerifyReport {
    VerifyReport {
        ok: false,
        entries,
        broken: Some(Broken {
            file: file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            line,
            reason: reason.into(),
        }),
    }
}

/// Check the whole chain in `dir`: line format, sequence numbers, `prev` links, each line's own
/// hash, and finally the head file. An empty or missing folder is a valid, empty log.
pub fn verify_dir(dir: &Path) -> VerifyReport {
    let mut prev_hash = GENESIS.to_string();
    let mut expected_seq = 1_u64;
    let mut checked = 0_u64;
    for path in log_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            return broken(&path, 0, "the file cannot be read", checked);
        };
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let at = index + 1;
            let Ok(entry) = serde_json::from_str::<Entry>(line) else {
                return broken(&path, at, "the line is not a valid entry", checked);
            };
            if entry.seq != expected_seq {
                return broken(&path, at, format!("expected entry {expected_seq}, found {}: a line was removed or added", entry.seq), checked);
            }
            if entry.prev != prev_hash {
                return broken(&path, at, "the link to the previous line does not match: an earlier line was changed or removed", checked);
            }
            if entry.hash != entry_hash(&entry) {
                return broken(&path, at, "the line was changed after it was written", checked);
            }
            prev_hash = entry.hash;
            expected_seq += 1;
            checked += 1;
        }
    }
    if let Some(head) = read_head(dir) {
        if head.seq != expected_seq - 1 || head.hash != prev_hash {
            let file = log_files(dir).last().cloned().unwrap_or_else(|| dir.join(HEAD_FILE));
            return broken(&file, 0, "lines at the end of the log are missing", checked);
        }
    }
    VerifyReport { ok: true, entries: checked, broken: None }
}
