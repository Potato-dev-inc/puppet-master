//! Resolving the save point a branch starts from (SPEC n33 section 3.4). Pure.
//!
//! The window sends a `BranchFrom`. This module turns it into the save point (`Point`, shown in the
//! sheet), a `Cut` (which chat turns go into the brief) and an optional prefill (the person's own message,
//! for a "Before this turn" branch). It never reads a file and never runs git. It is run again on every
//! start, so a forged or stale id from the window is refused here.

use crate::checkpoints::Checkpoint;
use crate::clip::window::{find_turn, item_ms, window_for, STILL_RUNNING};
use crate::session_chat::model::ChatItem;
use crate::turn_summary::TurnSummary;

use super::model::{
    BranchFrom, Point, When, AFTER_SLACK_MS, BEFORE_SLACK_MS, ERR_BRANCH_NO_SAVE_POINT, ERR_BRANCH_NO_TIMES,
    ERR_BRANCH_STILL_RUNNING, MSG_BRANCH_NO_SAVE_POINT, MSG_BRANCH_NO_TIMES, MSG_BRANCH_STILL_RUNNING,
};

/// The save point and the chat cut of one branch.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub point: Point,
    pub cut: Cut,
    /// The person's own message, for "Before this turn": the box is filled with it. `None` otherwise.
    pub prefill: Option<String>,
}

/// Which chat turns go into the brief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The turns before the turn that starts at this `User` item index (that turn is left out).
    BeforeTurnAt(usize),
    /// The turns up to and including the turn that starts at this `User` item index.
    ThroughTurnAt(usize),
    /// The turns that started at or before this unix time in milliseconds.
    UpToMs(u64),
}

/// Resolve the save point and the chat cut for `from`.
///
/// `items` are the chat items of the source pane, `summaries` its turn summaries and `checkpoints` its save
/// points, oldest first. The error is `(code, message)`; the code is one of the `ERR_BRANCH_*` consts.
pub fn resolve(
    from: &BranchFrom,
    items: &[ChatItem],
    summaries: &[TurnSummary],
    checkpoints: &[Checkpoint],
) -> Result<Resolved, (&'static str, String)> {
    match from {
        BranchFrom::SavePoint { checkpoint_id } => {
            let found = if checkpoint_id_ok(checkpoint_id) {
                checkpoints.iter().find(|c| c.id == *checkpoint_id)
            } else {
                None
            };
            let checkpoint = found.ok_or_else(no_save_point)?;
            Ok(Resolved { point: point_of(checkpoint), cut: Cut::UpToMs(checkpoint.at_ms), prefill: None })
        }
        BranchFrom::Message { user_item_id, when } => {
            let turn = find_turn(items, Some(user_item_id)).map_err(|e| (ERR_BRANCH_NO_SAVE_POINT, e))?;
            let start = user_index(items, user_item_id).ok_or_else(no_save_point)?;
            match when {
                When::Before => {
                    let at = turn.user.and_then(item_ms).ok_or_else(no_times)?;
                    let checkpoint = newest_at_or_before(checkpoints, at + BEFORE_SLACK_MS).ok_or_else(no_save_point)?;
                    Ok(Resolved {
                        point: point_of(checkpoint),
                        cut: Cut::BeforeTurnAt(start),
                        prefill: user_text(turn.user),
                    })
                }
                When::After => {
                    let (_, end_ms, _) = window_for(&turn, summaries).map_err(window_error)?;
                    let mut bound = end_ms.saturating_add(AFTER_SLACK_MS);
                    // The item right after this turn is the next `User` item (a turn ends there).
                    if let Some(next) = items.get(start + turn.items.len()).and_then(item_ms) {
                        bound = bound.min(next + BEFORE_SLACK_MS);
                    }
                    let checkpoint = newest_at_or_before(checkpoints, bound).ok_or_else(no_save_point)?;
                    Ok(Resolved { point: point_of(checkpoint), cut: Cut::ThroughTurnAt(start), prefill: None })
                }
            }
        }
    }
}

/// A save point id is 1 to 9 ASCII digits (`^[0-9]{1,9}$`). Anything else is refused before any lookup.
pub fn checkpoint_id_ok(id: &str) -> bool {
    (1..=9).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

fn no_save_point() -> (&'static str, String) {
    (ERR_BRANCH_NO_SAVE_POINT, MSG_BRANCH_NO_SAVE_POINT.to_string())
}

fn no_times() -> (&'static str, String) {
    (ERR_BRANCH_NO_TIMES, MSG_BRANCH_NO_TIMES.to_string())
}

/// `window_for` refuses a running turn with `STILL_RUNNING`; every other refusal means the turn has no times.
fn window_error(e: String) -> (&'static str, String) {
    if e == STILL_RUNNING {
        (ERR_BRANCH_STILL_RUNNING, MSG_BRANCH_STILL_RUNNING.to_string())
    } else {
        no_times()
    }
}

/// The newest save point at or before `bound` (the list is oldest first; ties keep the later one).
fn newest_at_or_before(checkpoints: &[Checkpoint], bound: u64) -> Option<&Checkpoint> {
    checkpoints.iter().filter(|c| c.at_ms <= bound).max_by_key(|c| c.at_ms)
}

/// Index of the `User` item with this id. `find_turn` has already accepted the id, so this matches it.
fn user_index(items: &[ChatItem], id: &str) -> Option<usize> {
    items.iter().position(|item| matches!(item, ChatItem::User { .. }) && item.id() == id)
}

fn user_text(item: Option<&ChatItem>) -> Option<String> {
    match item {
        Some(ChatItem::User { text, .. }) => Some(text.clone()),
        _ => None,
    }
}

fn point_of(checkpoint: &Checkpoint) -> Point {
    Point {
        checkpoint_id: checkpoint.id.clone(),
        turn: checkpoint.turn,
        kind: checkpoint.kind.clone(),
        at_ms: checkpoint.at_ms,
        label: checkpoint.label.clone(),
        files: checkpoint.files,
    }
}

#[cfg(test)]
#[path = "point_tests.rs"]
mod point_tests;
