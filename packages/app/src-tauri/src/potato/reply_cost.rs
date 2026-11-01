//! What one Fish reply cost, in nano-dollars (a billionth of a dollar). The cost is an estimate from the tokens the
//! brain reports. It lives in a small sidecar file (`reply_costs.json` in the Potato root), keyed by message id, so the
//! conversation files stay as they are. `service` records the cost when a turn's usage arrives and reads it back when
//! the messages are listed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::store;

/// The newest costs kept. Older ones are dropped; their replies just show no cost.
pub const KEEP: usize = 2000;

#[derive(Serialize, Deserialize, Default)]
struct Costs {
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    id: String,
    nano: u64,
}

pub fn path(root: &Path) -> PathBuf {
    root.join("reply_costs.json")
}

fn read(root: &Path) -> Costs {
    fs::read_to_string(path(root)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// Remember what the reply `message_id` cost. Best effort: a failed write leaves the reply without a cost.
pub fn record(root: &Path, message_id: &str, cost_nano: u64) -> Result<(), String> {
    let _files = store::files_lock();
    let mut costs = read(root);
    costs.entries.push(Entry { id: message_id.to_string(), nano: cost_nano });
    let extra = costs.entries.len().saturating_sub(KEEP);
    costs.entries.drain(..extra);
    let text = serde_json::to_string(&costs).map_err(|e| e.to_string())?;
    crate::secure_file::write_private(&path(root), &text)
}

/// Message id to cost in nano-dollars, for the replies that have one.
pub fn load(root: &Path) -> HashMap<String, u64> {
    read(root).entries.into_iter().map(|e| (e.id, e.nano)).collect()
}
