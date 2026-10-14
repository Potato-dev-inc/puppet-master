//! Live activity line per pane ("Running npm test", "Waiting: allow `npm test`?").
//!
//! Cost model: nothing runs per byte. `note_output` (called from the bridge's SSE push helpers,
//! never from the PTY reader) only marks a pane dirty. One ticker thread wakes about once a
//! second, and for each dirty pane reads the visible screen tail, skips the work when the tail
//! is byte-identical to last time, otherwise asks the pane's profile for its `activity` and
//! emits `pty://activity` (Tauri) plus a `pane-activity` SSE event only when the phrase changed.
//! A silent pane costs nothing; a chatty one costs at most one screen read per second.

use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use crate::tui::Activity;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

/// Cadence of the ticker, and so the minimum gap between two recomputations of one pane.
pub const TICK: Duration = Duration::from_millis(1000);
/// Visible non-empty lines handed to the profile.
const TAIL_LINES: usize = 24;

/// What the ticker needs to know about one pane.
pub struct PaneSnapshot {
    pub agent_type: String,
    pub screen: String,
    pub status: PaneStatus,
}

#[derive(Default)]
struct State {
    dirty: HashSet<String>,
    /// Last tail hash and phrase per pane.
    last: HashMap<String, (u64, Activity)>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(Default::default);

/// Mark a pane as changed (new output or a status change). O(1), no I/O.
pub fn note_output(pane_id: &str) {
    let mut state = STATE.lock();
    if !state.dirty.contains(pane_id) {
        state.dirty.insert(pane_id.to_string());
    }
}

/// The current phrase of every pane that has one.
pub fn snapshot() -> HashMap<String, Activity> {
    STATE.lock().last.iter().map(|(id, (_, a))| (id.clone(), a.clone())).collect()
}

/// The last `TAIL_LINES` non-empty lines of a screen.
pub fn visible_tail(screen: &str) -> String {
    let mut lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).rev().take(TAIL_LINES).collect();
    lines.reverse();
    lines.join("\n")
}

fn hash_of(tail: &str, status: PaneStatus) -> u64 {
    let mut hasher = DefaultHasher::new();
    tail.hash(&mut hasher);
    status.as_str().hash(&mut hasher);
    hasher.finish()
}

/// One pass over the dirty panes. `read` fetches a pane (None when it is gone), `emit` receives
/// each changed phrase, `alive` lists the panes that still exist (others are forgotten).
/// Returns how many panes were actually recomputed (for tests and logging).
pub fn tick_with(
    read: &dyn Fn(&str) -> Option<PaneSnapshot>,
    alive: &dyn Fn(&str) -> bool,
    emit: &dyn Fn(&str, &Activity),
) -> usize {
    let dirty: Vec<String> = {
        let mut state = STATE.lock();
        state.last.retain(|id, _| alive(id));
        state.dirty.drain().collect()
    };
    let mut recomputed = 0;
    for id in dirty {
        let Some(pane) = read(&id) else { continue };
        let tail = visible_tail(&pane.screen);
        let hash = hash_of(&tail, pane.status);
        if STATE.lock().last.get(&id).is_some_and(|(old, _)| *old == hash) {
            continue; // screen and status unchanged: nothing to reclassify
        }
        recomputed += 1;
        // Dev-server URLs ride on the same throttle: only panes whose screen changed, once per tick.
        crate::pane_preview::observe(&id, &tail);
        crate::loop_guard::observe_pane(&id, &pane.agent_type, &tail, pane.status);
        // A screen the profile cannot read, held still, is saved for the person to look at.
        crate::canary::unknown::observe(&id, &pane.agent_type, &pane.screen, pane.status);
        let Some(activity) = crate::tui::profile_for(&pane.agent_type).activity(&tail, pane.status) else {
            STATE.lock().last.remove(&id);
            continue;
        };
        let changed = {
            let mut state = STATE.lock();
            let changed = state.last.get(&id).is_none_or(|(_, old)| *old != activity);
            state.last.insert(id.clone(), (hash, activity.clone()));
            changed
        };
        if changed {
            emit(&id, &activity);
        }
    }
    recomputed
}

/// Start the ticker. Call once at startup with the shared registry.
pub fn init(registry: Arc<Mutex<PaneRegistry>>, app: tauri::AppHandle) {
    let sink_app = app.clone();
    crate::pane_preview::install_sink(Box::new(move |id, urls| {
        use tauri::Emitter;
        let _ = sink_app.emit("pty://preview", serde_json::json!({ "pane_id": id, "urls": urls }));
        crate::bridge::push_pane_preview_sse(id, urls);
    }));
    std::thread::Builder::new()
        .name("pane-activity".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            let read = |id: &str| {
                let reg = registry.lock();
                let pane = reg.panes.get(id)?;
                let screen = pane.screen.lock().screen().contents();
                let status = *pane.status.lock();
                Some(PaneSnapshot { agent_type: pane.info.agent_type.clone(), screen, status })
            };
            let alive = |id: &str| registry.lock().panes.contains_key(id);
            let running = |id: &str| registry.lock().panes.get(id).is_some_and(|p| !*p.exited.lock());
            let emit = |id: &str, activity: &Activity| {
                use tauri::Emitter;
                let payload = serde_json::json!({ "pane_id": id, "activity": activity });
                let _ = app.emit("pty://activity", payload);
                crate::bridge::push_pane_activity_sse(id, activity);
            };
            tick_with(&read, &alive, &emit);
            crate::pane_preview::retain_alive(&running);
            crate::loop_guard::tick(&alive);
            crate::canary::unknown::tick(&alive);
        })
        .ok();
}

/// Tests that tick share the process-global state; they take this lock (also used by `pane_preview`).
#[cfg(test)]
pub(crate) static TEST_SERIAL: Mutex<()> = parking_lot::const_mutex(());

#[cfg(test)]
mod tests;
