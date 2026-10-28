//! What the Settings page and the commands read (SPEC n32 section 3.7): the config, the GitHub
//! status, one row per watched repo and the recent runs. Plain data; the shapes match
//! `src/lib/issue-intake.ts`. A run row never carries the issue body (the card has it).

use super::model::{Config, IssueRun, RepoWatch, RunState};
use serde::Serialize;

/// How many runs the view lists (newest first).
pub const RECENT_RUNS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GhView {
    /// `ready`, `missing` or `signed_out`.
    pub state: String,
    pub login: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoStatusView {
    pub repo: RepoWatch,
    pub agent_label: String,
    /// One plain sentence: `Watching`, `Paused`, or what went wrong on the last look.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub labeled_by: String,
    pub author: String,
    /// The `RunState` tag (`asking`, `running`, `pr_open`, ...).
    pub state: String,
    pub state_text: String,
    pub agent_label: String,
    pub pr_url: Option<String>,
    pub pane_id: Option<String>,
    pub at_ms: u64,
    pub forced: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntakeView {
    pub config: Config,
    pub gh: GhView,
    pub repos: Vec<RepoStatusView>,
    pub runs: Vec<RunView>,
}

/// The `state` tag of a run, as the saved file spells it.
pub fn state_tag(state: &RunState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.get("state").and_then(|tag| tag.as_str().map(str::to_string)))
        .unwrap_or_default()
}

/// One plain sentence for a state. Never says "auto", "merge" or "fix it".
pub fn state_text(state: &RunState) -> String {
    match state {
        RunState::Skipped { why } => format!("Skipped: {why}"),
        RunState::Asking { .. } => "Waiting for you".to_string(),
        RunState::Declined => "Not now".to_string(),
        RunState::Ignored => "Ignored".to_string(),
        RunState::Expired => "The card timed out".to_string(),
        RunState::Queued { why } if !why.is_empty() => why.clone(),
        RunState::Queued { .. } => "Waiting for a free slot".to_string(),
        RunState::Starting => "Starting".to_string(),
        RunState::Running { .. } => "Working".to_string(),
        RunState::Checking => "Checking the change".to_string(),
        RunState::AskPush { .. } => "Waiting for you to open the draft PR".to_string(),
        RunState::Pushing => "Opening the draft PR".to_string(),
        RunState::PrOpen { number, .. } => format!("Draft PR #{number}"),
        RunState::NoChanges { .. } => "No changes".to_string(),
        RunState::Held { message, .. } if !message.is_empty() => message.clone(),
        RunState::Held { .. } => "Puppet Master won't push this. Check the copy yourself.".to_string(),
        RunState::ThrownAway => "Thrown away".to_string(),
        RunState::Failed { message, .. } if !message.is_empty() => format!("Failed: {message}"),
        RunState::Failed { .. } => "Failed".to_string(),
    }
}

/// The list row of one run.
pub fn run_view(run: &IssueRun) -> RunView {
    let pr_url = match &run.state {
        RunState::PrOpen { url, .. } => Some(url.clone()),
        _ => None,
    };
    let pane_id = match &run.state {
        RunState::Running { pane_id, .. } => pane_id.clone(),
        _ => None,
    };
    RunView {
        id: run.id.clone(),
        repo: run.issue.owner_repo.clone(),
        number: run.issue.number,
        title: run.issue.title.clone(),
        url: run.issue.url.clone(),
        labeled_by: run.issue.labeled_by.clone(),
        author: run.issue.author.clone(),
        state: state_tag(&run.state),
        state_text: state_text(&run.state),
        agent_label: run.agent_label.clone(),
        pr_url,
        pane_id,
        at_ms: run.at_ms,
        forced: run.forced_ask.clone(),
    }
}

/// The newest [`RECENT_RUNS`] runs (the store keeps them newest first).
pub fn recent(runs: &[IssueRun]) -> Vec<RunView> {
    runs.iter().take(RECENT_RUNS).map(run_view).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_match_the_saved_spelling() {
        assert_eq!(state_tag(&RunState::AskPush {
            question_id: "q".into(),
            facts: crate::issue_intake::model::PushFacts {
                files: 0,
                additions: 0,
                deletions: 0,
                ci_files: vec![],
                secret_files: vec![],
                secret_lines: vec![],
                home_path_lines: vec![],
                internal_files: vec![],
                git_changed: false,
                remote_branch_exists: false,
                run_status: "completed".into(),
                turns: 1,
                minutes: 1,
            },
        }), "ask_push");
        assert_eq!(state_tag(&RunState::ThrownAway), "thrown_away");
    }

    #[test]
    fn state_sentences_avoid_the_banned_words() {
        let states = [
            RunState::Skipped { why: "x".into() },
            RunState::Declined,
            RunState::Expired,
            RunState::Queued { why: String::new() },
            RunState::Starting,
            RunState::Checking,
            RunState::Pushing,
            RunState::PrOpen { number: 57, url: "u".into() },
            RunState::NoChanges { message: "m".into() },
            RunState::Held { code: "c".into(), message: String::new() },
            RunState::Failed { code: "c".into(), message: String::new() },
        ];
        for state in states {
            let text = state_text(&state).to_lowercase();
            for banned in ["auto", "merge", "fix it"] {
                assert!(!text.contains(banned), "{text}");
            }
        }
    }
}
