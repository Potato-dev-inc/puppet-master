//! One attention model: what needs a person, in what order, in which lane. Every surface (Inbox,
//! sidebar, tray, phone, push, away digest) renders a view of the same list, so they can never
//! disagree about how many things need you. See `docs/features/attention.md`.
//!
//! Inputs are small hooks (`on_pane_status`, `on_prompt`, `on_changed`) that only mark the store
//! dirty; one `attention-tick` thread rebuilds the snapshot (debounced) and broadcasts it as
//! `event: attention` on the SSE / WebSocket / poll fanout and as the Tauri event
//! `attention://changed`. Producers with their own state call `raise` / `clear`.

pub mod caps;
pub mod copy;
pub mod answer;
pub mod derive;
pub mod fingerprint;
pub mod layout;
pub mod model;
pub mod routes;
pub mod store;

use model::{Counts, Facts, NoticeFact, PaneFact, QuestionFact, Raised, Resolution, Snapshot, SNAPSHOT_DEBOUNCE_MS};
use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::time::Duration;
use store::{PromptRec, Store};

static STORE: Lazy<Mutex<Store>> = Lazy::new(|| Mutex::new(Store::default()));
static WAKE: Lazy<(Mutex<bool>, Condvar)> = Lazy::new(|| (Mutex::new(false), Condvar::new()));

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// --- reading ----------------------------------------------------------------------------------

/// The current snapshot (what `GET /attention` returns).
pub fn snapshot() -> Snapshot {
    STORE.lock().snapshot().clone()
}

/// The one number every surface shows: open blocking items.
pub fn counts() -> Counts {
    STORE.lock().counts()
}

// --- hooks ------------------------------------------------------------------------------------

/// Something changed that attention reads (a question, a loop-guard signal, an Ask-bar notice, a
/// pane list). Cheap: marks the store dirty and wakes the tick thread.
pub fn on_changed() {
    let (flag, cv) = &*WAKE;
    *flag.lock() = true;
    cv.notify_all();
}

/// A pane reported its status (`bridge::push_pane_status_sse`).
pub fn on_pane_status(pane_id: &str, status: &str) {
    STORE.lock().note_status(pane_id, status, now_ms());
    on_changed();
}

/// The prompt tracker (`pty://prompt`) emitted an event for a pane.
pub fn on_prompt(pane_id: &str, waiting: bool, prompt: Option<&crate::pane_prompt::PanePrompt>) {
    STORE.lock().on_prompt(pane_id, waiting, prompt.map(prompt_rec));
    on_changed();
}

fn prompt_rec(p: &crate::pane_prompt::PanePrompt) -> PromptRec {
    PromptRec {
        prompt_id: Some(p.prompt_id.clone()),
        kind: Some(p.kind.clone()),
        summary: p.summary.clone().or_else(|| (!p.text.is_empty()).then(|| p.text.clone())).map(|s| clean(&s)),
        risk_level: p.risk.as_ref().map(|r| model::Level::from(r.level).as_str().to_string()),
        headline: p.risk.as_ref().map(|r| clean(&r.headline)).filter(|h| !h.is_empty()),
        choices: p
            .choices
            .iter()
            .map(|c| model::ChoiceFact { id: c.id.clone(), label: clean(&c.label), broad: c.broad })
            .collect(),
        reasons: p.risk.as_ref().map(|r| r.reasons.iter().take(6).map(|reason| clean(reason)).collect()).unwrap_or_default(),
        occurrence: 0,
    }
}

/// The prompt as the store records it, for `attention::answer` to re-read the live screen the same
/// way a card was built.
pub(crate) fn live_prompt_rec(p: &crate::pane_prompt::PanePrompt) -> PromptRec {
    prompt_rec(p)
}

/// How many times this pane has asked, as the store counts it.
pub(crate) fn occurrence_of(pane_id: &str) -> u32 {
    STORE.lock().occurrence(pane_id)
}

/// Agent-originated text reaches clients only without bidi and control characters.
fn clean(text: &str) -> String {
    crate::push::triggers::sanitize(text)
}

// --- producers --------------------------------------------------------------------------------

/// A producer raises an item (budget, limit, consent, tamper, setup, review, done). Raising the
/// same key again updates it in place.
pub fn raise(raised: Raised) {
    STORE.lock().raise(raised, now_ms());
    on_changed();
}

/// The producer is done with the item; `resolution` says how it ended.
pub fn clear(key: &str, resolution: Option<Resolution>) {
    if STORE.lock().clear(key, resolution) {
        on_changed();
    }
}

/// Someone answered the item with this key; its tombstone will say who.
pub fn note_resolution(key: &str, resolution: Resolution) {
    STORE.lock().note_resolution(key, resolution);
    on_changed();
}

/// The person answered the prompt on `pane_id` at the desktop (the Inbox card): the item's tombstone
/// says so, and a phone that answers late is told "Already answered on your PC: Allowed once".
pub fn note_desktop_answer(pane_id: &str, choice_id: &str) {
    let key = format!("approval:{pane_id}");
    let label = {
        let store = STORE.lock();
        store
            .snapshot()
            .items
            .iter()
            .find(|i| i.key == key && i.state == model::State::Open)
            .and_then(|i| i.prompt.as_ref())
            .and_then(|p| p.choices.iter().find(|c| c.id == choice_id))
            .map(|c| c.label.clone())
    };
    let label = answer::answered_label(choice_id, label.as_deref().unwrap_or(choice_id));
    let resolution = Resolution { state: "answered".into(), by: Some("desktop".into()), choice_label: Some(label), at_ms: now_ms(), ..Resolution::default() };
    note_resolution(&key, resolution);
}

// --- collecting the live facts ----------------------------------------------------------------

/// The facts `derive` reads, from the live app: the pane list, the pending questions, the loop
/// guard's flags and the Ask-bar answers nobody saw.
pub fn collect_facts(now_ms: u64) -> Facts {
    let labels: HashMap<String, String> = crate::tui::descriptors().into_iter().map(|d| (d.id.to_string(), d.label)).collect();
    let label_of = |agent_type: &str| labels.get(agent_type).cloned().unwrap_or_else(|| agent_type.to_string());

    let infos = crate::bridge::live_registry().map(|registry| registry.lock().list()).unwrap_or_default();
    let names: Vec<copy::PaneName> = infos
        .iter()
        .map(|p| copy::PaneName { id: p.id.clone(), agent_type: p.agent_type.clone(), created_at: p.created_at, title: p.title.clone() })
        .collect();
    let headings = copy::headings(&names, &label_of);
    let panes = infos
        .iter()
        .map(|p| PaneFact {
            id: p.id.clone(),
            status: p.status.clone(),
            heading: headings.get(&p.id).map(|h| clean(h)),
            agent_label: Some(label_of(&p.agent_type)),
            project: project_name(&p.cwd),
        })
        .collect();

    let questions = crate::questions::store()
        .list_pending(now_ms)
        .into_iter()
        .map(|q| QuestionFact {
            id: q.id,
            pane_id: q.pane_id,
            created_at_ms: Some(q.created_at_ms),
            expires_at_ms: Some(q.expires_at_ms),
            text: Some(clean(&q.question)),
            agent: q.agent,
            consent: q.consent.and_then(|c| serde_json::to_value(c).ok()),
            kind: q.kind,
            limit: q.limit.and_then(|l| serde_json::to_value(l).ok()),
        })
        .collect();

    Facts {
        panes,
        prompts: HashMap::new(),
        questions,
        stuck_pane_ids: crate::loop_guard::all_active().into_iter().map(|s| s.pane_id).collect(),
        notices: crate::askbar::notices()
            .into_iter()
            .map(|n| NoticeFact { id: n.id, at_ms: Some(n.at_ms), text: Some(clean(&n.question)) })
            .collect(),
        status_since: HashMap::new(),
    }
}

fn project_name(cwd: &str) -> Option<String> {
    cwd.split(['\\', '/']).filter(|s| !s.is_empty()).last().map(str::to_string)
}

// --- the tick thread --------------------------------------------------------------------------

/// Rebuild the snapshot now. Returns the new snapshot when it changed.
pub fn refresh() -> Option<Snapshot> {
    let now = now_ms();
    let facts = collect_facts(now);
    let mut store = STORE.lock();
    store.recompute(&facts, now).then(|| store.snapshot().clone())
}

/// A snapshot that is up to date right now (the tick thread may be up to a debounce behind), for a
/// reader that answers a request, like the away digest or a held push. It is computed on a copy of the
/// store and changes nothing: the tick thread stays the only thing that commits and broadcasts, so
/// no client is left without the frame for a change.
pub fn snapshot_now() -> Snapshot {
    let now = now_ms();
    let facts = collect_facts(now);
    let mut copy = STORE.lock().clone();
    copy.recompute(&facts, now);
    copy.snapshot().clone()
}

/// Rebuild the snapshot and broadcast it now, on the caller's thread, instead of waiting for the tick
/// thread's debounce. For a producer that is about to tell someone "a card is waiting" (the 409 of an
/// exhausted budget): the Inbox and `GET /attention` must already show it when that reply lands.
/// Must not be called while holding the pane registry lock (the facts read the pane list).
pub fn settle() {
    if let Some(snapshot) = refresh() {
        publish(&snapshot);
    }
}

/// Start the `attention-tick` thread (once, from `setup`).
pub fn init() {
    let _ = std::thread::Builder::new().name("attention-tick".into()).spawn(run);
}

fn run() {
    loop {
        let woken_by_change = {
            let (flag, cv) = &*WAKE;
            let mut dirty = flag.lock();
            if !*dirty {
                let wait = match STORE.lock().next_deadline() {
                    Some(at) => Duration::from_millis(at.saturating_sub(now_ms()).clamp(50, 30_000)),
                    None => Duration::from_secs(30),
                };
                cv.wait_for(&mut dirty, wait);
            }
            std::mem::take(&mut *dirty)
        };
        if woken_by_change {
            // A burst of screen updates produces one snapshot.
            std::thread::sleep(Duration::from_millis(SNAPSHOT_DEBOUNCE_MS));
            *WAKE.0.lock() = false;
        }
        if let Some(snapshot) = refresh() {
            publish(&snapshot);
        }
    }
}

/// `event: attention` on the shared fanout (SSE, WebSocket, poll ring) and `attention://changed`
/// on the desktop. The frame is the full snapshot; there are no deltas to drift.
fn publish(snapshot: &Snapshot) {
    if let Ok(json) = serde_json::to_string(snapshot) {
        crate::bridge::push_sse(frame(&json));
    }
    // Not in unit tests: emitting through the app handle links the whole windowing runtime into the
    // lib test exe, which then needs a Common Controls v6 manifest it does not have (it would not
    // load). A test has no app handle anyway.
    #[cfg(not(test))]
    if let Some(app) = crate::bridge::live_app() {
        use tauri::Emitter;
        let _ = app.emit("attention://changed", snapshot);
    }
    crate::ambient::notify_changed();
    // The tray tooltip shows this count too (`tray::summary_line`). Not in unit tests, for the same
    // reason as the emit above: the tray's windowing code would load in the test exe.
    #[cfg(not(test))]
    {
        crate::tray::request_refresh();
    }
}

/// The SSE frame for a snapshot body.
pub fn frame(json: &str) -> String {
    format!("event: attention\ndata: {json}\n\n")
}

#[cfg(test)]
mod tests;
