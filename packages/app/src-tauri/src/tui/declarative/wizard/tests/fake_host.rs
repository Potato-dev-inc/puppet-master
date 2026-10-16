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
}

type Hook = Box<dyn Fn(&FakeHost, &str, &str) -> Delivered + Send + Sync>;

pub struct FakeHost {
    state: Mutex<State>,
    on_deliver: Mutex<Option<Hook>>,
}

impl std::fmt::Debug for FakeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakeHost")
    }
}

impl FakeHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { state: Mutex::new(State::default()), on_deliver: Mutex::new(None) })
    }

    /// The next delivery calls `hook(host, pane_id, text)`.
    pub fn on_deliver(&self, hook: impl Fn(&FakeHost, &str, &str) -> Delivered + Send + Sync + 'static) {
        *self.on_deliver.lock() = Some(Box::new(hook));
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
        }
    }

    pub fn set_status(&self, status: &str) {
        let pane = self.last();
        if let Some(p) = self.state.lock().panes.get_mut(&pane) {
            p.status = status.to_string();
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
            status: p.status.clone(),
            alive: p.alive,
            output_pos: Some(st.counter),
            hello_written: false,
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
        },
        test: TestLimits {
            start: Duration::from_millis(80),
            ready: Duration::from_secs(5),
            person: Duration::from_secs(5),
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
