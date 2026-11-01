//! The session record: what survives an app restart for each chat conversation.
//!
//! One record per conversation: which agent, the agent's own session id, the folder, how it is run, and the
//! last state it was in. The agent keeps the conversation itself (its own session files); this is only the
//! pointer back to it. Kept in `<app data>/acp/conversations.json`, written atomically.
//!
//! The file is read record by record: one record this build cannot read (written by a newer build) is
//! skipped and the others still load. Notices (history lost, handed over) live with the record so a viewer
//! that connects later still sees them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::lane::RunLane;

const FILE: &str = "conversations.json";
const FILE_VERSION: u32 = 1;

/// Records kept, and notices kept per record. The oldest go first.
pub const MAX_RECORDS: usize = 200;
pub const MAX_NOTICES: usize = 20;

/// Where the conversation was when the app last wrote the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LastState {
    /// The chat has it (the adapter may be napping): brought back lazily after a restart.
    Live,
    /// Handed to a terminal pane. After a restart the pane is looked for; if it is gone the chat takes it back.
    Terminal,
    /// The person closed it. Not brought back.
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// The agent could not bring the earlier conversation back; a new one was started.
    HistoryLost,
    HandedToTerminal,
    BackInChat,
    /// The terminal pane ended by itself and the chat has the conversation again.
    TerminalEnded,
    /// The conversation could not be brought back after a restart (the agent is no longer available).
    RestoreFailed,
}

/// A line the chat shows between messages. Plain words; no path and no chat text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
    pub at_ms: u64,
    #[serde(default)]
    pub seen: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationRecord {
    /// Ours: the key of the conversation in the app (also the key of its `AcpSession`).
    pub id: String,
    pub agent: String,
    /// The agent's own session id. `None` until the first start has created it.
    pub session_id: Option<String>,
    pub cwd: String,
    pub lane: RunLane,
    pub last_known_state: LastState,
    /// The terminal pane that has the conversation while `last_known_state` is `Terminal`.
    #[serde(default)]
    pub terminal_pane: Option<String>,
    #[serde(default)]
    pub notices: Vec<Notice>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

impl ConversationRecord {
    pub fn push_notice(&mut self, kind: NoticeKind, text: impl Into<String>, at_ms: u64) {
        self.notices.push(Notice { kind, text: text.into(), at_ms, seen: false });
        let extra = self.notices.len().saturating_sub(MAX_NOTICES);
        self.notices.drain(..extra);
        self.updated_ms = at_ms;
    }
}

#[derive(Debug, Clone)]
pub struct RecordStore {
    path: PathBuf,
}

impl RecordStore {
    pub fn at(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_path_buf() }
    }

    pub fn in_app_data() -> Self {
        Self::at(crate::app_paths::app_data_dir().join("acp").join(FILE))
    }

    /// Every record this build can read. A missing or broken file is an empty list.
    pub fn load(&self) -> Vec<ConversationRecord> {
        let Ok(text) = std::fs::read_to_string(&self.path) else { return Vec::new() };
        let Ok(file) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
        let Some(items) = file.get("conversations").and_then(Value::as_array) else { return Vec::new() };
        items.iter().filter_map(|item| serde_json::from_value(item.clone()).ok()).collect()
    }

    pub fn save(&self, records: &[ConversationRecord]) -> Result<(), String> {
        let skip = records.len().saturating_sub(MAX_RECORDS);
        let body = serde_json::json!({ "version": FILE_VERSION, "conversations": &records[skip..] });
        let text = serde_json::to_vec_pretty(&body).map_err(|e| e.to_string())?;
        crate::settings_patch::write_atomic(&self.path, &text).map_err(|e| format!("could not save: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str) -> ConversationRecord {
        ConversationRecord {
            id: id.into(),
            agent: "codex".into(),
            session_id: Some("0123456789abcdef".into()),
            cwd: "D:\\work".into(),
            lane: RunLane::Acp,
            last_known_state: LastState::Live,
            terminal_pane: None,
            notices: Vec::new(),
            created_ms: 1,
            updated_ms: 1,
        }
    }

    fn temp_store() -> (RecordStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("pm-acp-records-{}", uuid::Uuid::new_v4()));
        (RecordStore::at(dir.join(FILE)), dir)
    }

    #[test]
    fn records_survive_a_save_and_a_load() {
        let (store, dir) = temp_store();
        assert!(store.load().is_empty(), "no file is an empty list");
        let mut a = record("a");
        a.push_notice(NoticeKind::HistoryLost, "the earlier chat could not be restored", 5);
        store.save(&[a.clone(), record("b")]).unwrap();
        let back = RecordStore::at(dir.join(FILE)).load();
        assert_eq!(back, vec![a, record("b")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn one_unreadable_record_does_not_cost_the_others() {
        let (store, dir) = temp_store();
        store.save(&[record("a")]).unwrap();
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        let mut value: Value = serde_json::from_str(&text).unwrap();
        value["conversations"].as_array_mut().unwrap().insert(0, serde_json::json!({"id": "future", "last_known_state": "hologram"}));
        std::fs::write(dir.join(FILE), value.to_string()).unwrap();
        let back = store.load();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, "a");
        std::fs::write(dir.join(FILE), "{ not json").unwrap();
        assert!(store.load().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_oldest_notices_and_records_go_first() {
        let mut r = record("a");
        for i in 0..(MAX_NOTICES + 5) {
            r.push_notice(NoticeKind::BackInChat, format!("n{i}"), i as u64);
        }
        assert_eq!(r.notices.len(), MAX_NOTICES);
        assert_eq!(r.notices[0].text, "n5");
        let (store, dir) = temp_store();
        let many: Vec<_> = (0..MAX_RECORDS + 3).map(|i| record(&format!("c{i}"))).collect();
        store.save(&many).unwrap();
        let back = store.load();
        assert_eq!(back.len(), MAX_RECORDS);
        assert_eq!(back[0].id, "c3");
        let _ = std::fs::remove_dir_all(dir);
    }
}
