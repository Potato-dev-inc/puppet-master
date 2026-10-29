//! Reads the live sources behind the Scores page (SC-10): operation records, turn summaries (in
//! memory and in each project's ring file) and the undo log. Read-only.
//!
//! The same sources as "Your plans" (`plans_report`), with one difference: a copy keeps its own
//! ring file, so a ring is read from the folder it lives in, while operation records are read from
//! the ORIGIN project. `join` maps every folder to its origin project.
//!
//! What broke it findings (`checkpoints::bisect`) are read here too, as plain facts: only a culprit
//! turn with a known agent is kept, and only as a count.

use super::join::{join, ProjectSummary};
use super::model::BrokeFact;
use super::{last_name, undo_log, Inputs};
use crate::checkpoints::bisect::model::{Finding, Outcome};
use crate::checkpoints::bisect::store::Store;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The facts of the findings that blamed a turn. Only `Outcome::Culprit` counts, and only when the
/// suspect has an agent id (no id = no row to join). Every other outcome is skipped.
pub fn broke_facts(findings: &[Finding]) -> Vec<BrokeFact> {
    findings
        .iter()
        .filter_map(|finding| match &finding.outcome {
            Outcome::Culprit { suspect, .. } => Some(BrokeFact {
                project: finding.project.clone(),
                agent_id: suspect.agent_id.clone()?,
                account: suspect.account.clone(),
                at_ms: finding.finished_ms,
            }),
            _ => None,
        })
        .collect()
}

/// The facts saved in one store. A missing or unreadable `findings.json` gives none: the store reads
/// it as empty and never fails here.
pub fn broke_facts_in(store: &Store) -> Vec<BrokeFact> {
    broke_facts(&store.findings())
}

/// The app's own What broke it facts. Tests never read the app's folder, so they get none.
#[cfg(not(test))]
fn live_broke() -> Vec<BrokeFact> {
    broke_facts_in(&Store::live())
}

#[cfg(test)]
fn live_broke() -> Vec<BrokeFact> {
    Vec::new()
}

/// Every record this computer keeps, joined into turns and runs. Slow-ish (it reads each
/// project's operation records): call it off the UI thread.
pub fn live_inputs(_now: u64) -> Inputs {
    // Folders that may hold a ring file: indexed projects, plus the folder of every pane the
    // tracker knows (a copy's folder included).
    let mut folders: Vec<String> = crate::operations::indexed_project_paths();
    let mut summaries: Vec<ProjectSummary> = Vec::new();
    let mut memory: Vec<ProjectSummary> = Vec::new();
    for (summary, project) in crate::turn_summary::tracker().since_with_project(0) {
        match project {
            Some(folder) => {
                let folder = folder.to_string_lossy().to_string();
                folders.push(folder.clone());
                memory.push(ProjectSummary { project: folder, summary });
            }
            None => memory.push(ProjectSummary { project: String::new(), summary }),
        }
    }
    let mut seen = BTreeSet::new();
    folders.retain(|p| !p.trim().is_empty() && seen.insert(crate::project_path::comparable_project_path(Path::new(p))));

    // Operation records live in the origin project; a copy's folder maps to it.
    let mut origins: BTreeMap<String, String> = BTreeMap::new();
    let mut project_names: BTreeMap<String, String> = BTreeMap::new();
    for folder in &folders {
        let origin = crate::worktrees::origin_of_cwd(folder).unwrap_or_else(|| folder.clone());
        let key = crate::project_path::comparable_project_path(Path::new(&origin));
        project_names.entry(key.clone()).or_insert_with(|| last_name(&origin));
        origins.entry(key).or_insert(origin);
    }

    let mut ops = Vec::new();
    let mut unreadable_records = 0;
    for origin in origins.values() {
        // One bad record skips only that record; the rest of the project still counts.
        if let Ok((found, skipped)) = crate::operations::list_operations_lenient(origin) {
            ops.extend(found);
            unreadable_records += skipped;
        }
    }
    // Ring files first, memory last: the same turn in both keeps the one with a known folder.
    for folder in &folders {
        summaries.extend(
            crate::turn_summary::store::read_project(&PathBuf::from(folder))
                .into_iter()
                .map(|summary| ProjectSummary { project: folder.clone(), summary }),
        );
    }
    summaries.extend(memory);

    let descriptors = crate::tui::descriptors();
    let agent_of_label = |label: &str| descriptors.iter().find(|d| d.label == label).map(|d| d.id.to_string());
    let entries = undo_log::read(&undo_log::default_path());
    let (turns, runs) = join(&ops, &summaries, &undo_log::marks(&entries), undo_log::counting_since(&entries), &agent_of_label);
    Inputs {
        turns,
        runs,
        undo_since_ms: undo_log::counting_since(&entries),
        project_names,
        unreadable_records,
        broke: live_broke(),
    }
}
