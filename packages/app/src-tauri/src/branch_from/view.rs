//! What the window sees of a branch (SPEC n33 section 3.8). Carries no brief, message or path.
//!
//! `state` serializes as `preparing | starting | running | failed`, the names `src/lib/branch-from.ts`
//! uses. Ids, labels, counts and times only. The brief text is read through its own desktop command
//! (`branch_brief`) and, for the sheet, through [`Preview::brief`].

use super::brief::hhmm_of_ms;
use super::model::{Branch, State};
use crate::handoff::keep_going::model::Offer;
use serde::Serialize;

/// The save point in the sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointView {
    pub checkpoint_id: String,
    pub label: String,
    pub at_ms: u64,
    pub files: u32,
}

/// The brief the new agent would get (already cleaned), for "Show what the agent gets".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BriefPreview {
    pub text: String,
    pub bytes: usize,
    pub trimmed: Vec<String>,
}

/// What the branch sheet shows before anything is written (`branch_preview`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub from_label: String,
    pub point: Option<PointView>,
    pub can_branch: bool,
    pub why_not: Option<String>,
    pub offers: Vec<Offer>,
    pub prefill: Option<String>,
    pub turns: u32,
    pub open_branches: u32,
    pub brief: Option<BriefPreview>,
}

impl Preview {
    /// A preview of a branch that cannot be made: the reason only.
    pub fn refused(from_label: &str, why_not: String) -> Preview {
        Preview {
            from_label: from_label.to_string(),
            point: None,
            can_branch: false,
            why_not: Some(why_not),
            offers: Vec::new(),
            prefill: None,
            turns: 0,
            open_branches: 0,
            brief: None,
        }
    }
}

/// One branch as the chat banner shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchView {
    pub id: String,
    pub from_label: String,
    pub to_label: String,
    pub from_pane_id: String,
    pub to_pane_id: String,
    pub at_ms: u64,
    /// "after turn 5 · 14:02"
    pub point_label: String,
    /// `preparing` | `starting` | `running` | `failed`
    pub state: &'static str,
    pub state_text: String,
    pub turns: u32,
    pub brief_bytes: usize,
    pub branch_name: String,
    /// `kept` | `thrown_away` once the copy was merged or thrown away in Changes.
    pub closed: Option<&'static str>,
    pub failed_files: u32,
}

impl BranchView {
    /// `closed` comes from the copy (`LiveIo::worktree_closed`): `kept` or `thrown_away`.
    pub fn of(record: &Branch, closed: Option<&'static str>) -> BranchView {
        let (state, state_text) = match &record.state {
            State::Preparing => ("preparing", "Making a copy from the save point…".to_string()),
            State::Starting => ("starting", format!("{} is starting", record.to.label)),
            State::Running => ("running", "Running in a separate copy".to_string()),
            State::Failed { message, .. } => ("failed", message.clone()),
        };
        BranchView {
            id: record.id.clone(),
            from_label: record.from.label.clone(),
            to_label: record.to.label.clone(),
            from_pane_id: record.from.pane_id.clone(),
            to_pane_id: record.to.pane_id.clone(),
            at_ms: record.at_ms,
            point_label: point_label(&record.point.label, record.point.at_ms),
            state,
            state_text,
            turns: record.brief.turns,
            brief_bytes: record.brief.bytes,
            branch_name: match &record.copy {
                Some(copy) => copy.branch.clone(),
                None => format!("pm/{}", record.id),
            },
            closed,
            failed_files: record.copy.as_ref().map_or(0, |copy| u32::try_from(copy.failed.len()).unwrap_or(u32::MAX)),
        }
    }
}

/// Both sides of one pane's branches (`branch_lineage`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lineage {
    /// The branch that made this pane: "Branched from ...".
    pub from: Option<BranchView>,
    /// The branches made from this pane, newest first: "Branches ...".
    pub children: Vec<BranchView>,
}

/// "after turn 5 · 14:02" from the save point's label ("After turn 5") and time.
pub fn point_label(label: &str, at_ms: u64) -> String {
    let text = point_text(label);
    match hhmm_of_ms(at_ms) {
        Some(time) => format!("{text} · {time}"),
        None => text,
    }
}

/// "After turn 5" -> "after turn 5": a label in the middle of a sentence.
pub fn point_text(label: &str) -> String {
    let mut chars = label.trim().chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_reads_in_the_middle_of_a_sentence() {
        assert_eq!(point_text("After turn 5"), "after turn 5");
        assert_eq!(point_text("  Before turn 6 "), "before turn 6");
        assert_eq!(point_text(""), "");
    }

    #[test]
    fn a_refused_preview_serializes_camel_case() {
        let json = serde_json::to_value(Preview::refused("", "no".into())).unwrap();
        assert_eq!(json["canBranch"], false);
        assert_eq!(json["whyNot"], "no");
        assert_eq!(json["openBranches"], 0);
    }
}
