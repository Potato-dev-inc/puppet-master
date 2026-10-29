//! "Scores on your code": which agent works best in THIS project, counted from the person's own
//! history on this computer. Read-only and local. Agents never see this page.
//!
//! - `model`: the types and the sample-size constants.
//! - `stats`: rates, the 90% Wilson interval, sample tiers and the `ahead` rule.
//! - `undo_log`: the one new write, an append-only log of Undo (nothing else recorded it).
//! - `join`: pairs turn summaries with operation records; builds the turns and runs.
//! - `aggregate`: the pure roll-up into one row per agent and account.
//! - `gather`: reads the live sources. `tests`: guard and end-to-end tests.
//! - this file: `Inputs`, the pure `build` of the report, and the live `agent_scores` behind the
//!   Tauri command of the same name.
//!
//! Generic: agents are only ids, labelled from the registry (wizard agents included).
//! Private: no folder, pane id, prompt, reply, file name or approval text leaves this module. A
//! project is shown by its last folder name only. Account labels can be e-mail addresses, so a
//! future export or copy path must go through `redact::strict`.

// The scaffold's types are read by the later SC tasks (stats, join, aggregate, gather, page).
#![allow(dead_code)]

pub mod aggregate;
pub mod gather;
pub mod join;
pub mod model;
pub mod pick;
pub mod stats;
pub mod undo_log;
#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod tests;

use crate::plans_report;
use aggregate::{aggregate, Labels};
use chrono::TimeZone;
use model::{ProjectInfo, ScoreRun, ScoreTurn, ScoresReport, MIN_SAMPLE};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// The longest window the page can ask for (ten years).
pub const MAX_DAYS: u32 = 3650;
/// Projects the switcher lists at most (newest activity first).
const MAX_PROJECTS: usize = 50;
const DAY_MS: u64 = 86_400_000;

/// Everything the report is built from: the joined turns and runs, and a few facts about them.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub turns: Vec<ScoreTurn>,
    pub runs: Vec<ScoreRun>,
    /// The day the undo log began; `None` when there is no log yet.
    pub undo_since_ms: Option<u64>,
    /// Project key (as in `ScoreTurn::project`) -> the folder's last name. A key with no name here
    /// is named from the key itself.
    pub project_names: BTreeMap<String, String>,
    /// Operation records that could not be read (skipped, so their turns are not counted).
    pub unreadable_records: usize,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The last folder name of a path or key, either slash style. Never more than that.
pub(crate) fn last_name(path: &str) -> String {
    path.split(['/', '\\']).filter(|part| !part.trim().is_empty()).last().unwrap_or("Project").trim().to_string()
}

/// The first 12 hex characters of the hash of the project key: the id the page sends back.
pub fn project_id(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// A project key from a folder path: a copy's folder maps to its origin first.
pub fn key_of_path(path: &str) -> String {
    let origin = crate::worktrees::origin_of_cwd(path).unwrap_or_else(|| path.to_string());
    crate::project_path::comparable_project_path(Path::new(&origin))
}

/// `days` must be 1 to 3650 (`None` = all time).
pub fn check_days(days: Option<u32>) -> Result<(), String> {
    match days {
        Some(d) if d == 0 || d > MAX_DAYS => Err(format!("INVALID_ARGUMENT: days must be from 1 to {MAX_DAYS}, got {d}")),
        _ => Ok(()),
    }
}

/// The projects with any history, newest activity first, with display labels (duplicates get
/// ` (2)`). `turns` counts the turns inside the window.
fn project_list(inputs: &Inputs, since_ms: u64) -> Vec<(String, ProjectInfo)> {
    let mut seen: BTreeMap<&str, (u64, u32)> = BTreeMap::new();
    for t in inputs.turns.iter().filter(|t| !t.project.is_empty()) {
        let entry = seen.entry(&t.project).or_default();
        entry.0 = entry.0.max(t.started_ms);
        if t.started_ms >= since_ms {
            entry.1 = entry.1.saturating_add(1);
        }
    }
    for r in inputs.runs.iter().filter(|r| !r.project.is_empty()) {
        let entry = seen.entry(&r.project).or_default();
        entry.0 = entry.0.max(r.last_started_ms);
    }
    let mut ordered: Vec<(&str, (u64, u32))> = seen.into_iter().collect();
    ordered.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(b.0)));
    let mut used: BTreeMap<String, u32> = BTreeMap::new();
    ordered
        .into_iter()
        .map(|(key, (_, turns))| {
            let base = inputs.project_names.get(key).map(|n| last_name(n)).unwrap_or_else(|| last_name(key));
            let count = used.entry(base.to_lowercase()).or_insert(0);
            *count += 1;
            let label = if *count == 1 { base } else { format!("{base} ({count})") };
            (key.to_string(), ProjectInfo { id: project_id(key), label, turns })
        })
        .collect()
}

/// The project the page asked for.
struct Chosen {
    /// The key to filter by; `None` for an unknown id, which can match nothing.
    key: Option<String>,
    info: ProjectInfo,
    /// Has history.
    known: bool,
}

/// An id from an earlier answer, or else a folder path (a copy's folder maps to its origin).
fn choose(text: &str, list: &[(String, ProjectInfo)]) -> Chosen {
    if let Some((key, info)) = list.iter().find(|(_, info)| info.id.eq_ignore_ascii_case(text)) {
        return Chosen { key: Some(key.clone()), info: info.clone(), known: true };
    }
    if text.len() == 12 && text.chars().all(|c| c.is_ascii_hexdigit()) {
        let info = ProjectInfo { id: text.to_ascii_lowercase(), label: "Unknown project".to_string(), turns: 0 };
        return Chosen { key: None, info, known: false };
    }
    let key = key_of_path(text);
    match list.iter().find(|(k, _)| *k == key) {
        Some((k, info)) => Chosen { key: Some(k.clone()), info: info.clone(), known: true },
        None => {
            let info = ProjectInfo { id: project_id(&key), label: last_name(text), turns: 0 };
            Chosen { key: Some(key), info, known: false }
        }
    }
}

/// The report for `project` (an id from an earlier answer or a folder path; `None` = all projects)
/// and the last `days` days (`None` = all time), at `now_ms`, by the clock of `tz`.
pub fn build_in<Tz: TimeZone>(
    inputs: &Inputs,
    project: Option<&str>,
    days: Option<u32>,
    now_ms: u64,
    tz: &Tz,
    labels: &Labels,
) -> ScoresReport {
    let since_ms = days.map_or(0, |d| now_ms.saturating_sub(u64::from(d) * DAY_MS));
    let list = project_list(inputs, since_ms);
    let chosen = project.map(str::trim).filter(|p| !p.is_empty()).map(|text| choose(text, &list));

    // An unknown project has no history: nothing is counted for it.
    let nothing = chosen.as_ref().is_some_and(|c| c.key.is_none());
    let (turns, runs): (&[ScoreTurn], &[ScoreRun]) = if nothing { (&[], &[]) } else { (&inputs.turns, &inputs.runs) };
    let key = chosen.as_ref().and_then(|c| c.key.as_deref());
    let rows = aggregate(turns, runs, key, since_ms, labels);
    let scoped: Vec<&ScoreTurn> =
        turns.iter().filter(|t| key.map_or(true, |k| k == t.project) && t.started_ms >= since_ms).collect();

    let mut notes = Vec::new();
    let began = inputs.undo_since_ms.and_then(|ms| plans_report::month::day_label(ms, tz).map(|day| (ms, day)));
    match began {
        Some((ms, day)) if ms > since_ms => {
            notes.push(format!("Counting Undo since {day}. Earlier undos were not recorded."));
        }
        Some((_, day)) => notes.push(format!("Counting Undo since {day}.")),
        None => notes.push("Counting Undo from now on.".to_string()),
    }
    // A turn with no summary has no files and no prompt answers, so Undone and Asked you leave it out.
    // A summary without files still counts in Asked you (`join` sets `asked_you` for every summary).
    let no_summary = scoped.iter().filter(|t| t.asked_you.is_none()).count();
    if no_summary > 0 {
        let (what, they) = if no_summary == 1 { ("turn had", "it is") } else { ("turns had", "they are") };
        notes.push(format!("{no_summary} {what} no summary, so {they} left out of Undone and Asked you."));
    }
    if !scoped.is_empty() {
        let with_checks = scoped.iter().filter(|t| t.checks_green.is_some()).count();
        notes.push(format!(
            "{with_checks} of {} turns had checks. Checks count only when a run is started with them.",
            scoped.len()
        ));
    }
    notes.push(
        "Tokens are not counted. Only usage an agent reports itself could show here, and it is not saved per turn."
            .to_string(),
    );
    if inputs.unreadable_records > 0 {
        let left_out = if inputs.unreadable_records == 1 { "Its turns are not counted." } else { "Their turns are not counted." };
        notes.push(format!("{} could not be read. {left_out}", plural(inputs.unreadable_records, "record", "records")));
    }
    if chosen.as_ref().is_some_and(|c| !c.known) {
        notes.push("No turns have been recorded for this project yet.".to_string());
    }

    ScoresReport {
        scope: if chosen.is_some() { "project" } else { "all" }.to_string(),
        project: chosen.map(|c| c.info),
        projects: list.into_iter().take(MAX_PROJECTS).map(|(_, info)| info).collect(),
        days,
        since_ms,
        min_sample: MIN_SAMPLE,
        rows,
        notes,
    }
}

/// `build_in` by the clock zone of this computer.
pub fn build(inputs: &Inputs, project: Option<&str>, days: Option<u32>, now_ms: u64, labels: &Labels) -> ScoresReport {
    build_in(inputs, project, days, now_ms, &chrono::Local, labels)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Read every record this computer keeps and build the report. Slow-ish (it reads each project's
/// operation records): call it off the UI thread. Read-only; nothing here controls an agent.
pub fn agent_scores(project: Option<&str>, days: Option<u32>) -> Result<ScoresReport, String> {
    check_days(days)?;
    let now = now_ms();
    let inputs = gather::live_inputs(now);
    let agent = |id: &str| crate::tui::descriptors().into_iter().find(|d| d.id == id).map(|d| d.label).unwrap_or_else(|| id.to_string());
    let account = |id: &str| crate::accounts::label_of(id);
    Ok(build(&inputs, project, days, now, &Labels { agent: &agent, account: &account }))
}
