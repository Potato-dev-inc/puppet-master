//! Per-pane hook ledger (§4.6): the last 256 events plus a small state machine (connected, turn in
//! flight, open turn, pending permission requests, edited paths). Other modules read
//! [`HookFacts`], never raw events. Memory only; nothing here is written to disk.

use std::collections::{HashMap, VecDeque};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use super::ingest::{project, Dropped};
use super::model::{HookEvent, HookKind};
use crate::tui::declarative::hooks::HooksTable;

pub const RING: usize = 256;
const CLOSED_TURNS: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub events: u64,
    pub foreign: u64,
    pub subagent: u64,
    pub late: u64,
    pub unparsed: u64,
    pub unknown_event: u64,
}

#[derive(Default)]
struct PaneLedger {
    ring: VecDeque<HookEvent>,
    seq: u64,
    pinned_session: Option<String>,
    connected: bool,
    turn_in_flight: Option<bool>,
    open_turn: Option<String>,
    closed_turns: VecDeque<String>,
    pending_permissions: Vec<(Option<String>, u64)>,
    attributed: Vec<String>,
    last_turn_end_at: Option<u64>,
    last_final_text: Option<crate::redact::Redacted<String>>,
    counts: Counts,
}

/// What other modules may know about a pane's hooks (§4.6). No raw payloads, no free text.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HookFacts {
    /// At least one event arrived from this pane's agent.
    pub connected: bool,
    /// `None` = no hook evidence about turns in this session yet.
    pub turn_in_flight: Option<bool>,
    pub open_turn: Option<String>,
    pub pending_permissions: usize,
    pub attributed_paths: Vec<String>,
    pub last_kind: Option<HookKind>,
    pub last_at_ms: Option<u64>,
    pub last_turn_end_at_ms: Option<u64>,
    pub counts: Counts,
}

static LEDGERS: Lazy<Mutex<HashMap<String, PaneLedger>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// Agents that reported at least once since launch (kept after their panes close).
static EVER_CONNECTED: Lazy<Mutex<std::collections::HashSet<String>>> = Lazy::new(|| Mutex::new(Default::default()));

/// Project `payload` and apply it to `pane_id`'s ledger. Returns the stored event and whether it
/// changed the pane's state (`false` for a foreign, subagent or late event: recorded, nothing
/// more), or why nothing was stored.
pub fn append(
    scheme: &HooksTable,
    payload: &Value,
    pane_id: &str,
    agent: &str,
    project_dir: &str,
    now_ms: u64,
) -> Result<(HookEvent, bool), Dropped> {
    let mut all = LEDGERS.lock();
    let ledger = all.entry(pane_id.to_string()).or_default();
    let event = match project(scheme, payload, pane_id, agent, project_dir, ledger.seq + 1, now_ms) {
        Ok(event) => event,
        Err(dropped) => {
            match dropped {
                Dropped::Unparsed => ledger.counts.unparsed += 1,
                Dropped::UnknownEvent => ledger.counts.unknown_event += 1,
            }
            return Err(dropped);
        }
    };
    ledger.seq = event.seq;
    let (mut event, applied) = ledger.apply(event);
    event.applied = applied;
    if !event.foreign {
        EVER_CONNECTED.lock().insert(agent.to_string());
    }
    if ledger.ring.len() == RING {
        ledger.ring.pop_front();
    }
    ledger.ring.push_back(event.clone());
    Ok((event, applied))
}

impl PaneLedger {
    fn apply(&mut self, mut event: HookEvent) -> (HookEvent, bool) {
        self.counts.events += 1;
        // Session pinning (§4.1): the first session seen pins; a new session may replace it only
        // after this one ended or restarted (clear/resume/startup).
        let restarts = event.kind == HookKind::SessionStart
            && matches!(event.detail.source.as_deref(), Some("startup" | "clear" | "resume" | "compact"));
        match (&self.pinned_session, &event.session) {
            (None, Some(session)) => self.pinned_session = Some(session.clone()),
            (Some(pinned), Some(session)) if pinned != session => {
                if restarts {
                    self.pinned_session = Some(session.clone());
                    self.open_turn = None;
                    self.turn_in_flight = Some(false);
                } else {
                    event.foreign = true;
                    self.counts.foreign += 1;
                    return (event, false);
                }
            }
            _ => {}
        }
        self.connected = true;
        if event.subagent.is_some() || event.kind == HookKind::SubagentEnd {
            // A subagent never opens or closes the user's turn.
            self.counts.subagent += 1;
            return (event, false);
        }
        if let Some(turn) = &event.turn_key {
            if self.closed_turns.contains(turn) && self.open_turn.as_ref() != Some(turn) {
                // Async delivery: a late event for a turn that already ended never reopens it.
                // An Interrupt right after its own Stop is the end reason, not a new state.
                self.counts.late += 1;
                return (event, false);
            }
        }
        match event.kind {
            HookKind::SessionStart => {
                if self.turn_in_flight.is_none() {
                    self.turn_in_flight = Some(false);
                }
            }
            HookKind::PromptSubmitted => {
                self.open_turn = event.turn_key.clone();
                self.turn_in_flight = Some(true);
                self.pending_permissions.clear();
                self.attributed.clear();
            }
            HookKind::PermissionRequest => {
                self.turn_in_flight = Some(true);
                self.pending_permissions.push((event.detail.tool.clone(), event.at_ms));
            }
            HookKind::ToolPre | HookKind::ToolPost => {
                self.turn_in_flight = Some(true);
                if let Some(i) = self.pending_permissions.iter().position(|(tool, _)| *tool == event.detail.tool) {
                    self.pending_permissions.remove(i);
                }
                for path in &event.detail.paths {
                    if !self.attributed.contains(path) && self.attributed.len() < 256 {
                        self.attributed.push(path.clone());
                    }
                }
            }
            HookKind::TurnEnd | HookKind::TurnFailed | HookKind::TurnInterrupted => {
                if event.detail.continuing != Some(true) || event.kind != HookKind::TurnEnd {
                    self.turn_in_flight = Some(false);
                }
                self.pending_permissions.clear();
                self.last_turn_end_at = Some(event.at_ms);
                if let Some(text) = &event.detail.final_text {
                    self.last_final_text = Some(text.clone());
                }
                if let Some(turn) = event.turn_key.clone().or_else(|| self.open_turn.clone()) {
                    if !self.closed_turns.contains(&turn) {
                        if self.closed_turns.len() == CLOSED_TURNS {
                            self.closed_turns.pop_front();
                        }
                        self.closed_turns.push_back(turn);
                    }
                }
                self.open_turn = None;
            }
            HookKind::SessionEnd => {
                self.pinned_session = None;
                self.turn_in_flight = Some(false);
                self.open_turn = None;
                self.pending_permissions.clear();
            }
            HookKind::SubagentEnd | HookKind::Notice => {}
        }
        (event, true)
    }

    fn facts(&self) -> HookFacts {
        let last = self.ring.back();
        HookFacts {
            connected: self.connected,
            turn_in_flight: self.turn_in_flight,
            open_turn: self.open_turn.clone(),
            pending_permissions: self.pending_permissions.len(),
            attributed_paths: self.attributed.clone(),
            last_kind: last.map(|e| e.kind),
            last_at_ms: last.map(|e| e.at_ms),
            last_turn_end_at_ms: self.last_turn_end_at,
            counts: self.counts.clone(),
        }
    }
}

pub fn facts(pane_id: &str) -> Option<HookFacts> {
    LEDGERS.lock().get(pane_id).map(PaneLedger::facts)
}

/// The agent's own last reply on this pane (redacted), for the open turn's receipt in v1.
pub fn last_final_text(pane_id: &str) -> Option<crate::redact::Redacted<String>> {
    LEDGERS.lock().get(pane_id).and_then(|l| l.last_final_text.clone())
}

/// The last events of a pane (tests and the shadow report).
pub fn recent(pane_id: &str) -> Vec<HookEvent> {
    LEDGERS.lock().get(pane_id).map(|l| l.ring.iter().cloned().collect()).unwrap_or_default()
}

/// True once any pane of this agent has reported (the doctor's "connected").
pub fn agent_ever_connected(agent: &str) -> bool {
    EVER_CONNECTED.lock().contains(agent)
}

pub fn forget(pane_id: &str) {
    LEDGERS.lock().remove(pane_id);
}
