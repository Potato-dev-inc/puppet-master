//! Follows pane status to find turns and keeps what happened during them (answered prompts), then
//! builds the summary when the turn comes to rest. Agent-agnostic: it sees only pane status,
//! prompts answered, the checkpoint diff and the run receipt, never an agent name.
//!
//! A turn opens when the pane starts running and stays open across `waiting_input` (an approval
//! resumes the same turn); it closes at `idle` or `error`. Every rest point (`idle`,
//! `waiting_input`, `error`) yields a summary of the turn so far, replacing the previous one of
//! the same turn, so "now waiting: allow `npm test`?" is shown as soon as the agent stops.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use super::model::{assemble, Approval, Facts, FileStats, ReceiptFacts, TurnSummary};
use super::store::Store;

#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub cwd: String,
    /// Display name of the agent ("Claude Code"); the agent id when it has none.
    pub label: String,
    /// Plain shells have no agent turns (their status flips on every command).
    pub has_turns: bool,
}

/// What the tracker needs from the outside world (a fake in tests).
pub trait Env: Send + Sync {
    fn now_ms(&self) -> u64;
    fn pane(&self, pane_id: &str) -> Option<PaneInfo>;
    /// Files changed since `started_at_ms` (`None` = unknown).
    fn files(&self, pane_id: &str, started_at_ms: u64) -> Option<FileStats>;
    /// The prompt the pane is showing now, shortened.
    fn end_prompt(&self, pane_id: &str) -> Option<String>;
    fn receipt(&self, pane_id: &str) -> Option<ReceiptFacts>;
}

/// A turn that has just come to rest, before its facts are read.
#[derive(Debug, Clone)]
pub struct Ended {
    pub pane_id: String,
    pub turn: u32,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub end_status: String,
    /// The pane was running just before (a checkpoint "after" snapshot is on its way).
    pub from_running: bool,
    pub approvals: Vec<Approval>,
    /// The pane's folder and agent label as they were when the turn came to rest (the status event
    /// runs while the pane is alive). `finish` runs later on a worker and must not need the pane: a
    /// run's pane is closed as soon as the run completes, usually before the summary is built.
    pub pane: Option<PaneInfo>,
}

#[derive(Default)]
struct Live {
    turn: u32,
    open: bool,
    started_at_ms: u64,
    last_status: String,
    approvals: Vec<Approval>,
    /// A prompt was typed into this turn (`note_dispatched`). A turn only the status opened is
    /// either the user typing in the pane themselves or the agent starting up.
    dispatched: bool,
    /// Startup gates (folder trust, sign-in) answered while no turn was open, with the time they were
    /// answered (tracker clock). The answer is sent before the pane's own `running` event arrives
    /// (live: 104 ms before), so it finds no turn to belong to; the turn the status then opens takes
    /// it over (see `on_status`), so the start-up can be told from a real turn.
    startup_answers: Vec<(u64, Approval)>,
    /// The last pane facts seen while the pane was alive (the fallback when it is gone at a rest point).
    pane: Option<PaneInfo>,
}

pub struct Tracker {
    env: Arc<dyn Env>,
    live: Mutex<HashMap<String, Live>>,
    store: Mutex<Store>,
}

/// A "turn" that runs for less than this, with no approval and no dispatched prompt, is a status flicker
/// (a redraw or resize read as "running"), not an agent turn. Live evidence: two 0.8s "turns"
/// that followed a real 166s one and became its "Last turn".
pub const BLIP_MS: u64 = 1_000;

/// How long an answered startup gate waits for the pane's `running` event (live: about 0.1 s). A turn
/// that opens later than this is not the agent starting up.
pub const STARTUP_CARRY_MS: u64 = 10_000;

/// Only startup screens were answered (at least one): the folder-trust dialog, a sign-in or setup screen.
fn is_startup(approvals: &[Approval]) -> bool {
    !approvals.is_empty() && approvals.iter().all(|a| crate::pane_prompt::STARTUP_GATE_KINDS.contains(&a.kind.as_str()))
}

fn is_rest(status: &str) -> bool {
    matches!(status, "idle" | "waiting_input" | "error")
}

impl Tracker {
    pub fn new(env: Arc<dyn Env>) -> Self {
        Self { env, live: Mutex::new(HashMap::new()), store: Mutex::new(Store::default()) }
    }

    /// Feed a status change. Returns the turn that just came to rest, if any.
    pub fn on_status(&self, pane_id: &str, status: &str) -> Option<Ended> {
        let mut live = self.live.lock();
        let entry = live.entry(pane_id.to_string()).or_default();
        let prev = std::mem::replace(&mut entry.last_status, status.to_string());
        if prev == status {
            return None;
        }
        if status == "running" {
            if !entry.open {
                let pane = self.env.pane(pane_id);
                if !pane.as_ref().is_some_and(|p| p.has_turns) {
                    return None;
                }
                entry.pane = pane;
                entry.turn += 1;
                entry.open = true;
                let now = self.env.now_ms();
                entry.started_at_ms = now;
                entry.approvals.clear();
                entry.dispatched = false;
                // A startup gate answered just before this turn opened belongs to it.
                let answers = std::mem::take(&mut entry.startup_answers);
                entry.approvals.extend(answers.into_iter().filter(|(at, _)| now.saturating_sub(*at) <= STARTUP_CARRY_MS).map(|(_, a)| a));
            }
            return None;
        }
        if !(entry.open && is_rest(status)) {
            return None;
        }
        let now = self.env.now_ms();
        // A turn PM itself dispatched (`note_dispatched`) is a turn however short: a trivial task a fast
        // agent answers in under a second must still get its Last turn / History / digest entry.
        if prev == "running"
            && matches!(status, "idle" | "error")
            && !entry.dispatched
            && entry.approvals.is_empty()
            && now.saturating_sub(entry.started_at_ms) < BLIP_MS
        {
            // Not a turn: give its number back so numbering stays the one checkpoints use.
            entry.turn = entry.turn.saturating_sub(1);
            entry.open = false;
            return None;
        }
        if matches!(status, "idle" | "error") && !entry.dispatched && is_startup(&entry.approvals) {
            // The agent starting up (its folder-trust or sign-in screen answered, then its composer),
            // not a turn: nobody asked it anything yet. Counting it made the first real turn "Turn 2".
            entry.turn = entry.turn.saturating_sub(1);
            entry.open = false;
            return None;
        }
        // Read the pane now, while it is alive; fall back to what was seen when the turn opened.
        let pane = self.env.pane(pane_id).or_else(|| entry.pane.clone());
        entry.pane = pane.clone();
        let ended = Ended {
            pane_id: pane_id.to_string(),
            turn: entry.turn,
            started_at_ms: entry.started_at_ms,
            ended_at_ms: self.env.now_ms(),
            end_status: status.to_string(),
            from_running: prev == "running",
            approvals: entry.approvals.clone(),
            pane,
        };
        if status != "waiting_input" {
            entry.open = false;
        }
        Some(ended)
    }

    /// A prompt was typed into `pane_id`: the turn starts now, even if the pane never reported
    /// `running` for it. A pane the run just spawned goes from its start to the end of its first
    /// task without ever reading `idle` in between, so its first status event is the `idle` that ends
    /// the turn, which used to find no turn open (no Last turn, no "after" snapshot).
    pub fn note_dispatched(&self, pane_id: &str) {
        let mut live = self.live.lock();
        let entry = live.entry(pane_id.to_string()).or_default();
        entry.last_status = "running".to_string();
        if entry.open {
            entry.dispatched = true;
            return;
        }
        let pane = self.env.pane(pane_id);
        if !pane.as_ref().is_some_and(|p| p.has_turns) {
            return;
        }
        entry.pane = pane;
        entry.turn += 1;
        entry.open = true;
        entry.dispatched = true;
        entry.started_at_ms = self.env.now_ms();
        entry.approvals.clear();
        entry.startup_answers.clear();
    }

    /// The turn number a checkpoint taken now belongs to. This is the single source of turn
    /// numbering: checkpoints (History) and summaries (Last turn) both use it. A "before"
    /// snapshot taken while no turn is open belongs to the turn about to start.
    pub fn turn_for_checkpoint(&self, pane_id: &str, before: bool) -> Option<u32> {
        let live = self.live.lock();
        let entry = live.get(pane_id)?;
        Some(if before && !entry.open { entry.turn + 1 } else { entry.turn.max(1) })
    }

    /// An answered prompt belongs to the open turn. With no turn open it is ignored, except an answer
    /// to a startup gate, which is kept for the turn the pane opens next (`startup_answers`).
    /// Returns whether it was kept (a replayed answer, or one with no turn to belong to, is not).
    pub fn record_approval(&self, pane_id: &str, approval: Approval) -> bool {
        let mut live = self.live.lock();
        let now = self.env.now_ms();
        let entry = live.entry(pane_id.to_string()).or_default();
        if entry.open {
            let duplicate = !approval.prompt_id.is_empty() && entry.approvals.iter().any(|a| a.prompt_id == approval.prompt_id);
            if !duplicate {
                entry.approvals.push(approval);
            }
            !duplicate
        } else if is_startup(std::slice::from_ref(&approval)) {
            entry.startup_answers.push((now, approval));
            true
        } else {
            false
        }
    }

    /// Sends an answer (`send` types the key) and keeps it on the turn. The ANSWER IS NOTED BEFORE
    /// `send` RUNS: typing it lets the agent carry on, and a fast agent finishes the turn (the pane goes
    /// `idle`, `on_status` copies the approvals) before the answering thread gets back from `send`. An
    /// answer noted after `send` was then missing from that summary ("approvals": []) and, the turn
    /// being closed, dropped for good. If `send` says the answer was not delivered (`delivered`
    /// returns false: refused, or a repeat that pressed nothing) the note is taken back.
    pub fn answering<T>(
        &self,
        pane_id: &str,
        approval: Approval,
        send: impl FnOnce() -> T,
        delivered: impl FnOnce(&T) -> bool,
    ) -> T {
        let noted = self.record_approval(pane_id, approval.clone());
        let result = send();
        if noted && !delivered(&result) {
            self.discard_approval(pane_id, &approval);
        }
        result
    }

    /// Takes back an approval [`answering`](Self::answering) noted.
    fn discard_approval(&self, pane_id: &str, approval: &Approval) {
        let mut live = self.live.lock();
        let Some(entry) = live.get_mut(pane_id) else { return };
        if let Some(at) = entry.approvals.iter().rposition(|a| a == approval) {
            entry.approvals.remove(at);
        } else if let Some(at) = entry.startup_answers.iter().rposition(|(_, a)| a == approval) {
            entry.startup_answers.remove(at);
        }
    }

    /// Read the facts of a turn that came to rest, build its summary and keep it. Works from the pane
    /// facts captured at the rest point, so a pane that was closed meanwhile (the default for a run)
    /// still gets its summary; only the file counts and receipt, which need the live pane, may then be
    /// unknown.
    pub fn finish(&self, ended: Ended) -> Option<TurnSummary> {
        let pane = ended.pane.clone().or_else(|| self.env.pane(&ended.pane_id))?;
        let end_prompt = (ended.end_status == "waiting_input").then(|| self.env.end_prompt(&ended.pane_id)).flatten();
        let summary = assemble(Facts {
            pane_id: ended.pane_id.clone(),
            agent: pane.label,
            turn: ended.turn,
            started_at_ms: ended.started_at_ms,
            ended_at_ms: ended.ended_at_ms,
            files: self.env.files(&ended.pane_id, ended.started_at_ms),
            approvals: ended.approvals,
            end_status: ended.end_status,
            end_prompt,
            receipt: self.env.receipt(&ended.pane_id),
        });
        // Secrets in a command or a prompt never reach the ring file (or the phone).
        let summary = crate::redact::scrub(summary);
        if let Err(e) = self.store.lock().record(&PathBuf::from(&pane.cwd), summary.clone()) {
            tracing::warn!(pane = %summary.get().pane_id, error = %e, "could not save turn summary ring file");
        }
        Some(summary.into_inner())
    }

    /// Oldest first; loads the pane's project file on first use so history survives a restart.
    pub fn summaries(&self, pane_id: &str) -> Vec<TurnSummary> {
        let mut store = self.store.lock();
        if let Some(pane) = self.env.pane(pane_id) {
            store.ensure_loaded(&PathBuf::from(pane.cwd));
        }
        store.for_pane(pane_id)
    }

    /// Everything that ended at or after `since_ms`, across panes.
    pub fn since(&self, since_ms: u64) -> Vec<TurnSummary> {
        self.store.lock().since(since_ms)
    }

    /// `since`, each with the project folder of its pane (the companion's `today.md` shows its name).
    pub fn since_with_project(&self, since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)> {
        self.store.lock().since_with_project(since_ms)
    }
}
