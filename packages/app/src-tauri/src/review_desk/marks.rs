//! "Looked at" marks for in-folder turns (`<app data>/review-desk/marks.json`). Private, atomic, and
//! never inside a project folder, so nothing can be committed by accident. A pane id is a map key
//! only, never a path. A corrupt file counts as no marks: the desk then shows the turns again.

use std::fs;
use std::path::{Path, PathBuf};

use super::model::{How, Mark, Marks, MARKS_CAP};
use crate::secure_file;

pub const MARKS_FILE: &str = "marks.json";
pub const MARKS_VERSION: u32 = 1;

/// The folder the marks live in: `<app data>/review-desk`.
#[cfg(not(test))]
#[allow(dead_code)] // the live layer (RD-6) reads it
pub fn marks_root() -> PathBuf {
    crate::app_paths::app_data_dir().join("review-desk")
}

impl Marks {
    pub fn empty() -> Marks {
        Marks { version: MARKS_VERSION, panes: Default::default() }
    }

    /// Reads `<root>/marks.json`. A missing or unreadable file gives empty marks.
    pub fn load(root: &Path) -> Marks {
        let path = root.join(MARKS_FILE);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Marks::empty();
        };
        match serde_json::from_str::<Marks>(&raw) {
            Ok(marks) if marks.version == MARKS_VERSION => marks,
            _ => Marks::empty(),
        }
    }

    /// Writes `<root>/marks.json` atomically with private permissions.
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|err| format!("encode marks: {err}"))?;
        secure_file::write_private(&root.join(MARKS_FILE), &json)
    }

    pub fn get(&self, pane: &str) -> Option<&Mark> {
        self.panes.get(pane)
    }

    /// Marks `pane` as looked at through `through_ms`. Keeps the old `through_ms` in `before` for Undo.
    /// Over `MARKS_CAP`, the entries with the oldest `at_ms` go first.
    pub fn set(&mut self, pane: &str, through_ms: u64, how: How, now: u64) {
        let before = self.panes.get(pane).map(|mark| mark.through_ms);
        self.panes.insert(pane.to_string(), Mark { through_ms, at_ms: now, how, before });
        while self.panes.len() > MARKS_CAP {
            let oldest = self
                .panes
                .iter()
                .min_by(|a, b| a.1.at_ms.cmp(&b.1.at_ms).then_with(|| a.0.cmp(b.0)))
                .map(|(key, _)| key.clone());
            match oldest {
                Some(key) => {
                    self.panes.remove(&key);
                }
                None => break,
            }
        }
    }

    /// Steps back one mark. With no earlier `through_ms`, the mark is removed. Returns `false` when
    /// the pane had no mark.
    pub fn undo(&mut self, pane: &str) -> bool {
        let Some(mark) = self.panes.get(pane).cloned() else {
            return false;
        };
        match mark.before {
            None => {
                self.panes.remove(pane);
            }
            Some(previous) => {
                self.panes.insert(
                    pane.to_string(),
                    Mark { through_ms: previous, at_ms: mark.at_ms, how: mark.how, before: None },
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test, under the OS temp dir. Removed at the end of each test.
    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pm-rd-marks-{}-{name}", std::process::id()))
    }

    #[test]
    fn set_and_undo_step_back_then_remove() {
        let mut marks = Marks::empty();
        marks.set("p1", 100, How::Kept, 1_000);
        assert_eq!(marks.get("p1").map(|m| m.through_ms), Some(100));
        assert_eq!(marks.get("p1").and_then(|m| m.before), None);

        marks.set("p1", 200, How::ThrownAway, 2_000);
        let mark = marks.get("p1").unwrap();
        assert_eq!((mark.through_ms, mark.before, mark.how, mark.at_ms), (200, Some(100), How::ThrownAway, 2_000));

        assert!(marks.undo("p1"));
        let mark = marks.get("p1").unwrap();
        assert_eq!((mark.through_ms, mark.before), (100, None));

        assert!(marks.undo("p1"));
        assert!(marks.get("p1").is_none(), "undo to nothing removes the mark");
        assert!(!marks.undo("p1"), "no mark, nothing to undo");
    }

    #[test]
    fn cap_drops_the_oldest_marks() {
        let mut marks = Marks::empty();
        for i in 0..=MARKS_CAP {
            marks.set(&format!("p{i}"), 1, How::Kept, i as u64 + 1);
        }
        assert_eq!(marks.panes.len(), MARKS_CAP);
        assert!(marks.get("p0").is_none(), "the oldest mark is dropped");
        assert!(marks.get(&format!("p{MARKS_CAP}")).is_some(), "the newest mark stays");
    }

    #[test]
    fn save_and_load_round_trip() {
        let root = scratch("round-trip");
        let _ = fs::remove_dir_all(&root);
        let mut marks = Marks::empty();
        marks.set("pane-a", 42, How::Kept, 7);
        marks.save(&root).unwrap();
        assert_eq!(Marks::load(&root), marks);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_file_loads_as_empty_marks() {
        let root = scratch("corrupt");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MARKS_FILE), "{ not json").unwrap();
        assert_eq!(Marks::load(&root), Marks::empty());
        fs::write(root.join(MARKS_FILE), r#"{"version": 99, "panes": {}}"#).unwrap();
        assert_eq!(Marks::load(&root), Marks::empty(), "another version is not trusted");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_is_written_outside_the_root() {
        let base = scratch("confined");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let root = base.join("review-desk");
        let mut marks = Marks::empty();
        marks.set("../escape", 1, How::Kept, 1);
        marks.save(&root).unwrap();
        let outside: Vec<String> = fs::read_dir(&base)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(outside, vec!["review-desk".to_string()], "only the root folder was created in the base");
        let inside: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(inside, vec![MARKS_FILE.to_string()], "only marks.json, no temp file left behind");
        let _ = fs::remove_dir_all(&base);
    }
}
