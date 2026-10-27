//! The keeper: one pure state machine per companion (spec section 4). It never touches a pane or a
//! clock; the caller says what the pane looks like and what time it is, and the keeper says what to do
//! (`Action`). `service.rs` runs the actions against the real registry, the tests run them on nothing.
//!
//! - `Keeper::step`: keep one pane alive, restart a crashed one with a growing wait, give up after
//!   three restarts in 15 minutes, and turn a pane the person closed into "sleeping" (never restarted).
//! - `derive`: the state and the line the face shows (the table of section 4, first match wins).
//! - `nap_due`: the fresh start at night, only when the companion has been idle for a while.
//!
//! Nothing here names an agent.

use super::model::{CompanionState, HhMm};
use super::schedule;

/// Crashes older than this do not count.
pub const RESTART_WINDOW_MS: i64 = 15 * 60 * 1000;
/// The wait before restart 1, 2 and 3. A fourth crash inside the window stops the companion.
pub const BACKOFF_MS: [i64; 3] = [5_000, 30_000, 120_000];
/// A companion must have been idle this long before its nap.
pub const NAP_IDLE_MS: i64 = 30 * 60 * 1000;
/// A nap that could not run within this long after its time waits for the next day.
pub const NAP_WINDOW_MS: i64 = 60 * 60 * 1000;
/// A pane that never reads idle after this long counts as awake (some screens never settle).
pub const BOOT_LIMIT_MS: i64 = 90_000;

pub const SLEEPING_LINE: &str = "Sleeping. Wake me when you need me.";
pub const STARTING_LINE: &str = "Waking up…";
pub const IDLE_LINE: &str = "Here. Ask me anything.";
pub const IDLE_QUEUED_LINE: &str = "Will read your message next";
pub const WORKING_LINE: &str = "Working…";
pub const WAITING_LINE: &str = "Waiting on the computer screen";

/// The wait before restart number `crashes_so_far + 1`; `None` once the companion has crashed three
/// times already in the window (it stops and asks the person).
pub fn backoff_ms(crashes_so_far: usize) -> Option<i64> {
    BACKOFF_MS.get(crashes_so_far).copied()
}

/// What a live pane reports (`running | idle | waiting_input`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Idle,
    WaitingInput,
}

/// What the registry shows for the companion's pane id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    /// No pane with that id.
    Missing,
    Alive(Status),
    /// The pane exists but its process has ended (status `error`, or the agent exited).
    Dead,
}

impl Pane {
    /// From a pane's status word. An unknown word counts as running.
    pub fn from_status(status: &str) -> Pane {
        match status {
            "error" => Pane::Dead,
            "idle" => Pane::Alive(Status::Idle),
            "waiting_input" => Pane::Alive(Status::WaitingInput),
            _ => Pane::Alive(Status::Running),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing started yet, or switched off.
    Off,
    /// Spawned; the pane has not read idle yet.
    Starting,
    Running,
    /// A crash happened; the next spawn waits until `until_ms`.
    Waiting { until_ms: i64 },
    /// Three restarts did not help. Only a person's "try again" starts it.
    Stopped,
}

/// What the caller must do after a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the pane (write the brief first). The caller reports back with `spawned` or `spawn_failed`.
    /// `restart`: this follows a crash.
    Spawn { restart: bool },
    /// Close the pane. The keeper's own kill: the caller must not report it as "the person closed it".
    KillPane,
    /// The pane vanished without the keeper closing it: the person closed it. The keeper has already
    /// switched itself off; the caller saves `sleeping` and writes the feed line.
    PersonClosed,
    /// Raise the "stopped" card. The service clears it on wake and restart.
    RaiseStopped,
}

#[derive(Clone, Debug)]
pub struct Keeper {
    /// The companion should be running (not asleep, not archived).
    want: bool,
    phase: Phase,
    /// Crashes inside the window (epoch ms).
    restarts_ms: Vec<i64>,
    /// A pane the keeper started is believed to exist. Cleared by the keeper's own kills.
    own_pane: bool,
    started_ms: Option<i64>,
    booted: bool,
    idle_since_ms: Option<i64>,
}

impl Keeper {
    pub fn new(want: bool) -> Self {
        Self { want, phase: Phase::Off, restarts_ms: Vec::new(), own_pane: false, started_ms: None, booted: false, idle_since_ms: None }
    }

    pub fn want(&self) -> bool {
        self.want
    }

    #[cfg(test)]
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn is_stopped(&self) -> bool {
        self.phase == Phase::Stopped
    }

    pub fn booted(&self) -> bool {
        self.booted
    }

    pub fn idle_since_ms(&self) -> Option<i64> {
        self.idle_since_ms
    }

    /// Crashes in the last 15 minutes, as of the last step.
    pub fn restarts(&self) -> usize {
        self.restarts_ms.len()
    }

    /// The person (or a check-in) wakes it: it starts at the next step, with a clean crash record.
    pub fn wake(&mut self) {
        self.want = true;
        self.restarts_ms.clear();
        if matches!(self.phase, Phase::Stopped | Phase::Waiting { .. }) {
            self.phase = Phase::Off;
        }
    }

    /// Switch off (sleep or archive). The next step closes a pane that is still there.
    pub fn sleep(&mut self) {
        self.want = false;
    }

    /// The caller is about to close the pane itself (restart, nap): that is not "the person closed it".
    pub fn note_own_kill(&mut self) {
        self.own_pane = false;
        self.booted = false;
        self.started_ms = None;
        self.idle_since_ms = None;
        if self.phase != Phase::Stopped {
            self.phase = Phase::Off;
        }
    }

    /// A person's "try again" or "restart": forget the crashes and start at the next step.
    pub fn retry(&mut self) {
        self.want = true;
        self.restarts_ms.clear();
        self.note_own_kill();
        self.phase = Phase::Off;
    }

    /// The pane was started.
    pub fn spawned(&mut self, now_ms: i64) {
        self.phase = Phase::Starting;
        self.own_pane = true;
        self.booted = false;
        self.started_ms = Some(now_ms);
        self.idle_since_ms = None;
    }

    /// The pane could not be started. It counts as a crash.
    pub fn spawn_failed(&mut self, now_ms: i64) -> Vec<Action> {
        self.own_pane = false;
        self.booted = false;
        self.crash(now_ms)
    }

    /// One look at the pane. Returns what to do, in order.
    pub fn step(&mut self, now_ms: i64, pane: Pane) -> Vec<Action> {
        self.restarts_ms.retain(|at| now_ms - *at < RESTART_WINDOW_MS);
        let mut out = Vec::new();
        if !self.want {
            if pane != Pane::Missing {
                out.push(Action::KillPane);
            }
            self.park();
            return out;
        }
        match pane {
            Pane::Alive(status) => {
                self.phase = Phase::Running;
                self.own_pane = true;
                self.note_status(now_ms, status);
            }
            Pane::Dead => {
                out.push(Action::KillPane);
                self.own_pane = false;
                self.booted = false;
                self.idle_since_ms = None;
                out.extend(self.crash(now_ms));
            }
            Pane::Missing if self.own_pane => {
                // The pane was there and nobody here closed it: the person did. Never restart that.
                self.want = false;
                self.park();
                out.push(Action::PersonClosed);
            }
            Pane::Missing => match self.phase {
                Phase::Waiting { until_ms } if now_ms >= until_ms => out.push(Action::Spawn { restart: true }),
                Phase::Waiting { .. } | Phase::Stopped => {}
                Phase::Off | Phase::Starting | Phase::Running => out.push(Action::Spawn { restart: false }),
            },
        }
        out
    }

    fn park(&mut self) {
        self.phase = Phase::Off;
        self.own_pane = false;
        self.booted = false;
        self.started_ms = None;
        self.idle_since_ms = None;
    }

    /// Counts a crash and decides the wait before the next start, or gives up.
    fn crash(&mut self, now_ms: i64) -> Vec<Action> {
        let wait = backoff_ms(self.restarts_ms.len());
        self.restarts_ms.push(now_ms);
        match wait {
            Some(wait) => {
                self.phase = Phase::Waiting { until_ms: now_ms + wait };
                Vec::new()
            }
            None => {
                self.phase = Phase::Stopped;
                vec![Action::RaiseStopped]
            }
        }
    }

    fn note_status(&mut self, now_ms: i64, status: Status) {
        if status == Status::Idle {
            self.booted = true;
            self.idle_since_ms.get_or_insert(now_ms);
        } else {
            self.idle_since_ms = None;
        }
        if !self.booted && self.started_ms.is_some_and(|at| now_ms - at >= BOOT_LIMIT_MS) {
            self.booted = true;
        }
    }
}

/// Everything `derive` reads. The service fills it from the registry and the attention list.
#[derive(Clone, Debug)]
pub struct DeriveInput<'a> {
    pub name: &'a str,
    pub agent_label: &'a str,
    /// The companion is switched off (sleep, archive, or never woken).
    pub sleeping: bool,
    /// Three restarts did not help.
    pub stopped: bool,
    /// The live pane's status, or `None` when there is no pane (starting, or waiting to restart).
    pub pane: Option<Status>,
    /// The pane has read idle at least once since it started.
    pub booted: bool,
    /// Title of the attention item that blocks the pane.
    pub blocking: Option<&'a str>,
    /// A startup screen only the person may pass (sign-in), with no attention item of its own.
    pub sign_in: bool,
    /// Messages waiting in the pane's outbox.
    pub queued: usize,
    /// The pane's activity phrase.
    pub activity: Option<&'a str>,
}

/// The state and the line for the face (spec section 4). First match wins.
pub fn derive(i: &DeriveInput) -> (CompanionState, String) {
    if i.sleeping {
        return (CompanionState::Sleeping, SLEEPING_LINE.to_string());
    }
    if i.stopped {
        return (CompanionState::Stopped, format!("{} keeps stopping. Open the terminal to see why.", i.name));
    }
    let Some(status) = i.pane else {
        return (CompanionState::Starting, STARTING_LINE.to_string());
    };
    if let Some(title) = i.blocking {
        return (CompanionState::NeedsYou, format!("Needs you: {title}"));
    }
    if i.sign_in {
        return (CompanionState::NeedsYou, format!("Sign in to {} on the computer", i.agent_label));
    }
    match status {
        Status::WaitingInput => (CompanionState::NeedsYou, WAITING_LINE.to_string()),
        Status::Running if !i.booted => (CompanionState::Starting, STARTING_LINE.to_string()),
        Status::Running => {
            let line = i.activity.map(str::trim).filter(|a| !a.is_empty()).unwrap_or(WORKING_LINE);
            (CompanionState::Working, line.to_string())
        }
        Status::Idle if i.queued > 0 => (CompanionState::Idle, IDLE_QUEUED_LINE.to_string()),
        Status::Idle => (CompanionState::Idle, IDLE_LINE.to_string()),
    }
}

/// What the nap rule reads.
#[derive(Clone, Copy, Debug)]
pub struct NapInput {
    pub now_ms: i64,
    pub offset_secs: i32,
    pub nap_at: Option<HhMm>,
    pub last_nap_ms: Option<i64>,
    /// Since when the pane has read idle.
    pub idle_since_ms: Option<i64>,
    pub queued: usize,
}

/// The nap (fresh start with new notes) is due: its time came within the last hour, there has not been
/// one since, the pane has been idle for 30 minutes and its outbox is empty. Otherwise it is skipped
/// and tried again the next day.
pub fn nap_due(i: &NapInput) -> bool {
    let Some(at) = i.nap_at else { return false };
    let today = schedule::local_date(i.now_ms, i.offset_secs);
    let Some(slot) = schedule::instant_on(today, at, i.offset_secs) else { return false };
    if i.now_ms < slot || i.now_ms >= slot + NAP_WINDOW_MS {
        return false;
    }
    if i.last_nap_ms.is_some_and(|last| last >= slot) {
        return false;
    }
    i.queued == 0 && i.idle_since_ms.is_some_and(|since| i.now_ms - since >= NAP_IDLE_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = 1_000;
    const MIN: i64 = 60_000;

    fn input<'a>() -> DeriveInput<'a> {
        DeriveInput {
            name: "Mochi",
            agent_label: "Claude Code",
            sleeping: false,
            stopped: false,
            pane: Some(Status::Idle),
            booted: true,
            blocking: None,
            sign_in: false,
            queued: 0,
            activity: None,
        }
    }

    // ---- derive: one test per row of the table ----

    #[test]
    fn row_sleeping_wins_over_everything() {
        let i = DeriveInput { sleeping: true, stopped: true, blocking: Some("Allow?"), ..input() };
        assert_eq!(derive(&i), (CompanionState::Sleeping, "Sleeping. Wake me when you need me.".into()));
    }

    #[test]
    fn row_stopped_names_the_companion() {
        let i = DeriveInput { stopped: true, pane: None, ..input() };
        assert_eq!(derive(&i), (CompanionState::Stopped, "Mochi keeps stopping. Open the terminal to see why.".into()));
    }

    #[test]
    fn row_no_pane_is_starting() {
        let i = DeriveInput { pane: None, ..input() };
        assert_eq!(derive(&i), (CompanionState::Starting, "Waking up…".into()));
    }

    #[test]
    fn row_blocking_item_shows_its_title() {
        let i = DeriveInput { blocking: Some("Trust this folder?"), pane: Some(Status::WaitingInput), ..input() };
        assert_eq!(derive(&i), (CompanionState::NeedsYou, "Needs you: Trust this folder?".into()));
        // It wins over a running pane too (a limit card while the pane still draws).
        let running = DeriveInput { blocking: Some("Out of messages until 5 PM"), pane: Some(Status::Running), ..input() };
        assert_eq!(derive(&running).0, CompanionState::NeedsYou);
    }

    #[test]
    fn row_sign_in_names_the_agent() {
        let i = DeriveInput { sign_in: true, pane: Some(Status::Idle), ..input() };
        assert_eq!(derive(&i), (CompanionState::NeedsYou, "Sign in to Claude Code on the computer".into()));
    }

    #[test]
    fn row_waiting_input_without_an_item() {
        let i = DeriveInput { pane: Some(Status::WaitingInput), ..input() };
        assert_eq!(derive(&i), (CompanionState::NeedsYou, "Waiting on the computer screen".into()));
    }

    #[test]
    fn row_running_is_working_with_the_activity_phrase() {
        let plain = DeriveInput { pane: Some(Status::Running), ..input() };
        assert_eq!(derive(&plain), (CompanionState::Working, "Working…".into()));
        let phrase = DeriveInput { pane: Some(Status::Running), activity: Some("Reading notes.md"), ..input() };
        assert_eq!(derive(&phrase), (CompanionState::Working, "Reading notes.md".into()));
        let blank = DeriveInput { pane: Some(Status::Running), activity: Some("  "), ..input() };
        assert_eq!(derive(&blank).1, "Working…");
    }

    #[test]
    fn a_pane_that_never_read_idle_is_still_waking_up() {
        let i = DeriveInput { pane: Some(Status::Running), booted: false, ..input() };
        assert_eq!(derive(&i), (CompanionState::Starting, "Waking up…".into()));
    }

    #[test]
    fn row_idle_says_when_a_message_is_queued() {
        assert_eq!(derive(&input()), (CompanionState::Idle, "Here. Ask me anything.".into()));
        let queued = DeriveInput { queued: 2, ..input() };
        assert_eq!(derive(&queued), (CompanionState::Idle, "Will read your message next".into()));
    }

    // ---- the state machine ----

    /// A pane that reads `status` for the keeper at virtual time `now`.
    fn alive(keeper: &mut Keeper, now: i64, status: Status) -> Vec<Action> {
        keeper.step(now, Pane::Alive(status))
    }

    #[test]
    fn a_new_keeper_spawns_once_and_then_keeps_the_pane() {
        let mut k = Keeper::new(true);
        assert_eq!(k.step(0, Pane::Missing), vec![Action::Spawn { restart: false }]);
        k.spawned(0);
        assert_eq!(*k.phase(), Phase::Starting);
        assert!(alive(&mut k, S, Status::Running).is_empty());
        assert_eq!(*k.phase(), Phase::Running);
        assert!(!k.booted(), "running is not yet awake");
        assert!(alive(&mut k, 5 * S, Status::Idle).is_empty());
        assert!(k.booted());
        assert_eq!(k.idle_since_ms(), Some(5 * S));
        // The pane stays: no second spawn, and the idle clock keeps its start.
        assert!(alive(&mut k, 60 * S, Status::Idle).is_empty());
        assert_eq!(k.idle_since_ms(), Some(5 * S));
        alive(&mut k, 61 * S, Status::Running);
        assert_eq!(k.idle_since_ms(), None, "work resets the idle clock");
    }

    #[test]
    fn a_keeper_that_is_not_wanted_never_spawns() {
        let mut k = Keeper::new(false);
        assert!(k.step(0, Pane::Missing).is_empty());
        assert!(k.step(10 * S, Pane::Missing).is_empty());
        k.wake();
        assert_eq!(k.step(11 * S, Pane::Missing), vec![Action::Spawn { restart: false }]);
    }

    #[test]
    fn a_pane_that_never_reads_idle_counts_as_awake_after_the_boot_limit() {
        let mut k = Keeper::new(true);
        k.step(0, Pane::Missing);
        k.spawned(0);
        alive(&mut k, BOOT_LIMIT_MS - 1, Status::Running);
        assert!(!k.booted());
        alive(&mut k, BOOT_LIMIT_MS, Status::Running);
        assert!(k.booted());
    }

    #[test]
    fn backoff_is_5s_30s_2min_then_nothing() {
        assert_eq!(backoff_ms(0), Some(5 * S));
        assert_eq!(backoff_ms(1), Some(30 * S));
        assert_eq!(backoff_ms(2), Some(2 * MIN));
        assert_eq!(backoff_ms(3), None);
    }

    #[test]
    fn a_crash_kills_the_dead_pane_waits_and_restarts_with_growing_waits() {
        let mut k = Keeper::new(true);
        k.step(0, Pane::Missing);
        k.spawned(0);
        alive(&mut k, 10 * S, Status::Idle);

        // Crash 1 at 20 s: kill, wait 5 s.
        assert_eq!(k.step(20 * S, Pane::Dead), vec![Action::KillPane]);
        assert_eq!(*k.phase(), Phase::Waiting { until_ms: 25 * S });
        assert!(k.step(24 * S, Pane::Missing).is_empty(), "still waiting");
        assert_eq!(k.step(25 * S, Pane::Missing), vec![Action::Spawn { restart: true }]);
        k.spawned(25 * S);

        // Crash 2 at 40 s: wait 30 s.
        assert_eq!(k.step(40 * S, Pane::Dead), vec![Action::KillPane]);
        assert_eq!(*k.phase(), Phase::Waiting { until_ms: 70 * S });
        assert!(k.step(69 * S, Pane::Missing).is_empty());
        assert_eq!(k.step(70 * S, Pane::Missing), vec![Action::Spawn { restart: true }]);
        k.spawned(70 * S);

        // Crash 3 at 80 s: wait 2 min.
        assert_eq!(k.step(80 * S, Pane::Dead), vec![Action::KillPane]);
        assert_eq!(*k.phase(), Phase::Waiting { until_ms: 80 * S + 2 * MIN });
        assert_eq!(k.restarts(), 3);
        assert_eq!(k.step(80 * S + 2 * MIN, Pane::Missing), vec![Action::Spawn { restart: true }]);
        k.spawned(80 * S + 2 * MIN);

        // Crash 4 inside 15 minutes: stopped, and one card.
        let at = 200 * S;
        assert_eq!(k.step(at, Pane::Dead), vec![Action::KillPane, Action::RaiseStopped]);
        assert!(k.is_stopped());
        // Stopped means stopped: no spawn however long it waits.
        assert!(k.step(at + 60 * MIN, Pane::Missing).is_empty());
    }

    #[test]
    fn crashes_older_than_fifteen_minutes_do_not_count() {
        let mut k = Keeper::new(true);
        k.step(0, Pane::Missing);
        k.spawned(0);
        k.step(1 * S, Pane::Dead);
        k.step(6 * S, Pane::Missing);
        k.spawned(6 * S);
        k.step(7 * S, Pane::Dead);
        assert_eq!(k.restarts(), 2);
        // Twenty minutes later, a new crash starts over at the 5 s wait.
        let later = 20 * MIN;
        k.step(later, Pane::Missing); // the wait of crash 2 is long over: spawn
        k.spawned(later);
        assert_eq!(k.restarts(), 0, "the old crashes aged out");
        k.step(later + S, Pane::Dead);
        assert_eq!(*k.phase(), Phase::Waiting { until_ms: later + S + 5 * S });
    }

    #[test]
    fn a_spawn_that_fails_counts_as_a_crash_and_can_stop_the_companion() {
        let mut k = Keeper::new(true);
        assert_eq!(k.step(0, Pane::Missing), vec![Action::Spawn { restart: false }]);
        assert!(k.spawn_failed(0).is_empty());
        assert_eq!(*k.phase(), Phase::Waiting { until_ms: 5 * S });
        assert_eq!(k.step(5 * S, Pane::Missing), vec![Action::Spawn { restart: true }]);
        k.spawn_failed(5 * S);
        k.step(35 * S, Pane::Missing);
        k.spawn_failed(35 * S);
        k.step(35 * S + 2 * MIN, Pane::Missing);
        assert_eq!(k.spawn_failed(35 * S + 2 * MIN), vec![Action::RaiseStopped]);
        assert!(k.is_stopped());
    }

    #[test]
    fn a_pane_the_person_closed_means_sleeping_and_is_never_restarted() {
        let mut k = Keeper::new(true);
        k.step(0, Pane::Missing);
        k.spawned(0);
        alive(&mut k, 10 * S, Status::Idle);
        // The pane vanishes and nobody here closed it.
        assert_eq!(k.step(20 * S, Pane::Missing), vec![Action::PersonClosed]);
        assert!(!k.want());
        for t in [21 * S, 40 * S, 10 * MIN] {
            assert!(k.step(t, Pane::Missing).is_empty(), "no restart at {t}");
        }
    }

    #[test]
    fn the_keepers_own_kill_is_not_a_person_closing_it() {
        let mut k = Keeper::new(true);
        k.step(0, Pane::Missing);
        k.spawned(0);
        alive(&mut k, 10 * S, Status::Idle);
        k.note_own_kill(); // restart or nap: the caller closes the pane itself
        assert_eq!(k.step(11 * S, Pane::Missing), vec![Action::Spawn { restart: false }]);
    }

    #[test]
    fn sleep_closes_a_pane_that_is_still_there_and_wake_starts_again() {
        let mut k = Keeper::new(true);
        k.step(0, Pane::Missing);
        k.spawned(0);
        alive(&mut k, 10 * S, Status::Idle);
        k.sleep();
        assert_eq!(k.step(11 * S, Pane::Alive(Status::Idle)), vec![Action::KillPane]);
        assert!(k.step(12 * S, Pane::Missing).is_empty());
        k.wake();
        assert_eq!(k.step(13 * S, Pane::Missing), vec![Action::Spawn { restart: false }]);
    }

    #[test]
    fn retry_clears_a_stopped_keeper() {
        let mut k = Keeper::new(true);
        for round in 0..4 {
            let t = round * 3 * MIN;
            k.step(t, Pane::Missing);
            k.spawn_failed(t);
            k.step(t + 2 * MIN + S, Pane::Missing);
        }
        assert!(k.is_stopped());
        k.retry();
        assert!(!k.is_stopped());
        assert_eq!(k.restarts(), 0);
        assert_eq!(k.step(100 * MIN, Pane::Missing), vec![Action::Spawn { restart: false }]);
    }

    // ---- the nap rule ----

    const OFFSET: i32 = 8 * 3600;

    fn nap_input(now_h: u32, now_m: u32) -> NapInput {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        let now_ms = schedule::instant_on(date, HhMm { h: now_h as u8, m: now_m as u8 }, OFFSET).unwrap();
        NapInput {
            now_ms,
            offset_secs: OFFSET,
            nap_at: Some(HhMm { h: 4, m: 0 }),
            last_nap_ms: None,
            idle_since_ms: Some(now_ms - 31 * MIN),
            queued: 0,
        }
    }

    #[test]
    fn a_nap_needs_its_time_thirty_idle_minutes_and_an_empty_outbox() {
        assert!(nap_due(&nap_input(4, 0)));
        assert!(!nap_due(&nap_input(3, 59)), "before its time");

        let mut recent = nap_input(4, 10);
        recent.idle_since_ms = Some(recent.now_ms - 29 * MIN);
        assert!(!nap_due(&recent), "idle for only 29 minutes");
        recent.idle_since_ms = Some(recent.now_ms - 30 * MIN);
        assert!(nap_due(&recent), "30 minutes is enough");

        let mut busy = nap_input(4, 10);
        busy.queued = 1;
        assert!(!nap_due(&busy), "a message is waiting");

        let mut working = nap_input(4, 10);
        working.idle_since_ms = None;
        assert!(!nap_due(&working), "not idle");
    }

    #[test]
    fn a_nap_that_could_not_run_waits_for_the_next_day() {
        assert!(nap_due(&nap_input(4, 59)));
        assert!(!nap_due(&nap_input(5, 0)), "an hour after its time it is skipped");
        assert!(!nap_due(&nap_input(14, 0)));
    }

    #[test]
    fn one_nap_per_night_and_none_when_turned_off() {
        let mut done = nap_input(4, 30);
        let slot = nap_input(4, 0).now_ms;
        done.last_nap_ms = Some(slot + MIN);
        assert!(!nap_due(&done), "it already napped tonight");
        done.last_nap_ms = Some(slot - 24 * 60 * MIN);
        assert!(nap_due(&done), "yesterday's nap does not count");
        let mut off = nap_input(4, 30);
        off.nap_at = None;
        assert!(!nap_due(&off));
    }
}
