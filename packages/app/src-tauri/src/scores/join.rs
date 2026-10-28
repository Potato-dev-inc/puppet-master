//! Pairs turn summaries with operation records into score turns and runs (SC-4).
//!
//! One list of turns from two records of the same thing. A run's turn is an operation record
//! (agent, account, outcome, checks, times) and a turn summary (files, answered prompts); a turn
//! typed into a pane by hand has only a summary. They are paired by pane and time (the rule of
//! "Your plans", `plans_report::merge`) so no turn is counted twice.
//!
//! Where a wrong join would give a believable but false number:
//! - A summary pairs with at most one record and a record with at most one summary. Pairs are
//!   taken closest start first, so two runs on one pane never swap their summaries.
//! - A summary of a turn whose record has not finished (running, waiting for an answer) is left
//!   out with it. The turn is not over, so it has no outcome yet.
//! - A copy's merge or discard is written only on the LAST record of a run (`close_worktree`), so
//!   it is a fact of the run, never of a turn. Runs are grouped by handle, not by agent.
//! - `undone` is a fact only for a turn that changed files, has a pane, and started after the undo
//!   log began. Everything else stays unknown, never "not undone".
//! - Only fields on a fixed list are copied. The task, the result, prompts, sentences and file
//!   names are never read, so none can reach the report.

use super::model::{CopyState, End, ScoreRun, ScoreTurn, UndoMark};
use crate::operations::{OperationSnapshot, OperationStatus};
use crate::plans_report::merge::overlaps_times;
use crate::turn_summary::TurnSummary;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// A turn summary and the project it was read from (the folder of its ring file; a copy's folder is
/// mapped to the origin project here).
#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub project: String,
    pub summary: TurnSummary,
}

/// A turn counts as undone when its time sits inside an Undo's span, within this much. The
/// checkpoint taken at a turn's start and the clock of its summary are not the same instant.
/// Known limit: a turn shorter than this, right before an Undo to the point after it, can read as
/// undone.
pub const UNDO_SLACK_MS: u64 = 2_000;

/// The origin project's comparable key. A copy's folder maps to its origin; results are cached
/// because the key reads the disk.
struct Projects {
    cache: HashMap<String, String>,
}

impl Projects {
    fn key(&mut self, path: &str) -> String {
        if let Some(found) = self.cache.get(path) {
            return found.clone();
        }
        let origin = crate::worktrees::origin_of_cwd(path).unwrap_or_else(|| path.to_string());
        let key = crate::project_path::comparable_project_path(Path::new(&origin));
        self.cache.insert(path.to_string(), key.clone());
        key
    }
}

fn op_start(op: &OperationSnapshot) -> u64 {
    op.started_at_ms.unwrap_or(op.created_at_ms)
}

fn op_end(op: &OperationSnapshot) -> u64 {
    op.finished_at_ms.unwrap_or(op.updated_at_ms).max(op_start(op))
}

fn account_of(raw: Option<&String>) -> Option<String> {
    raw.filter(|a| !a.trim().is_empty()).cloned()
}

/// A finished turn's outcome; `None` for a turn still going. Only turns that counted reach here,
/// so a cancelled one did start: the person stopped it.
fn end_of_op(status: OperationStatus) -> Option<End> {
    match status {
        OperationStatus::Completed => Some(End::Ok),
        OperationStatus::Failed => Some(End::Failed),
        OperationStatus::Cancelled => Some(End::Stopped),
        _ => None,
    }
}

/// Did the checks pass? `None` when the turn had none, or they never got an answer: a turn that
/// failed for another reason never ran its checks, and a stopped one did not either.
fn checks_of_op(op: &OperationSnapshot) -> Option<bool> {
    if op.checks.is_empty() {
        return None;
    }
    match op.status {
        OperationStatus::Failed if op.error.as_ref().is_some_and(|e| e.code == "ACCEPTANCE_FAILED") => Some(false),
        OperationStatus::Completed => {
            let all = op.check_results.len() >= op.checks.len() && op.check_results.iter().all(|r| r.passed);
            all.then_some(true)
        }
        _ => None,
    }
}

/// A turn that has no record: the pane status and the run receipt (a pane of a run whose record
/// was lost) are all there is.
fn end_of_summary(summary: &TurnSummary) -> End {
    match summary.receipt.as_ref().map(|r| r.outcome.as_str()) {
        Some("failed" | "failed_checks") => return End::Failed,
        Some("cancelled") => return End::Stopped,
        _ => {}
    }
    if summary.end.kind == "error" {
        End::Failed
    } else {
        End::Ok
    }
}

fn checks_of_summary(summary: &TurnSummary) -> Option<bool> {
    match summary.receipt.as_ref().map(|r| r.outcome.as_str()) {
        Some("passed_checks") => Some(true),
        Some("failed_checks") => Some(false),
        _ => None,
    }
}

fn asked_you(summary: &TurnSummary) -> bool {
    !summary.approvals.is_empty() || summary.end.kind == "waiting_input"
}

/// Files and lines of the summary's checkpoint diff. `None` when the pane had no checkpoints.
fn size_of(summary: &TurnSummary) -> (Option<u32>, Option<u32>) {
    match summary.files.as_ref() {
        Some(f) => (Some(f.count), Some(f.added.saturating_add(f.deleted))),
        None => (None, None),
    }
}

/// Was this turn undone? Needs files changed (nothing to put back otherwise), a pane, and a start
/// after the log began (an earlier Undo was never written down). The times are the summary's, the
/// same clock as the checkpoints an Undo goes back to.
fn undone_of(summary: &TurnSummary, files: Option<u32>, marks: &[UndoMark], log_began_ms: Option<u64>) -> Option<bool> {
    let began = log_began_ms?;
    if files.unwrap_or(0) == 0 || summary.pane_id.is_empty() || summary.started_at_ms < began {
        return None;
    }
    let ended = summary.ended_at_ms.max(summary.started_at_ms);
    let hit = marks.iter().any(|m| {
        m.pane == summary.pane_id
            && summary.started_at_ms.saturating_add(UNDO_SLACK_MS) >= m.since_ms
            && ended <= m.at_ms.saturating_add(UNDO_SLACK_MS)
    });
    Some(hit)
}

/// The same turn can arrive from memory and from a project's ring file, and a turn that waited for
/// an answer has a summary "so far" and a final one. Keep the latest of each.
fn unique(summaries: &[ProjectSummary]) -> Vec<&ProjectSummary> {
    let mut index: HashMap<(&str, u32, u64), usize> = HashMap::new();
    let mut out: Vec<&ProjectSummary> = Vec::new();
    for item in summaries {
        let s = &item.summary;
        match index.get(&(s.pane_id.as_str(), s.turn, s.started_at_ms)) {
            Some(&at) if out[at].summary.ended_at_ms >= s.ended_at_ms => {}
            Some(&at) => out[at] = item,
            None => {
                index.insert((s.pane_id.as_str(), s.turn, s.started_at_ms), out.len());
                out.push(item);
            }
        }
    }
    out
}

/// Pair summaries with records: same pane, overlapping times, closest start first. Returns, per
/// summary, the index of its record.
fn pair(ops: &[&OperationSnapshot], summaries: &[&ProjectSummary]) -> Vec<Option<usize>> {
    let mut by_pane: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, op) in ops.iter().enumerate() {
        if let Some(pane) = op.pane_id.as_deref().filter(|p| !p.is_empty()) {
            by_pane.entry(pane).or_default().push(i);
        }
    }
    // (start gap, end gap, summary, record): the smallest gaps win, ties go to the lower index.
    let mut candidates: Vec<(u64, u64, usize, usize)> = Vec::new();
    for (si, item) in summaries.iter().enumerate() {
        let s = &item.summary;
        for &oi in by_pane.get(s.pane_id.as_str()).into_iter().flatten() {
            let op = ops[oi];
            let (start, end) = (op_start(op), op.finished_at_ms.map(|f| f.max(op_start(op))));
            if overlaps_times(start, end, s.started_at_ms, s.ended_at_ms) {
                let end_gap = end.unwrap_or(start).abs_diff(s.ended_at_ms);
                candidates.push((start.abs_diff(s.started_at_ms), end_gap, si, oi));
            }
        }
    }
    candidates.sort_unstable();
    let mut taken = vec![false; ops.len()];
    let mut paired = vec![None; summaries.len()];
    for (_, _, si, oi) in candidates {
        if paired[si].is_none() && !taken[oi] {
            paired[si] = Some(oi);
            taken[oi] = true;
        }
    }
    paired
}

/// The turns and runs of everything the person did with agents.
///
/// - `ops`: operation records (any project; a record's project is its `project_path`, mapped to the
///   origin when it names a copy).
/// - `summaries`: turn summaries with the project they were read from.
/// - `marks`: Undos from the undo log; `log_began_ms`: when the log began (`None` = no log yet).
/// - `agent_of_label`: turns a summary's display name into an agent id, for summaries saved before
///   they carried the id.
///
/// The turns come out oldest first, the runs by last start.
pub fn join(
    ops: &[OperationSnapshot],
    summaries: &[ProjectSummary],
    marks: &[UndoMark],
    log_began_ms: Option<u64>,
    agent_of_label: &dyn Fn(&str) -> Option<String>,
) -> (Vec<ScoreTurn>, Vec<ScoreRun>) {
    let mut projects = Projects { cache: HashMap::new() };
    // A turn cancelled before it started used nothing: it is neither a turn nor a summary's partner.
    let counted: Vec<&OperationSnapshot> = ops.iter().filter(|op| crate::budgets::counts_as_turn(op)).collect();
    let summaries = unique(summaries);
    let paired = pair(&counted, &summaries);

    let mut turns = Vec::with_capacity(counted.len() + summaries.len());
    let mut used = vec![false; counted.len()];
    for (si, item) in summaries.iter().enumerate() {
        let s = &item.summary;
        let (files, lines) = size_of(s);
        let undone = undone_of(s, files, marks, log_began_ms);
        match paired[si] {
            Some(oi) => {
                used[oi] = true;
                let op = counted[oi];
                let Some(end) = end_of_op(op.status) else { continue };
                let start = op_start(op);
                turns.push(ScoreTurn {
                    project: projects.key(&op.project_path),
                    agent_id: op.agent_type.clone(),
                    account: account_of(op.worker.account.as_ref()).or_else(|| account_of(s.account.as_ref())),
                    pane_id: op.pane_id.clone(),
                    started_ms: start,
                    ended_ms: op_end(op),
                    end,
                    files_changed: files,
                    lines,
                    asked_you: Some(asked_you(s)),
                    checks_green: checks_of_op(op),
                    undone,
                });
            }
            None => turns.push(ScoreTurn {
                project: projects.key(&item.project),
                agent_id: s.agent_id.clone().or_else(|| agent_of_label(&s.agent)).unwrap_or_else(|| s.agent.clone()),
                account: account_of(s.account.as_ref()),
                pane_id: Some(s.pane_id.clone()).filter(|p| !p.is_empty()),
                started_ms: s.started_at_ms,
                ended_ms: s.ended_at_ms.max(s.started_at_ms),
                end: end_of_summary(s),
                files_changed: files,
                lines,
                asked_you: Some(asked_you(s)),
                checks_green: checks_of_summary(s),
                undone,
            }),
        }
    }
    for op in counted.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(_, op)| op) {
        let Some(end) = end_of_op(op.status) else { continue };
        turns.push(ScoreTurn {
            project: projects.key(&op.project_path),
            agent_id: op.agent_type.clone(),
            account: account_of(op.worker.account.as_ref()),
            pane_id: op.pane_id.clone().filter(|p| !p.is_empty()),
            started_ms: op_start(op),
            ended_ms: op_end(op),
            end,
            files_changed: None,
            lines: None,
            asked_you: None,
            checks_green: checks_of_op(op),
            undone: None,
        });
    }
    turns.sort_by(|a, b| {
        (a.started_ms, a.ended_ms, &a.project, &a.agent_id, &a.account, &a.pane_id)
            .cmp(&(b.started_ms, b.ended_ms, &b.project, &b.agent_id, &b.account, &b.pane_id))
    });

    (turns, runs_of(ops, &mut projects))
}

/// The run's newest record: the order `latest_agent_run` uses; the operation id only settles exact ties.
fn latest_of<'a>(ops: &[&'a OperationSnapshot]) -> Option<&'a OperationSnapshot> {
    ops.iter().copied().max_by(|a, b| {
        (a.turn_index, a.created_at_ms, &a.operation_id).cmp(&(b.turn_index, b.created_at_ms, &b.operation_id))
    })
}

/// One run per `(project, agent_run_id)`; a record with no handle is a run of its own.
fn runs_of(ops: &[OperationSnapshot], projects: &mut Projects) -> Vec<ScoreRun> {
    let mut groups: BTreeMap<(String, String), Vec<&OperationSnapshot>> = BTreeMap::new();
    for op in ops {
        let handle = if op.agent_run_id.trim().is_empty() { op.operation_id.clone() } else { op.agent_run_id.clone() };
        groups.entry((projects.key(&op.project_path), handle)).or_default().push(op);
    }
    let mut runs = Vec::with_capacity(groups.len());
    for ((project, _), group) in groups {
        let counted: Vec<&OperationSnapshot> = group.iter().copied().filter(|op| crate::budgets::counts_as_turn(op)).collect();
        // The same order `latest_agent_run` uses; the operation id only settles exact ties.
        let (Some(latest_counted), Some(latest_any)) = (latest_of(&counted), latest_of(&group)) else {
            continue; // every record was withdrawn before it started: nothing ran
        };
        let finished: Vec<&&OperationSnapshot> = counted.iter().filter(|op| op.status.terminal()).collect();
        runs.push(ScoreRun {
            project,
            agent_id: latest_counted.agent_type.clone(),
            account: account_of(latest_counted.worker.account.as_ref()),
            turns: finished.len() as u32,
            busy_ms: finished
                .iter()
                .filter_map(|op| op.finished_at_ms.map(|end| end.saturating_sub(op_start(op))))
                .fold(0u64, |sum, ms| sum.saturating_add(ms)),
            last_started_ms: counted.iter().map(|op| op_start(op)).max().unwrap_or(0),
            copy: copy_of(latest_any),
        });
    }
    runs.sort_by(|a, b| {
        (a.last_started_ms, &a.project, &a.agent_id, &a.account).cmp(&(b.last_started_ms, &b.project, &b.agent_id, &b.account))
    });
    runs
}

/// The state of the run's own copy, from the run's latest record (the only one that is marked
/// when the person merges or throws the copy away).
fn copy_of(latest: &OperationSnapshot) -> CopyState {
    let Some(worktree) = latest.worker.worktree.as_ref() else { return CopyState::NoCopy };
    match worktree.closed.as_deref() {
        Some("merged") => CopyState::Merged,
        Some("discarded") => CopyState::Discarded,
        Some(_) => CopyState::NoCopy, // a word we do not know: counted nowhere
        None if latest.status.terminal() => CopyState::Waiting,
        None => CopyState::Active,
    }
}

#[cfg(test)]
mod tests;
