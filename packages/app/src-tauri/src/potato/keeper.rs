//! The keeper: one pure state machine per bot's brain process (spec section 1.5). It never starts a
//! process and never reads a clock; the caller says what happened and what time it is, and the keeper
//! says what to do (`Action`). `sidecar.rs` and `service.rs` run the actions, the tests run them on
//! nothing.
//!
//! - Lazy start: a message (or opening the thread) asks for the brain; it starts on the next step.
//! - Ready: the `hello` must arrive within 10 seconds of the start, or the process is killed (a crash).
//! - Idle: 30 minutes without a message or an event and the brain is stopped (not a crash). The next
//!   message starts it again.
//! - Crash: wait 1, 2, 4, 8 seconds (never more than 60) before starting again; the fifth crash in
//!   10 minutes stops it for good. Only the person's "Start again" starts a stopped brain.
//! - A stop the person asked for, or a switch-off, is never counted as a crash and never restarted.
//!
//! Nothing here names an agent or a chat app.

pub const HELLO_LIMIT_MS: i64 = 10_000;
pub const IDLE_LIMIT_MS: i64 = 30 * 60 * 1000;
pub const CRASH_WINDOW_MS: i64 = 10 * 60 * 1000;
pub const MAX_CRASHES: usize = 5;
pub const BACKOFF_FIRST_MS: i64 = 1_000;
pub const BACKOFF_MAX_MS: i64 = 60_000;

/// The wait before the next start after `crashes_so_far` crashes in the window (0 = the first crash).
pub fn backoff_ms(crashes_so_far: usize) -> i64 {
    let shift = crashes_so_far.min(16) as u32;
    (BACKOFF_FIRST_MS << shift).min(BACKOFF_MAX_MS)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// No process, and none wanted until a message asks.
    Off,
    /// Spawned; no `hello` yet.
    Starting { since_ms: i64 },
    Ready,
    /// A crash happened; the next start waits until `until_ms`.
    Waiting { until_ms: i64 },
    /// Five crashes did not help. Only a person's "Start again" starts it.
    Stopped,
}

/// What the caller must do after a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the process. The caller reports back with `spawned` or `spawn_failed`.
    Spawn,
    /// Ask the brain to leave (`shutdown`), wait 2 seconds, then kill its process tree. Not a crash.
    Shutdown,
    /// Kill the process tree at once (it never said hello). Counted as a crash by the keeper already.
    Kill,
    /// Raise the "stopped" card.
    RaiseStopped,
}

#[derive(Clone, Debug)]
pub struct Keeper {
    /// A start is wanted: set by a message, cleared by an idle stop, a person's stop and a switch-off.
    want: bool,
    phase: Phase,
    /// Crashes inside the window (epoch ms).
    crashes_ms: Vec<i64>,
    last_active_ms: i64,
    /// A process is believed to exist (spawned and not yet seen to leave).
    alive: bool,
}

impl Default for Keeper {
    fn default() -> Self {
        Self::new()
    }
}

impl Keeper {
    pub fn new() -> Self {
        Self { want: false, phase: Phase::Off, crashes_ms: Vec::new(), last_active_ms: 0, alive: false }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn is_stopped(&self) -> bool {
        self.phase == Phase::Stopped
    }

    #[cfg(test)]
    pub fn crashes(&self) -> usize {
        self.crashes_ms.len()
    }

    /// A message (or an opened thread) wants the brain. It starts at the next step, unless it is waiting
    /// out a crash or has been stopped for good.
    pub fn want_run(&mut self, now_ms: i64) {
        self.want = true;
        self.last_active_ms = now_ms;
    }

    /// Anything from the brain or to the brain counts as activity for the idle timer.
    pub fn touch(&mut self, now_ms: i64) {
        self.last_active_ms = now_ms;
    }

    /// The person's "Start again": forget the crashes and start at the next step.
    pub fn retry(&mut self, now_ms: i64) {
        self.crashes_ms.clear();
        self.want = true;
        self.last_active_ms = now_ms;
        if matches!(self.phase, Phase::Stopped | Phase::Waiting { .. }) {
            self.phase = Phase::Off;
        }
    }

    /// The person stopped this bot, or Potato Bot was switched off: nothing is wanted. The next step
    /// asks a live process to leave.
    pub fn person_stop(&mut self) {
        self.want = false;
    }

    /// The process was started.
    pub fn spawned(&mut self, now_ms: i64) {
        self.phase = Phase::Starting { since_ms: now_ms };
        self.alive = true;
        self.last_active_ms = now_ms;
    }

    /// The process could not be started. It counts as a crash.
    pub fn spawn_failed(&mut self, now_ms: i64) -> Vec<Action> {
        self.alive = false;
        self.crash(now_ms)
    }

    /// `hello` arrived.
    pub fn hello(&mut self, now_ms: i64) {
        if matches!(self.phase, Phase::Starting { .. }) {
            self.phase = Phase::Ready;
        }
        self.last_active_ms = now_ms;
    }

    /// The process is gone. `asked`: the keeper (or the person) asked it to leave, so this is no crash.
    pub fn exited(&mut self, now_ms: i64, asked: bool) -> Vec<Action> {
        self.alive = false;
        if asked || !self.want {
            self.phase = Phase::Off;
            return Vec::new();
        }
        self.crash(now_ms)
    }

    /// One look at the clock. Returns what to do, in order.
    pub fn step(&mut self, now_ms: i64) -> Vec<Action> {
        self.crashes_ms.retain(|at| now_ms - *at < CRASH_WINDOW_MS);
        let mut out = Vec::new();
        if !self.want {
            if self.alive {
                out.push(Action::Shutdown);
                // The caller reports `exited(.., true)` when the process is gone.
            } else if !matches!(self.phase, Phase::Stopped) {
                self.phase = Phase::Off;
            }
            return out;
        }
        match self.phase.clone() {
            Phase::Off if !self.alive => out.push(Action::Spawn),
            Phase::Off => {}
            Phase::Starting { since_ms } => {
                if now_ms - since_ms >= HELLO_LIMIT_MS {
                    out.push(Action::Kill);
                    self.alive = false;
                    out.extend(self.crash(now_ms));
                }
            }
            Phase::Ready => {
                if now_ms - self.last_active_ms >= IDLE_LIMIT_MS {
                    self.want = false;
                    out.push(Action::Shutdown);
                }
            }
            Phase::Waiting { until_ms } => {
                if now_ms >= until_ms {
                    self.phase = Phase::Off;
                    out.push(Action::Spawn);
                }
            }
            Phase::Stopped => {}
        }
        out
    }

    /// Counts a crash and decides the wait before the next start, or gives up.
    fn crash(&mut self, now_ms: i64) -> Vec<Action> {
        let so_far = self.crashes_ms.len();
        self.crashes_ms.push(now_ms);
        if self.crashes_ms.len() >= MAX_CRASHES {
            self.phase = Phase::Stopped;
            self.want = false;
            return vec![Action::RaiseStopped];
        }
        self.phase = Phase::Waiting { until_ms: now_ms + backoff_ms(so_far) };
        Vec::new()
    }
}
