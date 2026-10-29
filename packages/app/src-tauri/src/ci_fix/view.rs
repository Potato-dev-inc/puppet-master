//! What the Settings page, the PR block and the card's `Show log` read (SPEC n33 section 3.6): plain
//! data, in the shapes `src/lib/ci-fix.ts` declares. A view never holds a log line, except
//! [`FixLogView`], which is the stored, cleaned snapshot the card shows and is empty once the fix ended.

use super::model::{fix_active, FixRun, FixState, LogNotes, Settings, Watch, WatchState};
use super::text;
use serde::Serialize;
use std::collections::BTreeMap;

/// How many fixes the view lists (newest first).
pub const RECENT_FIXES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GhView {
    /// `ready`, `missing` or `signed_out`.
    pub state: String,
    /// The `gh` login; empty when not signed in.
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixView {
    pub id: String,
    pub watch_id: String,
    pub try_no: u32,
    /// The `FixState` tag (`asking`, `running`, `ask_push`, ...).
    pub state: String,
    pub state_text: String,
    /// The failing check names, already cleaned.
    pub checks: Vec<String>,
    pub notes: LogNotes,
    pub pane_id: Option<String>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchView {
    pub id: String,
    pub repo: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub branch: String,
    pub agent_label: String,
    /// The `WatchState` tag (`watching`, `paused`, ...).
    pub state: String,
    pub state_text: String,
    pub tries_used: u32,
    pub tries_max: u32,
    pub last_fix: Option<FixView>,
    pub project: String,
    pub handle: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CiFixView {
    pub settings: Settings,
    pub gh: GhView,
    pub watches: Vec<WatchView>,
    pub fixes: Vec<FixView>,
}

/// The cleaned log a fix started with: only the stored snapshot, never a fresh read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FixLogView {
    pub checks: Vec<String>,
    pub lines_by_check: BTreeMap<String, Vec<String>>,
    pub notes: LogNotes,
    pub sha8: String,
}

/// The tag a state serializes with.
fn tag_of<T: Serialize>(state: &T) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.get("state").and_then(|tag| tag.as_str().map(str::to_string)))
        .unwrap_or_default()
}

pub fn watch_tag(state: &WatchState) -> String {
    tag_of(state)
}

pub fn fix_tag(state: &FixState) -> String {
    tag_of(state)
}

/// The pane a fix works in, while it runs.
fn pane_of(fix: &FixRun) -> Option<String> {
    match &fix.state {
        FixState::Running { pane_id, .. } => pane_id.clone(),
        _ => None,
    }
}

/// One plain sentence for a fix: the row text of a watch that is watching and whose latest fix is this one.
fn fix_text(w: &Watch, fix: &FixRun) -> String {
    if let FixState::Failed { code, .. } = &fix.state {
        if code == super::model::CI_FIX_TRIES_USED {
            return format!("CI still fails after {} tries.", w.tries_used);
        }
    }
    let mut watching = w.clone();
    watching.state = WatchState::Watching;
    text::row_text(&watching, Some(fix))
}

pub fn fix_view(w: &Watch, fix: &FixRun) -> FixView {
    FixView {
        id: fix.id.clone(),
        watch_id: fix.watch_id.clone(),
        try_no: fix.try_no,
        state: fix_tag(&fix.state),
        state_text: fix_text(w, fix),
        checks: fix.checks.iter().map(|check| check.name.clone()).collect(),
        notes: fix.log.notes.clone(),
        pane_id: pane_of(fix),
        at_ms: fix.at_ms,
    }
}

/// The row of one watch. `note` is a sentence from the last look (for example the day cap) that
/// replaces the plain "Watching CI" text.
pub fn watch_view(w: &Watch, fixes: &[FixRun], note: Option<&str>, tries_max: u32) -> WatchView {
    let last = fixes.iter().find(|fix| fix.watch_id == w.id);
    let mut state_text = match last {
        Some(fix) if matches!(w.state, WatchState::Watching) => fix_text(w, fix),
        _ => text::row_text(w, last),
    };
    if let (Some(note), WatchState::Watching) = (note, &w.state) {
        if last.is_none_or(|fix| !fix_active(&fix.state) && !matches!(fix.state, FixState::Pushed { .. })) {
            state_text = note.to_string();
        }
    }
    WatchView {
        id: w.id.clone(),
        repo: w.owner_repo.clone(),
        pr_number: w.pr_number,
        pr_url: w.pr_url.clone(),
        branch: w.branch.clone(),
        agent_label: w.agent_label.clone(),
        state: watch_tag(&w.state),
        state_text,
        tries_used: w.tries_used,
        tries_max,
        last_fix: last.map(|fix| fix_view(w, fix)),
        project: w.project.clone(),
        handle: w.handle.clone(),
    }
}

/// The stored snapshot of a fix as the card shows it. Empty lines once the fix ended.
pub fn log_view(fix: &FixRun) -> FixLogView {
    let checks: Vec<String> = fix.checks.iter().map(|check| check.name.clone()).collect();
    let mut lines_by_check = BTreeMap::new();
    for (name, lines) in &fix.log.jobs {
        lines_by_check.insert(name.clone(), lines.clone());
    }
    FixLogView { checks, lines_by_check, notes: fix.log.notes.clone(), sha8: fix.log.sha8.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_match_the_saved_spelling() {
        assert_eq!(fix_tag(&FixState::Declined), "declined");
        assert_eq!(fix_tag(&FixState::NotPushed), "not_pushed");
        assert_eq!(watch_tag(&WatchState::Paused { why: "x".into() }), "paused");
    }
}
