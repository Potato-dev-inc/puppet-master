//! Keep going: the two parsers the brief takes its facts from. Pure, no I/O.
//!
//! `parse_numstat` reads `git diff --numstat` rows; `open_plan_items` folds the agent's task-list
//! updates into the list still open. `brief.rs` re-exports both.

use crate::session_chat::{ChatItem, PlanStatus};

/// Parse `git diff --numstat` output: `added<TAB>deleted<TAB>path`. Binary files (`-`) count as 0.
/// A rename (`old => new`, or `dir/{old => new}/file`) keeps the new path.
pub fn parse_numstat(text: &str) -> Vec<(String, u32, u32)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let added = parts.next()?;
            let deleted = parts.next()?;
            let path = parts.next()?.trim_end_matches('\r');
            if path.is_empty() {
                return None;
            }
            Some((rename_target(path), count(added), count(deleted)))
        })
        .collect()
}

fn count(field: &str) -> u32 {
    field.trim().parse().unwrap_or(0)
}

fn rename_target(path: &str) -> String {
    if let (Some(open), Some(close)) = (path.find('{'), path.find('}')) {
        if open < close {
            if let Some((_, new)) = path[open + 1..close].split_once(" => ") {
                return format!("{}{}{}", &path[..open], new, &path[close + 1..]).replace("//", "/");
            }
        }
    }
    match path.split_once(" => ") {
        Some((_, new)) => new.to_string(),
        None => path.to_string(),
    }
}

/// The task list the person's agent still has open. Plan items are folded in order: a full list
/// (`replace`) starts over; an update changes the task with the same `task_id` (a `Deleted` status
/// removes it). Completed tasks are dropped.
pub fn open_plan_items(items: &[ChatItem]) -> Vec<String> {
    let mut list: Vec<PlanEntry> = Vec::new();
    for item in items {
        if let ChatItem::Plan { items, replace, .. } = item {
            if *replace {
                list.clear();
            }
            for update in items {
                fold(&mut list, update.task_id.clone(), &update.text, update.status);
            }
        }
    }
    list.into_iter().filter(|entry| entry.status != PlanStatus::Completed).map(|entry| entry.text).collect()
}

struct PlanEntry {
    task_id: Option<String>,
    text: String,
    status: PlanStatus,
}

fn fold(list: &mut Vec<PlanEntry>, task_id: Option<String>, text: &str, status: Option<PlanStatus>) {
    let found = match &task_id {
        Some(id) => list.iter().position(|entry| entry.task_id.as_deref() == Some(id.as_str())),
        None if !text.is_empty() => list.iter().position(|entry| entry.task_id.is_none() && entry.text == text),
        None => None,
    };
    match found {
        Some(i) if status == Some(PlanStatus::Deleted) => {
            list.remove(i);
        }
        Some(i) => {
            if !text.is_empty() {
                list[i].text = text.to_string();
            }
            if let Some(status) = status {
                list[i].status = status;
            }
        }
        None if status != Some(PlanStatus::Deleted) && !text.is_empty() => list.push(PlanEntry {
            task_id,
            text: text.to_string(),
            status: status.unwrap_or(PlanStatus::Pending),
        }),
        None => {}
    }
}

