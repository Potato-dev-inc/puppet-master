//! The usage-limit log: one JSON line when an agent hits its limit and one when it is over, appended
//! to `<app data>/plans-limit-log.jsonl`. Limit times were never kept anywhere else (the Inbox card
//! goes away with the limit), so "how long was it limited" is only known from the day this log began.
//!
//! No chat text, no folder, no screen text: the agent id, the account id, a pane id (to pair the two
//! lines) and times. Append-only; a damaged line is skipped.

use super::model::LimitFact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "plans-limit-log.jsonl";

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Entry {
    /// The log began (written once, so "counting since" has a day even before the first limit).
    Begin { at_ms: u64 },
    /// An agent stopped at its usage limit.
    Start {
        at_ms: u64,
        pane: String,
        agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        /// When the agent said it can continue, if it said.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reset_at_ms: Option<u64>,
    },
    /// The limit is over for that pane (the agent carried on, or the screen moved on).
    End { at_ms: u64, pane: String },
}

pub fn default_path() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE_NAME)
}

/// Add one line. Errors are returned for the caller to ignore: a log must never break the app.
pub fn append(path: &Path, entry: &Entry) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// Write the `begin` line if the log does not exist yet. (The live tick calls it; not built in unit tests.)
#[cfg_attr(test, allow(dead_code))]
pub fn ensure_begun(path: &Path, now_ms: u64) {
    if !path.exists() {
        let _ = append(path, &Entry::Begin { at_ms: now_ms });
    }
}

/// Every readable line, oldest first.
pub fn read(path: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines().filter_map(|line| serde_json::from_str::<Entry>(line.trim()).ok()).collect()
}

/// When the log began: the earliest time on any line.
pub fn counting_since(entries: &[Entry]) -> Option<u64> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Begin { at_ms } | Entry::Start { at_ms, .. } | Entry::End { at_ms, .. } => *at_ms,
        })
        .min()
}

struct Open {
    agent: String,
    account: Option<String>,
    start_ms: u64,
    reset_at_ms: Option<u64>,
}

impl Open {
    /// `end`, but never later than the reset time the agent gave: a screen that still shows the limit
    /// after the plan renewed is not time spent limited.
    fn capped(&self, end: u64) -> u64 {
        match self.reset_at_ms {
            Some(reset) if reset > self.start_ms => end.min(reset),
            _ => end,
        }
    }

    fn into_fact(self, end_ms: Option<u64>) -> LimitFact {
        LimitFact { agent_id: self.agent, account: self.account, start_ms: self.start_ms, end_ms }
    }
}

/// The hits in the log, each with its end.
///
/// - An `end` closes the open limit of its pane.
/// - A limit with no `end` that is still on a live pane (`still_limited`) lasts until `now`.
///   Otherwise its end is the agent's reset time once that has passed, else unknown (`None`: it
///   counts as a hit and adds no waiting time).
pub fn hits(entries: &[Entry], now_ms: u64, still_limited: &dyn Fn(&str) -> bool) -> Vec<LimitFact> {
    // Keyed by pane in a BTreeMap so the walk over open limits is in the same order every run.
    let mut done: Vec<(String, LimitFact)> = Vec::new();
    let mut open: BTreeMap<String, Open> = BTreeMap::new();
    for entry in entries {
        match entry {
            Entry::Begin { .. } => {}
            Entry::Start { at_ms, pane, agent, account, reset_at_ms } => {
                let next = Open { agent: agent.clone(), account: account.clone(), start_ms: *at_ms, reset_at_ms: *reset_at_ms };
                if let Some(old) = open.insert(pane.clone(), next) {
                    // A newer limit with no end between: the old one was over by then.
                    let end = old.capped(*at_ms);
                    done.push((pane.clone(), old.into_fact(Some(end))));
                }
            }
            Entry::End { at_ms, pane } => {
                if let Some(old) = open.remove(pane) {
                    let end = old.capped((*at_ms).max(old.start_ms));
                    done.push((pane.clone(), old.into_fact(Some(end))));
                }
            }
        }
    }
    for (pane, old) in open {
        let end = if still_limited(&pane) {
            Some(old.capped(now_ms.max(old.start_ms)))
        } else {
            old.reset_at_ms.filter(|reset| *reset > old.start_ms && *reset <= now_ms)
        };
        done.push((pane, old.into_fact(end)));
    }
    // A total order: start time, then pane id, then end time, agent and account. Two hits can only
    // be equal in every field that is kept, so the output never depends on the order of the input map.
    done.sort_by(|(pa, a), (pb, b)| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| pa.cmp(pb))
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| a.agent_id.cmp(&b.agent_id))
            .then_with(|| a.account.cmp(&b.account))
    });
    done.into_iter().map(|(_, fact)| fact).collect()
}
