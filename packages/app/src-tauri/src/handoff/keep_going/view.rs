//! Keep going: what the windows are told about a hand-over (`keep_going_lineage`).
//!
//! Ids, labels, counts and times only. The brief, the folder and the goal stay in the private store
//! (S3): the brief is read through its own desktop command.

use super::model::{Back, Handover, Reason, State};
use serde::Serialize;

/// One hand-over as the chat banner shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverView {
    pub id: String,
    pub from_label: String,
    pub to_label: String,
    pub from_pane_id: String,
    pub to_pane_id: String,
    pub at_ms: u64,
    /// `stopping` | `starting` | `running` | `failed`
    pub state: &'static str,
    pub state_text: String,
    pub turns: u32,
    pub brief_bytes: usize,
    /// When the move-back card is due, while it still waits for the reset.
    pub back_at_ms: Option<u64>,
    pub reason: Reason,
}

impl HandoverView {
    pub fn of(record: &Handover) -> HandoverView {
        let (state, state_text) = match &record.state {
            State::Stopping => ("stopping", format!("Moving to {}", record.to.label)),
            State::Starting => ("starting", format!("Moving to {}", record.to.label)),
            State::Running => ("running", format!("Continued on {}", record.to.label)),
            State::Failed { message, .. } => ("failed", message.clone()),
        };
        HandoverView {
            id: record.id.clone(),
            from_label: record.from.label.clone(),
            to_label: record.to.label.clone(),
            from_pane_id: record.from.pane_id.clone(),
            to_pane_id: record.to.pane_id.clone(),
            at_ms: record.at_ms,
            state,
            state_text,
            turns: record.brief.turns,
            brief_bytes: record.brief.bytes,
            back_at_ms: match record.back {
                Back::AtReset { due_ms, .. } => Some(due_ms),
                _ => None,
            },
            reason: record.reason,
        }
    }
}

/// Both sides of one pane's hand-overs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lineage {
    /// The hand-over that made this pane (it is the new side): "Continued from ...".
    pub from: Option<HandoverView>,
    /// The hand-over this pane made (it is the old side): "Continued on ...".
    pub to: Option<HandoverView>,
}

/// `records` is newest first.
pub fn lineage_of(records: &[Handover], pane_id: &str) -> Lineage {
    if pane_id.is_empty() {
        return Lineage { from: None, to: None };
    }
    Lineage {
        from: records.iter().find(|r| r.to.pane_id == pane_id).map(HandoverView::of),
        to: records.iter().find(|r| r.from.pane_id == pane_id).map(HandoverView::of),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::keep_going::model::{BriefMeta, Side};

    fn record(id: &str, from: &str, to: &str, state: State) -> Handover {
        let side = |pane: &str, label: &str| Side {
            pane_id: pane.into(),
            agent_id: label.into(),
            account: None,
            label: label.into(),
            handle: None,
            operation_id: None,
        };
        Handover {
            id: id.into(),
            chain_id: id.into(),
            hop: 1,
            at_ms: 5,
            reason: Reason::UsageLimit,
            from: side(from, "Agent A"),
            to: side(to, "Agent B"),
            project: "p".into(),
            folder: "C:\\Users\\you\\proj".into(),
            worktree_handle: None,
            from_reset_at_ms: None,
            goal: "secret goal".into(),
            back: Back::AtReset { due_ms: 99, tries: 0 },
            brief: BriefMeta { file: "C:\\Users\\you\\brief.md".into(), sha8: "abcd1234".into(), bytes: 321, trimmed: vec![], turns: 14 },
            state,
        }
    }

    #[test]
    fn both_sides_of_a_pane_are_found() {
        let records = vec![
            record("kg-000000000002", "pane-b", "pane-c", State::Running),
            record("kg-000000000001", "pane-a", "pane-b", State::Running),
        ];
        let lineage = lineage_of(&records, "pane-b");
        assert_eq!(lineage.from.as_ref().map(|v| v.id.as_str()), Some("kg-000000000001"));
        assert_eq!(lineage.to.as_ref().map(|v| v.id.as_str()), Some("kg-000000000002"));
        assert_eq!(lineage_of(&records, "pane-x"), Lineage { from: None, to: None });
        assert_eq!(lineage_of(&records, ""), Lineage { from: None, to: None });
    }

    #[test]
    fn the_view_carries_no_path_goal_or_brief_text() {
        let view = HandoverView::of(&record("kg-000000000001", "pane-a", "pane-b", State::Running));
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("Users") && !json.contains("secret goal") && !json.contains("brief.md"), "{json}");
        assert!(
            json.contains("\"fromLabel\":\"Agent A\"") && json.contains("\"briefBytes\":321") && json.contains("\"backAtMs\":99"),
            "{json}"
        );
        assert_eq!(view.turns, 14);
        assert_eq!(view.state, "running");
        assert_eq!(view.state_text, "Continued on Agent B");
    }

    #[test]
    fn states_read_in_plain_words() {
        let moving = HandoverView::of(&record("kg-000000000001", "a", "b", State::Stopping));
        assert_eq!((moving.state, moving.state_text.as_str()), ("stopping", "Moving to Agent B"));
        let failed = HandoverView::of(&record(
            "kg-000000000001",
            "a",
            "b",
            State::Failed { code: "X".into(), message: "Agent B couldn't start.".into() },
        ));
        assert_eq!((failed.state, failed.state_text.as_str()), ("failed", "Agent B couldn't start."));
    }
}
