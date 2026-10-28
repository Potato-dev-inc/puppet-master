//! Night Shift: the supervisor (SPEC n32 3.5).
//!
//! One thread, `night-shift`, ticks every `TICK_MS` and calls [`step`]. `step` mutates the run
//! record and talks to the world only through [`NightIo`], so the tests drive it with a fake and a
//! fake clock. The runner can start a task, read what it is doing and stop it. It has no way to
//! answer anything: a trust, permission or sign-in question ends the try as a `Gate` and the task
//! waits for the person (SPEC section 4, S1).
//!
//! Order of one tick: clock, hard stop, person stop, watch every open attempt, fill free slots,
//! settle tasks that wait, save, finish when every task has an outcome.

use std::collections::HashSet;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};

use super::classify::{classify, Facts};
use super::io::{NightIo, StartFail, Started, Watch};
use super::model::*;
use super::plan::{self, Cand, LimitAction, LimitFacts, WaitWhy};
use super::{quiet, report};
use crate::missions::model::MissionState;

/// Who stopped a turn, as the mission and run records show it.
const BY: &str = "night-shift";
/// A start that found no free mission slot is tried again after this long (not a failure).
const SLOT_WAIT_MS: u64 = 30_000;
const SLOT_WHY: &str = "Waiting for a free slot.";

pub enum Next {
    Again,
    Finished,
}

// ---- who may stop a run, and the thread -----------------------------------------------------------

static STOPS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
/// Runs asked to stop by "Stop all agents". Only a run that is Running is stopped by it: a night that
/// has not started yet has no agents to stop and keeps its start time.
static STOP_RUNNING: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static ACTIVE: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static WAKE: Lazy<(Mutex<u64>, Condvar)> = Lazy::new(|| (Mutex::new(0), Condvar::new()));

/// The person pressed Stop. The next tick stops every open attempt and finishes the run.
pub fn request_stop(run_id: &str) {
    STOPS.lock().insert(run_id.to_string());
    wake();
}

/// "Stop all agents" (the palette, the tray, the shortcut, the phone): every night that is running
/// stops like the Stop button, so no new task starts behind the panic. Returns how many supervisors
/// were asked. Their next tick (woken now) stops the open attempts, releases the keep-awake hold and
/// saves the report; copies stay.
pub fn stop_running_nights() -> usize {
    let ids: Vec<String> = ACTIVE.lock().iter().cloned().collect();
    STOP_RUNNING.lock().extend(ids.iter().cloned());
    if !ids.is_empty() {
        wake();
    }
    ids.len()
}

pub fn stop_requested(run_id: &str) -> bool {
    STOPS.lock().contains(run_id)
}

/// Whether a supervisor thread is running for this run in this process.
pub fn is_running(run_id: &str) -> bool {
    ACTIVE.lock().contains(run_id)
}

/// Cut the wait between ticks short (a stop, a new run).
pub fn wake() {
    let (lock, cv) = &*WAKE;
    *lock.lock() += 1;
    cv.notify_all();
}

fn pause() {
    let (lock, cv) = &*WAKE;
    let mut generation = lock.lock();
    cv.wait_for(&mut generation, Duration::from_millis(TICK_MS));
}

/// Removes the run from `ACTIVE` when the thread ends, even by a panic.
struct Registered(String);

impl Drop for Registered {
    fn drop(&mut self) {
        ACTIVE.lock().remove(&self.0);
    }
}

/// Start the supervisor for `run_id`. `None` when one already runs for it, or the thread failed.
pub fn spawn(io: Arc<dyn NightIo>, run_id: String) -> Option<JoinHandle<()>> {
    if !ACTIVE.lock().insert(run_id.clone()) {
        return None;
    }
    let guard = Registered(run_id.clone());
    let id = run_id.clone();
    let spawned = std::thread::Builder::new().name("night-shift".into()).spawn(move || {
        let _guard = guard;
        supervise(io.as_ref(), &id);
    });
    match spawned {
        Ok(handle) => Some(handle),
        Err(error) => {
            tracing::warn!(error = %error, "night shift: could not start the supervisor");
            ACTIVE.lock().remove(&run_id);
            None
        }
    }
}

fn supervise(io: &dyn NightIo, run_id: &str) {
    let Some(mut run) = io.load().filter(|r| r.id == run_id) else { return };
    adopt(&mut run, io);
    loop {
        merge_person_edits(&mut run, io.load());
        if let Next::Finished = step(&mut run, io) {
            break;
        }
        pause();
    }
}

/// App start: take over a run that was Running or Scheduled when the app closed. Missions resume
/// themselves (`missions::resume_all`); this finds them again by id.
#[cfg(not(test))]
pub fn resume() -> bool {
    let Some(run) = super::store::Store::live().load() else { return false };
    if !matches!(run.state, RunState::Scheduled { .. } | RunState::Running) {
        return false;
    }
    spawn(super::io::live(&run.id), run.id.clone()).is_some()
}

#[cfg(test)]
pub fn resume() -> bool {
    false
}

/// A new supervisor starts from the saved record: the keep-awake hold died with the old process, a
/// task whose start never got an id is queued again (the start key returns the same mission if it
/// did get created), and the quiet set is rebuilt.
pub fn adopt(run: &mut NightRun, io: &dyn NightIo) {
    run.last_tick_ms = io.now_ms();
    run.awake_ok = false;
    for ti in 0..run.tasks.len() {
        if run.tasks[ti].state != TaskState::Working {
            continue;
        }
        let unrecorded = run.tasks[ti].attempts.last().is_some_and(|a| a.end.is_none() && a.mission_id.is_none() && a.handle.is_none());
        let none_open = run.tasks[ti].attempts.last().is_none_or(|a| a.end.is_some());
        if unrecorded {
            run.tasks[ti].attempts.pop();
        }
        if unrecorded || none_open {
            run.tasks[ti].state = TaskState::Queued;
        }
    }
    let run_id = run.id.clone();
    for task in &run.tasks {
        for a in task.attempts.iter().filter(|a| a.end.is_none()) {
            mute_attempt(&run_id, &task.spec.project, a);
            let touched = io.touched(task, a);
            for pane in &touched.pane_ids {
                quiet::mute_pane(&run_id, pane);
            }
            for handle in &touched.handles {
                quiet::mute_handle(&run_id, &task.spec.project, handle);
            }
        }
    }
}

/// What the person changed in the saved record while the night ran (Keep, Throw away, opening the
/// Morning page) must survive the runner's next save.
pub fn merge_person_edits(run: &mut NightRun, stored: Option<NightRun>) {
    let Some(stored) = stored.filter(|s| s.id == run.id) else { return };
    if stored.seen_ms.is_some() {
        run.seen_ms = stored.seen_ms;
    }
    for task in &mut run.tasks {
        if let Some(saved) = stored.tasks.iter().find(|t| t.spec.id == task.spec.id) {
            if saved.review != Review::Pending {
                task.review = saved.review.clone();
            }
        }
    }
}

// ---- one tick ------------------------------------------------------------------------------------

/// One tick of the supervisor. Pure apart from `io`.
pub fn step(run: &mut NightRun, io: &dyn NightIo) -> Next {
    if matches!(run.state, RunState::Done | RunState::Stopped) {
        return Next::Finished;
    }
    let now = io.now_ms();
    let last = run.last_tick_ms;
    run.last_tick_ms = now;
    let before = run.clone();
    let offset = io.utc_offset_min();

    if now >= run.stop_at_ms {
        return stop_everything(run, io, now, Why::TimeUp);
    }
    let panic_stop = STOP_RUNNING.lock().remove(&run.id) && run.state == RunState::Running;
    if stop_requested(&run.id) || panic_stop {
        return stop_everything(run, io, now, Why::Person);
    }
    if let RunState::Scheduled { start_at_ms } = run.state {
        if now < start_at_ms {
            return Next::Again;
        }
        run.state = RunState::Running;
        push_event(run, now, "started", "~", format!("Night Shift started. It stops at {}.", report::hhmm(run.stop_at_ms, offset)), None);
        io.audit("started", &format!("Night Shift started with {} tasks", run.tasks.len()));
    } else if last > 0 && now.saturating_sub(last) > SLEEP_GAP_MS {
        let text = format!("The computer slept {} to {}", report::hhmm(last, offset), report::hhmm(now, offset));
        push_event(run, now, "slept", ":", text, None);
    }
    ensure_awake(run, io, now);

    watch_attempts(run, io, now);
    let cands = io.candidates(run);
    fill(run, io, &cands, now);
    settle(run, io, &cands, now);

    if run.tasks.iter().all(|t| t.state == TaskState::Finished) {
        return finish(run, io, now, RunState::Done);
    }
    if *run != before {
        io.persist(run);
    }
    Next::Again
}

fn ensure_awake(run: &mut NightRun, io: &dyn NightIo, now: u64) {
    let failed_before = run.events.iter().any(|e| e.kind == "awake_failed");
    if !run.keep_awake || run.awake_ok || failed_before {
        return;
    }
    match io.keep_awake(true) {
        Ok(()) => run.awake_ok = true,
        Err(_) => push_event(
            run,
            now,
            "awake_failed",
            "+",
            "The computer may have gone to sleep. Check its power settings.".to_string(),
            None,
        ),
    }
}

// ---- watching ------------------------------------------------------------------------------------

fn open_attempt(task: &Task) -> Option<&Attempt> {
    task.attempts.last().filter(|a| a.end.is_none())
}

fn watch_attempts(run: &mut NightRun, io: &dyn NightIo, now: u64) {
    for ti in 0..run.tasks.len() {
        let task = run.tasks[ti].clone();
        let Some(a) = open_attempt(&task).cloned() else { continue };
        keep_quiet(run, io, ti, &task, &a);
        match io.watch(&task, &a) {
            Watch::Working => {}
            Watch::Asked { question, waited_ms } => {
                if waited_ms >= ASK_GRACE_MS {
                    end_attempt(&mut run.tasks[ti], now, AttemptEnd::Asked { question });
                    io.stop(&task, &a, BY);
                    push_event(run, now, "asked", "+", format!("Task {} stopped to ask a question. It was not answered.", ti + 1), Some(&task.spec.id));
                    finish_task(run, io, ti, None, now);
                }
            }
            Watch::Gate { code, sentence } => on_gate(run, io, ti, &task, &a, code, sentence, now),
            Watch::Limit { reset_at_ms, self_resume_armed, parked_ms } => {
                let facts = LimitFacts { reset_at_ms, self_resume_armed, parked_ms };
                match plan::limit_action(&facts, now, run.stop_at_ms, false, None) {
                    LimitAction::WaitInPlace { until_ms } => {
                        let when = report::hhmm(until_ms, io.utc_offset_min());
                        let waiting = run.events.iter().rev().find(|e| e.task.as_deref() == Some(task.spec.id.as_str()));
                        if waiting.is_none_or(|e| e.kind != "waiting") {
                            let text = format!("{} is at its usage limit. Waiting in place, checking again at {when}.", a.label);
                            push_event(run, now, "waiting", ":", text, Some(&task.spec.id));
                        }
                    }
                    LimitAction::EndAttempt { reset_at_ms } => on_out_of_usage(run, io, ti, &task, &a, reset_at_ms, now),
                    // Keep Going is not wired in v1: `limit_action` is given no target, so this never happens.
                    LimitAction::KeepGoing { .. } => {}
                }
            }
            Watch::Done => on_done(run, io, ti, &task, &a, now),
            Watch::Failed { code, sentence } => {
                end_attempt(&mut run.tasks[ti], now, AttemptEnd::Failed { code, sentence });
                finish_task(run, io, ti, None, now);
            }
            Watch::Gone => {
                let sentence = "The copy or its record disappeared".to_string();
                end_attempt(&mut run.tasks[ti], now, AttemptEnd::Failed { code: "GONE".into(), sentence });
                finish_task(run, io, ti, None, now);
            }
        }
    }
}

/// Keep the attempt's panes and handles silent and listed as the attempt grows turns.
fn keep_quiet(run: &mut NightRun, io: &dyn NightIo, ti: usize, task: &Task, a: &Attempt) {
    let touched = io.touched(task, a);
    for pane in &touched.pane_ids {
        quiet::mute_pane(&run.id, pane);
    }
    for handle in &touched.handles {
        quiet::mute_handle(&run.id, &task.spec.project, handle);
    }
    if let Some(open) = run.tasks[ti].attempts.last_mut().filter(|x| x.end.is_none()) {
        for pane in touched.pane_ids {
            if !open.pane_ids.contains(&pane) {
                open.pane_ids.push(pane);
            }
        }
    }
}

fn mute_attempt(run_id: &str, project: &str, a: &Attempt) {
    for pane in &a.pane_ids {
        quiet::mute_pane(run_id, pane);
    }
    if let Some(handle) = &a.handle {
        quiet::mute_handle(run_id, project, handle);
    }
}

#[allow(clippy::too_many_arguments)]
fn on_gate(run: &mut NightRun, io: &dyn NightIo, ti: usize, task: &Task, a: &Attempt, code: String, sentence: String, now: u64) {
    end_attempt(&mut run.tasks[ti], now, AttemptEnd::Gate { code, sentence: sentence.clone() });
    io.stop(task, a, BY);
    let key = plan_key(&a.plan);
    if !run.blocked.iter().any(|b| b.plan_key == key && b.project == task.spec.project) {
        run.blocked.push(Blocked { plan_key: key.clone(), project: task.spec.project.clone(), sentence: sentence.clone() });
    }
    push_event(run, now, "blocked", "+", format!("{sentence} It was not answered."), Some(&task.spec.id));
    let other_plan = matches!(task.spec.who, Who::Any)
        && io.candidates(run).iter().any(|c| {
            let k = plan_key(&c.agent);
            c.runnable.is_ok() && k != key && !run.blocked.iter().any(|b| b.plan_key == k && b.project == task.spec.project)
        });
    if other_plan && run.tasks[ti].attempts.len() < usize::from(MAX_ATTEMPTS) {
        run.tasks[ti].state = TaskState::Queued;
    } else {
        finish_task(run, io, ti, None, now);
    }
}

fn on_out_of_usage(run: &mut NightRun, io: &dyn NightIo, ti: usize, task: &Task, a: &Attempt, reset: Option<u64>, now: u64) {
    end_attempt(&mut run.tasks[ti], now, AttemptEnd::OutOfUsage { reset_at_ms: reset });
    io.stop(task, a, BY);
    let key = plan_key(&a.plan);
    let until = Some(reset.unwrap_or(now + NO_RESET_WAIT_MS));
    match run.plan_out.iter_mut().find(|p| p.plan_key == key) {
        Some(out) => {
            out.until_ms = until;
            out.since_ms = now;
        }
        None => run.plan_out.push(PlanOut { plan_key: key, until_ms: until, since_ms: now }),
    }
    let when = report::hhmm(now, io.utc_offset_min());
    push_event(run, now, "out_of_usage", "+", format!("{} ran out of usage at {when}.", a.label), Some(&task.spec.id));
    if run.tasks[ti].attempts.len() < usize::from(MAX_ATTEMPTS) {
        run.tasks[ti].state = TaskState::Queued;
    } else {
        finish_task(run, io, ti, None, now);
    }
}

/// The mission is over or the turn completed. A mission that wants the person is stopped after its
/// facts are read, so it gives its slot back.
fn on_done(run: &mut NightRun, io: &dyn NightIo, ti: usize, task: &Task, a: &Attempt, now: u64) {
    let mut facts = io.facts(run, task);
    let end = end_from(&facts);
    let still_open = facts.mission.as_ref().is_some_and(|m| !m.state.terminal());
    end_attempt(&mut run.tasks[ti], now, end);
    if still_open {
        io.stop(task, a, BY);
    }
    fill_stop_time(&mut facts, run, io);
    conclude(run, ti, facts, now);
}

/// How a finished attempt ended, from the mission it ran as (a single turn that is done completed).
fn end_from(facts: &Facts) -> AttemptEnd {
    let Some(m) = &facts.mission else { return AttemptEnd::Completed };
    match m.state {
        MissionState::Green => AttemptEnd::Completed,
        MissionState::NeedsYou => {
            let why = m.needs.map(|n| n.as_str()).unwrap_or("needs_you").to_string();
            AttemptEnd::NeedsYou { why, sentence: "It needs you before it can go on.".to_string() }
        }
        MissionState::Failed => AttemptEnd::Failed { code: "MISSION_FAILED".into(), sentence: "The mission could not go on.".into() },
        _ => AttemptEnd::Completed,
    }
}

// ---- starting ------------------------------------------------------------------------------------

/// A task that waits for a free mission slot is skipped until its retry time.
fn held(task: &Task, now: u64) -> bool {
    matches!(&task.state, TaskState::Waiting { until_ms: Some(t), why } if why == SLOT_WHY && *t > now)
}

fn fill(run: &mut NightRun, io: &dyn NightIo, cands: &[Cand], now: u64) {
    if !plan::time_left_ok(run, now) {
        return;
    }
    let slots = plan::free_slots(run, io.other_active_missions(run));
    if slots == 0 {
        return;
    }
    let mut view = run.clone();
    for task in view.tasks.iter_mut().filter(|t| held(t, now)) {
        task.state = TaskState::Finished;
    }
    for start in plan::next_starts(&view, cands, &[], slots, now) {
        let ti = start.task;
        let task = run.tasks[ti].clone();
        let cand = &cands[start.cand];
        let n = u8::try_from(task.attempts.len() + 1).unwrap_or(u8::MAX);
        match io.start(run, &task, &cand.agent, n) {
            Ok(started) => record_started(run, ti, cand, n, started, now),
            Err(StartFail::NoSlot) => {
                run.tasks[ti].state = TaskState::Waiting { until_ms: Some(now + SLOT_WAIT_MS), why: SLOT_WHY.to_string() };
                break;
            }
            Err(StartFail::Refused { sentence, .. }) => {
                push_event(run, now, "note", "x", format!("Task {} did not start: {sentence}", ti + 1), Some(&task.spec.id));
                finish_task(run, io, ti, Some(sentence.trim_end_matches('.').to_string()), now);
            }
        }
    }
}

fn record_started(run: &mut NightRun, ti: usize, cand: &Cand, n: u8, started: Started, now: u64) {
    let project = run.tasks[ti].spec.project.clone();
    let kind = if run.tasks[ti].checks.is_empty() { AttemptKind::Single } else { AttemptKind::Mission };
    let attempt = Attempt {
        n,
        plan: cand.agent.clone(),
        label: cand.label.clone(),
        kind,
        mission_id: started.mission_id,
        handle: Some(started.handle),
        pane_ids: started.pane_ids,
        started_ms: now,
        ended_ms: None,
        end: None,
    };
    mute_attempt(&run.id, &project, &attempt);
    let task_id = run.tasks[ti].spec.id.clone();
    run.tasks[ti].attempts.push(attempt);
    run.tasks[ti].state = TaskState::Working;
    let text = if n == 1 {
        format!("Task {} started on {}.", ti + 1, cand.label)
    } else {
        format!("Moved task {} to {}.", ti + 1, cand.label)
    };
    push_event(run, now, if n == 1 { "task_started" } else { "moved" }, "~", text, Some(&task_id));
}

// ---- waiting -------------------------------------------------------------------------------------

/// Tasks that cannot start now: out of time, no plan with usage, or blocked. A task that only waits
/// its turn behind a busy project or plan stays `Queued`.
fn settle(run: &mut NightRun, io: &dyn NightIo, cands: &[Cand], now: u64) {
    let offset = io.utc_offset_min();
    for ti in 0..run.tasks.len() {
        let task = run.tasks[ti].clone();
        if !matches!(task.state, TaskState::Queued | TaskState::Waiting { .. }) || held(&task, now) {
            continue;
        }
        if !plan::time_left_ok(run, now) {
            let why = format!("no time left before {}", report::hhmm(run.stop_at_ms, offset));
            finish_task(run, io, ti, Some(why), now);
            continue;
        }
        if task.attempts.len() >= usize::from(MAX_ATTEMPTS) {
            finish_task(run, io, ti, None, now);
            continue;
        }
        let Some(why) = stuck(run, &task, cands, now) else {
            if matches!(task.state, TaskState::Waiting { .. }) {
                run.tasks[ti].state = TaskState::Queued;
            }
            continue;
        };
        let reset = match &why {
            WaitWhy::OutOfUsage { until_ms } => Some(*until_ms),
            _ => None,
        };
        let waits_for_reset = match reset {
            Some(Some(t)) => t + MIN_START_LEFT_MS <= run.stop_at_ms,
            Some(None) => now + NO_RESET_WAIT_MS + MIN_START_LEFT_MS <= run.stop_at_ms,
            None => false,
        };
        if waits_for_reset {
            let until_ms = reset.flatten();
            let text = plan::waiting_text(&why, &|ms| report::hhmm(ms, offset));
            let new_state = TaskState::Waiting { until_ms, why: text.clone() };
            if run.tasks[ti].state != new_state {
                let was_waiting = matches!(run.tasks[ti].state, TaskState::Waiting { .. });
                run.tasks[ti].state = new_state;
                if !was_waiting {
                    push_event(run, now, "waiting", ":", format!("Task {}: {text}", ti + 1), Some(&task.spec.id));
                }
            }
        } else {
            finish_task(run, io, ti, Some(not_run_text(&why, cands, offset)), now);
        }
    }
}

/// Why this task has no plan at all, even if the whole night were free. `None`: some plan could
/// take it (it only waits for a slot, its project, or a plan another task holds).
fn stuck(run: &NightRun, task: &Task, cands: &[Cand], now: u64) -> Option<WaitWhy> {
    let mut probe = run.clone();
    probe.parallel = MAX_PARALLEL;
    probe.tasks = vec![Task { state: TaskState::Queued, ..task.clone() }];
    if !plan::next_starts(&probe, cands, &[], 1, now).is_empty() {
        return None;
    }
    Some(plan::wait_why(run, task, cands, now))
}

/// The "why" of `Didn't run: ...` for a task that will never start.
fn not_run_text(why: &WaitWhy, cands: &[Cand], offset: i32) -> String {
    match why {
        WaitWhy::OutOfUsage { until_ms: Some(t) } => format!("every plan is out of usage until {}", report::hhmm(*t, offset)),
        WaitWhy::OutOfUsage { until_ms: None } => "every plan is out of usage".to_string(),
        WaitWhy::Blocked => "a plan stopped on a question for this project".to_string(),
        WaitWhy::NoPlan => match cands.iter().find_map(|c| c.runnable.as_ref().err()) {
            Some(reason) => reason.trim_end_matches('.').to_string(),
            None => "no plan is ready".to_string(),
        },
    }
}

// ---- outcomes and the end ------------------------------------------------------------------------

fn end_attempt(task: &mut Task, now: u64, end: AttemptEnd) {
    if let Some(a) = task.attempts.last_mut().filter(|a| a.end.is_none()) {
        a.ended_ms = Some(now);
        a.end = Some(end);
    }
}

fn empty_facts() -> Facts {
    Facts { mission: None, changes: None, summaries: Vec::new(), copy: None, stop_hhmm: String::new(), not_run: None }
}

fn fill_stop_time(facts: &mut Facts, run: &NightRun, io: &dyn NightIo) {
    if facts.stop_hhmm.is_empty() {
        facts.stop_hhmm = report::hhmm(run.stop_at_ms, io.utc_offset_min());
    }
}

/// Read the task's facts and give it its outcome. `not_run` is the reason when it never started.
fn finish_task(run: &mut NightRun, io: &dyn NightIo, ti: usize, not_run: Option<String>, now: u64) {
    let task = run.tasks[ti].clone();
    let mut facts = if task.attempts.is_empty() { empty_facts() } else { io.facts(run, &task) };
    if facts.not_run.is_none() {
        facts.not_run = not_run;
    }
    fill_stop_time(&mut facts, run, io);
    conclude(run, ti, facts, now);
}

fn conclude(run: &mut NightRun, ti: usize, facts: Facts, now: u64) {
    let outcome = classify(&run.tasks[ti], &facts);
    let glyph = match outcome.class {
        Class::Ready | Class::NoChecks => "v",
        Class::NeedsLook => "+",
        Class::NoChanges => ":",
        Class::DidntRun => "x",
    };
    let text = format!("Task {}: {}", ti + 1, outcome.headline);
    let id = run.tasks[ti].spec.id.clone();
    run.tasks[ti].outcome = Some(outcome);
    run.tasks[ti].state = TaskState::Finished;
    push_event(run, now, "task_done", glyph, text, Some(&id));
}

enum Why {
    TimeUp,
    Person,
}

/// The hard stop or the person's Stop: stop every open attempt, give every unfinished task an
/// outcome (copies stay) and finish the run.
fn stop_everything(run: &mut NightRun, io: &dyn NightIo, now: u64, why: Why) -> Next {
    let offset = io.utc_offset_min();
    let (end, not_run, state, kind, glyph, line) = match why {
        Why::TimeUp => (
            AttemptEnd::CutOff,
            format!("no time left before {}", report::hhmm(run.stop_at_ms, offset)),
            RunState::Done,
            "cut_off",
            "x",
            format!("Stop time {} reached. Everything still running was stopped.", report::hhmm(run.stop_at_ms, offset)),
        ),
        Why::Person => (
            AttemptEnd::StoppedByPerson,
            "you stopped Night Shift".to_string(),
            RunState::Stopped,
            "stopped",
            ":",
            "You stopped Night Shift. Everything still running was stopped.".to_string(),
        ),
    };
    push_event(run, now, kind, glyph, line, None);
    for ti in 0..run.tasks.len() {
        if run.tasks[ti].state == TaskState::Finished {
            continue;
        }
        let task = run.tasks[ti].clone();
        if let Some(a) = open_attempt(&task).cloned() {
            io.stop(&task, &a, BY);
            end_attempt(&mut run.tasks[ti], now, end.clone());
        }
        finish_task(run, io, ti, Some(not_run.clone()), now);
    }
    io.audit(kind, &line_of(run));
    finish(run, io, now, state)
}

fn line_of(run: &NightRun) -> String {
    report::line(&report::summary(run))
}

/// Every task has an outcome (or the stop came): release the computer, keep the record, send the
/// one morning push unless the person already looked.
fn finish(run: &mut NightRun, io: &dyn NightIo, now: u64, state: RunState) -> Next {
    run.state = state;
    run.finished_ms = Some(now);
    if run.keep_awake || run.awake_ok {
        let _ = io.keep_awake(false);
        run.awake_ok = false;
    }
    quiet::unmute_run(&run.id);
    STOPS.lock().remove(&run.id);
    let line = line_of(run);
    push_event(run, now, "finished", "v", format!("Finished. {line}"), None);
    let push = !run.push_sent && run.seen_ms.is_none();
    if push {
        run.push_sent = true;
    }
    io.persist(run);
    io.archive(run);
    if push {
        io.push_morning(run);
    }
    io.audit("finished", &line);
    Next::Finished
}

fn push_event(run: &mut NightRun, at_ms: u64, kind: &str, glyph: &str, text: String, task: Option<&str>) {
    run.events.push(Event { at_ms, kind: kind.into(), glyph: glyph.into(), text, task: task.map(str::to_string) });
    if run.events.len() > EVENTS_CAP {
        let extra = run.events.len() - EVENTS_CAP;
        run.events.drain(..extra);
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
