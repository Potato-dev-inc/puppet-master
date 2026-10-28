//! A scripted pane host for the visible path: no process, no registry. A test sets the screen the pane
//! shows (from its own thread, any time), reads what the wizard delivered, and can make the next
//! delivery do something (draw a spinner, a reply).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value;

use crate::tui::declarative::wizard::session::{Limits, WizardEnv};
use crate::tui::declarative::wizard::visible::capture::Config;
use crate::tui::declarative::wizard::visible::answer::AnswerKey;
use crate::tui::declarative::wizard::visible::host::{Delivered, Look, PaneHost, PaneSpec};
use crate::tui::declarative::wizard::visible::runner::{RunnerCfg, TestLimits};

#[derive(Debug, Clone)]
pub struct FakePane {
    pub screen: String,
    pub watch: Option<String>,
    pub status: String,
    pub alive: bool,
    pub cols: u16,
    pub rows: u16,
    pub card: Option<Value>,
    pub profile_id: String,
    pub cwd: PathBuf,
    /// The spawn asked for a pop-out window.
    pub window: bool,
    /// A status the test forces (otherwise: `running` for a few looks after the screen changed, like a pane).
    pub forced_status: Option<String>,
    /// Rows that scrolled off the top (the viewer's history above the screen).
    pub scrollback: Vec<String>,
    /// Looks at the pane since its screen last changed. Counted in looks, not in time, so a loaded machine
    /// sees the same sequence of pictures as an idle one.
    looks_since_change: u32,
}

/// How many looks after a change the pane still reports `running` (what a pane does while it draws).
const RUNNING_LOOKS: u32 = 4;

#[derive(Default)]
struct State {
    panes: HashMap<String, FakePane>,
    order: Vec<String>,
    delivered: Vec<(String, String)>,
    killed: Vec<String>,
    spawn_error: Option<String>,
    /// The first screen the next pane shows (default: the banner). A test sets it to a dialog it needs in place from
    /// the first look, rather than racing the wizard's first look at a banner.
    next_spawn_screen: Option<String>,
    shown: Vec<String>,
    counter: u64,
    /// Every key press the wizard made: (pane, keys).
    pressed: Vec<(String, Vec<AnswerKey>)>,
    /// The scripted agent's next screens per pane: `(looks, screen)`, see `FakeHost::schedule`.
    scripts: HashMap<String, std::collections::VecDeque<(u32, Step)>>,
}

type Hook = Box<dyn Fn(&FakeHost, &str, &str) -> Delivered + Send + Sync>;
type ResizeHook = Box<dyn Fn(&FakeHost, &str, u16, u16) + Send + Sync>;
type AnswerHook = Box<dyn Fn(&FakeHost, &str, &[AnswerKey]) + Send + Sync>;

pub struct FakeHost {
    state: Mutex<State>,
    on_deliver: Mutex<Option<Hook>>,
    on_answer: Mutex<Option<AnswerHook>>,
    on_resize: Mutex<Option<ResizeHook>>,
    /// Hosts that can show a scrollback and resize (the default fake cannot, like a host without a viewer).
    can_resize: std::sync::atomic::AtomicBool,
    /// The host shows the scrollback (but cannot resize).
    history_on: std::sync::atomic::AtomicBool,
    /// Every resize the wizard asked for: (pane, cols, rows).
    resized: Mutex<Vec<(String, u16, u16)>>,
}

impl std::fmt::Debug for FakeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakeHost")
    }
}

impl FakeHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            on_deliver: Mutex::new(None),
            on_answer: Mutex::new(None),
            on_resize: Mutex::new(None),
            can_resize: std::sync::atomic::AtomicBool::new(false),
            history_on: std::sync::atomic::AtomicBool::new(false),
            resized: Mutex::new(Vec::new()),
        })
    }

    /// This host resizes panes and shows their scrollback; `hook(host, pane, cols, rows)` then plays what the agent
    /// does about its new size (reprints its conversation, redraws only what is visible).
    pub fn on_resize(&self, hook: impl Fn(&FakeHost, &str, u16, u16) + Send + Sync + 'static) {
        *self.on_resize.lock() = Some(Box::new(hook));
        self.can_resize.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// This host shows the pane's scrollback (it still cannot resize a pane).
    pub fn enable_history(&self) {
        self.history_on.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Every resize the wizard asked for.
    pub fn resizes(&self) -> Vec<(String, u16, u16)> {
        self.resized.lock().clone()
    }

    /// Rows pushed into the scrollback (above the screen).
    pub fn push_scrollback(&self, pane: &str, rows: &[&str]) {
        if let Some(p) = self.state.lock().panes.get_mut(pane) {
            p.scrollback.extend(rows.iter().map(|r| r.to_string()));
        }
    }

    /// Drops the scrollback (what a viewer does at a resize when the profile says the agent reprints).
    pub fn clear_scrollback(&self, pane: &str) {
        if let Some(p) = self.state.lock().panes.get_mut(pane) {
            p.scrollback.clear();
        }
    }

    /// The next delivery calls `hook(host, pane_id, text)`.
    pub fn on_deliver(&self, hook: impl Fn(&FakeHost, &str, &str) -> Delivered + Send + Sync + 'static) {
        *self.on_deliver.lock() = Some(Box::new(hook));
    }

    /// The next key press calls `hook(host, pane_id, keys)` (a scripted menu moves its highlight, a dialog closes).
    pub fn on_answer(&self, hook: impl Fn(&FakeHost, &str, &[AnswerKey]) + Send + Sync + 'static) {
        *self.on_answer.lock() = Some(Box::new(hook));
    }

    /// Every key the wizard pressed, in order, flattened.
    pub fn pressed(&self) -> Vec<AnswerKey> {
        self.state.lock().pressed.iter().flat_map(|(_, k)| k.clone()).collect()
    }

    /// The first screen the next pane spawned shows.
    pub fn set_next_spawn_screen(&self, text: &str) {
        self.state.lock().next_spawn_screen = Some(text.to_string());
    }

    pub fn fail_spawn(&self, message: &str) {
        self.state.lock().spawn_error = Some(message.to_string());
    }

    fn last(&self) -> String {
        self.state.lock().order.last().cloned().expect("a pane was spawned")
    }

    pub fn pane_id(&self) -> String {
        self.last()
    }

    pub fn set_screen(&self, text: &str) {
        let pane = self.last();
        self.set_screen_of(&pane, text);
    }

    pub fn set_screen_of(&self, pane: &str, text: &str) {
        let mut st = self.state.lock();
        st.counter += 1;
        if let Some(p) = st.panes.get_mut(pane) {
            p.screen = text.to_string();
            p.watch = None;
            p.looks_since_change = 0;
        }
    }

    /// The scripted agent's next pictures for `pane`: each `(looks, screen)` shows `screen` after the wizard has
    /// looked at the pane `looks` more times. The wizard's own polling is the clock, so the order of the pictures
    /// never depends on how fast a test thread runs under load.
    pub fn schedule(&self, pane: &str, steps: &[(u32, String)]) {
        for (looks, screen) in steps {
            let screen = screen.clone();
            self.then(pane, *looks, move |h, p| h.set_screen_of(p, &screen));
        }
    }

    /// The scripted agent's next action on `pane`: `step` runs after `looks` more looks at the pane (it may change
    /// the screen, push scrollback, anything a pane does on its own). It runs on the wizard's thread, outside the
    /// host's lock.
    pub fn then(&self, pane: &str, looks: u32, step: impl FnOnce(&FakeHost, &str) + Send + 'static) {
        let mut st = self.state.lock();
        st.scripts.entry(pane.to_string()).or_default().push_back((looks, Box::new(step) as Step));
    }

    pub fn set_status(&self, status: &str) {
        let pane = self.last();
        if let Some(p) = self.state.lock().panes.get_mut(&pane) {
            p.forced_status = Some(status.to_string());
        }
    }

    pub fn set_card(&self, card: Option<Value>) {
        let pane = self.last();
        if let Some(p) = self.state.lock().panes.get_mut(&pane) {
            p.card = card;
        }
    }

    pub fn die(&self) {
        let pane = self.last();
        if let Some(p) = self.state.lock().panes.get_mut(&pane) {
            p.alive = false;
        }
    }

    pub fn delivered(&self) -> Vec<String> {
        self.state.lock().delivered.iter().map(|(_, t)| t.clone()).collect()
    }

    pub fn killed(&self) -> Vec<String> {
        self.state.lock().killed.clone()
    }

    pub fn shown(&self) -> Vec<String> {
        self.state.lock().shown.clone()
    }

    pub fn pane(&self, id: &str) -> Option<FakePane> {
        self.state.lock().panes.get(id).cloned()
    }

    pub fn spawned(&self) -> Vec<FakePane> {
        let st = self.state.lock();
        st.order.iter().filter_map(|id| st.panes.get(id).cloned()).collect()
    }
}

impl PaneHost for FakeHost {
    fn spawn(&self, spec: &PaneSpec) -> Result<String, String> {
        let mut st = self.state.lock();
        if let Some(e) = st.spawn_error.take() {
            return Err(e);
        }
        st.counter += 1;
        let id = format!("fake-pane-{}", st.counter);
        let first_screen = st.next_spawn_screen.take().unwrap_or_else(|| screens::BANNER.to_string());
        st.panes.insert(
            id.clone(),
            FakePane {
                // the agent has drawn its banner by the time the pane exists (the start check reads a screen)
                screen: first_screen,
                watch: None,
                status: "idle".into(),
                alive: true,
                cols: spec.cols,
                rows: spec.rows,
                card: None,
                profile_id: spec.profile.id().to_string(),
                cwd: spec.cwd.clone(),
                window: spec.window,
                forced_status: None,
                scrollback: Vec::new(),
                looks_since_change: 0,
            },
        );
        st.order.push(id.clone());
        Ok(id)
    }

    fn look(&self, pane: &str) -> Option<Look> {
        let due = take_due(&mut self.state.lock(), pane);
        if let Some(step) = due {
            step(self, pane);
        }
        let mut st = self.state.lock();
        let output_pos = Some(st.counter);
        // a pane that still has pictures to show is working (the agent is busy until its reply is up)
        let working = st.scripts.get(pane).is_some_and(|q| !q.is_empty());
        let p = st.panes.get_mut(pane)?;
        p.looks_since_change = p.looks_since_change.saturating_add(1);
        let p = &*p;
        Some(Look {
            screen: p.screen.clone(),
            watch: p.watch.clone().unwrap_or_else(|| p.screen.clone()),
            status: p.forced_status.clone().unwrap_or_else(|| {
                if p.alive && (working || p.looks_since_change <= RUNNING_LOOKS) {
                    "running".into()
                } else {
                    p.status.clone()
                }
            }),
            alive: p.alive,
            output_pos,
            hello_written: false,
            grid: Some((p.cols, p.rows)),
            history: String::new(),
            styled: None,
        })
    }

    fn deliver(&self, pane: &str, text: &str, _trace: &mut Vec<String>) -> Delivered {
        self.state.lock().delivered.push((pane.to_string(), text.to_string()));
        let hook = self.on_deliver.lock();
        match hook.as_ref() {
            Some(hook) => hook(self, pane, text),
            None => Delivered::Submitted,
        }
    }

    fn card(&self, pane: &str) -> Option<Value> {
        self.state.lock().panes.get(pane).and_then(|p| p.card.clone())
    }

    fn grid(&self, pane: &str) -> Option<(u16, u16)> {
        self.state.lock().panes.get(pane).map(|p| (p.cols, p.rows))
    }

    fn show(&self, pane: &str) {
        self.state.lock().shown.push(pane.to_string());
    }

    fn kill(&self, pane: &str) {
        let mut st = self.state.lock();
        st.killed.push(pane.to_string());
        if let Some(p) = st.panes.get_mut(pane) {
            p.alive = false;
        }
    }

    fn cwd(&self, pane: &str) -> Option<PathBuf> {
        self.state.lock().panes.get(pane).map(|p| p.cwd.clone())
    }

    fn resize(&self, pane: &str, cols: u16, rows: u16) -> Result<(), String> {
        if !self.can_resize.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("this host cannot resize a pane".into());
        }
        {
            let mut st = self.state.lock();
            let p = st.panes.get_mut(pane).ok_or("no such pane")?;
            p.cols = cols;
            p.rows = rows;
            // What the pane registry does at a resize for a profile that says the agent reprints its conversation:
            // the emulator drops its history (`wipe_history`) and the reprint that follows fills it again.
            if crate::tui::registry::registry().find(&p.profile_id).is_some_and(|profile| profile.reprints_on_resize()) {
                p.scrollback.clear();
            }
        }
        self.resized.lock().push((pane.to_string(), cols, rows));
        if let Some(hook) = self.on_resize.lock().as_ref() {
            hook(self, pane, cols, rows);
        }
        Ok(())
    }

    fn history(&self, pane: &str) -> Option<String> {
        if !self.can_resize.load(std::sync::atomic::Ordering::SeqCst) && !self.history_on.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        let st = self.state.lock();
        let p = st.panes.get(pane)?;
        Some(format!("{}
{}", p.scrollback.join("
"), p.screen))
    }

    fn viewer_grid(&self, pane: &str) -> Option<(u16, u16)> {
        self.state.lock().panes.get(pane).map(|p| (p.cols, p.rows))
    }

    fn answer(&self, pane: &str, keys: &[AnswerKey]) -> Result<(), String> {
        {
            let mut st = self.state.lock();
            if !st.panes.contains_key(pane) {
                return Err("this is not a pane the wizard started, so the wizard presses nothing in it".into());
            }
            st.pressed.push((pane.to_string(), keys.to_vec()));
        }
        if let Some(hook) = self.on_answer.lock().as_ref() {
            hook(self, pane, keys);
        }
        Ok(())
    }
}

/// One scripted action of the fake agent (see `FakeHost::then`).
type Step = Box<dyn FnOnce(&FakeHost, &str) + Send>;

/// Counts one look at `pane` against its next scripted action and takes the action out when its looks are up.
fn take_due(st: &mut State, pane: &str) -> Option<Step> {
    let queue = st.scripts.get_mut(pane)?;
    let front = queue.front_mut()?;
    front.0 = front.0.saturating_sub(1);
    if front.0 == 0 {
        return queue.pop_front().map(|(_, step)| step);
    }
    None
}

/// Limits for tests that run the real threads. The fake agent moves on the wizard's own looks (see `schedule`), so
/// these only bound a wait that is meant to end in failure. A success path gets a long bound: a wall-clock limit of
/// a few hundred milliseconds breaks under load, and that is not what these tests check.
pub fn quick_cfg() -> RunnerCfg {
    RunnerCfg {
        capture: Config {
            stable_ms: 60,
            start_limit_ms: 60_000,
            message_limit_ms: 60_000,
            no_reaction_ms: 60_000,
            unsure_ms: 300,
            reply_unsure_ms: 300,
            permission_wait_ms: 60_000,
            exit_wait_ms: 60_000,
            frozen_ms: 60_000,
            idle_done_ms: 60_000,
            question_ms: 60_000,
            work_step: false,
        },
        test: TestLimits {
            start: Duration::from_millis(80),
            ready: Duration::from_secs(20),
            person: Duration::from_secs(30),
            step: Duration::from_secs(2),
            busy: Duration::from_secs(20),
            done: Duration::from_secs(20),
            permission: Duration::from_secs(6),
            tick: Duration::from_millis(5),
        },
        tick: Duration::from_millis(5),
        window: false,
    }
}

pub fn quick_limits() -> Limits {
    Limits {
        version_timeout: Duration::from_secs(30),
        probe_timeout: Duration::from_secs(30),
        live_timeout: Duration::from_secs(90),
        live_grace: Duration::from_millis(300),
        ..Limits::default()
    }
}

/// Wait (polling, up to `secs`) until `check` returns Some.
/// How long a test waits for the test pane to exist. The visible test first makes a git project (three `git`
/// processes), which can take many seconds on a loaded CI runner, so the wait is long. It only bounds a wait that is
/// meant to succeed.
pub const PANE_WAIT_SECS: u64 = 60;

pub fn wait_for<T>(secs: u64, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(found) = check() {
            return Some(found);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[allow(dead_code)]
pub fn env_with(host: Arc<FakeHost>, base: WizardEnv) -> WizardEnv {
    WizardEnv { host, visible: quick_cfg(), ..base }
}

/// The screens of the made-up agent used by the visible-path tests (the same words the fake TUI's
/// `nimbus` layout draws, so one recorded draft reads both).
pub mod screens {
    pub const BANNER: &str = "NIMBUS 0.4\n";
    pub const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";
    pub const TRUST: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n  1. Yes, trust this folder\n  2. No, exit\n";
    pub const PERMISSION: &str = "NIMBUS\n\nAllow nimbus to create hello.txt? [y/n]\n";
    pub const COMMAND_PERMISSION: &str = "NIMBUS\n\nAllow nimbus to run `echo cccccc`? [y/n]\n";

    pub fn working(word: &str, n: usize) -> String {
        let spinner = ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}"][n % 4];
        format!("NIMBUS 0.4\n\n> Reply with exactly the word: {word}\n\n{spinner} Thinking...  ctrl+c to stop\n\n| > Type a task...             |\n")
    }

    pub fn reply(word: &str) -> String {
        format!("NIMBUS 0.4\n\n> Reply with exactly the word: {word}\nreply: {word}\n\ndone in 2s\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n  /help for commands | ctrl+c twice to quit\n")
    }
}

/// The made-up agent: typing a task makes it work and then answer; the hello.txt task asks permission
/// (and the pane shows an approval card). `answer` is what it says (empty: the word it was asked for).
pub fn script_agent(host: &Arc<FakeHost>, answer: &'static str) {
    host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            h.set_screen_of(pane, screens::PERMISSION);
            h.set_card(Some(serde_json::json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &screens::working(&word, 0));
        let said = if answer.is_empty() { word.clone() } else { answer.to_string() };
        h.schedule(pane, &working_then_reply(&word, &said));
        Delivered::Submitted
    });
}

/// The pictures of a plain turn: the work goes on for a few looks, then the reply is up (see `FakeHost::schedule`).
pub fn working_then_reply(word: &str, said: &str) -> Vec<(u32, String)> {
    vec![(3, screens::working(word, 1)), (3, screens::reply(said))]
}

/// Run the visible test of the session's verified draft on a thread, show the idle screen once the pane
/// exists, and return the reply. The agent is `script_agent`'s.
pub fn run_visible_test(
    wizard: &Arc<crate::tui::declarative::wizard::Wizard>,
    host: &Arc<FakeHost>,
    id: &str,
    plan: &str,
) -> Result<Value, crate::tui::declarative::wizard::WizardError> {
    script_agent(host, "");
    let (w, i, p) = (wizard.clone(), id.to_string(), plan.to_string());
    let before = host.spawned().len();
    let run = std::thread::spawn(move || {
        crate::tui::declarative::wizard::api::handle(&w, "visibletest", &serde_json::json!({"session": i, "confirm": true, "plan": p}))
    });
    wait_for(PANE_WAIT_SECS, || (host.spawned().len() > before).then_some(())).expect("the test pane is started");
    host.set_screen(screens::IDLE);
    run.join().unwrap()
}
