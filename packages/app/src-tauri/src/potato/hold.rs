//! "Stop everything" hold for Potato Bot (UX-1). The panic button pauses every bot, holds every routine and puts
//! Fish's computer to sleep. The hold is one small file (`hold.json`) in the Potato folder, so a restart keeps it.
//! While it is on: no brain is given a turn (`send_as` refuses), and a routine that comes due is written into its
//! bot's thread as "Skipped while paused" instead of being sent, so nothing is silently lost.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::routines;
use super::service::Service;

pub const FILE: &str = "hold.json";

/// What the hold holds, and what it stopped (the sheet shows the counts).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Hold {
    pub held: bool,
    pub since_ms: Option<i64>,
    pub bots_paused: u32,
    pub routines_held: u32,
    pub computer_asleep: bool,
}

pub fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// A missing file means not held. A damaged file means held (the safe side) until "Start bots again".
pub fn load(root: &Path) -> Hold {
    match std::fs::read_to_string(path(root)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(%e, "hold.json is damaged; Potato Bot stays paused until Start bots again");
            Hold { held: true, ..Hold::default() }
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Hold::default(),
        Err(e) => {
            tracing::warn!(%e, "hold.json could not be read; Potato Bot stays paused");
            Hold { held: true, ..Hold::default() }
        }
    }
}

pub fn is_held(root: &Path) -> bool {
    load(root).held
}

/// Writes the hold through a temporary file, so a crash never leaves half a file.
pub fn save(root: &Path, hold: &Hold) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let tmp = root.join("hold.json.tmp");
    let text = serde_json::to_string_pretty(hold).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path(root))
}

/// The line a routine leaves in its bot's thread when it came due while the hold was on.
pub fn skipped_note(title: &str) -> String {
    format!("Skipped while paused: {title}")
}

/// "Stop everything": saves the hold FIRST (so a routine tick cannot slip through), then stops every brain
/// (asked to leave, killed after `grace`). Returns what was held.
pub fn pause(root: &Path, service: Option<&Service>, computer_asleep: bool, grace: Duration, now_ms: i64) -> Hold {
    let bots_paused = service.map(|s| s.running_bots()).unwrap_or(0) as u32;
    let routines_held = routines::list(root, None).map(|v| v.iter().filter(|r| r.status == "active").count() as u32).unwrap_or(0);
    let hold = Hold { held: true, since_ms: Some(now_ms), bots_paused, routines_held, computer_asleep };
    if let Err(e) = save(root, &hold) {
        tracing::warn!(%e, "could not save the Potato Bot hold");
    }
    if let Some(service) = service {
        service.stop_all(grace);
    }
    hold
}

/// "Start bots again": clears the hold. Agents stay stopped; routines run again from the next tick.
pub fn resume(root: &Path) -> std::io::Result<Hold> {
    let hold = Hold::default();
    save(root, &hold)?;
    Ok(hold)
}
