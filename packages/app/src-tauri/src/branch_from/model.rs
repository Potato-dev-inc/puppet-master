//! Branch from here: the data model (SPEC n33 section 3.2).
//!
//! Plain data, consts and error codes only. No I/O here (that is `store.rs`). Generic code: no
//! agent names; the agent a branch starts with comes from the agent registry at run time.

use serde::{Deserialize, Serialize};

/// Largest brief written for the new agent (SPEC n33 3.5).
pub const BRIEF_MAX_BYTES: usize = 6 * 1024;
/// Most characters in the person's message to the new agent.
pub const MESSAGE_MAX_CHARS: usize = 4_000;
/// Branch records kept (oldest dropped first).
pub const RECORDS_CAP: usize = 200;
/// Open branch copies allowed per source pane.
pub const OPEN_PER_PANE_CAP: usize = 5;
/// A checkpoint up to this long after a turn started still counts as its "before" (= checkpoints TURN_START_SLACK_MS).
pub const BEFORE_SLACK_MS: u64 = 2_000;
/// A turn's "after" checkpoint can land one debounce window after the turn ended (DEBOUNCE_MS + 2 s).
pub const AFTER_SLACK_MS: u64 = 12_000;
/// How long the new agent may run before it is stopped.
pub const RUN_TIMEOUT_MS: u64 = 30 * 60_000;

pub const ERR_BRANCH_NOT_HERE: &str = "BRANCH_NOT_HERE";
pub const ERR_BRANCH_NO_SAVE_POINT: &str = "BRANCH_NO_SAVE_POINT";
pub const ERR_BRANCH_NO_TIMES: &str = "BRANCH_NO_TIMES";
pub const ERR_BRANCH_STILL_RUNNING: &str = "BRANCH_STILL_RUNNING";
pub const ERR_BRANCH_NEEDS_GIT: &str = "BRANCH_NEEDS_GIT";
pub const ERR_BRANCH_NO_COMMITS: &str = "BRANCH_NO_COMMITS";
pub const ERR_BRANCH_TOO_MANY: &str = "BRANCH_TOO_MANY";
pub const ERR_BRANCH_BUSY: &str = "BRANCH_BUSY";
pub const ERR_BRANCH_BAD_TARGET: &str = "BRANCH_BAD_TARGET";
pub const ERR_BRANCH_TARGET_LIMITED: &str = "BRANCH_TARGET_LIMITED";
pub const ERR_BRANCH_EMPTY_MESSAGE: &str = "BRANCH_EMPTY_MESSAGE";
pub const ERR_BRANCH_MESSAGE_TOO_LONG: &str = "BRANCH_MESSAGE_TOO_LONG";
pub const ERR_BRANCH_COPY_FAILED: &str = "BRANCH_COPY_FAILED";
pub const ERR_BRANCH_START_FAILED: &str = "BRANCH_START_FAILED";

/// Shown in the sheet or the banner. `<Agent>`, `<git reason, cleaned>` and `17:00` are filled in by the caller.
pub const MSG_BRANCH_NOT_HERE: &str = "Branching isn't available for this pane.";
pub const MSG_BRANCH_NO_SAVE_POINT: &str = "There is no save point for this turn. Save points are made while Undo is on.";
pub const MSG_BRANCH_NO_TIMES: &str = "This message has no time, so its save point can't be found. Pick one in History.";
pub const MSG_BRANCH_STILL_RUNNING: &str = "This turn is still running. Branch after it ends, or pick \"Before this turn\".";
pub const MSG_BRANCH_NEEDS_GIT: &str = "Branching needs Git in this folder. Use \"Set up Git here\" first.";
pub const MSG_BRANCH_NO_COMMITS: &str = "Branching needs one commit in this folder first.";
pub const MSG_BRANCH_TOO_MANY: &str = "This chat has 5 open branches. Keep or throw away one in Changes first.";
pub const MSG_BRANCH_BUSY: &str = "A branch is already being made from this chat. Wait a moment.";
pub const MSG_BRANCH_BAD_TARGET: &str = "That agent can't be picked.";
pub const MSG_BRANCH_TARGET_LIMITED: &str = "<Agent> is out of usage until 17:00.";
pub const MSG_BRANCH_EMPTY_MESSAGE: &str = "Tell the agent what to do.";
pub const MSG_BRANCH_MESSAGE_TOO_LONG: &str = "Make your message shorter (4,000 characters at most).";
pub const MSG_BRANCH_COPY_FAILED: &str = "The copy couldn't be made: <git reason, cleaned>. Your folder wasn't changed.";
pub const MSG_BRANCH_START_FAILED: &str = "<Agent> couldn't start. The copy is kept: throw it away in Changes if you don't need it.";

/// Which side of a turn a chat button branches from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Before,
    After,
}

/// Where the branch starts. Comes from the window; re-resolved on the server every time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BranchFrom {
    /// Chat: the User item that starts the turn.
    Message { user_item_id: String, when: When },
    /// History row; the id is digits only (`^[0-9]{1,9}$`).
    SavePoint { checkpoint_id: String },
}

/// One save point, as the History tab lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub checkpoint_id: String,
    pub turn: u32,
    /// Checkpoint kind: before | after | answer | stop | undo.
    pub kind: String,
    pub at_ms: u64,
    /// "After turn 5" / "Before turn 6" (history-model wording).
    pub label: String,
    /// Files that checkpoint changed vs the one before (0 = unknown/none).
    pub files: u32,
}

/// The pane the branch was made from. `cwd` is local only and never sent to a window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSide {
    pub pane_id: String,
    pub agent_id: String,
    pub account: Option<String>,
    pub label: String,
    pub cwd: String,
}

/// The pane the branch runs in. `pane_id` is empty until the pane exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSide {
    pub agent_id: String,
    pub account: Option<String>,
    pub label: String,
    pub pane_id: String,
    pub handle: Option<String>,
    pub operation_id: Option<String>,
}

/// Facts about the separate copy. `path` is local only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyFacts {
    pub handle: String,
    pub branch: String,
    pub path: String,
    pub start_commit: Option<String>,
    pub written: u32,
    pub removed: u32,
    /// First 20 paths that could not be written.
    pub failed: Vec<String>,
}

/// Where a branch is in its life. `Failed` keeps a stable code and a plain message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Preparing,
    Starting,
    Running,
    Failed { code: String, message: String },
}

/// The brief file the new agent got. Its text is private (`branches/briefs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefMeta {
    pub file: String,
    pub sha8: String,
    pub bytes: usize,
    pub turns: u32,
    pub trimmed: Vec<String>,
}

/// One branch, as kept in `records.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    /// `bf-` + 12 lowercase hex; also the worktree handle.
    pub id: String,
    pub at_ms: u64,
    pub from: SourceSide,
    pub point: Point,
    /// None for a History row.
    pub when: Option<When>,
    pub to: NewSide,
    /// Origin repo (run project key).
    pub project: String,
    pub copy: Option<CopyFacts>,
    /// Set when the source pane was itself a branch.
    pub parent_branch: Option<String>,
    pub brief: BriefMeta,
    pub state: State,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn branch_from_when_and_state_use_stable_tags() {
        let message = BranchFrom::Message { user_item_id: "u-1".into(), when: When::Before };
        assert_eq!(serde_json::to_value(&message).unwrap(), json!({"kind": "message", "user_item_id": "u-1", "when": "before"}));
        let save_point = BranchFrom::SavePoint { checkpoint_id: "5".into() };
        assert_eq!(serde_json::to_value(&save_point).unwrap(), json!({"kind": "save_point", "checkpoint_id": "5"}));
        assert_eq!(serde_json::to_value(When::After).unwrap(), json!("after"));
        assert_eq!(serde_json::to_value(State::Preparing).unwrap(), json!({"state": "preparing"}));
        assert_eq!(serde_json::to_value(State::Starting).unwrap(), json!({"state": "starting"}));
        assert_eq!(serde_json::to_value(State::Running).unwrap(), json!({"state": "running"}));
        let failed = State::Failed { code: ERR_BRANCH_COPY_FAILED.into(), message: "cut".into() };
        assert_eq!(
            serde_json::to_value(&failed).unwrap(),
            json!({"state": "failed", "code": "BRANCH_COPY_FAILED", "message": "cut"})
        );
        let back: BranchFrom = serde_json::from_value(json!({"kind": "message", "user_item_id": "u-1", "when": "before"})).unwrap();
        assert_eq!(back, message);
        let back: State = serde_json::from_value(json!({"state": "running"})).unwrap();
        assert_eq!(back, State::Running);
    }
}
