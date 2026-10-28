//! The Focus engine: start, extend, end, tick, the gate for pushes, and the real clock and sinks (SPEC n33 3.4 A, F).
//!
//! `Engine<I>` holds the one `Mutex<State>` and every rule. It reaches the world only through `Io` (clock, attention
//! facts, finished turns, the state file, events, the desktop digest, the end notes, the audit log), so the engine
//! tests run on a fake `Io` and a virtual clock. `LiveIo` is the real one.
//!
//! Locking (SPEC S10): hooks take the lock only to bump a counter. The engine never calls a sink (`emit`,
//! `offer_digest`, `notes`, `audit`) while it holds the lock, and never calls a hook.
//!
//! Fails open: every hook asks the lock-free `Flag` first, and the flag compares its own deadline with the clock.
//! A dead tick thread cannot keep pings held past the end.

use super::model::{
    clamp_end, validate_minutes, By, Class, Held, Ledger, Session, Settings, State, FocusError, EXTEND_MINUTES,
    HELD_LIST_CAP, MAX_MINUTES, MINUTE_MS, SENTENCE_CAP, TICK_MS, WAITERS_CAP,
};
use super::policy::{class_of_push, decide, tier_of_push, Gate};
use super::scan::{self, ItemFact};
use super::summary::{self, EndedBy, FocusSummary, TurnFact};
use super::store::{new_id, Store};
use super::view::View;
use super::{nudge, Flag};
use crate::attention::model::Level;
use crate::push::triggers::Kind as PushKind;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the end of Focus (and its reminder) says to the phone and the chat. Counts only: no name, no pane, no text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// `body` is the counts part (`summary::body`), `line` the full sentence (`summary.line`).
    Summary { body: String, line: String },
    /// `waiting` quiet cards stayed open for 20 minutes.
    Reminder { waiting: u32 },
}

impl Note {
    /// The phone push for this note (`push::focus::Message`).
    pub fn push_message(&self) -> crate::push::focus::Message {
        match self {
            Note::Summary { body, line } => {
                crate::push::focus::Message::summary(if body.is_empty() { line } else { body })
            }
            Note::Reminder { waiting } => crate::push::focus::Message::reminder(*waiting),
        }
    }

    /// The Telegram note text (counts only; short mode swaps it for a fixed sentence in the notifier).
    pub fn chat_line(&self) -> String {
        match self {
            Note::Summary { line, .. } => line.clone(),
            Note::Reminder { waiting } => nudge::text(*waiting as usize),
        }
    }
}

/// Everything the engine needs from outside. Methods that can block (`notes`) must hand the work to a thread.
pub trait Io: Send + Sync + 'static {
    fn now_ms(&self) -> u64;
    /// The attention items right now, as `scan` reads them.
    fn snapshot(&self) -> Vec<ItemFact>;
    /// Finished turns that ended at or after `since_ms`.
    fn turns_since(&self, since_ms: u64) -> Vec<TurnFact>;
    /// How many blocking cards are open now.
    fn open_blocking(&self) -> u32;
    fn load(&self) -> State;
    fn save(&self, state: &State);
    /// The state changed: desktop event, phone SSE, tray.
    fn emit(&self, view: &View);
    /// Show the end summary on the desktop (the digest card).
    fn offer_digest(&self, summary: &FocusSummary);
    /// "Got it": drop the pending digest.
    fn dismiss_digest(&self);
    /// The end message or the reminder, to the phones and the chat.
    fn notes(&self, note: &Note);
    /// One audit line. `who` is `None` for the timer. Counts only.
    fn audit(&self, who: Option<By>, summary: &str, detail: Value);
}

/// The Focus engine over an `Io`.
pub struct Engine<I: Io> {
    io: I,
    flag: &'static Flag,
    state: Mutex<State>,
    /// The ledger changed through a hook; the next tick saves it and tells the views.
    dirty: AtomicBool,
}

impl<I: Io> Engine<I> {
    pub fn new(io: I, flag: &'static Flag) -> Self {
        Self { io, flag, state: Mutex::new(State::default()), dirty: AtomicBool::new(false) }
    }

    /// Read the saved state. A session still running is resumed; one that ended while the app was closed (or whose
    /// end is further away than the 240 minute cap, a clock set back) is ended now, once, and its summary waits as
    /// the pending digest.
    pub fn load_and_resume(&self) {
        let loaded = self.io.load();
        let now = self.io.now_ms();
        let session = loaded.session.clone();
        *self.state.lock() = loaded;
        match session {
            Some(s) if now >= s.ends_at_ms || s.ends_at_ms.saturating_sub(now) > u64::from(MAX_MINUTES) * MINUTE_MS => {
                self.finish(EndedBy::Timer, None);
            }
            Some(s) => self.flag.set(Some(s.ends_at_ms)),
            None => self.flag.set(None),
        }
    }

    // ---- reading ----

    /// The fast flag this engine sets.
    #[cfg(test)]
    pub(crate) fn flag(&self) -> &'static Flag {
        self.flag
    }

    /// Both views at the clock now.
    pub fn view(&self) -> View {
        let now = self.io.now_ms();
        View::from(&self.state.lock(), now)
    }

    /// What the phone reads (counts only).
    pub fn phone_view(&self) -> super::view::PhoneView {
        self.view().phone
    }

    /// The saved settings.
    pub fn settings(&self) -> Settings {
        self.state.lock().settings.clone()
    }

    /// Minutes left, rounded down, at least 1 while on. `None` when off.
    pub fn minutes_left(&self) -> Option<u32> {
        let now = self.io.now_ms();
        let st = self.state.lock();
        let session = st.session.as_ref().filter(|s| now < s.ends_at_ms)?;
        let left = (session.ends_at_ms - now) / MINUTE_MS;
        Some(u32::try_from(left).unwrap_or(u32::MAX).max(1))
    }

    // ---- the hooks (cheap) ----

    /// What a push hook does right now for this kind. One atomic read while Focus is off.
    pub fn gate_push(&self, kind: PushKind, risk: Option<Level>) -> Gate {
        if !self.flag.is_on_at(self.io.now_ms()) {
            return Gate::Send;
        }
        let settings = self.state.lock().settings.clone();
        decide(tier_of_push(kind, risk), &settings)
    }

    /// True when the push goes out, but without the buzz.
    pub fn quiet_for(&self, kind: PushKind, risk: Option<Level>) -> bool {
        self.gate_push(kind, risk) == Gate::Quiet
    }

    /// Count a held ping (and list it, up to the cap). `false` when Focus is not on: the caller then sends as today.
    /// `sentence` is cleaned here (redacted, paths cut, capped).
    pub fn hold(&self, class: Class, agent: &str, sentence: Option<&str>) -> bool {
        let now = self.io.now_ms();
        if !self.flag.is_on_at(now) {
            return false;
        }
        let mut st = self.state.lock();
        if !st.session.as_ref().is_some_and(|s| now < s.ends_at_ms) {
            return false;
        }
        st.ledger.counts.add(class);
        if st.ledger.held.len() < HELD_LIST_CAP {
            st.ledger.held.push(Held {
                at_ms: now,
                class,
                agent: clean_text(agent, 60),
                sentence: sentence.map(|s| clean_text(s, SENTENCE_CAP)).filter(|s| !s.is_empty()),
            });
        }
        self.dirty.store(true, Ordering::SeqCst);
        true
    }

    // ---- start, extend, end ----

    /// Start Focus, or change its length from now (the start time and the held list are kept). Only a person
    /// starts: `By::Local` is refused.
    pub fn start(&self, minutes: u32, by: By) -> Result<View, FocusError> {
        if by == By::Local {
            return Err(FocusError::person_only());
        }
        validate_minutes(minutes)?;
        self.close_if_expired();
        let now = self.io.now_ms();
        let (view, fresh) = {
            let mut st = self.state.lock();
            let fresh = st.session.is_none();
            let deadline = match st.session.as_mut() {
                Some(session) => {
                    session.minutes = minutes;
                    session.ends_at_ms = clamp_end(session.started_at_ms, now.saturating_add(u64::from(minutes) * MINUTE_MS));
                    session.ends_at_ms
                }
                None => {
                    let session = Session::new(now, minutes, by, new_id());
                    let deadline = session.ends_at_ms;
                    st.session = Some(session);
                    st.ledger = Ledger::default();
                    st.last_summary = None;
                    deadline
                }
            };
            self.flag.set(Some(deadline));
            self.io.save(&st);
            (View::from(&st, now), fresh)
        };
        self.io.emit(&view);
        let word = if fresh { "started" } else { "changed" };
        self.io.audit(Some(by), &format!("Focus {word}: {minutes} min"), json!({ "minutes": minutes, "by": by }));
        Ok(view)
    }

    /// `+15 min` on a Focus that is on (the end stays inside the 240 minute cap).
    pub fn extend(&self, by: By) -> Result<View, FocusError> {
        if by == By::Local {
            return Err(FocusError::person_only());
        }
        self.close_if_expired();
        let now = self.io.now_ms();
        let view = {
            let mut st = self.state.lock();
            let Some(session) = st.session.as_mut() else { return Err(FocusError::not_on()) };
            session.ends_at_ms = clamp_end(session.started_at_ms, session.ends_at_ms.saturating_add(u64::from(EXTEND_MINUTES) * MINUTE_MS));
            session.extended = session.extended.saturating_add(1);
            let deadline = session.ends_at_ms;
            self.flag.set(Some(deadline));
            self.io.save(&st);
            View::from(&st, now)
        };
        self.io.emit(&view);
        self.io.audit(Some(by), &format!("Focus extended: +{EXTEND_MINUTES} min"), json!({ "by": by }));
        Ok(view)
    }

    /// End Focus now. Idempotent: while off it changes nothing and returns the off view.
    pub fn end(&self, by: By) -> View {
        self.finish(EndedBy::Early, Some(by));
        self.view()
    }

    /// Save new settings. The default length must be 5 to 240.
    pub fn settings_set(&self, settings: Settings) -> Result<View, FocusError> {
        validate_minutes(settings.default_minutes)?;
        let now = self.io.now_ms();
        let view = {
            let mut st = self.state.lock();
            st.settings = settings.clone();
            self.io.save(&st);
            View::from(&st, now)
        };
        self.io.emit(&view);
        self.io.audit(
            Some(By::Desktop),
            "Focus settings changed",
            json!({ "default_minutes": settings.default_minutes, "quiet_blocking": settings.quiet_blocking }),
        );
        Ok(view)
    }

    /// "Got it": clear the last summary and the pending desktop digest.
    pub fn summary_dismiss(&self) {
        let now = self.io.now_ms();
        let view = {
            let mut st = self.state.lock();
            if st.last_summary.take().is_some() {
                self.io.save(&st);
            }
            View::from(&st, now)
        };
        self.io.dismiss_digest();
        self.io.emit(&view);
    }

    // ---- the tick ----

    /// Every `TICK_MS`: end a session that ran out, look for new waiting cards, send the one reminder, save what the
    /// hooks changed. Does nothing while Focus is off.
    pub fn tick(&self) {
        let now = self.io.now_ms();
        let Some(session) = self.state.lock().session.clone() else { return };
        if now >= session.ends_at_ms {
            self.finish(EndedBy::Timer, None);
            return;
        }
        let facts = self.io.snapshot();
        let (reminder, view) = {
            let mut st = self.state.lock();
            if st.session.as_ref().map(|s| s.id.as_str()) != Some(session.id.as_str()) {
                return;
            }
            let mut changed = self.dirty.swap(false, Ordering::SeqCst);
            let known = st.ledger.waiters.clone();
            let found = scan::scan(&facts, session.started_at_ms, &known, &st.settings);
            if !found.is_empty() {
                changed = true;
                st.ledger.waiters.extend(found);
                st.ledger.waiters.truncate(WAITERS_CAP);
            }
            let open: HashSet<String> = facts.iter().filter(|f| f.open).map(|f| f.key.clone()).collect();
            let due = nudge::due(&st.ledger.waiters, &open, now);
            if !due.is_empty() {
                changed = true;
                nudge::mark(&mut st.ledger.waiters, &due);
            }
            if changed {
                self.io.save(&st);
            }
            // With the switch off these cards went out at normal strength; a reminder would repeat them.
            let reminder = (st.settings.quiet_blocking && !due.is_empty()).then_some(due.len());
            (reminder, changed.then(|| View::from(&st, now)))
        };
        if let Some(view) = view {
            self.io.emit(&view);
        }
        if let Some(waiting) = reminder {
            let waiting = u32::try_from(waiting).unwrap_or(u32::MAX);
            self.io.notes(&Note::Reminder { waiting });
            self.io.audit(None, &format!("Focus reminder: {waiting} waiting"), json!({ "waiting": waiting }));
        }
    }

    // ---- inside ----

    /// A session whose end passed but that the tick has not ended yet: end it first, so its summary is not lost.
    fn close_if_expired(&self) {
        let now = self.io.now_ms();
        let expired = self.state.lock().session.as_ref().is_some_and(|s| now >= s.ends_at_ms);
        if expired {
            self.finish(EndedBy::Timer, None);
        }
    }

    /// End the session: build the one summary, clear the session and the ledger, then tell the views, the desktop
    /// card, the phones and the chat. Exactly once per session id; `false` when there was nothing to end.
    fn finish(&self, ended_by: EndedBy, who: Option<By>) -> bool {
        let now = self.io.now_ms();
        let Some(seen) = self.state.lock().session.clone() else { return false };
        let ended_at = match ended_by {
            EndedBy::Timer => seen.ends_at_ms.min(now),
            EndedBy::Early => now,
        };
        // Turns that ended after the end (a late restart) are not part of this session.
        let turns: Vec<TurnFact> =
            self.io.turns_since(seen.started_at_ms).into_iter().filter(|t| t.ended_at_ms <= ended_at).collect();
        let open_now = self.io.open_blocking();
        let (summary, view) = {
            let mut st = self.state.lock();
            if st.session.as_ref().map(|s| s.id.as_str()) != Some(seen.id.as_str()) {
                return false;
            }
            let Some(session) = st.session.take() else { return false };
            let ledger = std::mem::take(&mut st.ledger);
            let summary = summary::build(&session, &ledger, &turns, open_now, ended_at, ended_by);
            // An empty summary shows no card and sends nothing, so nothing is kept for "Got it" either.
            st.last_summary = (!summary::is_empty(&summary)).then(|| summary.clone());
            self.flag.set(None);
            self.dirty.store(false, Ordering::SeqCst);
            self.io.save(&st);
            (summary, View::from(&st, now))
        };
        self.io.emit(&view);
        if !summary::is_empty(&summary) {
            self.io.offer_digest(&summary);
            self.io.notes(&Note::Summary { body: summary::body(&summary.counts), line: summary.line.clone() });
        }
        let c = &summary.counts;
        let done = if c.turns_done == 1 { "1 turn done".to_string() } else { format!("{} turns done", c.turns_done) };
        let text = match (c.turns_done, c.failed) {
            (0, 0) => "Focus ended".to_string(),
            (_, 0) => format!("Focus ended: {done}"),
            (_, failed) => format!("Focus ended: {done}, {failed} failed"),
        };
        self.io.audit(
            who,
            &text,
            json!({
                "ended_by": ended_by,
                "turns_done": c.turns_done,
                "failed": c.failed,
                "asked": c.asked,
                "urgent": c.urgent,
                "held": c.held.total(),
            }),
        );
        true
    }
}

/// One line of held text for the list: redacted first (a secret cut in half would no longer match its rule), paths
/// cut to their last name, white space folded, then capped.
fn clean_text(raw: &str, cap: usize) -> String {
    let ctx = crate::redact::known::context();
    let redacted = crate::redact::text_with(raw, &ctx);
    let one_line = redacted.as_str().split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = crate::session_chat::export_text::strip_paths(&one_line, ctx.home.as_deref());
    cut.chars().take(cap).collect()
}

// ---- the real thing ----

/// The real clock, attention list, turn tracker, state file, events, digest and sinks.
pub struct LiveIo {
    store: Store,
}

impl LiveIo {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

impl Io for LiveIo {
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
        if let Err(err) = self.store.save(state) {
            tracing::warn!(%err, "could not save the Focus state");
        }
    }

    fn emit(&self, view: &View) {
        use tauri::Emitter;
        if let Some(app) = crate::bridge::live_app() {
            let _ = app.emit("focus://changed", &view.desktop);
        }
        crate::bridge::push_focus_sse(&view.phone);
        crate::tray::request_refresh();
    }

    fn offer_digest(&self, summary: &FocusSummary) {
        crate::turn_summary::offer_focus_digest(summary.clone());
    }

    fn dismiss_digest(&self) {
        crate::turn_summary::dismiss_digest();
    }

    fn notes(&self, note: &Note) {
        let note = note.clone();
        // The push service call blocks; the chat note queues. Neither may hold up the tick or a click.
        let spawned = std::thread::Builder::new().name("focus-notes".into()).spawn(move || {
            crate::push::notify_focus(&note.push_message());
            crate::channels::focus_note(&note.chat_line());
        });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not start the Focus note thread");
        }
    }

    fn audit(&self, who: Option<By>, summary: &str, detail: Value) {
        use crate::audit::Who;
        let who = match who {
            Some(By::Phone) => Who::Device(None),
            Some(By::Local) => Who::Local,
            Some(By::Desktop | By::Tray) | None => Who::Desktop,
        };
        crate::audit::record(crate::audit::Kind::Focus, who, summary, detail);
    }
}

#[cfg(not(test))]
fn live_store() -> Store {
    Store::live()
}

/// Unit tests that reach the global engine never touch the real app data folder.
#[cfg(test)]
fn live_store() -> Store {
    Store::at(std::env::temp_dir().join(format!("pm-focus-live-test-{}", std::process::id())))
}

static ENGINE: once_cell::sync::Lazy<Engine<LiveIo>> =
    once_cell::sync::Lazy::new(|| Engine::new(LiveIo::new(live_store()), &super::GLOBAL));

/// The app's engine. Built on first use; `start` loads the saved state into it.
pub fn engine() -> &'static Engine<LiveIo> {
    &ENGINE
}

/// Load the saved state, resume or end the session, tell the desktop, and start the `focus-tick` thread.
/// Call once from `setup`.
pub fn start(app: &tauri::AppHandle) {
    use tauri::Emitter;
    engine().load_and_resume();
    let _ = app.emit("focus://changed", &engine().view().desktop);
    let spawned = std::thread::Builder::new().name("focus-tick".into()).spawn(|| {
        let mut ticks: u64 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
            engine().tick();
            ticks = ticks.wrapping_add(1);
            // The tray shows "31 min left": redraw it about once a minute while Focus is on.
            if ticks % (60_000 / TICK_MS) == 0 && super::is_on() {
                crate::tray::request_refresh();
            }
        }
    });
    if let Err(err) = spawned {
        tracing::warn!(%err, "Focus tick thread not started");
    }
}

// ---- the free hooks the rest of the app calls ----

/// What a push hook does right now. Outside Focus this is one atomic read and `Send`.
pub fn gate_push(kind: PushKind, risk: Option<Level>) -> Gate {
    #[cfg(test)]
    if let Some(settings) = testing::forced() {
        return decide(tier_of_push(kind, risk), &settings);
    }
    if !super::is_on() {
        return Gate::Send;
    }
    engine().gate_push(kind, risk)
}

/// True when the push goes out without the buzz (phone `quiet: true`, Telegram `disable_notification`).
pub fn quiet_for(kind: PushKind, risk: Option<Level>) -> bool {
    gate_push(kind, risk) == Gate::Quiet
}

/// Hold a ping: count it and list it. `false` when Focus is off (send as today).
pub fn hold(class: Class, agent: &str, sentence: Option<&str>) -> bool {
    #[cfg(test)]
    if testing::forced().is_some() {
        testing::note_hold(class, agent);
        return true;
    }
    if !super::is_on() {
        return false;
    }
    engine().hold(class, agent, sentence)
}

/// The hold of a push candidate: `true` when its kind is held right now and was counted. The one place the push
/// hooks, the chat and the tests share.
pub fn hold_push(kind: PushKind, risk: Option<Level>, agent: &str) -> bool {
    match gate_push(kind, risk) {
        Gate::Hold => class_of_push(kind).is_some_and(|class| hold(class, agent, None)),
        Gate::Send | Gate::Quiet => false,
    }
}

/// Minutes left for the tray, or `None` while off.
pub fn minutes_left() -> Option<u32> {
    if !super::is_on() {
        return None;
    }
    engine().minutes_left()
}

/// Test hooks: a test turns Focus on for its own thread only (the real tiers, these settings), so parallel tests
/// never see each other's Focus and the global flag stays off.
#[cfg(test)]
pub(crate) mod testing {
    use super::{Class, Settings};
    use std::cell::RefCell;

    thread_local! {
        static FORCED: RefCell<Option<Settings>> = const { RefCell::new(None) };
        static HELD: RefCell<Vec<(Class, String)>> = const { RefCell::new(Vec::new()) };
    }

    /// While this value lives, the free hooks on this thread act as if Focus were on with `settings`; `hold` only
    /// records.
    pub struct Forced;

    pub fn force(settings: Settings) -> Forced {
        FORCED.with(|f| *f.borrow_mut() = Some(settings));
        HELD.with(|h| h.borrow_mut().clear());
        Forced
    }

    /// Focus on with the default settings (quiet cards, 45 minutes).
    pub fn force_default() -> Forced {
        force(Settings::default())
    }

    impl Drop for Forced {
        fn drop(&mut self) {
            FORCED.with(|f| *f.borrow_mut() = None);
        }
    }

    pub fn forced() -> Option<Settings> {
        FORCED.with(|f| f.borrow().clone())
    }

    pub fn note_hold(class: Class, agent: &str) {
        HELD.with(|h| h.borrow_mut().push((class, agent.to_string())));
    }

    /// What `hold` recorded on this thread since `force`.
    pub fn held() -> Vec<(Class, String)> {
        HELD.with(|h| h.borrow().clone())
    }
}
