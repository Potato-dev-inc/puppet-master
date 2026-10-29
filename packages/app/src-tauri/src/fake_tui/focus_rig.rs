//! Focus for the end-to-end tests (`tests/fake_tui_focus.rs`, SPEC n33 FT-12): the real engine (`attention::focus::live::Engine`)
//! over the real attention list and the real turn tracker, with its own state folder (the test's temp folder) and its own
//! flag, so the process-wide Focus flag is never touched.
//!
//! What is real: the clock, the attention list (`attention::snapshot_now`), the finished turns (`turn_summary::tracker`),
//! the Focus state file, the tiers (`policy::decide`), the summary and its counts.
//! What is replaced: the sinks. The phone push, the chat note, the desktop card and the audit log are recorded here
//! instead of sent, so a test can read what would have gone out.
//!
//! The rig can start, end and tick Focus, and ask the engine what a push would do. It cannot type into a pane, answer
//! a card or change a budget: it has no handle to any of them.

use crate::attention::focus::live::{Engine, Io, Note};
use crate::attention::focus::model::{By, State};
use crate::attention::focus::policy::Gate;
use crate::attention::focus::scan::{self, ItemFact};
use crate::attention::focus::store::Store;
use crate::attention::focus::summary::{FocusSummary, TurnFact};
use crate::attention::focus::view::View;
use crate::attention::focus::Flag;
use crate::attention::model::Level;
use crate::push::triggers::Kind as PushKind;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// What the sinks received, in order. Counts and summaries only, as the engine builds them.
#[derive(Default)]
struct Sinks {
    emits: u32,
    digests: Vec<FocusSummary>,
    notes: Vec<Note>,
    audits: Vec<String>,
}

/// The engine's `Io`: real clock, attention list, turn tracker and state file; recorded sinks.
struct RecIo {
    store: Store,
    sinks: Arc<Mutex<Sinks>>,
}

impl Io for RecIo {
    fn now_ms(&self) -> u64 {
        crate::attention::now_ms()
    }

    fn snapshot(&self) -> Vec<ItemFact> {
        scan::facts_from(&crate::attention::snapshot_now())
    }

    fn turns_since(&self, since_ms: u64) -> Vec<TurnFact> {
        crate::turn_summary::tracker().since(since_ms).iter().map(TurnFact::from).collect()
    }

    fn open_blocking(&self) -> u32 {
        crate::attention::counts().blocking
    }

    fn load(&self) -> State {
        self.store.load()
    }

    fn save(&self, state: &State) {
        self.store.save(state).expect("the Focus state file is written");
    }

    fn emit(&self, _view: &View) {
        self.sinks.lock().emits += 1;
    }

    fn offer_digest(&self, summary: &FocusSummary) {
        self.sinks.lock().digests.push(summary.clone());
    }

    fn dismiss_digest(&self) {}

    fn notes(&self, note: &Note) {
        self.sinks.lock().notes.push(note.clone());
    }

    fn audit(&self, _who: Option<By>, summary: &str, _detail: Value) {
        self.sinks.lock().audits.push(summary.to_string());
    }
}

/// One test's Focus: an engine over a folder of its own.
pub struct FocusRig {
    engine: Engine<RecIo>,
    sinks: Arc<Mutex<Sinks>>,
    /// The clock when the rig started Focus (0 before). Turns that ended before it are not this session's.
    started_ms: AtomicU64,
}

impl FocusRig {
    /// A rig whose state file lives in `root` (a temp folder of the test). Focus starts off.
    pub fn new(root: &Path) -> Self {
        let sinks = Arc::new(Mutex::new(Sinks::default()));
        let io = RecIo { store: Store::at(root.join("focus")), sinks: sinks.clone() };
        let flag: &'static Flag = Box::leak(Box::new(Flag::new()));
        let engine = Engine::new(io, flag);
        engine.load_and_resume();
        Self { engine, sinks, started_ms: AtomicU64::new(0) }
    }

    /// Start Focus for `minutes` as the desktop does. The reply is the view the desktop reads.
    pub fn start(&self, minutes: u32) -> Result<Value, String> {
        self.started_ms.store(crate::attention::now_ms(), Ordering::SeqCst);
        self.engine.start(minutes, By::Desktop).map(|view| to_json(view.desktop)).map_err(|e| e.message)
    }

    /// End Focus now, as the desktop does. Idempotent.
    pub fn end(&self) -> Value {
        to_json(self.engine.end(By::Desktop).desktop)
    }

    /// True while Focus is on (the end is in the future).
    pub fn is_on(&self) -> bool {
        self.engine.minutes_left().is_some()
    }

    /// One engine tick: what the `focus-tick` thread does every few seconds.
    pub fn tick(&self) {
        self.engine.tick();
    }

    /// What a push of `kind` (`waiting_input`, `budget`, `error`, ...) with a risk (`low`, `medium`, `high`) does right
    /// now: `send`, `quiet` or `hold`. The push hooks read this same decision.
    pub fn gate(&self, kind: &str, risk: Option<&str>) -> &'static str {
        let kind = match kind {
            "waiting_input" => PushKind::WaitingInput,
            "budget" => PushKind::Budget,
            "error" => PushKind::Error,
            other => panic!("the rig has no push kind `{other}`"),
        };
        let risk = risk.map(|r| match r {
            "low" => Level::Low,
            "medium" => Level::Medium,
            "high" => Level::High,
            other => panic!("the rig has no risk `{other}`"),
        });
        match self.engine.gate_push(kind, risk) {
            Gate::Send => "send",
            Gate::Quiet => "quiet",
            Gate::Hold => "hold",
        }
    }

    /// The open attention cards right now: `{ kind, risk }` (lower case). Nothing else of a card is read.
    pub fn open_cards(&self) -> Vec<Value> {
        crate::attention::snapshot_now()
            .items
            .iter()
            .filter(|item| item.state == crate::attention::model::State::Open)
            .map(|item| {
                let risk = item.risk.as_ref().map(|r| format!("{:?}", r.level).to_lowercase());
                json!({ "kind": format!("{:?}", item.kind).to_lowercase(), "risk": risk })
            })
            .collect()
    }

    /// The turns that ended since this rig started Focus, as `{ pane_id, turn, end }`. Counts and kinds only.
    pub fn finished_turns(&self) -> Vec<Value> {
        let since = self.started_ms.load(Ordering::SeqCst);
        crate::turn_summary::tracker()
            .since(since)
            .iter()
            .map(|t| json!({ "pane_id": t.pane_id, "turn": t.turn, "end": t.end.kind }))
            .collect()
    }

    /// How many times each sink was used: the desktop card and the digest, the notes (phone and chat), the audit lines.
    pub fn sink_counts(&self) -> Value {
        let sinks = self.sinks.lock();
        json!({
            "emits": sinks.emits,
            "digests": sinks.digests.len(),
            "notes": sinks.notes.len(),
            "audits": sinks.audits.len(),
        })
    }

    /// The end summaries the desktop card got, in order (`counts`, `line`, `minutes`, `ended_by`).
    pub fn digests(&self) -> Vec<Value> {
        self.sinks.lock().digests.iter().map(|s| serde_json::to_value(s).unwrap_or(Value::Null)).collect()
    }

    /// The notes the phone and the chat got, in order: `{ kind: "summary", line }` or `{ kind: "reminder", waiting }`.
    pub fn notes(&self) -> Vec<Value> {
        self.sinks
            .lock()
            .notes
            .iter()
            .map(|note| match note {
                Note::Summary { line, .. } => json!({ "kind": "summary", "line": line }),
                Note::Reminder { waiting } => json!({ "kind": "reminder", "waiting": waiting }),
            })
            .collect()
    }

    /// The audit lines Focus wrote (the sentence only).
    pub fn audits(&self) -> Vec<String> {
        self.sinks.lock().audits.clone()
    }
}

/// A view as the desktop reads it.
fn to_json(view: impl serde::Serialize) -> Value {
    serde_json::to_value(view).unwrap_or(Value::Null)
}
