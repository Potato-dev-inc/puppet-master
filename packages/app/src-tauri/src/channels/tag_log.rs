//! Tag Your Agent: the record of what the bot answered in groups, for the Settings page.
//!
//! One record per question: the group's title, who asked (a display name), the question, the answer and
//! which agent answered. Only the last `KEEP` are kept, in `tag-bot-log.json` in the app data folder
//! (written privately), so other people's group messages do not pile up forever. The audit log gets a
//! counts-only line as well (`engine/group.rs`); the text lives only here. Question and answer are
//! redacted and cut before they reach this file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "tag-bot-log.json";
/// How many records are kept (the newest).
pub const KEEP: usize = 50;
/// The longest question and answer kept in a record.
pub const QUESTION_CHARS: usize = 400;
pub const ANSWER_CHARS: usize = 1200;

/// What happened to one question. `answer` is `None` when the agent could not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupAnswerRecord {
    pub at_ms: u64,
    pub group_title: String,
    pub asker: String,
    pub question: String,
    pub answer: Option<String>,
    pub agent: String,
    pub ok: bool,
}

pub fn path() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE)
}

/// The saved records, oldest first. A missing or unreadable file means none.
pub fn load_from(path: &Path) -> Vec<GroupAnswerRecord> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<GroupAnswerRecord>>(&text).ok())
        .unwrap_or_default()
}

/// Add one record and keep the newest `KEEP`.
pub fn append_to(path: &Path, record: &GroupAnswerRecord) -> Result<(), String> {
    let mut all = load_from(path);
    all.push(record.clone());
    if all.len() > KEEP {
        let extra = all.len() - KEEP;
        all.drain(..extra);
    }
    let text = serde_json::to_string(&all).map_err(|err| err.to_string())?;
    crate::secure_file::write_private(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-tag-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(FILE)
    }

    fn record(n: u64) -> GroupAnswerRecord {
        GroupAnswerRecord {
            at_ms: n,
            group_title: "Test Crew".into(),
            asker: "Sam".into(),
            question: format!("q{n}"),
            answer: Some(format!("a{n}")),
            agent: "Some Agent".into(),
            ok: true,
        }
    }

    #[test]
    fn a_missing_or_broken_file_means_no_records() {
        let path = temp_path();
        assert!(load_from(&path).is_empty());
        std::fs::write(&path, "{nope").unwrap();
        assert!(load_from(&path).is_empty());
    }

    #[test]
    fn records_round_trip_and_only_the_newest_are_kept() {
        let path = temp_path();
        for n in 0..(KEEP as u64 + 7) {
            append_to(&path, &record(n)).unwrap();
        }
        let all = load_from(&path);
        assert_eq!(all.len(), KEEP);
        assert_eq!(all.first().unwrap().at_ms, 7);
        assert_eq!(all.last().unwrap().at_ms, KEEP as u64 + 6);
        assert_eq!(all.last().unwrap(), &record(KEEP as u64 + 6));
    }
}
