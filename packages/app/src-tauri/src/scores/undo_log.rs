//! Append-only log of Undo, the one write of "Scores on your code" (SC-3).
//!
//! Nothing recorded an Undo before, so "how many turns were undone" is only known from the day this
//! log began. One JSON line per event in `<app data>/scores-undo-log.jsonl`: a `begin` line once, then
//! one `undo` line per restore (the pane id, two times). No prompt, reply, folder or file name is
//! written, and nothing is written per turn.
//!
//! The file never grows past `MAX_BYTES`: `append` writes nothing more once a line would not fit.
//! Reads are lenient: a damaged line is skipped, and only the last `MAX_LINES` lines are read.
//! Writes are best effort: the caller ignores errors, so Undo never breaks.

use super::model::UndoMark;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "scores-undo-log.jsonl";
/// Past this size `append` writes nothing more. Far more than a year of Undo clicks.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// `read` looks at the last lines only, so a big file cannot slow the page down.
const MAX_LINES: usize = 20_000;

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Entry {
    /// Written once, so "counting undo since" has a day.
    Begin { at_ms: u64 },
    /// Files were put back. Turns of `pane` that started at or after `since_ms` and ended by `at_ms` were undone.
    Undo { at_ms: u64, pane: String, since_ms: u64 },
}

pub fn default_path() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE_NAME)
}

/// Add one line. When the line would take the file past `MAX_BYTES` it is not written, and this
/// still returns `Ok`. Errors are returned for the caller to ignore: a log must never break Undo.
pub fn append(path: &Path, entry: &Entry) -> std::io::Result<()> {
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if size + line.len() as u64 > MAX_BYTES {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// Every readable line, oldest first, at most the last `MAX_LINES` lines. A damaged line is skipped.
pub fn read(path: &Path) -> Vec<Entry> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(MAX_LINES);
    lines[start..].iter().filter_map(|line| serde_json::from_str::<Entry>(line.trim()).ok()).collect()
}

/// When the log began: the earliest time on any line.
pub fn counting_since(entries: &[Entry]) -> Option<u64> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Begin { at_ms } | Entry::Undo { at_ms, .. } => *at_ms,
        })
        .min()
}

/// The Undo lines as marks, in file order. The `begin` line is not a mark.
pub fn marks(entries: &[Entry]) -> Vec<UndoMark> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Begin { .. } => None,
            Entry::Undo { at_ms, pane, since_ms } => {
                Some(UndoMark { pane: pane.clone(), since_ms: *since_ms, at_ms: *at_ms })
            }
        })
        .collect()
}

// Test seam: every `note_restore` call on this thread, as (pane, target kind, target time, now).
// A restore runs on the thread that asks for it, so a test reads exactly its own calls.
#[cfg(test)]
thread_local! {
    static NOTED_RESTORES: std::cell::RefCell<Vec<(String, String, u64, u64)>> = std::cell::RefCell::new(Vec::new());
}

/// The `note_restore` calls made on this thread since the last call, oldest first (tests only).
#[cfg(test)]
pub fn take_noted_restores() -> Vec<(String, String, u64, u64)> {
    NOTED_RESTORES.with(|noted| std::mem::take(&mut *noted.borrow_mut()))
}

/// Called by the checkpoints hook after a restore worked. Writes `begin` the first time, then one
/// `undo` line. A no-op under `cfg(test)` (it only records the call, see `take_noted_restores`), so
/// tests never touch the real app data folder.
///
/// `since_ms` is the checkpoint's time for every target kind (`before`, `after`, `answer`, `stop`,
/// `undo`): the turns that started after that point are the ones put back. So the kind is not needed.
pub fn note_restore(pane: &str, _target_kind: &str, target_at_ms: u64, now_ms: u64) {
    if cfg!(test) {
        #[cfg(test)]
        NOTED_RESTORES.with(|noted| {
            noted.borrow_mut().push((pane.to_string(), _target_kind.to_string(), target_at_ms, now_ms))
        });
        return;
    }
    let path = default_path();
    if !path.exists() {
        let _ = append(&path, &Entry::Begin { at_ms: now_ms });
    }
    let _ = append(&path, &Entry::Undo { at_ms: now_ms, pane: pane.to_string(), since_ms: target_at_ms });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in a new temp folder (the folder is created by `append`). Never the app data folder.
    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("pm-scores-undo-{}", uuid::Uuid::new_v4())).join(FILE_NAME)
    }

    fn undo(at_ms: u64, pane: &str, since_ms: u64) -> Entry {
        Entry::Undo { at_ms, pane: pane.into(), since_ms }
    }

    #[test]
    fn round_trip_keeps_order_and_fields() {
        let path = temp_path();
        let entries = vec![Entry::Begin { at_ms: 1_000 }, undo(2_000, "pane-a", 1_500), undo(3_000, "pane-b", 2_500)];
        for entry in &entries {
            append(&path, entry).unwrap();
        }
        assert_eq!(read(&path), entries);
    }

    #[test]
    fn lines_use_snake_case_event_names_and_no_other_text() {
        let path = temp_path();
        append(&path, &Entry::Begin { at_ms: 7 }).unwrap();
        append(&path, &undo(9, "p", 8)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "{\"event\":\"begin\",\"at_ms\":7}\n{\"event\":\"undo\",\"at_ms\":9,\"pane\":\"p\",\"since_ms\":8}\n"
        );
    }

    #[test]
    fn a_damaged_line_is_skipped() {
        let path = temp_path();
        append(&path, &Entry::Begin { at_ms: 1 }).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{not json\n\xff\xfe\n");
        // Missing `since_ms`: not a valid Undo line.
        bytes.extend_from_slice(b"{\"event\":\"undo\",\"at_ms\":5,\"pane\":\"p\"}\n");
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(format!("{}\n", serde_json::to_string(&undo(9, "p", 8)).unwrap()).as_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(read(&path), vec![Entry::Begin { at_ms: 1 }, undo(9, "p", 8)]);
    }

    #[test]
    fn read_of_a_missing_file_is_empty() {
        assert!(read(&temp_path()).is_empty());
    }

    #[test]
    fn read_looks_at_the_last_20_000_lines_only() {
        let path = temp_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = String::new();
        for i in 0..(MAX_LINES as u64 + 5) {
            text.push_str(&format!("{{\"event\":\"begin\",\"at_ms\":{i}}}\n"));
        }
        std::fs::write(&path, text).unwrap();
        let entries = read(&path);
        assert_eq!(entries.len(), MAX_LINES);
        assert_eq!(entries[0], Entry::Begin { at_ms: 5 });
    }

    #[test]
    fn append_stops_once_a_line_would_pass_the_size_cap() {
        let path = temp_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 100 bytes under the cap: a short `begin` line still fits (read skips the blank lines).
        std::fs::write(&path, vec![b'\n'; (MAX_BYTES - 100) as usize]).unwrap();
        append(&path, &Entry::Begin { at_ms: 1 }).unwrap();
        let after_fit = std::fs::metadata(&path).unwrap().len();
        assert!(after_fit <= MAX_BYTES);
        assert_eq!(read(&path), vec![Entry::Begin { at_ms: 1 }]);
        // Fill the rest of the way: the next line does not fit, so `append` writes nothing and says Ok.
        std::fs::write(&path, vec![b'\n'; (MAX_BYTES - 10) as usize]).unwrap();
        append(&path, &undo(2, "p", 1)).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_BYTES - 10);
        assert!(read(&path).is_empty());
    }

    #[test]
    fn counting_since_is_the_earliest_time_on_any_line() {
        assert_eq!(counting_since(&[]), None);
        assert_eq!(counting_since(&[undo(50, "p", 40), Entry::Begin { at_ms: 30 }]), Some(30));
    }

    #[test]
    fn marks_keep_undo_lines_and_ignore_begin() {
        let entries = vec![Entry::Begin { at_ms: 1 }, undo(20, "p1", 10), Entry::Begin { at_ms: 2 }, undo(40, "p2", 30)];
        assert_eq!(
            marks(&entries),
            vec![
                UndoMark { pane: "p1".into(), since_ms: 10, at_ms: 20 },
                UndoMark { pane: "p2".into(), since_ms: 30, at_ms: 40 },
            ]
        );
    }
}
