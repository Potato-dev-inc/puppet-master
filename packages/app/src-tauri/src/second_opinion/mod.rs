//! Second opinion: "ask another agent to review what this one just did".
//!
//! A read-only run of a reviewer agent (any headless-capable agent the user picks) gets the diff
//! of the pane's work plus the facts we recorded about the turn, and answers with structured
//! findings that the cockpit anchors to diff lines. Each finding can be sent back to the agent
//! that wrote the code as a follow-up. See `docs/features/second-opinion.md`.
//!
//! Everything in the submodules is pure or injectable (the diff sources, the run launcher, the
//! pane/run lookup), so it is tested without a window or a real agent. `service.rs` is the thin
//! live wiring used by `commands/second_opinion.rs`. Generic code never looks at agent names:
//! the reviewers on offer are whatever the registry says can answer a headless read-only run
//! (`tui::read_only_backends`).

pub mod parse;
pub mod prompt;
pub mod route;
pub mod service;
pub mod source;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

/// Where the diff under review came from, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The pane or run works in an isolated git worktree: everything it changed.
    Worktree,
    /// The shadow-repo checkpoints: "before turn n" against "after turn n".
    Checkpoint,
    /// Plain `git diff HEAD` of the project folder.
    Git,
}

impl SourceKind {
    /// Short plain label for the UI ("Reviewed: ...").
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Worktree => "the worktree changes",
            SourceKind::Checkpoint => "the last turn",
            SourceKind::Git => "uncommitted changes in the project",
        }
    }
}

/// The diff handed to the reviewer (and shown again, anchored, in the cockpit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDiff {
    pub source: SourceKind,
    pub patch: String,
    /// The patch was cut at the size cap (the reviewer is told so).
    pub truncated: bool,
    pub files: usize,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub file: String,
    /// `None` for a finding about the whole file.
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    pub severity: Severity,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictLevel {
    Approve,
    Concerns,
    Changes,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    pub level: VerdictLevel,
    pub text: String,
}

/// What the reviewer's answer boiled down to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParsedReview {
    pub findings: Vec<Finding>,
    pub verdict: Option<Verdict>,
    /// Set when nothing could be parsed: the reviewer's own words, shown as plain text.
    pub unstructured: Option<String>,
}
