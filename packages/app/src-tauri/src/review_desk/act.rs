//! Review Desk: the actions (SPEC n33 section 3.5 B to G). Keep, Throw away, unmark, Ask to fix and the detail.
//! Only the desktop commands call this file, and only after the person clicked (S1, S2): no MCP tool, bridge route,
//! companion or channel reaches it (`guard_tests`).
//!
//! Every action names its item by a strictly parsed key (S10), takes the fingerprint the person saw, reads that one
//! item again, and refuses `REVIEW_ITEM_CHANGED` when it differs (S3). One action per key runs at a time
//! (`REVIEW_BUSY`). Merge, discard, night and issue errors pass through unchanged so the window can say them in plain
//! words (`MERGE_CONFLICT`, `ORIGIN_DIRTY_OVERLAP`, `RUN_ACTIVE`, `ISSUE_BUSY`).
//!
//! Nothing here sends text or types into a pane (S7, S8): Ask to fix only keeps a draft for the Chat box.

use std::collections::HashSet;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::json;

use super::fix;
use super::gather::{copy_ref_of, folder_range, gather_one, title_line};
use super::live::{DeskIo, MARKS_LOCK};
use super::model::{How, Item, ItemKey, Keep, Marks, Source, Throw, DETAIL_PATCH_MAX};
use super::view::{ActView, DetailView, ItemView};
use crate::night_shift::model::Class;
use crate::operations::OperationError;

pub const BAD_KEY: &str = "REVIEW_BAD_KEY";
pub const NOT_FOUND: &str = "REVIEW_ITEM_NOT_FOUND";
pub const CHANGED: &str = "REVIEW_ITEM_CHANGED";
pub const CANNOT_KEEP: &str = "REVIEW_CANNOT_KEEP";
pub const CANNOT_THROW: &str = "REVIEW_CANNOT_THROW";
pub const NO_PANE: &str = "REVIEW_NO_PANE";
pub const BUSY: &str = "REVIEW_BUSY";
pub const SAVE_FAILED: &str = "REVIEW_SAVE_FAILED";

fn refuse(code: &str, message: impl Into<String>) -> OperationError {
    OperationError::new(code, message, true)
}

fn bad_key() -> OperationError {
    refuse(BAD_KEY, "That is not a valid review item.")
}

/// Keys with an action running now.
static IN_FLIGHT: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Held while an action on one key runs; the key is free again when it drops.
struct Busy(String);

impl Busy {
    /// One action per key and per desk (the marks folder names the desk, so two desks never share a key).
    fn begin(io: &dyn DeskIo, key: &ItemKey) -> Result<Busy, OperationError> {
        let scoped = format!("{}|{}", io.marks_root().display(), key.as_str());
        if IN_FLIGHT.lock().insert(scoped.clone()) {
            Ok(Busy(scoped))
        } else {
            Err(refuse(BUSY, "An action on this item is already running."))
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        IN_FLIGHT.lock().remove(&self.0);
    }
}

fn parse_key(raw: &str) -> Result<ItemKey, OperationError> {
    ItemKey::parse(raw).ok_or_else(bad_key)
}

/// The item as it is now, when it is still what the person saw.
fn fresh(io: &dyn DeskIo, key: &ItemKey, fingerprint: &str) -> Result<Item, OperationError> {
    let item = gather_one(io, key).ok_or_else(|| refuse(NOT_FOUND, "This is no longer waiting for review."))?;
    if item.fingerprint != fingerprint {
        return Err(refuse(CHANGED, "This changed since you looked. Look again."));
    }
    Ok(item)
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// One line of text from outside (a branch name) made safe for the window.
fn plain(io: &dyn DeskIo, raw: &str) -> String {
    title_line(&io.clean(), raw)
}

/// One audit line for a person's action: the action, the source, the key, the file count and the tier. No title and
/// no path (S13).
fn audit(io: &dyn DeskIo, action: &str, item: &Item) {
    let summary = match action {
        "keep" => "Kept an item in Review Desk",
        "throw_away" => "Threw away an item in Review Desk",
        _ => "Changed an item in Review Desk",
    };
    io.audit(
        summary,
        json!({
            "action": action,
            "source": item.source,
            "key": item.key.as_str(),
            "files": item.facts.files_total,
            "tier": item.risk.tier,
        }),
    );
}

fn set_mark(io: &dyn DeskIo, pane_id: &str, through_ms: u64, how: How) -> Result<(), OperationError> {
    let _lock = MARKS_LOCK.lock();
    let root = io.marks_root();
    let mut marks = Marks::load(&root);
    marks.set(pane_id, through_ms, how, io.now_ms());
    marks.save(&root).map_err(|_| refuse(SAVE_FAILED, "Could not save that. Try again."))
}

// ------------------------------------------------------------------------------------------------ Keep

/// Keep (SPEC 3.5 C). The window calls this only from the confirm sheet, or directly for "Looks good".
pub fn keep(io: &dyn DeskIo, raw_key: &str, fingerprint: &str) -> Result<ActView, OperationError> {
    let key = parse_key(raw_key)?;
    let _busy = Busy::begin(io, &key)?;
    let item = fresh(io, &key, fingerprint)?;
    let sentence = match &item.keep {
        Keep::Merge { project, handle, into } => {
            let outcome = io.merge(project, handle)?;
            if !outcome.merged {
                return Ok(ActView { sentence: "This copy has no changes to keep.".to_string(), gone: false });
            }
            merged_sentence(io, outcome.files.len(), &outcome.into, into)
        }
        Keep::NightKeep { task_id, into } => {
            let view = io.night_keep(task_id)?;
            if !view.merged {
                let why = view.reason.unwrap_or_else(|| "Nothing was merged.".to_string());
                return Ok(ActView { sentence: plain(io, &why), gone: gather_one(io, &key).is_none() });
            }
            merged_sentence(io, view.files.len(), &view.into, into)
        }
        Keep::IssuePush { run_id, .. } => {
            io.issue_act(run_id, "push")?;
            "Pushing it as a draft pull request. Puppet Master never merges it.".to_string()
        }
        Keep::Mark { pane_id, through_ms } => {
            set_mark(io, pane_id, *through_ms, How::Kept)?;
            "Marked as looked at.".to_string()
        }
        Keep::No { why } => return Err(refuse(CANNOT_KEEP, why.clone())),
    };
    audit(io, "keep", &item);
    Ok(ActView { sentence, gone: gather_one(io, &key).is_none() })
}

fn merged_sentence(io: &dyn DeskIo, files: usize, merged_into: &str, planned: &str) -> String {
    let into = plain(io, if merged_into.trim().is_empty() { planned } else { merged_into });
    format!("Merged {} into {into}.", plural(files, "file", "files"))
}

// ------------------------------------------------------------------------------------------------ Throw away

/// Throw away (SPEC 3.5 D). The window calls this only when the 10 s Undo window ends. For a folder item the three
/// rules of Restore are read again as part of the fresh read, at this moment.
pub fn throw_away(io: &dyn DeskIo, raw_key: &str, fingerprint: &str) -> Result<ActView, OperationError> {
    let key = parse_key(raw_key)?;
    let _busy = Busy::begin(io, &key)?;
    let item = fresh(io, &key, fingerprint)?;
    let sentence = match &item.throw {
        Throw::Discard { project, handle } => {
            io.discard(project, handle)?;
            "Threw the copy away.".to_string()
        }
        Throw::NightThrow { task_id } => {
            io.night_throw(task_id)?;
            "Threw the task away.".to_string()
        }
        Throw::IssueThrow { run_id } => {
            io.issue_act(run_id, "throw_away")?;
            "Threw the run away.".to_string()
        }
        Throw::Restore { pane_id, checkpoint_id, .. } => {
            io.restore(pane_id, checkpoint_id)?;
            let marked = set_mark(io, pane_id, item.finished_at_ms, How::ThrownAway);
            let mut text = "Put your files back to how they were before this work.".to_string();
            if marked.is_err() {
                text.push_str(" It may show up here again.");
            }
            text
        }
        Throw::No { why } => return Err(refuse(CANNOT_THROW, why.clone())),
    };
    audit(io, "throw_away", &item);
    Ok(ActView { sentence, gone: gather_one(io, &key).is_none() })
}

// ------------------------------------------------------------------------------------------------ Undo of a mark

/// Takes back a "Looks good" (SPEC 3.5 F). Safe to call twice.
pub fn unmark(io: &dyn DeskIo, pane_id: &str) -> Result<(), OperationError> {
    // A pane id is only ever a map key; it must still be a plain token.
    parse_key(&format!("folder:{pane_id}:0-0"))?;
    let _lock = MARKS_LOCK.lock();
    let root = io.marks_root();
    let mut marks = Marks::load(&root);
    if marks.undo(pane_id) {
        marks.save(&root).map_err(|_| refuse(SAVE_FAILED, "Could not save that. Try again."))?;
        io.audit(
            "Took back a looked-at mark in Review Desk",
            json!({ "action": "unmark", "source": Source::Folder, "key": format!("folder:{pane_id}") }),
        );
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------ Ask to fix

/// Ask to fix (SPEC 3.5 G): puts the fixed draft in the agent's chat box and returns the pane to open. Never sends.
pub fn fix(io: &dyn DeskIo, raw_key: &str, fingerprint: &str, note: &str) -> Result<String, OperationError> {
    let key = parse_key(raw_key)?;
    let _busy = Busy::begin(io, &key)?;
    let item = fresh(io, &key, fingerprint)?;
    let pane = item.fix.clone().map_err(|why| refuse(NO_PANE, why))?;
    io.put_draft(&pane, fix::draft(&item, note));
    Ok(pane)
}

// ------------------------------------------------------------------------------------------------ Look

/// The detail (SPEC 3.5 B): the item, its patch made safe to show, and the extra lines.
pub fn detail(io: &dyn DeskIo, raw_key: &str) -> Result<DetailView, OperationError> {
    let key = parse_key(raw_key)?;
    let item =
        gather_one(io, &key).ok_or_else(|| refuse(NOT_FOUND, "This is no longer waiting for review."))?;
    let mut extra = extra_lines(io, &item);

    let source = if let Some((project, handle)) = copy_ref_of(io, &item) {
        io.copy_patch(&project, &handle)
    } else if let Some((pane, from, to)) = folder_range(io, &key) {
        io.range_patch(&pane, &from, &to, DETAIL_PATCH_MAX)
    } else {
        None
    };
    let (patch, truncated) = match source {
        Some((raw, cut)) => {
            // Redact first, then cut, so a secret is never split at the cut.
            let (patch, over) = cut_at(io.clean_patch(&raw), DETAIL_PATCH_MAX);
            (patch, cut || over)
        }
        None => {
            extra.push("Puppet Master could not read the changes.".to_string());
            (String::new(), false)
        }
    };
    Ok(DetailView { item: ItemView::of(&item), patch, patch_truncated: truncated, extra })
}

/// Cuts `text` at `max` bytes on a character boundary. The flag says whether anything was cut.
fn cut_at(mut text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

/// Lines the row does not carry: the Night "needs a look" reason, the push check's reasons, the mission headline.
/// All pass through the live redaction.
fn extra_lines(io: &dyn DeskIo, item: &Item) -> Vec<String> {
    let clean = io.clean();
    let mut raw: Vec<String> = Vec::new();
    match item.source {
        Source::Night => {
            let id = item.key.as_str().strip_prefix("night:").unwrap_or_default();
            let reason = io.night_run().ok().flatten().and_then(|run| {
                let task = run.tasks.into_iter().find(|task| task.spec.id == id)?;
                let outcome = task.outcome?;
                (outcome.class == Class::NeedsLook).then_some(outcome.reason).flatten()
            });
            raw.extend(reason);
        }
        Source::Issue => {
            if let Keep::IssuePush { reasons, .. } = &item.keep {
                raw.extend(reasons.iter().cloned());
            }
        }
        Source::Mission => {
            let handle = item.key.as_str().strip_prefix("copy:").unwrap_or_default();
            if let (Some(id), Some((project, _))) =
                (handle.strip_prefix(crate::missions::model::HANDLE_PREFIX), copy_ref_of(io, item))
            {
                raw.extend(io.mission_headline(&project, id));
            }
        }
        Source::Copy | Source::Folder => {}
    }
    raw.iter().map(|line| title_line(&clean, line)).filter(|line| !line.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_is_cut_on_a_character_boundary() {
        let (cut, flag) = cut_at("aé".repeat(10), 5);
        assert!(flag && cut.len() <= 5 && cut.is_char_boundary(cut.len()));
        let (same, flag) = cut_at("abc".to_string(), 3);
        assert_eq!((same.as_str(), flag), ("abc", false));
    }

    #[test]
    fn plural_picks_the_word() {
        assert_eq!(plural(1, "file", "files"), "1 file");
        assert_eq!(plural(3, "file", "files"), "3 files");
    }
}
