//! Night Shift: the live `NightIo` (SPEC n32 3.5). Not compiled in unit tests, because it links the
//! windowing runtime; the runner tests use a fake.
//!
//! One `LiveIo` serves one run. A task with declared checks starts as a mission (`missions::start`),
//! a task without starts as a single turn in a separate copy (`agent_runs::run_agent`). Reading is
//! `missions::find`, the operation record and the plan's limit memory. Stopping is
//! `missions::stop` or `agent_runs::cancel_agent`, by handle. Nothing here types into a pane.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::{read_waiting, NightIo, Started, StartFail, Touched, Watch};
use crate::agent_runs::AgentRunRequest;
use crate::missions::decide::SETUP_CODES;
use crate::missions::model::{AgentChoice, Mission, MissionState, Needs, Starter};
use crate::night_shift::classify::{gate_sentence, Facts};
use crate::night_shift::io_facts::{facts_from_mission, facts_from_run, turn_lines};
use crate::night_shift::io_start::{self, candidates, mission_request, single_request, start_fail_from};
use crate::night_shift::model::{Attempt, AttemptKind, NightRun, Task};
use crate::night_shift::plan::Cand;
use crate::night_shift::power;
use crate::night_shift::report;
use crate::night_shift::store::Store;
use crate::night_shift::view;
use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus};
use crate::worktrees;

/// The live adapter for one run.
pub struct LiveIo {
    run_id: String,
    store: Store,
    /// When a waiting state (limit, question) was first seen, by attempt: the clock for "parked 3 min".
    seen: Mutex<HashMap<String, u64>>,
}

/// A live adapter for `run_id`, ready to hand to `runner::spawn`.
pub fn live(run_id: &str) -> Arc<dyn NightIo> {
    Arc::new(LiveIo::new(run_id))
}

impl LiveIo {
    pub fn new(run_id: &str) -> LiveIo {
        LiveIo { run_id: run_id.to_string(), store: Store::live(), seen: Mutex::new(HashMap::new()) }
    }

    /// How long `tag` has been seen waiting. The first sight starts the clock; any other state clears it.
    fn waited(&self, tag: &str, waiting: bool) -> u64 {
        let now = self.now_ms();
        let mut seen = self.seen.lock();
        if !waiting {
            seen.remove(tag);
            return 0;
        }
        let first = *seen.entry(tag.to_string()).or_insert(now);
        now.saturating_sub(first)
    }

    fn clean(&self) -> impl Fn(&str) -> String {
        let ctx = crate::redact::known::context();
        move |text: &str| view::clean(text, &ctx)
    }

    fn tag(task: &Task, a: &Attempt) -> String {
        format!("{}:{}", task.spec.id, a.n)
    }

    fn watch_mission(&self, task: &Task, a: &Attempt) -> Watch {
        let Some(m) = a.mission_id.as_deref().and_then(crate::missions::find) else { return Watch::Gone };
        match m.state {
            MissionState::Green | MissionState::Stopped | MissionState::Failed | MissionState::HandedOff => return Watch::Done,
            MissionState::NeedsYou => return setup_gate(&m, &a.label).unwrap_or(Watch::Done),
            MissionState::Paused => return Watch::Working,
            MissionState::Planning | MissionState::Working | MissionState::Checking => {}
        }
        let open = m.rounds.last().filter(|r| r.end.is_none()).and_then(|r| r.operation_id.clone());
        match open {
            Some(op_id) => self.watch_operation(task, a, &m.project, &op_id),
            None => {
                self.waited(&Self::tag(task, a), false);
                Watch::Working
            }
        }
    }

    fn watch_operation(&self, task: &Task, a: &Attempt, project: &str, operation_id: &str) -> Watch {
        let Ok(snapshot) = operations::get_operation(project, operation_id) else { return Watch::Working };
        self.read_operation(task, a, &snapshot, false)
    }

    /// `single`: the operation is the whole task, so its end is the task's end.
    fn read_operation(&self, task: &Task, a: &Attempt, snapshot: &OperationSnapshot, single: bool) -> Watch {
        let tag = Self::tag(task, a);
        let waiting = snapshot.status == OperationStatus::WaitingInput;
        let waited = self.waited(&tag, waiting);
        match snapshot.status {
            OperationStatus::WaitingInput => match snapshot.required_action.as_ref() {
                Some(action) => read_waiting(action, &a.label, waited, &self.clean()),
                None => Watch::Working,
            },
            OperationStatus::Completed if single => Watch::Done,
            OperationStatus::Failed if single => match snapshot.error.as_ref() {
                Some(e) if SETUP_CODES.contains(&e.code.as_str()) => Watch::Gate {
                    code: e.code.clone(),
                    sentence: gate_sentence(&e.code, &a.label).unwrap_or_else(|| (self.clean())(&e.message)),
                },
                Some(e) => Watch::Failed { code: e.code.clone(), sentence: (self.clean())(&e.message) },
                None => Watch::Failed { code: "FAILED".into(), sentence: "The agent stopped with an error.".into() },
            },
            OperationStatus::Cancelled if single => Watch::Failed {
                code: "CANCELLED".into(),
                sentence: "It was stopped before it finished.".into(),
            },
            _ => Watch::Working,
        }
    }

    fn single_operation(&self, run_id: &str, task: &Task, a: &Attempt) -> Option<OperationSnapshot> {
        let key = idempotency_key_of(run_id, task, a);
        operations::find_by_idempotency_key(&task.spec.project, &key).ok().flatten()
    }

    fn mission_of(a: &Attempt) -> Option<Mission> {
        a.mission_id.as_deref().and_then(crate::missions::find)
    }

    /// The pane ids of an attempt: the ones recorded and, for a mission, those of every turn.
    fn pane_ids(&self, task: &Task, a: &Attempt) -> Vec<String> {
        let mut found = a.pane_ids.clone();
        let more: Vec<String> = match a.kind {
            AttemptKind::Mission => Self::mission_of(a)
                .map(|m| m.rounds.iter().filter_map(|r| r.pane_id.clone()).collect())
                .unwrap_or_default(),
            AttemptKind::Single => self.single_operation(&self.run_id, task, a).and_then(|op| op.pane_id).into_iter().collect(),
        };
        for pane in more {
            if !found.contains(&pane) {
                found.push(pane);
            }
        }
        found
    }

    fn sentences(&self, panes: &[String]) -> Vec<String> {
        panes
            .iter()
            .flat_map(|pane| crate::turn_summary::tracker().summaries(pane))
            .map(|s| s.sentence)
            .collect()
    }
}

/// A mission that needs the person because its turn could not get going (trust, sign-in, startup
/// screen) is a gate, not a failure of the task. Other needs are read as `Done`.
fn setup_gate(m: &Mission, label: &str) -> Option<Watch> {
    if m.needs != Some(Needs::Setup) {
        return None;
    }
    let code = m.rounds.last()?.end.as_ref()?.code.clone()?;
    if !SETUP_CODES.contains(&code.as_str()) {
        return None;
    }
    let sentence = gate_sentence(&code, label)?;
    Some(Watch::Gate { code, sentence })
}

fn idempotency_key_of(run_id: &str, task: &Task, a: &Attempt) -> String {
    io_start::key_of(run_id, &task.spec.id, a.n)
}

fn no_app() -> OperationError {
    OperationError::new("APP_NOT_READY", "Puppet Master is still starting", true)
}

impl NightIo for LiveIo {
    fn now_ms(&self) -> u64 {
        crate::missions::store::now_ms()
    }

    fn utc_offset_min(&self) -> i32 {
        chrono::Local::now().offset().local_minus_utc() / 60
    }

    fn load(&self) -> Option<NightRun> {
        self.store.load().filter(|run| run.id == self.run_id)
    }

    fn persist(&self, run: &mut NightRun) {
        if let Err(error) = self.store.save(run) {
            tracing::warn!(error = %error, "night shift: could not save the run");
        }
        if let Some(app) = crate::bridge::live_app() {
            use tauri::Emitter;
            let state = serde_json::to_value(&run.state).unwrap_or(Value::Null);
            let _ = app.emit("night://changed", json!({ "rev": run.rev, "state": state }));
        }
    }

    fn archive(&self, run: &NightRun) {
        if let Err(error) = self.store.archive(run) {
            tracing::warn!(error = %error, "night shift: could not archive the run");
        }
    }

    fn candidates(&self, run: &NightRun) -> Vec<Cand> {
        let descriptors = crate::tui::descriptors();
        let accounts = crate::accounts::Store::default_location().list();
        // A plan must work in every remaining task's folder, as Start checks it (not only the first).
        let folders = io_start::folders_of(run);
        candidates(
            run,
            &descriptors,
            &accounts,
            &|a: &AgentChoice| crate::handoff::agent_limit(&a.agent_type, a.account.as_deref()),
            &|a: &AgentChoice| {
                io_start::ok_in_all(&folders, &|folder| crate::companion::agents::agent_ok(&a.agent_type, folder).map_err(str::to_string))
            },
        )
    }

    fn other_active_missions(&self, run: &NightRun) -> usize {
        let ours = run.open_attempts().iter().filter(|a| a.kind == AttemptKind::Mission).count();
        crate::missions::active_count().saturating_sub(ours)
    }

    fn start(&self, run: &NightRun, task: &Task, plan: &AgentChoice, n: u8) -> Result<Started, StartFail> {
        if !task.checks.is_empty() {
            let undo_on = crate::bridge::live_app()
                .map(|app| crate::settings_store::read_public_settings(&app))
                .and_then(|s| s.get("checkpoints_enabled").and_then(Value::as_bool))
                .unwrap_or(true);
            let request = mission_request(run, task, plan, n, undo_on);
            debug_assert!(matches!(request.started_by, Starter::Night { .. }));
            return match crate::missions::start(request) {
                Ok(m) => Ok(Started { mission_id: Some(m.id.clone()), handle: m.handle(), pane_ids: Vec::new() }),
                Err(error) => Err(start_fail_from(error)),
            };
        }
        let registry = crate::bridge::live_registry().ok_or_else(|| start_fail_from(no_app()))?;
        let app = crate::bridge::live_app().ok_or_else(|| start_fail_from(no_app()))?;
        let request: AgentRunRequest = serde_json::from_value(single_request(run, task, plan, n))
            .map_err(|e| start_fail_from(OperationError::new("INVALID_ARGUMENT", e.to_string(), false)))?;
        match crate::agent_runs::run_agent(request, registry, app, None) {
            Ok(view) => Ok(Started { mission_id: None, handle: view.handle, pane_ids: view.pane_id.into_iter().collect() }),
            Err(error) => Err(start_fail_from(error)),
        }
    }

    fn watch(&self, task: &Task, a: &Attempt) -> Watch {
        match a.kind {
            AttemptKind::Mission => self.watch_mission(task, a),
            AttemptKind::Single => match self.single_operation(&self.run_id, task, a) {
                Some(snapshot) => self.read_operation(task, a, &snapshot, true),
                None => Watch::Gone,
            },
        }
    }

    fn touched(&self, task: &Task, a: &Attempt) -> Touched {
        let mut handles: Vec<String> = a.handle.iter().cloned().collect();
        if let Some(m) = Self::mission_of(a) {
            handles.extend(m.rounds.iter().filter_map(|r| r.handle.clone()));
        }
        handles.sort();
        handles.dedup();
        Touched { pane_ids: self.pane_ids(task, a), handles }
    }

    fn stop(&self, task: &Task, a: &Attempt, by: &str) {
        match a.kind {
            AttemptKind::Mission => {
                if let Some(id) = a.mission_id.as_deref() {
                    let _ = crate::missions::stop(id, by);
                }
            }
            AttemptKind::Single => {
                let Some(handle) = a.handle.as_deref() else { return };
                if let (Some(registry), Some(app)) = (crate::bridge::live_registry(), crate::bridge::live_app()) {
                    let _ = crate::agent_runs::cancel_agent(&task.spec.project, handle, None, registry, app);
                }
            }
        }
    }

    fn facts(&self, run: &NightRun, task: &Task) -> Facts {
        let clean = self.clean();
        let Some(last) = task.attempts.last() else { return empty_facts() };
        let mut facts = match last.kind {
            AttemptKind::Mission => match Self::mission_of(last) {
                Some(m) => {
                    let mut f = facts_from_mission(&m, &clean);
                    f.changes = m.worktree.as_ref().and_then(|w| worktrees::changes(w).ok());
                    f
                }
                None => empty_facts(),
            },
            AttemptKind::Single => match self.single_operation(&run.id, task, last) {
                Some(op) => {
                    let changes = op.worker.worktree.as_ref().and_then(|w| worktrees::changes(w).ok());
                    facts_from_run(&op, changes, &clean)
                }
                None => empty_facts(),
            },
        };
        let panes = self.pane_ids(task, last);
        facts.summaries = turn_lines(&self.sentences(&panes), &clean);
        facts.stop_hhmm = report::hhmm(run.stop_at_ms, self.utc_offset_min());
        facts
    }

    fn keep_awake(&self, on: bool) -> Result<(), String> {
        // Held by run id in `power`, so the exit hook can let go of it too.
        if on {
            power::hold(&self.run_id, "Night Shift")
        } else {
            power::release(&self.run_id); // lets the computer sleep again
            Ok(())
        }
    }

    fn push_morning(&self, run: &NightRun) {
        let run_id = run.id.clone();
        let at_ms = run.finished_ms.unwrap_or(run.stop_at_ms).max(run.stop_at_ms);
        let store = self.store.clone();
        let spawned = std::thread::Builder::new().name("night-push".into()).spawn(move || {
            // Held until the stop time, so the person is not woken because the queue finished early.
            while crate::missions::store::now_ms() < at_ms {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            let Some(latest) = store.load().filter(|r| r.id == run_id) else { return };
            if latest.seen_ms.is_some() {
                return; // the person already opened the Morning page
            }
            deliver_push(&report::line(&report::summary(&latest)));
        });
        if let Err(error) = spawned {
            tracing::warn!(error = %error, "night shift: could not start the morning push thread");
        }
    }

    fn audit(&self, kind: &str, summary: &str) {
        crate::audit::record(
            crate::audit::Kind::NightShift,
            crate::audit::Who::Desktop,
            summary,
            json!({ "run": self.run_id, "event": kind }),
        );
    }
}

/// Sends the one morning push with this line (counts only). The caller already skipped it when the
/// person opened the Morning page; `notify_night` stays silent when no phone is paired or watching.
fn deliver_push(line: &str) {
    tracing::info!(line = %line, "night shift: morning push");
    crate::push::notify_night(line, true);
}

fn empty_facts() -> Facts {
    Facts { mission: None, changes: None, summaries: Vec::new(), copy: None, stop_hhmm: String::new(), not_run: None }
}
