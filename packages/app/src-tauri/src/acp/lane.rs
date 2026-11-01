//! "How to run": the per-agent choice between the terminal (today's way, the default) and chat over ACP.
//!
//! Stored in `<app data>/agent_lane.json`, one entry per agent that was changed. Like the run-mode and
//! authority switches it is not in the phone-patchable settings mirror: it changes how an agent runs, so
//! only the Settings window (a Tauri command) writes it. It is the default of the + menu's Terminal | Chat
//! switch and of `run_agent` without a `lane` (`acp::chat::surface::wants`).
//!
//! The terminal stays a full option, not only a fallback: this setting never removes it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "agent_lane.json";
const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunLane {
    /// The agent's own terminal UI in a pane. Today's behaviour.
    #[default]
    Terminal,
    /// Chat driven over the Agent Client Protocol.
    Acp,
    /// A normal terminal pane that opens on its Chat tab: the chat reads the agent's session file and sends by typing
    /// into the terminal (`crate::terminal_chat`). No adapter, no conversation record.
    TerminalChat,
}

impl RunLane {
    pub fn as_str(self) -> &'static str {
        match self {
            RunLane::Terminal => "terminal",
            RunLane::Acp => "acp",
            RunLane::TerminalChat => "terminal_chat",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "terminal" => Some(RunLane::Terminal),
            "acp" => Some(RunLane::Acp),
            "terminal_chat" => Some(RunLane::TerminalChat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoreFile {
    version: u32,
    /// Agent id (lower case) -> `terminal` | `acp` | `terminal_chat`. Text, so one value this build does not know never
    /// discards the others.
    agents: BTreeMap<String, String>,
}

/// The choices, kept in one file. `at` is a parameter so tests use a temp folder.
#[derive(Debug, Clone)]
pub struct LaneStore {
    path: PathBuf,
}

fn key(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

impl LaneStore {
    pub fn at(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_path_buf() }
    }

    /// The store in the app's data folder.
    pub fn in_app_data() -> Self {
        Self::at(crate::app_paths::app_data_dir().join(FILE))
    }

    fn read(&self) -> StoreFile {
        std::fs::read_to_string(&self.path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// A missing, unreadable or half-written file, or a value this build does not know, is the terminal.
    pub fn lane_for(&self, agent: &str) -> RunLane {
        self.read().agents.get(&key(agent)).and_then(|v| RunLane::parse(v)).unwrap_or_default()
    }

    /// Choosing the default removes the entry, so the file lists only what was changed.
    pub fn set(&self, agent: &str, lane: RunLane) -> Result<(), String> {
        let id = key(agent);
        if id.is_empty() {
            return Err("INVALID_ARGUMENT: agent id is required".into());
        }
        let mut file = self.read();
        file.version = FILE_VERSION;
        if lane == RunLane::Terminal {
            file.agents.remove(&id);
        } else {
            file.agents.insert(id, lane.as_str().to_string());
        }
        let text = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
        crate::settings_patch::write_atomic(&self.path, &text).map_err(|e| format!("could not save: {e}"))
    }

    /// Every agent whose lane was changed.
    pub fn all(&self) -> BTreeMap<String, RunLane> {
        self.read().agents.iter().filter_map(|(id, v)| RunLane::parse(v).map(|lane| (id.clone(), lane))).collect()
    }
}

/// The lane a new session of `agent` would use. Chat over ACP needs the Labs switch on, the person's
/// choice, and an ACP definition for the agent; anything else is the terminal (the automatic fallback).
pub fn effective_lane(chosen: RunLane, labs_on: bool, has_acp_definition: bool) -> RunLane {
    match chosen {
        RunLane::Acp if labs_on && has_acp_definition => RunLane::Acp,
        // Eligibility (a session reader) is checked where the pane starts (`terminal_chat::available_for`); it needs
        // no Labs switch and no ACP definition.
        RunLane::TerminalChat => RunLane::TerminalChat,
        _ => RunLane::Terminal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (LaneStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("pm-acp-lane-{}", uuid::Uuid::new_v4()));
        (LaneStore::at(dir.join(FILE)), dir)
    }

    #[test]
    fn the_default_is_the_terminal_for_every_agent() {
        let (store, _dir) = temp_store();
        assert_eq!(store.lane_for("codex"), RunLane::Terminal);
        assert_eq!(store.lane_for("anything"), RunLane::Terminal);
        assert!(store.all().is_empty());
    }

    #[test]
    fn a_choice_is_saved_per_agent_and_survives_a_reload() {
        let (store, dir) = temp_store();
        store.set("Codex", RunLane::Acp).unwrap();
        assert_eq!(store.lane_for("codex"), RunLane::Acp);
        assert_eq!(store.lane_for("claude"), RunLane::Terminal);
        assert_eq!(LaneStore::at(dir.join(FILE)).lane_for(" CODEX "), RunLane::Acp);
        store.set("codex", RunLane::Terminal).unwrap();
        assert!(store.all().is_empty(), "choosing the default removes the entry");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_file_or_an_unknown_value_means_the_terminal() {
        let (store, dir) = temp_store();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "{ not json").unwrap();
        assert_eq!(store.lane_for("codex"), RunLane::Terminal);
        std::fs::write(dir.join(FILE), r#"{"version":9,"agents":{"codex":"hologram","kilo":"acp"}}"#).unwrap();
        assert_eq!(store.lane_for("codex"), RunLane::Terminal);
        assert_eq!(store.lane_for("kilo"), RunLane::Acp, "one unknown value does not discard the others");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_agent_id_is_refused() {
        let (store, _dir) = temp_store();
        assert!(store.set("  ", RunLane::Acp).unwrap_err().contains("INVALID_ARGUMENT"));
    }

    #[test]
    fn chat_needs_the_labs_switch_the_choice_and_a_definition() {
        assert_eq!(effective_lane(RunLane::Acp, true, true), RunLane::Acp);
        assert_eq!(effective_lane(RunLane::Acp, false, true), RunLane::Terminal);
        assert_eq!(effective_lane(RunLane::Acp, true, false), RunLane::Terminal);
        assert_eq!(effective_lane(RunLane::Terminal, true, true), RunLane::Terminal);
        assert_eq!(effective_lane(RunLane::TerminalChat, false, false), RunLane::TerminalChat);
    }

    #[test]
    fn the_terminal_chat_word_is_the_serde_value() {
        assert_eq!(serde_json::to_string(&RunLane::TerminalChat).unwrap(), "\"terminal_chat\"");
        let (store, dir) = temp_store();
        store.set("kilo", RunLane::TerminalChat).unwrap();
        assert_eq!(store.lane_for("KILO"), RunLane::TerminalChat);
        store.set("kilo", RunLane::Terminal).unwrap();
        assert!(store.all().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lane_words_round_trip() {
        for lane in [RunLane::Terminal, RunLane::Acp, RunLane::TerminalChat] {
            assert_eq!(RunLane::parse(lane.as_str()), Some(lane));
        }
        assert_eq!(RunLane::parse("visible"), None);
    }
}
