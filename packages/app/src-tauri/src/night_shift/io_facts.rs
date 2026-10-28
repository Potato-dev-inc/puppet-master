//! Night Shift: Facts adapter over missions and worktrees (SPEC n32 3.5, NS-8).
//!
//! Turns what the mission engine and the operation layer already hold into the `Facts` that
//! `classify` reads. Every piece of text that can reach a page, push or report goes through the
//! `clean` function the caller passes in (strict redaction, home folder as `~`). Nothing here reads
//! the disk, the clock or a pane.
//!
//! Fields the runner (NS-10) fills after these calls, because they need I/O or a clock: `changes`
//! of a mission copy (`worktrees::changes`), `summaries` (`turn_lines` over the turn tracker) and
//! `stop_hhmm`. The adapters leave them empty.

use serde_json::Value;

use crate::missions::model::{Mission, Verdict};
use crate::operations::OperationSnapshot;
use crate::tamper::Finding;
use crate::worktrees::{Changes, WorktreeRef};

use super::classify::{Facts, MissionFacts, MAX_SUMMARIES};
use super::model::CopyRef;

/// Most characters of one turn sentence on the page and in the report.
pub const MAX_LINE_CHARS: usize = 140;
/// Most characters of an agent's question kept for the page.
pub const MAX_ASKED_CHARS: usize = 400;

/// The facts of a task that ran as a mission. `clean` redacts every text part that can reach a
/// result. `changes`, `summaries` and `stop_hhmm` are left empty for the runner to fill.
pub fn facts_from_mission(m: &Mission, clean: &dyn Fn(&str) -> String) -> Facts {
    Facts {
        mission: Some(MissionFacts {
            state: m.state,
            needs: m.needs,
            verdict: m.latest_verdict().map(|v| clean_verdict(v, clean)),
            baseline: m.baseline.as_ref().map(|v| clean_verdict(v, clean)),
            turns_used: m.turns_used(),
        }),
        changes: None,
        summaries: Vec::new(),
        copy: m.worktree.as_ref().map(|w| copy_ref(w, m.last_round().and_then(|r| r.pane_id.clone()))),
        stop_hhmm: String::new(),
        not_run: None,
    }
}

/// The facts of a single turn (no mission). `changes` are the copy's changes read by the runner.
/// `clean` is accepted so both adapters take the same redaction; nothing textual is copied here.
pub fn facts_from_run(op: &OperationSnapshot, changes: Option<Changes>, _clean: &dyn Fn(&str) -> String) -> Facts {
    Facts {
        mission: None,
        changes,
        summaries: Vec::new(),
        copy: op.worker.worktree.as_ref().map(|w| copy_ref(w, op.pane_id.clone())),
        stop_hhmm: String::new(),
        not_run: None,
    }
}

/// The copy's reference for the Morning page: where it is, which branch it holds and what it merges into.
pub fn copy_ref(w: &WorktreeRef, pane_id: Option<String>) -> CopyRef {
    CopyRef {
        project: w.repo.clone(),
        handle: w.handle.clone(),
        branch: w.branch.clone(),
        base: w.base.clone(),
        pane_id,
    }
}

/// Turn sentences for the page: redacted, one line each, at most `MAX_LINE_CHARS` characters, at
/// most `MAX_SUMMARIES` sentences. Empty sentences are dropped.
pub fn turn_lines(summaries: &[String], clean: &dyn Fn(&str) -> String) -> Vec<String> {
    summaries
        .iter()
        .map(|s| one_line(&clean(s)))
        .filter(|s| !s.is_empty())
        .take(MAX_SUMMARIES)
        .map(|s| cut(&s, MAX_LINE_CHARS))
        .collect()
}

/// The question an agent is waiting on, from a parked operation's `required_action`. The question
/// is read from the first key that holds text, in the order the headless view uses. Empty when the
/// action has no text.
pub fn asked_text(required_action: &Value, clean: &dyn Fn(&str) -> String) -> String {
    const KEYS: [&str; 4] = ["question", "detail", "text", "summary"];
    let text = KEYS
        .iter()
        .filter_map(|key| required_action.get(*key).and_then(Value::as_str))
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default();
    cut(&one_line(&clean(text)), MAX_ASKED_CHARS)
}

/// A check result with its free text redacted. Only the text parts are touched; counts and
/// pass/fail stay as they were.
fn clean_verdict(v: &Verdict, clean: &dyn Fn(&str) -> String) -> Verdict {
    let mut out = v.clone();
    for check in out.checks.iter_mut() {
        check.name = clean(&check.name);
        check.detail = clean(&check.detail);
        check.tail = clean(&check.tail);
    }
    out.failing_key = clean(&out.failing_key);
    for finding in out.test_changes.findings.iter_mut() {
        clean_finding(finding, clean);
    }
    out
}

fn clean_finding(f: &mut Finding, clean: &dyn Fn(&str) -> String) {
    f.summary = clean(&f.summary);
    f.before = f.before.as_deref().map(|s| clean(s));
    f.after = f.after.as_deref().map(|s| clean(s));
}

/// Collapses runs of whitespace (newlines too) into single spaces, so a sentence is one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first `max` characters, with the last one replaced by "…" when the text was longer.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
#[path = "io_facts_tests.rs"]
mod tests;
