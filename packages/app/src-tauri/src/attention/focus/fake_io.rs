//! A fake `Io` and a rig for tests: a virtual clock, an editable attention list and turn list, and a record of
//! every sink call. Each rig has its own `Flag`, so parallel tests never share the global one.

use super::live::{Engine, Io, Note};
use super::model::{By, State};
use super::scan::ItemFact;
use super::summary::{FocusSummary, TurnFact};
use super::view::View;
use super::Flag;
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// 2026-10-29 09:20 UTC, as a fixed starting time.
pub const T0: u64 = 1_793_265_600_000;
pub const MIN: u64 = 60_000;

#[derive(Default)]
pub struct FakeIo {
    pub now: AtomicU64,
    pub items: Mutex<Vec<ItemFact>>,
    pub turns: Mutex<Vec<TurnFact>>,
    pub open: AtomicU32,
    /// What `load` returns and `save` replaces: the "disk".
    pub disk: Mutex<State>,
    pub saves: AtomicU32,
    pub emits: Mutex<Vec<View>>,
    pub digests: Mutex<Vec<FocusSummary>>,
    pub dismissed: AtomicU32,
    pub notes: Mutex<Vec<Note>>,
    pub audits: Mutex<Vec<(Option<By>, String, Value)>>,
}

impl FakeIo {
    pub fn at(ms: u64) -> Arc<Self> {
        let io = Self::default();
        io.now.store(ms, Ordering::SeqCst);
        Arc::new(io)
    }

    pub fn set_now(&self, ms: u64) {
        self.now.store(ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

impl Io for Arc<FakeIo> {
    fn now_ms(&self) -> u64 {
        self.now()
    }
    fn snapshot(&self) -> Vec<ItemFact> {
        self.items.lock().clone()
    }
    fn turns_since(&self, since_ms: u64) -> Vec<TurnFact> {
        self.turns.lock().iter().filter(|t| t.ended_at_ms >= since_ms).cloned().collect()
    }
    fn open_blocking(&self) -> u32 {
        self.open.load(Ordering::SeqCst)
    }
    fn load(&self) -> State {
        self.disk.lock().clone()
    }
    fn save(&self, state: &State) {
        *self.disk.lock() = state.clone();
        self.saves.fetch_add(1, Ordering::SeqCst);
    }
    fn emit(&self, view: &View) {
        self.emits.lock().push(view.clone());
    }
    fn offer_digest(&self, summary: &FocusSummary) {
        self.digests.lock().push(summary.clone());
    }
    fn dismiss_digest(&self) {
        self.dismissed.fetch_add(1, Ordering::SeqCst);
    }
    fn notes(&self, note: &Note) {
        self.notes.lock().push(note.clone());
    }
    fn audit(&self, who: Option<By>, summary: &str, detail: Value) {
        self.audits.lock().push((who, summary.to_string(), detail));
    }
}

/// An engine over a fresh fake at `T0`, with the fake.
pub fn rig() -> (Engine<Arc<FakeIo>>, Arc<FakeIo>) {
    rig_at(T0)
}

pub fn rig_at(ms: u64) -> (Engine<Arc<FakeIo>>, Arc<FakeIo>) {
    let io = FakeIo::at(ms);
    (engine_over(&io), io)
}

/// A second engine over the same fake (a restart: same "disk", same clock).
pub fn engine_over(io: &Arc<FakeIo>) -> Engine<Arc<FakeIo>> {
    let flag: &'static Flag = Box::leak(Box::new(Flag::new()));
    Engine::new(io.clone(), flag)
}

/// The flag of a rig engine, for the fail-open checks.
pub fn flag_of(engine: &Engine<Arc<FakeIo>>) -> &'static Flag {
    engine.flag()
}
