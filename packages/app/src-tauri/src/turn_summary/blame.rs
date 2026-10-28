//! "Agent blame" for the Changes view: which agent (and pane) last changed each file, from the saved
//! turn summaries of one project folder. Read-only, and never a guess: a file no summary lists gets
//! no entry.
//!
//! A summary lists its first few changed paths (`FileStats::paths`), so a file past that cap in a
//! turn gets no blame from that turn.

use std::collections::HashMap;

use serde::Serialize;

use super::model::TurnSummary;

/// Who changed one file last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBlame {
    /// The agent's display name ("Codex").
    pub agent: String,
    /// The pane's title when the pane is still open ("helper"); `None` once it is gone or untitled.
    pub pane_title: Option<String>,
    /// When the turn that changed the file ended (epoch ms).
    pub at_ms: u64,
}

/// A folder-relative path with forward slashes, or `None` when it is empty, absolute, on another
/// drive, or leaves the folder with `..`. `.` parts and repeated slashes are dropped.
pub fn normalise(raw: &str) -> Option<String> {
    let slashed = raw.trim().replace('\\', "/");
    if slashed.starts_with('/') || slashed.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in slashed.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The blame of each of `paths` (normalised, see [`normalise`]) that some summary lists. The latest
/// turn that touched a file wins: the later end time, then the higher turn number. `title_of` names
/// a pane (`None` = no title). Paths that do not normalise are left out.
pub fn blame(
    summaries: &[TurnSummary],
    paths: &[String],
    title_of: &dyn Fn(&str) -> Option<String>,
) -> HashMap<String, FileBlame> {
    let wanted: std::collections::HashSet<String> = paths.iter().filter_map(|p| normalise(p)).collect();
    // The winning summary per path, with its sort key (end time, turn).
    let mut best: HashMap<String, (&TurnSummary, (u64, u32))> = HashMap::new();
    for summary in summaries {
        let Some(files) = summary.files.as_ref() else { continue };
        let key = (summary.ended_at_ms, summary.turn);
        for listed in &files.paths {
            let Some(path) = normalise(listed) else { continue };
            if !wanted.contains(&path) {
                continue;
            }
            if best.get(&path).is_none_or(|(_, seen)| key >= *seen) {
                best.insert(path, (summary, key));
            }
        }
    }
    best.into_iter()
        .map(|(path, (summary, _))| {
            let pane_title = title_of(&summary.pane_id).filter(|t| !t.trim().is_empty());
            (path, FileBlame { agent: summary.agent.clone(), pane_title, at_ms: summary.ended_at_ms })
        })
        .collect()
}
