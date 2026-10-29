//! Pure roll-up of score turns and runs into one row per agent and account (SC-7).
//!
//! Turns and runs in, rows out. No clock, no disk, no agent names. A turn counts in the window it
//! started in; a run in the window of its last start. The columns follow spec 3.2:
//! - Ended OK: out of the turns that ended OK or failed. A turn the person stopped is left out.
//! - Checks green: out of the turns whose checks got an answer.
//! - Merged: out of the runs whose copy was merged or thrown away. Waiting and active copies are
//!   not counted.
//! - Undone: out of the turns that changed files and are covered by the undo log. Lower is better.
//! - Asked you: out of the turns with a summary. Neutral: never marked.
//!
//! Only `Ended OK`, `Checks green`, `Merged` and `Undone` can carry an `ahead` mark, and only
//! through `stats::ahead`.

use super::model::{
    BrokeFact, CopyState, End, PerMerged, Rate, Sample, ScoreRow, ScoreRun, ScoreTurn, MIN_MERGED_FOR_TYPICAL,
};
use super::stats;
use std::collections::BTreeMap;

pub use crate::plans_report::aggregate::{Labels, DEFAULT_ACCOUNT, REMOVED_ACCOUNT};

type Key = (String, Option<String>);

/// The counts of one agent and account, before the rates are worked out.
#[derive(Default)]
struct Tally {
    turns: u32,
    ended_ok: u32,
    ended_failed: u32,
    checks_answered: u32,
    checks_green: u32,
    undone_known: u32,
    undone: u32,
    asked_known: u32,
    asked: u32,
    merged: u32,
    thrown_away: u32,
    waiting: u32,
    turn_ms: Vec<u64>,
    lines: Vec<u32>,
    merged_turns: Vec<u32>,
    merged_busy_ms: Vec<u64>,
}

fn bump(n: &mut u32) {
    *n = n.saturating_add(1);
}

impl Tally {
    fn add_turn(&mut self, t: &ScoreTurn) {
        bump(&mut self.turns);
        match t.end {
            End::Ok => bump(&mut self.ended_ok),
            End::Failed => bump(&mut self.ended_failed),
            End::Stopped => {}
        }
        if let Some(green) = t.checks_green {
            bump(&mut self.checks_answered);
            if green {
                bump(&mut self.checks_green);
            }
        }
        if let Some(undone) = t.undone {
            bump(&mut self.undone_known);
            if undone {
                bump(&mut self.undone);
            }
        }
        if let Some(asked) = t.asked_you {
            bump(&mut self.asked_known);
            if asked {
                bump(&mut self.asked);
            }
        }
        self.turn_ms.push(t.ended_ms.saturating_sub(t.started_ms));
        if let Some(lines) = t.lines {
            self.lines.push(lines);
        }
    }

    fn add_run(&mut self, r: &ScoreRun) {
        match r.copy {
            CopyState::Merged => {
                bump(&mut self.merged);
                self.merged_turns.push(r.turns);
                self.merged_busy_ms.push(r.busy_ms);
            }
            CopyState::Discarded => bump(&mut self.thrown_away),
            CopyState::Waiting => bump(&mut self.waiting),
            CopyState::Active | CopyState::NoCopy => {}
        }
    }
}

/// The label of an account; the default login has its own name.
fn account_label(account: &Option<String>, labels: &Labels) -> String {
    match account {
        None => DEFAULT_ACCOUNT.to_string(),
        Some(id) => (labels.account)(id).unwrap_or_else(|| REMOVED_ACCOUNT.to_string()),
    }
}

/// The typical merged run. Only with `MIN_MERGED_FOR_TYPICAL` merged runs; `None` otherwise.
fn per_merged(turns: &mut Vec<u32>, busy_ms: &mut Vec<u64>) -> Option<PerMerged> {
    let runs = u32::try_from(turns.len()).unwrap_or(u32::MAX);
    if runs < MIN_MERGED_FOR_TYPICAL {
        return None;
    }
    let turns_median = stats::median_u32(turns)?;
    let busy_ms_median = stats::median_u64(busy_ms)?;
    Some(PerMerged {
        runs,
        turns_median,
        busy_ms_median,
    })
}

fn row_of(agent: &str, account: &Option<String>, mut t: Tally, labels: &Labels) -> ScoreRow {
    let ended_ok = stats::rate(t.ended_ok, t.ended_ok.saturating_add(t.ended_failed));
    let checks_green = stats::rate(t.checks_green, t.checks_answered);
    let merged = stats::rate(t.merged, t.merged.saturating_add(t.thrown_away));
    let undone = stats::rate(t.undone, t.undone_known);
    let asked_you = stats::rate(t.asked, t.asked_known);
    let enough = [ended_ok, checks_green, merged, undone, asked_you]
        .iter()
        .any(|r| matches!(r.sample, Sample::Early | Sample::Steady));
    ScoreRow {
        agent_id: agent.to_string(),
        agent_label: (labels.agent)(agent),
        account_id: account.clone(),
        account_label: account_label(account, labels),
        turns: t.turns,
        enough,
        ended_ok,
        checks_green,
        merged,
        undone,
        asked_you,
        per_merged: per_merged(&mut t.merged_turns, &mut t.merged_busy_ms),
        waiting_review: t.waiting,
        turn_ms_median: stats::median_u64(&mut t.turn_ms),
        lines_median: stats::median_u32(&mut t.lines),
        ahead: Vec::new(),
        broke_tests: 0,
    }
}

/// The `ahead` marks: each of the four metrics on its own, over all rows (spec 3.3). The marks are
/// pushed in the page's order: endedOk, checksGreen, merged, undone.
fn mark_ahead(rows: &mut [ScoreRow]) {
    let metrics: [(&str, bool, fn(&ScoreRow) -> Rate); 4] = [
        ("endedOk", false, |r| r.ended_ok),
        ("checksGreen", false, |r| r.checks_green),
        ("merged", false, |r| r.merged),
        ("undone", true, |r| r.undone),
    ];
    for (name, lower_is_better, pick) in metrics {
        let rates: Vec<(usize, Rate)> =
            rows.iter().enumerate().map(|(i, r)| (i, pick(r))).collect();
        for i in stats::ahead(&rates, lower_is_better) {
            rows[i].ahead.push(name.to_string());
        }
    }
}

/// One row per agent and account, for one project (`None` = all) and the window from `since_ms`
/// (0 = all time). Sorted by turns (most first), then agent label, then account label. Never by
/// score.
pub fn aggregate(
    turns: &[ScoreTurn],
    runs: &[ScoreRun],
    project: Option<&str>,
    since_ms: u64,
    labels: &Labels,
) -> Vec<ScoreRow> {
    let in_scope = |key: &str| project.map_or(true, |p| p == key);
    let mut tallies: BTreeMap<Key, Tally> = BTreeMap::new();
    for t in turns
        .iter()
        .filter(|t| in_scope(&t.project) && t.started_ms >= since_ms)
    {
        tallies
            .entry((t.agent_id.clone(), t.account.clone()))
            .or_default()
            .add_turn(t);
    }
    for r in runs
        .iter()
        .filter(|r| in_scope(&r.project) && r.last_started_ms >= since_ms)
    {
        tallies
            .entry((r.agent_id.clone(), r.account.clone()))
            .or_default()
            .add_run(r);
    }
    let mut rows: Vec<ScoreRow> = tallies
        .into_iter()
        .map(|((agent, account), tally)| row_of(&agent, &account, tally, labels))
        .collect();
    rows.sort_by(|a, b| {
        b.turns
            .cmp(&a.turns)
            .then_with(|| {
                a.agent_label
                    .to_lowercase()
                    .cmp(&b.agent_label.to_lowercase())
            })
            .then_with(|| {
                a.account_label
                    .to_lowercase()
                    .cmp(&b.account_label.to_lowercase())
            })
    });
    mark_ahead(&mut rows);
    rows
}

/// Count the What broke it facts into `broke_tests` of the matching row (same agent and account).
/// Only that field is written. `ahead`, the rates and `enough` are already set, and none of them reads it.
/// A fact of another project, before `since_ms`, or with no row here is not counted.
pub fn count_broke(rows: &mut [ScoreRow], facts: &[BrokeFact], project: Option<&str>, since_ms: u64) {
    let in_scope = |fact: &&BrokeFact| project.map_or(true, |p| p == fact.project) && fact.at_ms >= since_ms;
    for fact in facts.iter().filter(in_scope) {
        if let Some(row) = rows.iter_mut().find(|r| r.agent_id == fact.agent_id && r.account_id == fact.account) {
            row.broke_tests = row.broke_tests.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests;
