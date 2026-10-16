//! The thread that drives one guided capture: it looks at the pane every tick, hands the look to the
//! engine (`capture::Capture`), and carries out the one thing the engine can ask for, a delivery.
//! Everything else (status, the person's buttons, finish) goes through `CaptureRun`, which shares the
//! engine with the thread behind one lock. The delivery itself runs outside the lock.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::super::exec;
use super::super::livetest::mask_words;
use super::super::session::TestWords;
use super::capture::{Action, Capture, CaptureAction, CaptureSet, Config, StepId};
use super::host::{Delivered, Look, PaneHost};

/// Rows of the screen the status carries (the pane is 40 rows).
const PREVIEW_ROWS: usize = 40;

/// How long each check of the visible test may take (SPEC 11.9). Tests shrink them.
#[derive(Debug, Clone)]
pub struct TestLimits {
    pub start: Duration,
    pub ready: Duration,
    /// How long the test waits for the person to answer a question on the way.
    pub person: Duration,
    pub busy: Duration,
    pub done: Duration,
    pub permission: Duration,
    pub tick: Duration,
}

impl Default for TestLimits {
    fn default() -> Self {
        Self {
            start: Duration::from_secs(3),
            ready: Duration::from_secs(90),
            person: Duration::from_secs(300),
            busy: Duration::from_secs(30),
            done: Duration::from_secs(180),
            permission: Duration::from_secs(120),
            tick: Duration::from_millis(250),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunnerCfg {
    pub capture: Config,
    pub test: TestLimits,
    /// Pause between two looks.
    pub tick: Duration,
    /// Open the pane as a pop-out window.
    pub window: bool,
}

impl Default for RunnerCfg {
    fn default() -> Self {
        Self { capture: Config::default(), test: TestLimits::default(), tick: Duration::from_millis(250), window: true }
    }
}

struct Shared {
    engine: Capture,
    pane: String,
    profile_id: String,
    project: PathBuf,
    /// The last look, for the status preview.
    look: Option<Look>,
    stop: bool,
    /// `finish` ran: the capture set is final.
    finished: bool,
}

/// One running (or finished) capture. The status and the buttons are methods of this.
pub struct CaptureRun {
    shared: Arc<Mutex<Shared>>,
    host: Arc<dyn PaneHost>,
    words: TestWords,
    started: Instant,
    cancel: Arc<AtomicBool>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl std::fmt::Debug for CaptureRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CaptureRun")
    }
}

impl CaptureRun {
    /// Start the thread for a pane that is already spawned.
    pub fn start(
        host: Arc<dyn PaneHost>,
        pane: String,
        profile_id: String,
        project: PathBuf,
        words: TestWords,
        grid: (u16, u16),
        cfg: RunnerCfg,
        cancel: Arc<AtomicBool>,
    ) -> Arc<CaptureRun> {
        let shared = Arc::new(Mutex::new(Shared {
            engine: Capture::new(words.clone(), grid, cfg.capture.clone()),
            pane,
            profile_id,
            project,
            look: None,
            stop: false,
            finished: false,
        }));
        let run = Arc::new(CaptureRun {
            shared: shared.clone(),
            host: host.clone(),
            words,
            started: Instant::now(),
            cancel: cancel.clone(),
            handle: Mutex::new(None),
        });
        let thread_run = run.clone();
        let tick = cfg.tick;
        let handle = std::thread::Builder::new()
            .name("wizard-capture".into())
            .spawn(move || thread_run.run_loop(tick))
            .ok();
        *run.handle.lock() = handle;
        run
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn run_loop(&self, tick: Duration) {
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                self.shut_down();
                return;
            }
            let (pane, project) = {
                let g = self.shared.lock();
                if g.stop {
                    return;
                }
                (g.pane.clone(), g.project.clone())
            };
            let mut look = self.host.look(&pane).unwrap_or(Look { alive: false, ..Look::default() });
            look.hello_written = project.join("hello.txt").is_file();
            let now = self.now();
            let actions = {
                let mut g = self.shared.lock();
                g.look = Some(look.clone());
                g.engine.tick(&look, now)
            };
            for action in actions {
                match action {
                    Action::Deliver { text } => {
                        let mut trace = Vec::new();
                        let outcome = self.host.deliver(&pane, &text, &mut trace);
                        let now = self.now();
                        let mut g = self.shared.lock();
                        match outcome {
                            Delivered::Submitted => g.engine.on_delivered(true, "", now),
                            Delivered::Refused(why) => g.engine.on_delivered(false, &why, now),
                        }
                    }
                }
            }
            std::thread::sleep(tick);
        }
    }

    /// Kill the pane, drop its profile, stop the thread. Safe to call twice.
    pub fn shut_down(&self) {
        let (pane, profile) = {
            let mut g = self.shared.lock();
            g.stop = true;
            (g.pane.clone(), g.profile_id.clone())
        };
        self.host.kill(&pane);
        crate::tui::registry::registry().remove_transient(&profile);
    }

    /// Stop and wait for the thread (never from the thread itself).
    pub fn stop_and_join(&self) {
        self.shut_down();
        if let Some(handle) = self.handle.lock().take() {
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
    }

    pub fn pane(&self) -> String {
        self.shared.lock().pane.clone()
    }

    pub fn show(&self) {
        let pane = self.pane();
        self.host.show(&pane);
    }

    pub fn act(&self, step: Option<StepId>, action: CaptureAction) -> Result<(), String> {
        let now = self.now();
        let mut g = self.shared.lock();
        let project = g.project.clone();
        let result = g.engine.act(step, action, now);
        if result.is_ok() && action == CaptureAction::Retry {
            // the file of the earlier try would look like this try's evidence
            let _ = std::fs::remove_file(project.join("hello.txt"));
        }
        result
    }

    /// The last looks of the capture and the engine's verdict for each, newest last. `masked` frames
    /// are what an AI or the wizard window may see; `raw` frames keep the real words and are for the
    /// benchmark's own files only.
    pub fn debug_json(&self, with_raw: bool) -> Value {
        let now = self.now();
        let g = self.shared.lock();
        let frames: Vec<Value> = g
            .engine
            .debug_frames()
            .iter()
            .map(|f| {
                let mut v = json!({
                    "ms": f.ms,
                    "step": f.step,
                    "step_status": f.step_status,
                    "classifier": f.kind,
                    "reading": f.reading,
                    "stable_ms": f.stable_ms,
                    "quiet_ms": f.quiet_ms,
                    "needs_you": f.needs,
                    "screen": mask_words(&f.text, &self.words),
                });
                if with_raw {
                    v["raw"] = json!(f.text);
                }
                v
            })
            .collect();
        json!({"at_ms": now, "frames": frames, "status": g.engine.status_json(now), "deliveries": g.engine.deliveries()})
    }

    pub fn is_finished(&self) -> bool {
        self.shared.lock().finished
    }

    /// The masked preview of the pane's screen (what the person sees in the wizard window).
    fn preview(&self, look: &Option<Look>) -> String {
        let Some(look) = look else { return String::new() };
        let text = mask_words(&exec::clean(&look.screen), &self.words);
        let rows: Vec<&str> = text.lines().collect();
        let from = rows.len().saturating_sub(PREVIEW_ROWS);
        rows[from..].join("\n")
    }

    pub fn status(&self) -> Value {
        let now = self.now();
        let g = self.shared.lock();
        let mut status = g.engine.status_json(now);
        let grid = self.host.grid(&g.pane).unwrap_or(g.engine.grid());
        status["state"] = json!(if g.finished { "finished" } else { "running" });
        status["pane"] = json!(g.pane);
        status["grid"] = json!({"cols": grid.0, "rows": grid.1});
        status["screen"] = json!(self.preview(&g.look));
        status["deliveries"] = json!(g.engine.deliveries());
        status
    }

    /// Stop typing, take the capture set, kill the pane.
    pub fn finish(&self) -> CaptureSet {
        let set = {
            let mut g = self.shared.lock();
            g.finished = true;
            let mut set = g.engine.into_set();
            if let Some(grid) = self.host.grid(&g.pane) {
                set.grid = grid;
            }
            set
        };
        self.stop_and_join();
        set
    }
}

impl Drop for CaptureRun {
    fn drop(&mut self) {
        // The thread holds an Arc of its own, so this only runs when it has ended.
        let mut g = self.shared.lock();
        g.stop = true;
    }
}
