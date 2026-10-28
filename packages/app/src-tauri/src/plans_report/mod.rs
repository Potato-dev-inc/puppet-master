//! "What your plans did this month": one row per subscription (an agent under one account) with its
//! turns, busy time, files changed, merged runs and how often and how long it hit its usage limit.
//! Only an app that sees every agent can set the plans side by side.
//!
//! - `merge`: the operation records and the turn summaries of the same turns become one list.
//! - `limit_log`: the append-only log of usage-limit starts and ends (`handoff` writes it; nothing
//!   else kept limit times). Counted from the day the log began, and the report says so.
//! - `aggregate`: the pure roll-up for one month. `month`: calendar months in the computer's zone.
//! - this file: the report for a month, and the live sources behind the `plans_report` command.
//!
//! Generic: agents are only ids, labelled from the registry (wizard agents included).
//! Tokens are not counted: no chat reports real usage for a whole month, and none is ever guessed.
//! Private: no folder, no pane id, no chat text leaves this module.

pub mod aggregate;
pub mod limit_log;
mod merge;
pub mod model;
pub mod month;
#[cfg(test)]
mod tests;

use aggregate::{aggregate, Labels};
use chrono::TimeZone;
use model::{LimitFact, MonthInfo, PlansReport, TurnFact};
use month::YearMonth;
use std::collections::BTreeSet;

/// Months the switcher lists at most (newest first).
const MAX_MONTHS: usize = 24;

/// Everything the report is built from.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub turns: Vec<TurnFact>,
    pub limits: Vec<LimitFact>,
    /// The day the limit log began; `None` when there is no log yet.
    pub limits_since_ms: Option<u64>,
}

fn info<Tz: TimeZone>(ym: YearMonth, tz: &Tz) -> MonthInfo {
    MonthInfo { id: ym.id(), label: ym.label(), start_ms: ym.start_ms(tz).unwrap_or(0), end_ms: ym.end_ms(tz).unwrap_or(u64::MAX) }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The report for `selected` (this month when `None`), by the clock of `tz`.
pub fn build<Tz: TimeZone>(inputs: &Inputs, selected: Option<YearMonth>, now_ms: u64, tz: &Tz, labels: &Labels) -> PlansReport {
    let current = YearMonth::of_ms(now_ms, tz).unwrap_or(YearMonth { year: 1970, month: 1 });
    let chosen = selected.unwrap_or(current);
    let month = info(chosen, tz);

    let mut seen: BTreeSet<YearMonth> = BTreeSet::from([current, chosen]);
    seen.extend(inputs.turns.iter().filter_map(|t| YearMonth::of_ms(t.started_ms, tz)));
    seen.extend(inputs.limits.iter().filter_map(|l| YearMonth::of_ms(l.start_ms, tz)));
    let months: Vec<MonthInfo> = seen.iter().rev().take(MAX_MONTHS).map(|ym| info(*ym, tz)).collect();

    let (rows, totals) = aggregate(&inputs.turns, &inputs.limits, month.start_ms, month.end_ms, labels);

    let mut notes = Vec::new();
    let since_label = inputs.limits_since_ms.and_then(|ms| month::day_label(ms, tz));
    match (&since_label, inputs.limits_since_ms) {
        (Some(day), Some(since)) if since > month.start_ms => {
            notes.push(format!("Counting limits since {day}. Earlier limits were not recorded."));
        }
        (Some(day), _) => notes.push(format!("Counting limits since {day}.")),
        _ => notes.push("Counting limits from now on. Earlier limits were not recorded.".to_string()),
    }
    let no_end = inputs
        .limits
        .iter()
        .filter(|l| l.start_ms >= month.start_ms && l.start_ms < month.end_ms && l.end_ms.is_none())
        .count();
    if no_end > 0 {
        notes.push(format!("{} no end time, so no waiting time is added for it.", plural(no_end, "limit has", "limits have")));
    }
    let unknown_files: u32 = rows.iter().map(|r| r.turns.saturating_sub(r.files.known_turns)).sum();
    if unknown_files > 0 {
        notes.push(format!("{} had no file information, so they add no files.", plural(unknown_files as usize, "turn", "turns")));
    }
    notes.push("Tokens are not counted. Only usage an agent reports itself would show here.".to_string());

    PlansReport {
        month,
        months,
        rows,
        totals,
        notes,
        limits_counted_since: inputs.limits_since_ms.and_then(|ms| month::day_id(ms, tz)),
    }
}

// ---- the live app -----------------------------------------------------------------------------

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Read every record this computer keeps and build the report. Slow-ish (it reads each project's
/// operation records): call it off the UI thread.
pub fn plans_report(month: Option<&str>) -> Result<PlansReport, String> {
    let selected = match month.map(str::trim).filter(|m| !m.is_empty()) {
        Some(text) => Some(YearMonth::parse(text).ok_or_else(|| format!("INVALID_ARGUMENT: month must look like 2026-10, got {text}"))?),
        None => None,
    };
    let now = now_ms();
    let inputs = live_inputs(now);
    let agent = |id: &str| crate::tui::descriptors().into_iter().find(|d| d.id == id).map(|d| d.label).unwrap_or_else(|| id.to_string());
    let account = |id: &str| crate::accounts::label_of(id);
    Ok(build(&inputs, selected, now, &chrono::Local, &Labels { agent: &agent, account: &account }))
}

fn live_inputs(now: u64) -> Inputs {
    use std::path::PathBuf;
    let mut projects: Vec<String> = crate::operations::indexed_project_paths();
    let mut summaries = crate::turn_summary::tracker().since(0);
    for (_, project) in crate::turn_summary::tracker().since_with_project(0) {
        projects.extend(project.map(|p| p.to_string_lossy().to_string()));
    }
    let mut seen = BTreeSet::new();
    projects.retain(|p| !p.trim().is_empty() && seen.insert(crate::project_path::comparable_project_path(std::path::Path::new(p))));

    let mut ops = Vec::new();
    for project in &projects {
        if let Ok(found) = crate::operations::list_operations(project) {
            ops.extend(found.iter().filter_map(merge::OpTurn::of));
        }
        summaries.extend(crate::turn_summary::store::read_project(&PathBuf::from(project)));
    }
    let descriptors = crate::tui::descriptors();
    let agent_of_label = |label: &str| descriptors.iter().find(|d| d.label == label).map(|d| d.id.to_string());
    let turns = merge::merge(&ops, &summaries, &agent_of_label);

    let entries = limit_log::read(&limit_log::default_path());
    let limited_now = |pane: &str| crate::handoff::is_parked(pane);
    Inputs { turns, limits: limit_log::hits(&entries, now, &limited_now), limits_since_ms: limit_log::counting_since(&entries) }
}
