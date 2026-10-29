//! The "Branch from here" end-to-end rig (`tests/fake_tui_branch.rs`): the real branch engine
//! (`branch_from::live::start`) with the real `AppIo`, as the desktop's `branch_start` command calls it,
//! on this world's pane registry and app handle. The only thing the test supplies is what the window
//! would send: the pane, where to branch from, the agent, the message, and the pane's chat items.
//!
//! `branch_from` is a private module and the commands need a window; this is the only seam the feature
//! needs. Dev only.

use super::world::World;
use crate::audit::Who;
use crate::branch_from::live::{self, PreviewInput, StartInput};
use crate::branch_from::live_app::app_io;
use crate::branch_from::model::BranchFrom;
use crate::session_chat::model::ChatItem;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Branches of this process (one store, like the app's).
pub struct BranchRig;

/// The chat items of a pane, as the window holds them and sends them along with a branch.
pub struct Chat(Vec<ChatItem>);

impl Chat {
    /// No items: what the window sends for a chat it cannot read (the fake agents write no session file and have
    /// no screen reader, so their chat is empty and a branch builds its notes from the turn summaries).
    pub fn empty() -> Chat {
        Chat(Vec::new())
    }

    /// A chat as a session file would give it: for each turn the person's message (at the turn's start), the agent's
    /// reply and the turn's end (at the turn's end). `turns` are `(message, reply)`; `windows` are the turns'
    /// `(started, ended)` unix milliseconds (the turn summaries' own).
    pub fn session(turns: &[(&str, &str)], windows: &[(u64, u64)]) -> Chat {
        let iso = |ms: u64| {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(i64::try_from(ms).unwrap_or(0))
                .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        };
        let mut items = Vec::new();
        for (index, (you, reply)) in turns.iter().enumerate() {
            let (start, end) = windows.get(index).copied().unwrap_or((0, 0));
            let n = index + 1;
            items.push(ChatItem::User { id: format!("u{n}"), at: iso(start), text: (*you).to_string() });
            items.push(ChatItem::Assistant {
                id: format!("a{n}"),
                at: iso(end),
                text: (*reply).to_string(),
                model: None,
                final_answer: true,
            });
            items.push(ChatItem::TurnEnd {
                id: format!("e{n}"),
                at: iso(end),
                outcome: crate::session_chat::model::TurnOutcome::Completed,
                duration_ms: Some(end.saturating_sub(start)),
            });
        }
        Chat(items)
    }

    /// The ids of the person's messages, oldest first.
    pub fn user_ids(&self) -> Vec<String> {
        self.0.iter().filter(|item| matches!(item, ChatItem::User { .. })).map(|item| item.id().to_string()).collect()
    }
}

fn error_json(error: &crate::operations::OperationError) -> Value {
    json!({ "code": error.code, "message": error.message })
}

impl BranchRig {
    pub fn new() -> BranchRig {
        BranchRig
    }

    /// The pane's save points, oldest first, as the History tab lists them (`id`, `turn`, `kind`, `label`, `at_ms`).
    pub fn checkpoints(&self, pane_id: &str) -> Vec<Value> {
        crate::checkpoints::live()
            .list(pane_id)
            .map(|list| list.iter().filter_map(|checkpoint| serde_json::to_value(checkpoint).ok()).collect())
            .unwrap_or_default()
    }

    /// `branch_start` as the desktop window calls it. `from` is the `BranchFrom` as the window sends it (JSON).
    /// Returns the `Preparing` view, or the refusal as `{ code, message }`.
    pub fn start(&self, pane_id: &str, from: Value, target: &str, message: &str, items: Chat) -> Result<Value, Value> {
        let from: BranchFrom = serde_json::from_value(from).map_err(|e| json!({ "code": "BAD_FROM", "message": e.to_string() }))?;
        let input = StartInput {
            pane_id: pane_id.to_string(),
            from,
            target: target.to_string(),
            message: message.to_string(),
            items: items.0,
            who: Who::Desktop,
        };
        live::start(app_io(), input)
            .map(|view| serde_json::to_value(view).unwrap_or_default())
            .map_err(|error| error_json(&error))
    }

    /// `branch_preview`: what the sheet shows.
    pub fn preview(&self, pane_id: &str, from: Value, message: Option<&str>, items: Chat) -> Value {
        let from: BranchFrom = serde_json::from_value(from).expect("a BranchFrom");
        let input = PreviewInput { pane_id, from: &from, target: None, message, items: &items.0 };
        serde_json::to_value(live::preview(&*app_io(), &input)).unwrap_or_default()
    }

    /// Every branch record, newest first, as the private store keeps them.
    pub fn records(&self) -> Vec<Value> {
        app_io().store().records().iter().filter_map(|record| serde_json::to_value(record).ok()).collect()
    }

    /// The record of branch `id`.
    pub fn record(&self, id: &str) -> Option<Value> {
        self.records().into_iter().find(|record| record["id"] == id)
    }

    /// What the windows are told about the branches of `pane_id` (`branch_lineage`), as JSON text.
    pub fn lineage_json(&self, pane_id: &str) -> String {
        serde_json::to_string(&live::lineage(&*app_io(), pane_id)).unwrap_or_default()
    }

    /// The stored brief of branch `id` (the desktop-only `branch_brief` command's answer).
    pub fn brief(&self, id: &str) -> Result<String, String> {
        live::brief(&*app_io(), id).map_err(|e| e.message)
    }

    /// The audit log's Branch lines (who, summary and detail as one JSON text each), oldest first.
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = crate::audit::global()
            .recent(10_000_000)
            .into_iter()
            .filter(|entry| entry.kind.starts_with("branch"))
            .map(|entry| json!({ "who": entry.who, "summary": entry.summary, "detail": entry.detail }).to_string())
            .collect();
        lines.reverse();
        lines
    }

    /// The folder `<app data>/branches`, where the records and briefs live.
    pub fn root(&self) -> PathBuf {
        crate::app_paths::app_data_dir().join("branches")
    }
}

impl Default for BranchRig {
    fn default() -> Self {
        Self::new()
    }
}
