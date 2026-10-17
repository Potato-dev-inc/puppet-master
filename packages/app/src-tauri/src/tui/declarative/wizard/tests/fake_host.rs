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
    /// A status the test forces (otherwise: `running` for a moment after the screen changed, like a pane).
    pub forced_status: Option<String>,
    changed_at: std::time::Instant,
}

#[derive(Default)]
struct State {
    panes: HashMap<String, FakePane>,
    order: Vec<String>,
    delivered: Vec<(String, String)>,
    killed: Vec<String>,
    spawn_error: Option<String>,
    shown: Vec<String>,
    counter: u64,
    /// Every key press the wizard made: (pane, keys).
    pressed: Vec<(String, Vec<AnswerKey>)>,
}

type Hook = Box<dyn Fn(&FakeHost, &str, &str) -> Delivered + Send + Sync>;
type AnswerHook = Box<dyn Fn(&FakeHost, &str, &[AnswerKey]) + Send + Sync>;

pub struct FakeHost {
    state: Mutex<State>,
    on_deliver: Mutex<Option<Hook>>,
    on_answer: Mutex<Option<AnswerHook>>,
}

impl std::fmt::Debug for FakeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakeHost")
    }
}

impl FakeHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { state: Mutex::new(State::default()), on_deliver: Mutex::new(None), on_answer: Mutex::new(None) })
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
            p.changed_at = std::time::Instant::now();
        }
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
        st.panes.insert(
            id.clone(),
            FakePane {
                screen: String::new(),
                watch: None,
                status: "idle".into(),
                alive: true,
                cols: spec.cols,
                rows: spec.rows,
                card: None,
                profile_id: spec.profile.id().to_string(),
                cwd: spec.cwd.clone(),
                forced_status: None,
                changed_at: std::time::Instant::now(),
            },
        );
        st.order.push(id.clone());
        Ok(id)
    }

    fn look(&self, pane: &str) -> Option<Look> {
        let st = self.state.lock();
        let p = st.panes.get(pane)?;
        Some(Look {
            screen: p.screen.clone(),
            watch: p.watch.clone().unwrap_or_else(|| p.screen.clone()),
            status: p
                .forced_status
                .clone()
                .unwrap_or_else(|| if p.alive && p.changed_at.elapsed() < Duration::from_millis(250) { "running".into() } else { p.status.clone() }),
            alive: p.alive,
            output_pos: Some(st.counter),
            hello_written: false,
            grid: Some((p.cols, p.rows)),
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

/// Short limits for tests that run the real threads.
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
        },
        test: TestLimits {
            start: Duration::from_millis(80),
            ready: Duration::from_secs(5),
            person: Duration::from_secs(5),
            step: Duration::from_secs(2),
            busy: Duration::from_secs(3),
            done: Duration::from_secs(5),
            permission: Duration::from_secs(3),
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
    pub const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";
    pub const TRUST: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n  1. Yes, trust this folder\n  2. No, exit\n";
    pub const PERMISSION: &str = "NIMBUS\n\nAllow nimbus to create hello.txt? [y/n]\n";

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
    let for_thread = host.clone();
    host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            h.set_screen_of(pane, screens::PERMISSION);
            h.set_card(Some(serde_json::json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &screens::working(&word, 0));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &screens::working(&word, 1));
            std::thread::sleep(Duration::from_millis(120));
            let said = if answer.is_empty() { word.clone() } else { answer.to_string() };
            host.set_screen_of(&pane, &screens::reply(&said));
        });
        Delivered::Submitted
    });
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
    wait_for(10, || (host.spawned().len() > before).then_some(())).expect("the test pane is started");
    host.set_screen(screens::IDLE);
    run.join().unwrap()
}
