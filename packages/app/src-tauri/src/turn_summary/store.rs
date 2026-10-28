//! Where summaries live: the last `KEEP_PER_PANE` per pane in memory, mirrored to a small JSON
//! ring file per project (`<project>/.puppet-master/turn-summaries.json`) so they survive a restart.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::TurnSummary;
use crate::redact::Redacted;

pub const KEEP_PER_PANE: usize = 100;
/// A project file keeps at most this many panes (the ones with the newest activity).
pub const MAX_PANES_IN_FILE: usize = 20;
const FILE_NAME: &str = "turn-summaries.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct RingFile {
    version: u32,
    panes: BTreeMap<String, Vec<TurnSummary>>,
}

pub fn ring_path(project: &Path) -> PathBuf {
    project.join(".puppet-master").join(FILE_NAME)
}

/// Reads the ring file; a missing or damaged file is an empty ring (it is rewritten on next save).
fn load_file(project: &Path) -> BTreeMap<String, Vec<TurnSummary>> {
    let Ok(text) = std::fs::read_to_string(ring_path(project)) else { return BTreeMap::new() };
    serde_json::from_str::<RingFile>(&text).map(|f| f.panes).unwrap_or_default()
}

/// Every summary saved in a project's ring file, oldest first (a missing or damaged file is empty).
/// Read-only: "Your plans" (`crate::plans_report`) uses it for months that are not in memory.
pub fn read_project(project: &Path) -> Vec<TurnSummary> {
    let mut all: Vec<TurnSummary> = load_file(project).into_values().flatten().collect();
    all.sort_by_key(|s| s.ended_at_ms);
    all
}

/// Atomic write (temp file, then rename). Errors are returned for logging; callers keep going.
fn save_file(project: &Path, panes: BTreeMap<String, Vec<TurnSummary>>) -> std::io::Result<()> {
    let path = ring_path(project);
    if let Some(dir) = path.parent() {
        crate::git_exclude::ensure_project_storage_excluded(project);
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_vec(&RingFile { version: 1, panes }).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    if std::fs::rename(&tmp, &path).is_err() {
        // Windows refuses to rename over a file another process has open: replace in place.
        std::fs::remove_file(&path).ok();
        std::fs::rename(&tmp, &path)?;
    }
    Ok(())
}

/// Insert or replace the summary of the same turn, keeping order by turn and at most `cap` entries.
pub(super) fn upsert(ring: &mut VecDeque<TurnSummary>, summary: TurnSummary, cap: usize) {
    if let Some(slot) = ring.iter_mut().find(|s| s.turn == summary.turn) {
        // Summaries are built on worker threads: one for an earlier rest point of the turn (the
        // approval prompt) can land after the one for the end of the turn. The later rest point wins.
        if slot.ended_at_ms <= summary.ended_at_ms {
            *slot = summary;
        }
    } else {
        // Summaries are built on worker threads and can land out of order.
        let at = ring.iter().position(|s| s.turn > summary.turn).unwrap_or(ring.len());
        ring.insert(at, summary);
    }
    while ring.len() > cap {
        ring.pop_front();
    }
}

#[derive(Default)]
pub struct Store {
    rings: HashMap<String, VecDeque<TurnSummary>>,
    /// Which project folder each pane's summaries belong to (for the ring file).
    project_of: HashMap<String, PathBuf>,
    loaded: HashSet<PathBuf>,
}

impl Store {
    /// Merge a project's ring file into memory (once per project per run).
    pub fn ensure_loaded(&mut self, project: &Path) {
        if !self.loaded.insert(project.to_path_buf()) {
            return;
        }
        for (pane, items) in load_file(project) {
            self.project_of.entry(pane.clone()).or_insert_with(|| project.to_path_buf());
            let ring = self.rings.entry(pane).or_default();
            for item in items {
                upsert(ring, item, KEEP_PER_PANE);
            }
        }
    }

    /// Record a finished turn and write the project's ring file. The ring file sits in the project
    /// folder, so only redacted text may be written to it.
    pub fn record(&mut self, project: &Path, summary: Redacted<TurnSummary>) -> std::io::Result<()> {
        let summary = summary.into_inner();
        self.ensure_loaded(project);
        self.project_of.insert(summary.pane_id.clone(), project.to_path_buf());
        upsert(self.rings.entry(summary.pane_id.clone()).or_default(), summary, KEEP_PER_PANE);
        self.persist(project)
    }

    fn persist(&self, project: &Path) -> std::io::Result<()> {
        let mut panes: Vec<(&String, &VecDeque<TurnSummary>)> = self
            .rings
            .iter()
            .filter(|(pane, _)| self.project_of.get(*pane).is_some_and(|p| p == project))
            .collect();
        let newest = |r: &VecDeque<TurnSummary>| r.iter().map(|s| s.ended_at_ms).max().unwrap_or(0);
        panes.sort_by_key(|(_, r)| std::cmp::Reverse(newest(r)));
        panes.truncate(MAX_PANES_IN_FILE);
        save_file(project, panes.into_iter().map(|(p, r)| (p.clone(), r.iter().cloned().collect())).collect())
    }

    /// Oldest first.
    pub fn for_pane(&self, pane_id: &str) -> Vec<TurnSummary> {
        self.rings.get(pane_id).map(|r| r.iter().cloned().collect()).unwrap_or_default()
    }

    /// Every pane's summaries that ended at or after `since_ms`, oldest first.
    pub fn since(&self, since_ms: u64) -> Vec<TurnSummary> {
        let mut out: Vec<TurnSummary> =
            self.rings.values().flat_map(|r| r.iter()).filter(|s| s.ended_at_ms >= since_ms).cloned().collect();
        out.sort_by_key(|s| s.ended_at_ms);
        out
    }

    /// Like `since`, with the project folder each pane's summaries belong to (`None` = not known).
    pub fn since_with_project(&self, since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)> {
        let mut out: Vec<(TurnSummary, Option<PathBuf>)> = self
            .rings
            .iter()
            .flat_map(|(pane, ring)| {
                let project = self.project_of.get(pane).cloned();
                ring.iter().filter(move |s| s.ended_at_ms >= since_ms).map(move |s| (s.clone(), project.clone()))
            })
            .collect();
        out.sort_by_key(|(s, _)| s.ended_at_ms);
        out
    }
}
