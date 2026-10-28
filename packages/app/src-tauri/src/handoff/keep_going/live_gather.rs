//! Keep going: reading what the old pane did, for the brief (SPEC n32 section 3.5 B, step 2).
//!
//! What is read: the task and the last message from the pane's chat, its task list, the one-line
//! turn summaries, the changed file NAMES from git (`--numstat`, never the diff), and an unanswered
//! question. What is never read: tool outputs, the terminal scrollback, the environment. Everything
//! here is raw agent text; the brief builder cleans and fences it.

use super::brief::{open_plan_items, parse_numstat, DiffStat, TurnLine};
use super::live_io::{Gathered, PaneFacts};
use crate::operations::OperationSnapshot;
use crate::session_chat::model::ChatItem;
use std::path::Path;
use std::time::Duration;

/// The chat reader is held only while its items are read.
const HOLDER: &str = "keep-going";
const HOLD_FOR: Duration = Duration::from_secs(30);
/// Most bytes read from one git command.
const GIT_MAX: usize = 256 * 1024;

/// Read the old pane's work. Anything that cannot be read is left out, never guessed.
pub fn gather(pane: &PaneFacts, run: Option<&OperationSnapshot>) -> Gathered {
    let items = chat_items(&pane.pane_id, &pane.agent_id);
    Gathered {
        goal: first_prompt(&items).or_else(|| run.map(|run| run.task.clone())),
        turns: turn_lines(&pane.pane_id),
        diff: diff_of(pane),
        open_plan: open_plan_items(&items),
        last_message: last_message(&items),
        open_question: open_question(&pane.pane_id),
    }
}

#[cfg(test)]
fn chat_items(_pane_id: &str, _agent_id: &str) -> Vec<ChatItem> {
    // Not in unit tests: the app handle's window code does not link into the test binary.
    Vec::new()
}

#[cfg(not(test))]
fn chat_items(pane_id: &str, agent_id: &str) -> Vec<ChatItem> {
    use tauri::Manager;
    let (Some(app), Some(registry)) = (crate::bridge::live_app(), crate::bridge::live_registry()) else { return Vec::new() };
    let Some(opening) = crate::session_chat::Opening::for_agent(agent_id) else { return Vec::new() };
    let Some(chat) = app.try_state::<crate::commands::SessionChatState>() else { return Vec::new() };
    let watchers = chat.watchers(&app, &registry);
    opening.open(&watchers, pane_id, crate::session_chat::Hold::lease(HOLDER, HOLD_FOR));
    let items = watchers.since(pane_id, None, usize::MAX).map(|delta| delta.items).unwrap_or_default();
    watchers.close(pane_id, HOLDER);
    items
}

/// What the person first asked for.
fn first_prompt(items: &[ChatItem]) -> Option<String> {
    items.iter().find_map(|item| match item {
        ChatItem::User { text, .. } if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    })
}

/// The agent's newest message that has text.
fn last_message(items: &[ChatItem]) -> Option<String> {
    items.iter().rev().find_map(|item| match item {
        ChatItem::Assistant { text, .. } if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    })
}

fn turn_lines(pane_id: &str) -> Vec<TurnLine> {
    use chrono::TimeZone;
    crate::turn_summary::tracker()
        .summaries(pane_id)
        .into_iter()
        .map(|summary| TurnLine {
            n: summary.turn,
            hhmm: i64::try_from(summary.ended_at_ms)
                .ok()
                .and_then(|ms| chrono::Local.timestamp_millis_opt(ms).single())
                .map(|at| at.format("%H:%M").to_string())
                .unwrap_or_default(),
            sentence: summary.sentence,
            files: summary.files.map(|files| files.count),
        })
        .collect()
}

/// Changed file names against the base: `git diff --numstat` plus the untracked names. `None` when
/// the folder is not a repository with a commit.
fn diff_of(pane: &PaneFacts) -> Option<DiffStat> {
    let (dir, base, label) = match &pane.worktree {
        Some(copy) => {
            let base = if copy.base_commit.trim().is_empty() { "HEAD".to_string() } else { copy.base_commit.clone() };
            (copy.cwd.clone(), base, copy.base.clone())
        }
        None => (pane.cwd.clone(), "HEAD".to_string(), "HEAD".to_string()),
    };
    let dir = Path::new(&dir);
    let numstat = crate::second_opinion::source::run_git_capped(dir, &["diff", "--numstat", &base], GIT_MAX)?;
    let untracked = crate::second_opinion::source::run_git_capped(dir, &["ls-files", "--others", "--exclude-standard"], GIT_MAX)
        .map(|raw| raw.patch.lines().map(str::to_string).filter(|line| !line.trim().is_empty()).collect())
        .unwrap_or_default();
    Some(DiffStat { files: parse_numstat(&numstat.patch), untracked, base: label })
}

/// A question the pane's agent asked that nobody has answered (not one of Puppet Master's own cards).
fn open_question(pane_id: &str) -> Option<String> {
    crate::questions::store()
        .list_pending(crate::questions::now_ms())
        .into_iter()
        .find(|q| q.pane_id.as_deref() == Some(pane_id) && q.limit.is_none() && q.consent.is_none() && q.kind.is_none())
        .map(|q| q.question)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> ChatItem {
        ChatItem::User { id: "0:0".into(), at: None, text: text.into() }
    }
    fn reply(text: &str) -> ChatItem {
        ChatItem::Assistant { id: "1:0".into(), at: None, text: text.into(), model: None, final_answer: false }
    }
    fn tool_result(output: &str) -> ChatItem {
        ChatItem::ToolResult { id: "2:0".into(), at: None, call_id: "c".into(), ok: true, output: output.into() }
    }

    #[test]
    fn the_task_is_the_first_prompt_and_the_last_message_the_newest_reply() {
        let items = vec![user("Round totals"), reply("first"), tool_result("TOKEN=hunter2"), reply("second"), reply("   ")];
        assert_eq!(first_prompt(&items).as_deref(), Some("Round totals"));
        assert_eq!(last_message(&items).as_deref(), Some("second"));
    }

    #[test]
    fn tool_output_is_never_picked() {
        let items = vec![tool_result("TOKEN=hunter2")];
        assert_eq!(first_prompt(&items), None);
        assert_eq!(last_message(&items), None);
    }
}
