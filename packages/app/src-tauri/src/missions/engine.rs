//! The mission loop (DESIGN-MISSIONS 3): one thread per active mission. It holds handles and never
//! runs an agent itself: every turn is an ordinary `run_agent` / `followup_task` operation, whose
//! idempotency key is saved before it is started (persist-before-act), so a restart finds the turn
//! again instead of starting a second one.
//!
//! The world is reached through `RoundIo` (live: `io::LiveIo`; tests: a scripted fake). Checks,
//! the test-change guard and the mission checkpoints run for real on the copy in both.

use super::cards;
use super::decide::{self, Signals};
use super::model::*;
use super::prompt;
use super::store::{self, LoadError};
use super::verdict;
use crate::operations::OperationError;
use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A turn the world started (or found again by its key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub operation_id: String,
    pub handle: String,
    pub pane_id: Option<String>,
}

pub trait RoundIo: Send + Sync {
    /// Start the round's turn: a new run in the mission's copy, or the next turn of `r.handle`.
    fn start_round(&self, m: &Mission, r: &Round, prompt: &str) -> Result<Started, OperationError>;
    /// The turn created with this idempotency key, if there is one.
    fn find_by_key(&self, project: &str, key: &str) -> Option<Started>;
    /// Blocks until the turn is over. `None` when `interrupt()` asked to stop waiting.
    fn wait_round(&self, project: &str, operation_id: &str, interrupt: &dyn Fn() -> bool) -> Option<RoundEnd>;
    /// Stop the turn now (the person stopped the mission).
    fn cancel_round(&self, project: &str, handle: &str);
    /// `BUDGET_EXHAUSTED` when no further turn may start on `key`.
    fn budget_gate(&self, project: &str, key: &str) -> Result<(), OperationError>;
    /// Whether the person chose "Stop here" on this budget.
    fn budget_stopped(&self, project: &str, key: &str) -> bool;
    /// The old agent hands off: its budget share is given back (it can start no more turns).
    fn budget_stop(&self, project: &str, handle: &str);
    /// "Try 5 more turns": add turns to the mission's budget.
    fn budget_add_turns(&self, project: &str, key: &str, turns: u32);
    fn loop_signal(&self, pane_id: &str) -> Option<String>;
    fn dismiss_loop(&self, pane_id: &str);
    /// A pane is working in the copy right now (a rollback would race it).
    fn panes_running_in(&self, copy: &Path) -> bool;
    /// Turns without progress before an agent counts as stuck (Settings, loop guard).
    fn stall(&self) -> u32 {
        DEFAULT_STALL
    }
    /// How often to look again while waiting for the person.
    fn poll(&self) -> Duration {
        Duration::from_secs(2)
    }
}

/// What the person asked for, applied by the engine thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonAction {
    UndoTestChanges { by: String },
    AcceptTestChanges { by: String },
    TryMoreTurns,
    HandToMe,
    TryAgain,
    Resume,
}

#[derive(Default)]
pub struct Control {
    pub stop: AtomicBool,
    /// "Pause after this turn" (1) or "never mind" (2), applied by the engine thread so the person's
    /// change is never overwritten by a save the engine was already making.
    pub pause: std::sync::atomic::AtomicU8,
    pub stop_by: Mutex<Option<String>>,
    action: Mutex<Option<PersonAction>>,
    wake_flag: Mutex<bool>,
    wake: Condvar,
}

impl Control {
    pub fn wake(&self) {
        *self.wake_flag.lock() = true;
        self.wake.notify_all();
    }

    fn sleep(&self, dur: Duration) {
        let mut flag = self.wake_flag.lock();
        if !*flag {
            self.wake.wait_for(&mut flag, dur);
        }
        *flag = false;
    }

    pub fn set_action(&self, action: PersonAction) {
        *self.action.lock() = Some(action);
        self.wake();
    }

    fn take_action(&self) -> Option<PersonAction> {
        self.action.lock().take()
    }
}

/// The engines running in this process, by mission id: two engines for one mission can't start.
static ENGINES: Lazy<Mutex<HashMap<String, Arc<Control>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn control_of(id: &str) -> Option<Arc<Control>> {
    ENGINES.lock().get(id).cloned()
}

/// Start the engine thread for a mission. `None` when one is already running for it.
pub fn spawn(io: Arc<dyn RoundIo>, project: String, id: String) -> Option<Arc<Control>> {
    let control = {
        let mut engines = ENGINES.lock();
        if engines.contains_key(&id) {
            return None;
        }
        let control = Arc::new(Control::default());
        engines.insert(id.clone(), control.clone());
        control
    };
    let ctl = control.clone();
    let thread_id = id.clone();
    let spawned = std::thread::Builder::new().name(format!("mission-{id}")).spawn(move || {
        run(io.as_ref(), &project, &thread_id, &ctl);
        ENGINES.lock().remove(&thread_id);
    });
    if spawned.is_err() {
        ENGINES.lock().remove(&id);
        return None;
    }
    Some(control)
}

fn now() -> u64 {
    store::now_ms()
}

fn copy_of(m: &Mission) -> Option<PathBuf> {
    m.worktree.as_ref().filter(|w| w.closed.is_none()).map(|w| PathBuf::from(&w.cwd))
}

/// Saves the mission. The ledger and the card go first, so whoever reads the new state finds the
/// card that goes with it already in the attention list.
fn save(m: &mut Mission) {
    super::ledger::write(m);
    cards::sync(m);
    if let Err(error) = store::save(m) {
        tracing::warn!(%error, mission = %m.id, "could not save the mission");
    }
}

fn log(m: &Mission, event: &str, extra: serde_json::Value) {
    let mut line = serde_json::json!({ "at_ms": now(), "event": event });
    if let (Some(map), Some(more)) = (line.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    store::log(&m.project, &m.id, &line);
}

/// The engine's loop. Returns when the mission is in a final state (or its record is gone).
pub fn run(io: &dyn RoundIo, project: &str, id: &str, ctl: &Control) {
    loop {
        let mut m = match store::load(project, id) {
            Ok(m) => m,
            Err(LoadError::NotFound) => return,
            Err(LoadError::Corrupt(error)) => {
                tracing::warn!(%error, mission = id, "a mission's record can't be read");
                return;
            }
        };
        if m.state.terminal() {
            cards::sync(&m);
            return;
        }
        if ctl.stop.load(Ordering::SeqCst) {
            stop(io, &mut m, ctl);
            continue;
        }
        match ctl.pause.swap(0, Ordering::SeqCst) {
            1 => {
                match m.state {
                    MissionState::NeedsYou => {
                        m.paused_from = Some(MissionState::NeedsYou);
                        m.state = MissionState::Paused;
                        m.push_event(now(), "paused", ":", "Paused by you", m.rounds.last().map(|r| r.n));
                    }
                    MissionState::Paused => {}
                    _ => m.pause_requested = true,
                }
                save(&mut m);
                continue;
            }
            2 => {
                m.pause_requested = false;
                save(&mut m);
                continue;
            }
            _ => {}
        }
        match m.state {
            MissionState::Planning => plan(io, &mut m),
            MissionState::Working => work(io, &mut m, ctl),
            MissionState::Checking => check(io, &mut m, ctl),
            MissionState::NeedsYou | MissionState::Paused => wait_for_person(io, &mut m, ctl),
            _ => return,
        }
    }
}

fn stop(io: &dyn RoundIo, m: &mut Mission, ctl: &Control) {
    if let Some(r) = m.rounds.last() {
        if r.end.is_none() {
            if let Some(handle) = r.handle.as_deref() {
                io.cancel_round(&m.project, handle);
            }
        }
    }
    let turn = m.rounds.iter().rev().find(|r| r.operation_id.is_some()).map(|r| r.n).unwrap_or(0);
    let by = ctl.stop_by.lock().clone().unwrap_or_else(|| "you".into());
    m.state = MissionState::Stopped;
    m.needs = None;
    m.sentence = Some(if turn > 0 { format!("Stopped by {by} after turn {turn}") } else { format!("Stopped by {by}") });
    m.push_event(now(), "stopped", ":", if by == "you" { "Stopped by you".to_string() } else { format!("Stopped by {by}") }, None);
    crate::audit::record(crate::audit::Kind::MissionStopped, who(&by), &format!("Stopped the mission \"{}\"", short(&m.objective)), serde_json::json!({ "mission": m.handle() }));
    log(m, "stopped", serde_json::json!({ "by": by }));
    save(m);
}

pub(super) fn who(by: &str) -> crate::audit::Who {
    match by {
        "you" | "desktop" => crate::audit::Who::Desktop,
        other if other.starts_with("device:") => crate::audit::Who::Device(Some(other.trim_start_matches("device:").to_string())),
        other if other.starts_with("agent:") => crate::audit::Who::Agent(other.trim_start_matches("agent:").to_string()),
        _ => crate::audit::Who::Local,
    }
}

pub(super) fn short(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut out: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        out.push('…');
    }
    out
}

fn needs_you(m: &mut Mission, why: Needs, sentence: String) {
    m.state = MissionState::NeedsYou;
    m.needs = Some(why);
    m.sentence = Some(sentence.clone());
    m.push_event(now(), why.as_str(), "+", sentence, m.rounds.last().map(|r| r.n));
}

fn fail(m: &mut Mission, sentence: &str) {
    m.state = MissionState::Failed;
    m.needs = None;
    m.sentence = Some(sentence.to_string());
    m.push_event(now(), "failed", "x", format!("Ended · {sentence}"), None);
}

// ---- planning ----------------------------------------------------------------------------------

fn plan(io: &dyn RoundIo, m: &mut Mission) {
    if m.worktree.is_none() {
        match crate::worktrees::create(&m.project, &m.handle()) {
            Ok(created) => {
                m.worktree = Some(created);
                log(m, "copy_created", serde_json::json!({}));
                save(m);
            }
            Err(error) => {
                needs_you(m, Needs::Setup, format!("The separate copy couldn't be made: {}", error.message));
                save(m);
                return;
            }
        }
    }
    let Some(copy) = copy_of(m) else {
        fail(m, "The separate copy was deleted.");
        save(m);
        return;
    };
    if let Some(report) = m.worktree.as_ref().and_then(|w| w.setup.as_ref()).filter(|r| r.failed()) {
        needs_you(m, Needs::Setup, format!("The separate copy couldn't be set up: {}", report.sentence()));
        save(m);
        return;
    }
    if m.baseline.is_none() {
        let outcomes = verdict::run_checks(&copy, &m.checks, m.check_timeout_ms);
        if let Some(c) = outcomes.iter().find(|c| c.not_started) {
            needs_you(m, Needs::Setup, format!("The checks can't run in the separate copy: {}", c.detail));
            save(m);
            return;
        }
        let point = match crate::checkpoints::mission_snapshot(&copy, &m.handle(), 1, "start") {
            Ok(point) => point,
            Err(error) if m.rollback => {
                needs_you(m, Needs::Setup, format!("Missions need Undo to go back when a turn makes things worse. {error}"));
                save(m);
                return;
            }
            Err(_) => crate::checkpoints::MissionPoint { id: String::new(), tree: String::new() },
        };
        let counts = verdict::summed_counts(&outcomes);
        match crate::tamper::take(&copy, &verdict::snapshot_options(&m.checks, counts)) {
            Ok(snap) => {
                let _ = store::save_baseline(&m.project, &m.id, &snap);
            }
            Err(error) => tracing::warn!(%error, mission = %m.id, "no test snapshot at the start"),
        }
        let base = verdict::build(outcomes, TestChanges::default(), point.tree.clone(), point.id);
        log(m, "baseline", serde_json::json!({ "passed": base.checks_passed, "total": base.checks_total, "counts": base.counts }));
        m.expected_tree = Some(point.tree);
        if base.all_pass() {
            m.baseline = Some(base);
            fail(m, "These checks already pass, so there was nothing to do.");
            m.sentence = Some("These checks already pass, so there was nothing to do. Add a check that fails now, or start a normal task.".into());
            if let Some(w) = m.worktree.as_ref() {
                if crate::worktrees::discard_if_pristine(w) {
                    if let Some(w) = m.worktree.as_mut() {
                        w.closed = Some("discarded".into());
                    }
                }
            }
            save(m);
            return;
        }
        let branch = m.worktree.as_ref().map(|w| (w.base.clone(), w.base_commit.chars().take(7).collect::<String>())).unwrap_or_default();
        m.push_event(now(), "started", ":", format!("Started · {} pass at the start · separate copy from {} at {}", base.short(), branch.0, branch.1), None);
        m.baseline = Some(base);
        save(m);
    }
    let _ = io;
    if m.rounds.is_empty() {
        let r = new_round(m, 0, RoundKind::First, None);
        m.rounds.push(r);
    }
    m.state = MissionState::Working;
    save(m);
}

fn new_round(m: &Mission, agent: usize, kind: RoundKind, handle: Option<String>) -> Round {
    let n = m.rounds.last().map(|r| r.n).unwrap_or(0) + 1;
    Round {
        n,
        agent,
        kind,
        idempotency_key: format!("{}-r{n}-a1", m.handle()),
        handle,
        operation_id: None,
        pane_id: None,
        end: None,
        verdict: None,
        decision: None,
        started_at_ms: now(),
        ended_at_ms: None,
        outside_edit: false,
    }
}

// ---- one turn ----------------------------------------------------------------------------------

/// The budget a round draws from: its own run for a turn on the same conversation, the mission's
/// for a new one.
fn gate_key(m: &Mission, r: &Round) -> String {
    match (r.kind.same_handle(), r.handle.as_deref()) {
        (true, Some(handle)) => handle.to_string(),
        _ => m.handle(),
    }
}

fn work(io: &dyn RoundIo, m: &mut Mission, ctl: &Control) {
    let Some(copy) = copy_of(m) else {
        fail(m, "The separate copy was deleted.");
        save(m);
        return;
    };
    if !copy.is_dir() {
        fail(m, "The separate copy was deleted.");
        save(m);
        return;
    }
    let Some(index) = m.rounds.len().checked_sub(1) else {
        m.state = MissionState::Planning;
        save(m);
        return;
    };
    if m.rounds[index].end.is_some() {
        m.state = MissionState::Checking;
        save(m);
        return;
    }
    if m.rounds[index].operation_id.is_none() {
        // A restart between "saved the key" and "saved the turn": find the turn by its key.
        if let Some(found) = io.find_by_key(&m.project, &m.rounds[index].idempotency_key) {
            let r = &mut m.rounds[index];
            r.operation_id = Some(found.operation_id);
            r.handle = Some(found.handle);
            r.pane_id = found.pane_id;
            log(m, "turn_found", serde_json::json!({ "round": m.rounds[index].n }));
            save(m);
            return;
        }
        // Rule 10: the budget before any next turn.
        let key = gate_key(m, &m.rounds[index]);
        if let Err(error) = io.budget_gate(&m.project, &key) {
            if error.code == "BUDGET_EXHAUSTED" {
                let used = m.turns_used();
                let sentence = match m.budget_turns {
                    Some(limit) if used >= limit => format!("Budget used up: {used} of {limit} turns"),
                    _ => "Budget used up".to_string(),
                };
                needs_you(m, Needs::Budget, sentence);
                save(m);
                return;
            }
        }
        // Someone edited the copy between turns: kept, and the next prompt says so.
        if let Ok(point) = crate::checkpoints::mission_snapshot(&copy, &m.handle(), m.rounds[index].n, "before turn") {
            if m.expected_tree.as_deref().is_some_and(|t| !t.is_empty() && t != point.tree) {
                let paths = crate::checkpoints::mission_changes(&copy, &m.handle(), &point.id).unwrap_or_default();
                for p in paths {
                    if !m.outside_paths.contains(&p) {
                        m.outside_paths.push(p);
                    }
                }
                m.rounds[index].outside_edit = true;
                m.push_event(now(), "note", "v", "Files changed outside the mission (by you?). They're kept; the next turn sees them.", Some(m.rounds[index].n));
            }
            m.expected_tree = Some(point.tree);
        }
        let text = prompt::for_round(m, &m.rounds[index].kind.clone(), &copy, m.rounds[index].outside_edit);
        log(m, "turn_start", serde_json::json!({ "round": m.rounds[index].n, "key": m.rounds[index].idempotency_key }));
        m.rounds[index].started_at_ms = now();
        let r = m.rounds[index].clone();
        match io.start_round(m, &r, &text) {
            Ok(started) => {
                let r = &mut m.rounds[index];
                r.operation_id = Some(started.operation_id.clone());
                r.handle = Some(started.handle.clone());
                r.pane_id = started.pane_id.clone();
                let n = r.n;
                log(m, "turn_created", serde_json::json!({ "round": n, "operation_id": started.operation_id, "handle": started.handle }));
                save(m);
            }
            Err(error) => {
                if error.code == "BUDGET_EXHAUSTED" {
                    needs_you(m, Needs::Budget, "Budget used up".into());
                    save(m);
                    return;
                }
                let r = &mut m.rounds[index];
                r.end = Some(RoundEnd {
                    status: "not_started".into(),
                    code: Some(error.code.clone()),
                    message: Some(error.message.clone()),
                    pane_id: None,
                });
                r.ended_at_ms = Some(now());
                m.state = MissionState::Checking;
                log(m, "turn_not_started", serde_json::json!({ "round": m.rounds[index].n, "code": error.code }));
                save(m);
            }
        }
        return;
    }
    let op = m.rounds[index].operation_id.clone().unwrap_or_default();
    let ended = io.wait_round(&m.project, &op, &|| ctl.stop.load(Ordering::SeqCst) || ctl.pause.load(Ordering::SeqCst) != 0);
    let Some(end) = ended else { return };
    // Re-read: the person may have paused while the turn ran.
    if let Ok(fresh) = store::load(&m.project, &m.id) {
        *m = fresh;
    }
    let Some(r) = m.rounds.get_mut(index) else { return };
    if r.pane_id.is_none() {
        r.pane_id = end.pane_id.clone();
    }
    r.end = Some(end);
    r.ended_at_ms = Some(now());
    m.state = MissionState::Checking;
    log(m, "turn_ended", serde_json::json!({ "round": m.rounds[index].n, "end": m.rounds[index].end }));
    save(m);
}

// ---- after a turn ------------------------------------------------------------------------------

fn check(io: &dyn RoundIo, m: &mut Mission, ctl: &Control) {
    let Some(copy) = copy_of(m).filter(|c| c.is_dir()) else {
        fail(m, "The separate copy was deleted.");
        save(m);
        return;
    };
    let Some(index) = m.rounds.len().checked_sub(1) else { return };
    let started = m.rounds[index].end.as_ref().is_none_or(|e| e.status != "not_started");
    if m.rounds[index].verdict.is_none() && started {
        let outcomes = verdict::run_checks(&copy, &m.checks, m.check_timeout_ms);
        let counts = verdict::summed_counts(&outcomes);
        let snap = store::load_baseline(&m.project, &m.id);
        let changes = verdict::test_changes(&copy, snap.as_ref(), &m.checks, counts, m.may_change_tests, &m.outside_paths);
        let n = m.rounds[index].n;
        let point = crate::checkpoints::mission_snapshot(&copy, &m.handle(), n, &format!("after turn {n}"))
            .unwrap_or(crate::checkpoints::MissionPoint { id: String::new(), tree: String::new() });
        let v = verdict::build(outcomes, changes, point.tree.clone(), point.id);
        log(m, "verdict", serde_json::json!({ "round": n, "passed": v.checks_passed, "total": v.checks_total, "counts": v.counts, "test_changes_block": v.test_changes.blocks }));
        m.expected_tree = Some(point.tree);
        m.rounds[index].verdict = Some(v);
        save(m);
    }
    if m.rounds[index].decision.is_none() {
        let pane = m.rounds[index].pane_id.clone();
        let sig = Signals {
            stop: ctl.stop.load(Ordering::SeqCst),
            pause: m.pause_requested,
            loop_signal: pane.as_deref().and_then(|p| io.loop_signal(p)),
            stall: io.stall(),
        };
        let decision = decide::decide(m, &sig);
        log(m, "decision", serde_json::to_value(&decision).unwrap_or_default());
        m.rounds[index].decision = Some(decision);
        save(m);
    }
    let decision = m.rounds[index].decision.clone().expect("decided above");
    act(io, m, &copy, decision.action, &decision.line);
}

/// Carries out a decision. Every step is safe to repeat after a restart: a round already pushed
/// is not pushed again, a restore by id gives the same files.
fn act(io: &dyn RoundIo, m: &mut Mission, copy: &Path, action: Action, line: &str) {
    let last = m.rounds.last().cloned().expect("a round was decided");
    match action {
        Action::Stop => {
            m.state = MissionState::Stopped;
            m.sentence = Some(format!("Stopped by you after turn {}", last.n));
            m.push_event(now(), "stopped", ":", "Stopped by you", Some(last.n));
            save(m);
        }
        Action::Pause => {
            m.state = MissionState::Paused;
            m.paused_from = Some(MissionState::Checking);
            m.pause_requested = false;
            m.push_event(now(), "paused", ":", "Paused by you", Some(last.n));
            save(m);
        }
        Action::Green => {
            m.state = MissionState::Green;
            m.needs = None;
            let v = last.verdict.clone().unwrap_or_default();
            m.sentence = Some(format!("Done · {} pass · ready to review", v.short()));
            m.push_event(now(), "done", "v", line, Some(last.n));
            save(m);
        }
        Action::NeedsYou { why } => {
            needs_you(m, why, line.to_string());
            save(m);
        }
        Action::FollowUp => push_next(m, RoundKind::FollowUp, last.agent, last.handle.clone()),
        Action::Resume { after_restart } => {
            // A new conversation: the old run can start no more turns, and its unused budget comes back.
            if let Some(handle) = last.handle.as_deref() {
                io.budget_stop(&m.project, handle);
            }
            if after_restart {
                m.push_event(now(), "restarted", ":", "Resumed after Puppet Master restarted", Some(last.n));
            } else {
                m.push_event(now(), "note", "v", line, Some(last.n));
            }
            push_next(m, RoundKind::Resume { after_restart }, last.agent, None)
        }
        Action::HandOff { to, why } => hand_off(io, m, &last, to, why, line),
        Action::Rollback { to_round, next } => {
            if io.panes_running_in(copy) {
                m.push_event(now(), "note", "v", "Didn't go back: someone is working in the copy.", Some(last.n));
            } else {
                let target = if to_round == 0 {
                    m.baseline.as_ref().map(|b| b.checkpoint.clone())
                } else {
                    m.rounds.iter().find(|r| r.n == to_round).and_then(|r| r.verdict.as_ref()).map(|v| v.checkpoint.clone())
                };
                match target.filter(|t| !t.is_empty()).map(|t| crate::checkpoints::mission_restore(copy, &m.handle(), &t)) {
                    Some(Ok(report)) => {
                        let point = crate::checkpoints::mission_snapshot(copy, &m.handle(), last.n, &format!("went back to turn {to_round}"));
                        if let Ok(point) = point {
                            m.expected_tree = Some(point.tree);
                        }
                        log(m, "rollback", serde_json::json!({ "round": last.n, "to": to_round, "written": report.written, "removed": report.removed }));
                        m.push_event(now(), "rollback", "v", line, Some(last.n));
                        crate::audit::record(
                            crate::audit::Kind::MissionRollback,
                            crate::audit::Who::Local,
                            &format!("Went back to turn {to_round}'s files in the mission \"{}\"", short(&m.objective)),
                            serde_json::json!({ "mission": m.handle(), "round": last.n, "to": to_round }),
                        );
                    }
                    Some(Err(error)) => m.push_event(now(), "note", "v", format!("Couldn't go back: {error}"), Some(last.n)),
                    None => m.push_event(now(), "note", "v", "Couldn't go back: there is no saved copy of those files.", Some(last.n)),
                }
            }
            match *next {
                Action::FollowUp => push_next(m, RoundKind::AfterRollback { to_round }, last.agent, last.handle.clone()),
                Action::HandOff { to, why } => hand_off(io, m, &last, to, why, &format!("Handed to {}: {} kept making the checks worse", m.agent_label(to), m.agent_label(last.agent))),
                other => act(io, m, copy, other, line),
            }
        }
    }
}

fn push_next(m: &mut Mission, kind: RoundKind, agent: usize, handle: Option<String>) {
    let last_n = m.rounds.last().map(|r| r.n).unwrap_or(0);
    let already = m.rounds.iter().any(|r| r.n == last_n + 1);
    if !already {
        let handle = if kind.same_handle() { handle } else { None };
        let r = new_round(m, agent, kind, handle);
        m.rounds.push(r);
    }
    m.roster.current = agent;
    m.state = MissionState::Working;
    m.needs = None;
    m.sentence = None;
    save(m);
}

fn hand_off(io: &dyn RoundIo, m: &mut Mission, last: &Round, to: usize, why: HandOffWhy, line: &str) {
    if let Some(handle) = last.handle.as_deref() {
        io.budget_stop(&m.project, handle);
    }
    if let Some(pane) = last.pane_id.as_deref() {
        io.dismiss_loop(pane);
    }
    let from = last.agent;
    // Wrapping: the next one was out (or is this agent): the list starts over, once.
    if to == from || m.roster.out.contains(&to) {
        m.roster.wrapped = true;
        m.roster.out.clear();
    } else if !m.roster.out.contains(&from) {
        m.roster.out.push(from);
    }
    let reason = match why {
        HandOffWhy::NoProgress => format!("{} made no progress", m.agent_label(from)),
        HandOffWhy::Exited => format!("{} closed twice", m.agent_label(from)),
        HandOffWhy::Regressed => format!("{} kept making the checks worse", m.agent_label(from)),
    };
    m.push_event(now(), "handed_off", "v", format!("Handed to {} · {reason}", m.agent_label(to)), Some(last.n));
    let _ = line;
    crate::audit::record(
        crate::audit::Kind::MissionHandedOff,
        crate::audit::Who::Local,
        &format!("Handed the mission \"{}\" to {}", short(&m.objective), m.agent_label(to)),
        serde_json::json!({ "mission": m.handle(), "round": last.n, "from": m.agents[from].agent_type, "to": m.agents[to].agent_type }),
    );
    push_next(m, RoundKind::HandOff { from, why }, to, None);
}

// ---- waiting for the person --------------------------------------------------------------------

fn wait_for_person(io: &dyn RoundIo, m: &mut Mission, ctl: &Control) {
    if let Some(action) = ctl.take_action() {
        apply(io, m, action);
        return;
    }
    if m.state == MissionState::NeedsYou && m.needs == Some(Needs::Budget) {
        let key = m.rounds.last().map(|r| gate_key(m, r)).unwrap_or_else(|| m.handle());
        if io.budget_stopped(&m.project, &key) || io.budget_stopped(&m.project, &m.handle()) {
            m.state = MissionState::Stopped;
            m.sentence = Some("Stopped at its budget".into());
            m.push_event(now(), "stopped", ":", "Stopped at its budget", None);
            save(m);
            return;
        }
        if io.budget_gate(&m.project, &key).is_ok() {
            m.state = MissionState::Working;
            m.needs = None;
            m.sentence = None;
            m.push_event(now(), "budget", "v", "Budget raised. Working again.", None);
            save(m);
            return;
        }
    }
    ctl.sleep(io.poll());
}

fn apply(io: &dyn RoundIo, m: &mut Mission, action: PersonAction) {
    let Some(copy) = copy_of(m) else { return };
    let last = m.rounds.last().cloned();
    match action {
        PersonAction::UndoTestChanges { by } if m.needs == Some(Needs::Tamper) => {
            let files = last.as_ref().and_then(|r| r.verdict.as_ref()).map(|v| {
                let mut files: Vec<String> = v
                    .test_changes
                    .findings
                    .iter()
                    .filter(|f| f.severity == crate::tamper::Severity::Blocks && !f.file.is_empty())
                    .map(|f| f.file.clone())
                    .collect();
                files.sort();
                files.dedup();
                files
            });
            if let (Some(snap), Some(files)) = (store::load_baseline(&m.project, &m.id), files) {
                let report = crate::tamper::undo(&copy, &snap, &files);
                log(m, "tests_undone", serde_json::to_value(&report).unwrap_or_default());
            }
            if let Ok(point) = crate::checkpoints::mission_snapshot(&copy, &m.handle(), last.as_ref().map(|r| r.n).unwrap_or(0), "put the tests back") {
                m.expected_tree = Some(point.tree);
            }
            m.push_event(now(), "tests_undone", "v", "Put the tests back", last.as_ref().map(|r| r.n));
            crate::audit::record(crate::audit::Kind::MissionTestsUndone, who(&by), &format!("Put the tests back in the mission \"{}\"", short(&m.objective)), serde_json::json!({ "mission": m.handle() }));
            let (agent, handle) = last.map(|r| (r.agent, r.handle)).unwrap_or((m.roster.current, None));
            push_next(m, RoundKind::AfterTestUndo, agent, handle);
        }
        PersonAction::AcceptTestChanges { by } if m.needs == Some(Needs::Tamper) => {
            let summary = last.as_ref().and_then(|r| r.verdict.as_ref()).map(|v| v.test_changes.blocking_sentences()).unwrap_or_default();
            let at = now();
            m.tests_accepted = Some(Accepted { at_ms: at, by: by.clone(), summary });
            m.state = MissionState::Green;
            m.needs = None;
            let v = last.as_ref().and_then(|r| r.verdict.clone()).unwrap_or_default();
            m.sentence = Some(format!("Done · {} pass · ready to review", v.short()));
            m.push_event(at, "tests_accepted", "v", format!("Test changes accepted by {}", if by == "you" { "you" } else { "a person" }), last.as_ref().map(|r| r.n));
            crate::audit::record(crate::audit::Kind::MissionTestsAccepted, who(&by), &format!("Accepted the test changes of the mission \"{}\"", short(&m.objective)), serde_json::json!({ "mission": m.handle() }));
            save(m);
        }
        PersonAction::TryMoreTurns if m.needs == Some(Needs::Stuck) => {
            if m.budget_turns.is_some() {
                io.budget_add_turns(&m.project, &m.handle(), 5);
                m.budget_turns = m.budget_turns.map(|t| t + 5);
            }
            if let Some(r) = last.as_ref() {
                if let Some(handle) = r.handle.as_deref() {
                    io.budget_stop(&m.project, handle);
                }
            }
            m.roster.out.clear();
            m.roster.wrapped = true;
            m.push_event(now(), "note", "v", "Trying 5 more turns with a fresh start", last.as_ref().map(|r| r.n));
            push_next(m, RoundKind::Retry, 0, None);
        }
        PersonAction::HandToMe if m.needs == Some(Needs::Stuck) || m.state == MissionState::NeedsYou => {
            let turn = last.as_ref().map(|r| r.n).unwrap_or(0);
            m.state = MissionState::HandedOff;
            m.needs = None;
            m.sentence = Some(format!("Handed to you after turn {turn}"));
            m.push_event(now(), "handed_off", "v", format!("Handed to you after turn {turn}"), Some(turn));
            save(m);
        }
        PersonAction::TryAgain if m.needs == Some(Needs::Setup) => {
            m.needs = None;
            m.sentence = None;
            if m.baseline.is_none() {
                m.state = MissionState::Planning;
                save(m);
                return;
            }
            match last {
                Some(r) if r.verdict.as_ref().is_some_and(|v| v.cant_run().is_some()) => {
                    if let Some(r) = m.rounds.last_mut() {
                        r.verdict = None;
                        r.decision = None;
                    }
                    m.state = MissionState::Checking;
                    save(m);
                }
                Some(r) => {
                    if let Some(handle) = r.handle.as_deref() {
                        io.budget_stop(&m.project, handle);
                    }
                    push_next(m, RoundKind::Resume { after_restart: false }, r.agent, None)
                }
                None => {
                    m.state = MissionState::Planning;
                    save(m);
                }
            }
        }
        PersonAction::Resume if m.state == MissionState::Paused => {
            m.push_event(now(), "resumed", ":", "Resumed", m.rounds.last().map(|r| r.n));
            match m.paused_from.take() {
                Some(MissionState::NeedsYou) => {
                    m.state = MissionState::NeedsYou;
                }
                Some(MissionState::Planning) => m.state = MissionState::Planning,
                _ => {
                    // Decide again, without the pause, from the last turn's results.
                    if let Some(r) = m.rounds.last_mut() {
                        if r.decision.as_ref().is_some_and(|d| d.action == Action::Pause) {
                            r.decision = None;
                        }
                    }
                    m.state = if m.rounds.last().is_some_and(|r| r.end.is_none()) { MissionState::Working } else { MissionState::Checking };
                }
            }
            save(m);
        }
        _ => {}
    }
}
