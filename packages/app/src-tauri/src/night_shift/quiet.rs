//! Night Shift: Quiet panes and runs (3.11).
//!
//! A pane or a mission that a Night Shift run started is "quiet": its cards and pushes stay silent,
//! because the person is asleep. The night report (the morning push and the Morning page) carries
//! what happened instead. The set lives in memory. Each attempt adds its pane (and its mission or
//! run handle) as soon as it has one. `unmute_run` drops everything a run added.
//!
//! The push, card and budget code asks `is_quiet_pane` / `is_quiet_project_handle` and returns early.
//! Nothing here writes to a pane.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Default)]
struct Quiet {
    /// Pane id -> the run that started it.
    panes: HashMap<String, String>,
    /// (project key, mission or run handle) -> the run that started it.
    handles: HashMap<(String, String), String>,
}

static QUIET: Lazy<Mutex<Quiet>> = Lazy::new(|| Mutex::new(Quiet::default()));

/// Mark a pane as quiet for this run. An empty id is ignored.
pub fn mute_pane(run_id: &str, pane_id: &str) {
    if pane_id.is_empty() {
        return;
    }
    QUIET.lock().panes.insert(pane_id.to_string(), run_id.to_string());
}

/// Mark a mission or run handle of one project as quiet for this run. Empty values are ignored.
pub fn mute_handle(run_id: &str, project: &str, handle: &str) {
    if project.is_empty() || handle.is_empty() {
        return;
    }
    QUIET.lock().handles.insert((project.to_string(), handle.to_string()), run_id.to_string());
}

/// Forget every pane and handle this run marked. Called when the run finishes or is stopped.
pub fn unmute_run(run_id: &str) {
    let mut quiet = QUIET.lock();
    quiet.panes.retain(|_, owner| owner != run_id);
    quiet.handles.retain(|_, owner| owner != run_id);
}

/// Whether a pane was started by a Night Shift run that has not finished.
pub fn is_quiet_pane(pane_id: &str) -> bool {
    !pane_id.is_empty() && QUIET.lock().panes.contains_key(pane_id)
}

/// Whether a mission or run handle of this project was started by a Night Shift run.
pub fn is_quiet_project_handle(project: &str, handle: &str) -> bool {
    QUIET.lock().handles.contains_key(&(project.to_string(), handle.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The set is process-wide, so every test uses its own run, pane and project names.

    #[test]
    fn a_muted_pane_is_quiet_until_its_run_is_unmuted() {
        mute_pane("ns-quiet00001", "pane-q1");
        assert!(is_quiet_pane("pane-q1"));
        unmute_run("ns-quiet00001");
        assert!(!is_quiet_pane("pane-q1"));
    }

    #[test]
    fn unrelated_panes_are_never_quiet() {
        mute_pane("ns-quiet00002", "pane-q2");
        assert!(!is_quiet_pane("pane-q2-other"));
        assert!(!is_quiet_pane("pane-q"), "a prefix of a quiet id is another pane");
        assert!(!is_quiet_pane(""), "an empty id is never quiet");
        unmute_run("ns-quiet00002");
    }

    #[test]
    fn an_empty_pane_id_is_not_stored() {
        mute_pane("ns-quiet00003", "");
        assert!(!is_quiet_pane(""));
        unmute_run("ns-quiet00003");
    }

    #[test]
    fn unmuting_one_run_leaves_the_other_run_quiet() {
        mute_pane("ns-quiet00004", "pane-q4a");
        mute_pane("ns-quiet00005", "pane-q4b");
        mute_handle("ns-quiet00004", "/proj/quiet-4", "mission-q4a");
        mute_handle("ns-quiet00005", "/proj/quiet-4", "mission-q4b");
        unmute_run("ns-quiet00004");
        assert!(!is_quiet_pane("pane-q4a"));
        assert!(is_quiet_pane("pane-q4b"));
        assert!(!is_quiet_project_handle("/proj/quiet-4", "mission-q4a"));
        assert!(is_quiet_project_handle("/proj/quiet-4", "mission-q4b"));
        unmute_run("ns-quiet00005");
    }

    #[test]
    fn a_muted_handle_is_quiet_only_for_its_own_project() {
        mute_handle("ns-quiet00006", "/proj/quiet-6", "mission-q6");
        assert!(is_quiet_project_handle("/proj/quiet-6", "mission-q6"));
        assert!(!is_quiet_project_handle("/proj/quiet-6-other", "mission-q6"));
        assert!(!is_quiet_project_handle("/proj/quiet-6", "mission-q6-other"));
        assert!(!is_quiet_project_handle("/proj/quiet-6", ""));
        unmute_run("ns-quiet00006");
        assert!(!is_quiet_project_handle("/proj/quiet-6", "mission-q6"));
    }

    #[test]
    fn empty_project_or_handle_is_not_stored() {
        mute_handle("ns-quiet00007", "", "mission-q7");
        mute_handle("ns-quiet00007", "/proj/quiet-7", "");
        assert!(!is_quiet_project_handle("", "mission-q7"));
        assert!(!is_quiet_project_handle("/proj/quiet-7", ""));
        unmute_run("ns-quiet00007");
    }
}
