//! The supervisor: the registry of live ACP sessions and the sweep that looks after them.
//!
//! One sweep (every few seconds, from [`Supervisor::start_thread`], or called directly with a chosen
//! `now` in tests) does three things:
//!
//! 1. **Turn clocks**: each session with a turn running is asked to check its inactivity and progress
//!    windows (`AcpSession::tick`): ask the agent to stop, then kill it if it does not.
//! 2. **Idle TTL**: a session that has been idle for its agent's TTL has its adapter stopped (`Slept`).
//!    The conversation stays and comes back, with `session/resume` or `session/load`, on the next prompt.
//! 3. **Cap**: more live adapters than `max_live` (each is a Node process plus the agent CLI, hundreds of
//!    MB) and the longest-idle ones are stopped, never one that is mid-turn or waiting for an answer.
//!
//! Respawn after a crash is lazy: the next prompt restarts the adapter and restores the session
//! (`AcpSession::ensure_running`), so a crashed idle adapter costs nothing until it is used again.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::session::{AcpSession, TickAction};

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Live adapters allowed at once.
    pub max_live: usize,
    pub sweep_interval: Duration,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self { max_live: 8, sweep_interval: Duration::from_secs(5) }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// What each session's own check did (sessions where nothing happened are left out).
    pub actions: Vec<(String, TickAction)>,
    /// Sessions stopped to get under `max_live`.
    pub evicted: Vec<String>,
}

pub struct Supervisor {
    cfg: SupervisorConfig,
    sessions: Mutex<BTreeMap<String, Arc<AcpSession>>>,
}

impl Supervisor {
    pub fn new(cfg: SupervisorConfig) -> Self {
        Self { cfg, sessions: Mutex::new(BTreeMap::new()) }
    }

    /// Registers a session under its key. A session already there is closed.
    pub fn insert(&self, session: Arc<AcpSession>) {
        let old = self.sessions.lock().insert(session.key().to_string(), session);
        if let Some(old) = old {
            old.close();
        }
    }

    pub fn get(&self, key: &str) -> Option<Arc<AcpSession>> {
        self.sessions.lock().get(key).cloned()
    }

    /// Removes and closes a session.
    pub fn remove(&self, key: &str) -> bool {
        let removed = self.sessions.lock().remove(key);
        match removed {
            Some(session) => {
                session.close();
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn live_count(&self) -> usize {
        self.sessions.lock().values().filter(|s| s.is_live()).count()
    }

    /// Closes everything (the app is exiting, or the Labs switch went off).
    pub fn close_all(&self) {
        let all: Vec<Arc<AcpSession>> = std::mem::take(&mut *self.sessions.lock()).into_values().collect();
        for session in all {
            session.close();
        }
    }

    pub fn sweep(&self, now: Instant) -> SweepReport {
        let sessions: Vec<(String, Arc<AcpSession>)> = self.sessions.lock().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut report = SweepReport::default();
        for (key, session) in &sessions {
            let action = session.tick(now);
            if action != TickAction::Nothing {
                report.actions.push((key.clone(), action));
            }
        }
        let mut live: Vec<(Instant, String, Arc<AcpSession>)> = sessions
            .iter()
            .filter(|(_, s)| s.is_live())
            .map(|(k, s)| (s.idle_since().unwrap_or(now), k.clone(), s.clone()))
            .collect();
        if live.len() > self.cfg.max_live {
            // Oldest idle first. A session in a turn has no idle_since (`now` here) and `sleep` refuses it anyway.
            live.sort_by_key(|(since, _, _)| *since);
            let mut excess = live.len() - self.cfg.max_live;
            for (_, key, session) in live {
                if excess == 0 {
                    break;
                }
                if matches!(session.sleep(), Ok(true)) {
                    report.evicted.push(key);
                    excess -= 1;
                }
            }
        }
        report
    }

    /// Runs `sweep` on a background thread until the handle is dropped.
    pub fn start_thread(self: &Arc<Self>) -> SweeperHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let me = self.clone();
        let flag = stop.clone();
        let interval = self.cfg.sweep_interval;
        let join = thread::Builder::new()
            .name("acp-supervisor".into())
            .spawn(move || {
                let step = Duration::from_millis(200);
                let mut waited = Duration::ZERO;
                while !flag.load(Ordering::SeqCst) {
                    thread::sleep(step);
                    waited += step;
                    if waited >= interval {
                        waited = Duration::ZERO;
                        me.sweep(Instant::now());
                    }
                }
            })
            .ok();
        SweeperHandle { stop, join }
    }
}

pub struct SweeperHandle {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for SweeperHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

static GLOBAL: OnceLock<Arc<Supervisor>> = OnceLock::new();

/// The app's one supervisor. Nothing starts it yet: the lane is behind a Labs switch and has no caller
/// until A2.
pub fn global() -> &'static Arc<Supervisor> {
    GLOBAL.get_or_init(|| Arc::new(Supervisor::new(SupervisorConfig::default())))
}
