//! Setting B: may an agent merge a separate copy (a git worktree) into the project?
//!
//! The only agent path to a merge is the MCP tool `review_worktree(action=merge)`, served by
//! `worktrees::routes`. A person merging in the cockpit goes through the Tauri command
//! `worktree_merge` and never reaches this gate.

use super::config;
use crate::audit::{self, Kind, Who};
use crate::operations::OperationError;
use serde_json::json;

/// The setting's name as the Settings window shows it (also used in the error text).
pub const SETTING_LABEL: &str = "Let agents merge separate copies";

fn who(session: Option<&str>) -> Who {
    Who::Agent(session.unwrap_or("unknown").to_string())
}

/// `Ok` when agents may merge; otherwise `MERGE_NOT_ALLOWED` with what to do instead.
pub fn check_agent_merge(session: Option<&str>, handle: &str) -> Result<(), OperationError> {
    if config::agents_may_merge() {
        return Ok(());
    }
    audit::record(
        Kind::Merge,
        who(session),
        "An agent tried to merge a separate copy; it was not allowed",
        json!({ "handle": handle, "allowed": false }),
    );
    let mut error = OperationError::new(
        "MERGE_NOT_ALLOWED",
        format!(
            "Agents may not merge separate copies on this computer, so nothing was merged. Only the person can: in Puppet Master, open this agent's pane, go to Changes and choose Merge. To let agents merge, the person can switch on \"{SETTING_LABEL}\" in Settings. You can still call review_worktree with action=diff to show them what changed, and tell them the copy is ready to review."
        ),
        false,
    );
    error.context = json!({
        "handle": handle,
        "setting": SETTING_LABEL,
        "next_steps": [
            { "tool": "review_worktree", "action": "diff", "why": "show the person what changed" },
            { "where": "app", "action": "Changes > Merge", "who": "the person" }
        ]
    });
    Err(error)
}

/// Written after an agent merge went through (setting B on).
pub fn note_agent_merged(session: Option<&str>, handle: &str, branch: &str, into: &str) {
    audit::record(
        Kind::Merge,
        who(session),
        "An agent merged a separate copy",
        json!({ "handle": handle, "branch": branch, "into": into, "allowed": true }),
    );
}
