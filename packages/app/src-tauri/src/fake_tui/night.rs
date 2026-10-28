//! Night Shift for the end-to-end tests (SPEC n32 NS-21): the real Start checks (`commands::night::start`),
//! the real runner on the real live adapter (`LiveIo`: missions, `run_agent`, the limit memory, the copies),
//! the real Keep and Throw away. Only two things are the test's: the agents (`pm-fake-tui`) and the clock.
//!
//! The clock is the adapter's own seam (`LiveIo::with_skew`): [`NightRig::advance_min`] moves it forward, so a
//! test crosses a stop time or a limit's three-minute grace without waiting for it. A wake-up thread nudges
//! the supervisor every half second instead of every five (the app's own tick), which changes how fast the
//! night goes, not what it does.
//!
//! Nothing here can type into a pane: the rig has `start`, `advance_min`, reads, `keep` and `throw_away`.

use crate::night_shift::io::LiveIo;
use crate::night_shift::model::NightRun;
use crate::night_shift::review::{self, Live};
use crate::night_shift::store::Store;
use crate::night_shift::{checks, report, runner, view};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often the supervisor is woken (the app's own tick is `TICK_MS`).
const NUDGE: Duration = Duration::from_millis(500);

pub struct NightRig {
    registry: Arc<Mutex<PaneRegistry>>,
    store: Store,
    project: PathBuf,
    skew_ms: Arc<AtomicU64>,
    nudging: Arc<AtomicBool>,
    nudger: Mutex<Option<JoinHandle<()>>>,
    runner: Mutex<Option<JoinHandle<()>>>,
}

fn error_json(error: crate::operations::OperationError) -> Value {
    json!({ "code": error.code, "message": error.message })
}

/// Whether the folder has a commit (what a copy needs), as Start checks it.
fn has_commit(key: &str) -> bool {
    Path::new(key).is_dir()
        && crate::proc_util::hidden_command("git")
            .args(["rev-parse", "--verify", "-q", "HEAD"])
            .current_dir(key)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
}

impl NightRig {
    pub(super) fn new(registry: Arc<Mutex<PaneRegistry>>, project: PathBuf) -> NightRig {
        // A limit note an earlier test of this process left behind would decide which plans look free.
        crate::handoff::forget_limit_notes_for_e2e();
        NightRig {
            registry,
            store: Store::live(),
            project,
            skew_ms: Arc::new(AtomicU64::new(0)),
            nudging: Arc::new(AtomicBool::new(true)),
            nudger: Mutex::new(None),
            runner: Mutex::new(None),
        }
    }

    /// The project's declared test command (Tonight > Tests), saved the way the popover saves it.
    pub fn declare_tests(&self, name: &str, argv: &[&str]) -> Result<(), String> {
        let check = checks::Declared {
            name: name.to_string(),
            argv: argv.iter().map(|word| word.to_string()).collect(),
            timeout_min: 2,
            declared_ms: 0,
        };
        checks::declare(&self.store, &self.project.to_string_lossy(), vec![check])
    }

    /// Start tonight like the sheet's Start: `tasks` are (words, plan key or `None` for any plan). The stop time
    /// is `stop_in_min` minutes ahead, rounded the way the sheet's `HH:MM` is. `Err` carries the refusal.
    pub fn start(&self, tasks: &[(&str, Option<&str>)], plans: &[&str], stop_in_min: u32, parallel: u8) -> Result<Value, Value> {
        use crate::commands::night::start::{self, Env, StartInput};
        let project = self.project.to_string_lossy().into_owned();
        let stop = crate::missions::store::now_ms() + u64::from(stop_in_min) * 60_000;
        let stop_hhmm = chrono::TimeZone::timestamp_millis_opt(&chrono::Local, stop as i64).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
        let input: StartInput = serde_json::from_value(json!({
            "tasks": tasks.iter().map(|(text, who)| json!({ "project": project, "text": text, "who": who })).collect::<Vec<_>>(),
            "plans": plans,
            "stopHhmm": stop_hhmm,
            "startHhmm": null,
            "parallel": parallel,
            "keepAwake": false,
        }))
        .map_err(|e| json!({ "code": "BAD_INPUT", "message": e.to_string() }))?;
        let known = start::known_plans(&[project.clone()]);
        let busy = self.store.load().is_some_and(|run| matches!(run.state, crate::night_shift::model::RunState::Scheduled { .. } | crate::night_shift::model::RunState::Running));
        let key_of = |raw: &str| crate::project_path::comparable_project_path(Path::new(raw));
        let declared = |key: &str| -> Vec<Vec<String>> { checks::declared(&self.store, key).into_iter().map(|check| check.argv).collect() };
        let env = Env {
            now_ms: crate::missions::store::now_ms(),
            enabled: true,
            undo_on: true,
            awake_asked: true,
            busy,
            plans: &known,
            key_of: &key_of,
            is_git: &has_commit,
            declared: &declared,
        };
        let mut run = start::build(&input, &env).map_err(error_json)?;
        self.store.save(&mut run).map_err(|message| json!({ "code": "SAVE_FAILED", "message": message }))?;
        let io = Arc::new(LiveIo::with_skew(&run.id, self.skew_ms.clone()));
        *self.runner.lock() = runner::spawn(io, run.id.clone());
        self.start_nudging();
        Ok(self.run())
    }

    fn start_nudging(&self) {
        self.nudging.store(true, Ordering::Relaxed);
        let flag = self.nudging.clone();
        let spawned = std::thread::Builder::new().name("night-e2e-nudge".into()).spawn(move || {
            while flag.load(Ordering::Relaxed) {
                runner::wake();
                std::thread::sleep(NUDGE);
            }
        });
        *self.nudger.lock() = spawned.ok();
    }

    /// Move the night's clock forward. The runner sees it on its next tick.
    pub fn advance_min(&self, minutes: u64) {
        self.skew_ms.fetch_add(minutes * 60_000, Ordering::Relaxed);
        runner::wake();
    }

    /// The saved run (current or last) as JSON: tasks, attempts, outcomes, events. `Null` when there is none.
    pub fn run(&self) -> Value {
        self.store.load().and_then(|run| serde_json::to_value(run).ok()).unwrap_or(Value::Null)
    }

    /// The saved run, typed (for the helpers below).
    fn record(&self) -> Option<NightRun> {
        self.store.load()
    }

    /// Polls the saved run until `check` is true or `secs` pass. Returns the last run read either way.
    pub fn wait_for(&self, secs: u64, mut check: impl FnMut(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let run = self.run();
            if check(&run) || Instant::now() >= deadline {
                return run;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits until the run has finished (done or stopped).
    pub fn wait_finished(&self, secs: u64) -> Value {
        self.wait_for(secs, |run| matches!(run["state"]["state"].as_str(), Some("done") | Some("stopped")) || matches!(run["state"].as_str(), Some("done") | Some("stopped")))
    }

    /// The Morning page's view of the run (what the desktop reads), as JSON.
    pub fn morning(&self) -> Value {
        let Some(run) = self.record() else { return Value::Null };
        let offset = chrono::Local::now().offset().local_minus_utc() / 60;
        let into = |project: &str| review::keep_target(project);
        let ctx = crate::redact::known::context();
        serde_json::to_value(view::view_of_with(&run, &ctx, offset, &into)).unwrap_or(Value::Null)
    }

    /// The one line the morning push carries (counts only).
    pub fn summary_line(&self) -> String {
        self.record().map(|run| report::line(&report::summary(&run))).unwrap_or_default()
    }

    fn task_id(&self, index: usize) -> Result<String, Value> {
        self.record()
            .and_then(|run| run.tasks.get(index).map(|task| task.spec.id.clone()))
            .ok_or_else(|| json!({ "code": "NO_SUCH_TASK", "message": format!("task {index}") }))
    }

    /// The person's Keep (desktop only in the app). `Ok` carries the result view: `merged`, `into`, `reason`.
    pub fn keep(&self, index: usize) -> Result<Value, Value> {
        let id = self.task_id(index)?;
        let ops = Live { registry: &self.registry };
        review::keep_with(&self.store, &ops, &id, "you", crate::missions::store::now_ms())
            .map(|keep| serde_json::to_value(keep).unwrap_or(Value::Null))
            .map_err(error_json)
    }

    /// The person's Throw away.
    pub fn throw_away(&self, index: usize) -> Result<(), Value> {
        let id = self.task_id(index)?;
        let ops = Live { registry: &self.registry };
        review::throw_away_with(&self.store, &ops, &id, "you", crate::missions::store::now_ms()).map_err(error_json)
    }

    /// Whether the folder of the task's result copy is still on disk. `None` when the task has no copy.
    pub fn copy_on_disk(&self, index: usize) -> Option<bool> {
        let run = self.record()?;
        let task = run.tasks.get(index)?;
        let copy = task.outcome.as_ref()?.copy.clone()?;
        let listed = crate::worktrees::list(&copy.project).into_iter().find(|entry| entry.worktree.handle == copy.handle);
        Some(listed.is_some_and(|entry| entry.exists))
    }

    /// The folder of the task's result copy.
    pub fn copy_path(&self, index: usize) -> Option<PathBuf> {
        let run = self.record()?;
        let copy = run.tasks.get(index)?.outcome.as_ref()?.copy.clone()?;
        crate::worktrees::list(&copy.project)
            .into_iter()
            .find(|entry| entry.worktree.handle == copy.handle)
            .map(|entry| PathBuf::from(entry.worktree.path))
    }

    /// Stops the night (like the Stop button) and waits for the supervisor to end. Safe to call twice.
    pub fn stop(&self) {
        if let Some(run) = self.record() {
            if runner::is_running(&run.id) {
                runner::request_stop(&run.id);
            }
        }
        if let Some(handle) = self.runner.lock().take() {
            // The supervisor ends on its next tick; the nudger is still waking it.
            let _ = handle.join();
        }
        self.nudging.store(false, Ordering::Relaxed);
        if let Some(handle) = self.nudger.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for NightRig {
    fn drop(&mut self) {
        self.stop();
    }
}
