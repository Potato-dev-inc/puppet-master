//! "While you were away": one calm summary built only from turn summaries and the panes that
//! need the user right now. Used by the desktop cockpit (on return) and the phone
//! (`GET /digest?since=`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::TurnSummary;

/// Lines in a digest (one per agent); the counter still reports every agent that worked.
pub const MAX_LINES: usize = 12;

/// One line of the digest: an agent's latest summary since the away start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestLine {
    pub pane_id: String,
    pub agent: String,
    /// How many turns this agent finished while you were away.
    pub turns: u32,
    pub sentence: String,
    pub ended_at_ms: u64,
    /// `idle`, `waiting_input` or `error`.
    pub end: String,
}

/// An agent that needs the user right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsYou {
    pub pane_id: String,
    pub agent: String,
    /// `waiting_input` or `error`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Digest {
    pub since_ms: u64,
    pub now_ms: u64,
    /// Agents that finished at least one turn since `since_ms`.
    pub agents_worked: u32,
    pub turns: u32,
    pub lines: Vec<DigestLine>,
    pub needs_you: Vec<NeedsYou>,
}

impl Digest {
    /// Nothing happened while away: show nothing.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Build the digest for `since_ms..now_ms` from every summary that ended in that window.
pub fn build(summaries: &[TurnSummary], needs_you: Vec<NeedsYou>, since_ms: u64, now_ms: u64) -> Digest {
    let mut per_pane: BTreeMap<&str, (&TurnSummary, u32)> = BTreeMap::new();
    for s in summaries.iter().filter(|s| s.ended_at_ms >= since_ms) {
        let slot = per_pane.entry(s.pane_id.as_str()).or_insert((s, 0));
        if s.ended_at_ms >= slot.0.ended_at_ms {
            slot.0 = s;
        }
        slot.1 += 1;
    }
    let mut lines: Vec<DigestLine> = per_pane
        .values()
        .map(|(s, turns)| DigestLine {
            pane_id: s.pane_id.clone(),
            agent: s.agent.clone(),
            turns: *turns,
            sentence: s.sentence.clone(),
            ended_at_ms: s.ended_at_ms,
            end: s.end.kind.clone(),
        })
        .collect();
    let agents_worked = lines.len() as u32;
    let turns = lines.iter().map(|l| l.turns).sum();
    // Newest first, so the agent that just stopped is at the top.
    lines.sort_by_key(|l| std::cmp::Reverse(l.ended_at_ms));
    lines.truncate(MAX_LINES);
    Digest { since_ms, now_ms, agents_worked, turns, lines, needs_you }
}

/// When each paired phone last looked at its digest, persisted so a restart keeps it.
#[derive(Debug, Default)]
pub struct LastSeen {
    path: Option<PathBuf>,
    by_device: HashMap<String, u64>,
}

#[derive(Serialize, Deserialize, Default)]
struct SeenFile {
    devices: BTreeMap<String, u64>,
}

impl LastSeen {
    pub fn load(path: &Path) -> Self {
        let by_device = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<SeenFile>(&t).ok())
            .map(|f| f.devices.into_iter().collect())
            .unwrap_or_default();
        Self { path: Some(path.to_path_buf()), by_device }
    }

    pub fn get(&self, device: &str) -> Option<u64> {
        self.by_device.get(device).copied()
    }

    pub fn set(&mut self, device: &str, at_ms: u64) {
        self.by_device.insert(device.to_string(), at_ms);
        // Keep the file small: at most 50 devices (the most recently seen).
        if self.by_device.len() > 50 {
            if let Some(oldest) = self.by_device.iter().min_by_key(|(_, v)| **v).map(|(k, _)| k.clone()) {
                self.by_device.remove(&oldest);
            }
        }
        if let Some(path) = &self.path {
            let file = SeenFile { devices: self.by_device.iter().map(|(k, v)| (k.clone(), *v)).collect() };
            if let Ok(body) = serde_json::to_vec(&file) {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, body);
            }
        }
    }
}
