//! Agent panes that can take a Point message, and their order (PF-6).
//!
//! This is the "To:" list of the Point box. Shells are left out, because the outbox never types into a
//! shell. Protocol panes and panes an agent run controls stay in the list, greyed out, with the sentence the
//! outbox would give. Labels and titles come from the caller: nothing here looks at an agent's name.

use crate::pane_outbox::PaneKind;
use serde::Serialize;
use std::cmp::Reverse;

/// The outbox's sentences for panes it refuses (`pane_outbox/model.rs`). The tests read that file and
/// check that these still match.
const NOT_SUPPORTED: &str = "Chat replies are not available for this agent yet.";
const RUN_CONTROLS: &str = "An agent run controls this pane. Use Terminal, or wait until the run ends.";

/// One pane in the "To:" list.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub pane_id: String,
    pub agent_label: String,
    pub title: String,
    pub same_folder: bool,
    pub can_send: bool,
    /// One plain sentence when `can_send` is false.
    pub reason: Option<String>,
}

/// What the caller knows about one open pane. Built from the pane registry.
#[derive(Debug, Clone)]
pub struct TargetFacts {
    pub pane_id: String,
    pub agent_label: String,
    pub title: String,
    pub cwd: String,
    pub kind: PaneKind,
    pub run_controls: bool,
    /// One plain sentence when the pane shows a dialog, a question or no input box right now (`pane_outbox::target_block`).
    /// Nothing is typed into such a pane, so it is greyed out and never the default.
    pub not_ready: Option<String>,
    /// The pane's last output time (any unit; only the order matters). `None` when it has no output yet.
    pub last_output_at: Option<i64>,
}

/// Builds the "To:" list and its default pane.
///
/// Order: the opener (only when it can take messages), then the panes in the preview's folder, then the rest.
/// Inside each group the newest output comes first. Shells are left out. The default is the opener when it
/// can take messages, else the newest sendable pane in the preview's folder, else none.
pub fn targets(panes: &[TargetFacts], opener: Option<&str>, preview_cwd: Option<&str>) -> (Vec<Target>, Option<String>) {
    let mut rows: Vec<(u8, Option<i64>, Target)> = panes
        .iter()
        .filter(|p| p.kind != PaneKind::Shell)
        .map(|p| {
            let (can_send, reason) = gate(p);
            let same_folder = preview_cwd.is_some_and(|dir| same_folder(&p.cwd, dir));
            let rank = if can_send && opener == Some(p.pane_id.as_str()) {
                0
            } else if same_folder {
                1
            } else {
                2
            };
            let target = Target {
                pane_id: p.pane_id.clone(),
                agent_label: p.agent_label.clone(),
                title: p.title.clone(),
                same_folder,
                can_send,
                reason,
            };
            (rank, p.last_output_at, target)
        })
        .collect();
    // Stable sort: equal rows keep the caller's order. Panes with no output yet go last in their group.
    rows.sort_by_key(|(rank, last, _)| (*rank, last.is_none(), Reverse(last.unwrap_or(0))));
    let list: Vec<Target> = rows.into_iter().map(|(_, _, t)| t).collect();

    let default = opener
        .and_then(|id| list.iter().find(|t| t.pane_id == id && t.can_send))
        .or_else(|| list.iter().find(|t| t.same_folder && t.can_send))
        .map(|t| t.pane_id.clone());
    (list, default)
}

/// Whether the outbox would take a message for this pane, and the sentence when it would not.
/// Same order as `pane_outbox::Outbox::send`: protocol first, then a run that controls the pane; then what the
/// pane shows right now (a dialog, no input box), which the outbox would only wait on.
pub(crate) fn gate(p: &TargetFacts) -> (bool, Option<String>) {
    if p.kind == PaneKind::Protocol {
        return (false, Some(NOT_SUPPORTED.to_string()));
    }
    if p.run_controls {
        return (false, Some(RUN_CONTROLS.to_string()));
    }
    if let Some(why) = &p.not_ready {
        return (false, Some(why.clone()));
    }
    (true, None)
}

/// Two folders are the same when their paths match. Slashes do not count, and on Windows case does not
/// count either. An empty path is never the same as another.
fn same_folder(pane_cwd: &str, dir: &str) -> bool {
    let norm = |p: &str| {
        let p = p.trim().replace('\\', "/");
        let p = p.trim_end_matches('/');
        if cfg!(windows) {
            p.to_ascii_lowercase()
        } else {
            p.to_string()
        }
    };
    let (a, b) = (norm(pane_cwd), norm(dir));
    !a.is_empty() && a == b
}
