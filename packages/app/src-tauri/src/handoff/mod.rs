//! Hand-off at the limit, step one: DETECT ONLY (`D:\pm-night\DESIGN-RELAY.md`, build steps 1-2).
//!
//! When an agent's plan runs out, Puppet Master notices from the screen and tells the person. It
//! does nothing else yet: no hand-off, no resume, no key pressed in the agent's pane.
//!
//! - `limits`: the `[limits]` table (rules as data per agent profile) and its validation.
//! - `detect`: the generic detector, one screen in and at most one [`LimitHit`] out, with the guards
//!   against false matches (an agent quoting the limit text is not a limit).
//! - `reset`: "resets 5pm" / "try again at 9:45 PM" read as a moment, in this machine's zone.
//! - `watch`: the per-pane state machine (a limit must hold still before it counts).
//! - `card`: the Inbox card ("Claude Code ran out of usage. It can continue at 14:05.") and its two
//!   answers, "Remind me at 14:05" and "Stop here".
//! - this file: the engine that ties them to live panes, runs and the questions store.
//!
//! What this step never does (each is tested):
//! - type into a pane, or press anything the agent itself would use to continue. Claude's own
//!   "continue automatically at usage limit" wait is left exactly as it is; the `/rate-limit-options`
//!   menu is never answered. Both choices on the card are bookkeeping inside Puppet Master.
//! - treat a transient rate limit as a limit: a retry is a status line, never a card.
//! - offer another login of the same vendor.
//!
//! Naming: this feature is "hand-off", never "relay" (`team/transport/relay.rs` is the team relay).

pub mod card;
pub mod detect;
pub mod limits;
pub mod reset;
pub mod watch;

#[cfg(test)]
mod tests;

pub use detect::LimitHit;
pub use limits::{validate_limits, CompiledLimits, LimitsTable};

use crate::operations::{OperationSnapshot, USAGE_LIMIT_KIND};
use crate::pty::status::PaneStatus;
use crate::questions::{Question, QuestionStore};
use crate::tui::{StartupGate, TuiProfile};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use reset::{Clock, SystemClock};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use watch::{Cleared, Event, Standing, Tracker};

/// The gate a usage limit puts on a pane: nothing is typed into it. No dismiss keys, ever: the
/// person (or the agent's own wait) moves past a limit, Puppet Master does not.
pub const USAGE_LIMIT_GATE: StartupGate = StartupGate {
    id: "usage_limit",
    summary: "The agent has stopped at its usage limit",
    dismiss: None,
};

/// Readiness input (`agent_runs::readiness::assess`): a gate for a screen that shows a usage limit,
/// an unavailable plan or the vendor's own limit menu. Whether the profile has a `[limits]` table
/// decides; no agent is named here.
pub fn limit_gate(profile: &dyn TuiProfile, screen: &str) -> Option<StartupGate> {
    let hit = profile.detect_limit(screen, &SystemClock)?;
    hit.kind.stops_typing().then_some(USAGE_LIMIT_GATE)
}

/// Where a pane is known and what its run is.
pub struct PaneCtx {
    pub agent_id: String,
    pub agent_label: String,
    pub project: String,
    pub status: String,
}

/// Everything the engine touches outside itself, so a test can run it without an app.
pub trait Env {
    fn pane(&self, pane_id: &str) -> Option<PaneCtx>;
    fn questions(&self) -> &QuestionStore;
    fn clock(&self) -> &dyn Clock;
    fn publish(&self, snapshot: &OperationSnapshot);
    /// A flight-recorder decision marker.
    fn mark(&self, pane_id: &str, text: &str);
}

struct CardRef {
    card_id: String,
    project: String,
    operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Reminder {
    pub pane_id: String,
    pub agent_label: String,
    pub window: Option<String>,
    pub due_ms: u64,
}

/// The detection engine: the tracker, the card each pane raised and the reminders people asked for.
#[derive(Default)]
pub struct Engine {
    tracker: Tracker,
    cards: HashMap<String, CardRef>,
    reminders: Vec<Reminder>,
    reminders_path: Option<PathBuf>,
}

impl Engine {
    pub fn with_reminders_file(path: PathBuf) -> Self {
        let reminders = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Reminder>>(&text).ok())
            .unwrap_or_default();
        Self { reminders, reminders_path: Some(path), ..Self::default() }
    }

    fn save_reminders(&self) {
        let Some(path) = &self.reminders_path else { return };
        if let Ok(text) = serde_json::to_string_pretty(&self.reminders) {
            let _ = std::fs::write(path, text);
        }
    }

    #[cfg(test)]
    pub fn reminders(&self) -> &[Reminder] {
        &self.reminders
    }

    /// One look at a pane whose screen changed.
    pub fn observe(&mut self, env: &dyn Env, pane_id: &str, profile: &dyn TuiProfile, screen: &str) -> Standing {
        if profile.limits().is_none() {
            return Standing::Clear;
        }
        let tail = crate::pane_activity::visible_tail(screen);
        let busy = profile.busy_marker(&tail);
        let hit = profile.detect_limit(screen, env.clock());
        let now = env.clock().now_ms().max(0) as u64;
        let (standing, events) = self.tracker.observe(now, pane_id, hit, busy);
        self.apply(env, events);
        standing
    }

    /// The turn ended (the pane is idle) and `screen` is its final, still screen: the same look as
    /// [`Engine::observe`], but a usage limit counts at once instead of after `stable_ms`. Without
    /// this a pane that goes idle on its limit lets the run settle as `completed` before the hold is
    /// over (the notice would be read as the reply).
    pub fn observe_final(&mut self, env: &dyn Env, pane_id: &str, profile: &dyn TuiProfile, screen: &str) -> Standing {
        if profile.limits().is_none() {
            return Standing::Clear;
        }
        let tail = crate::pane_activity::visible_tail(screen);
        let busy = profile.busy_marker(&tail);
        let hit = profile.detect_limit(screen, env.clock());
        let now = env.clock().now_ms().max(0) as u64;
        let (standing, events) = self.tracker.observe_final(now, pane_id, hit, busy);
        self.apply(env, events);
        standing
    }

    /// The once-a-second pass: promote limits that held still long enough, raise due reminders.
    /// Returns the panes whose limit state changed (their status line must be recomputed).
    pub fn tick(&mut self, env: &dyn Env, alive: &dyn Fn(&str) -> bool) -> Vec<String> {
        self.tracker.retain(alive);
        let now = env.clock().now_ms().max(0) as u64;
        let events = self.tracker.tick(now);
        let changed = events
            .iter()
            .map(|event| match event {
                Event::Fired { pane_id, .. } | Event::Cleared { pane_id, .. } => pane_id.clone(),
            })
            .collect();
        self.apply(env, events);
        self.remind_due(env, now);
        changed
    }

    fn apply(&mut self, env: &dyn Env, events: Vec<Event>) {
        for event in events {
            match event {
                Event::Fired { pane_id, hit } => self.fire(env, &pane_id, &hit),
                Event::Cleared { pane_id, hit, why } => self.clear(env, &pane_id, &hit, why),
            }
        }
    }

    fn fire(&mut self, env: &dyn Env, pane_id: &str, hit: &LimitHit) {
        let Some(ctx) = env.pane(pane_id) else { return };
        env.mark(
            pane_id,
            &format!("limit:detected {} {} reset={}", hit.rule_id, hit.window.as_deref().unwrap_or("-"), hit.reset_text.as_deref().unwrap_or("-")),
        );
        let run = crate::operations::active_operation_for_pane(&ctx.project, pane_id).ok().flatten();
        let mut operation_id = None;
        if let Some(run) = &run {
            let action = limit_action(&ctx, pane_id, hit, env.clock());
            if let Ok(parked) = crate::operations::mark_operation_usage_limit(&ctx.project, &run.operation_id, action) {
                env.publish(&parked);
                operation_id = Some(run.operation_id.clone());
            }
        }
        let now = env.clock().now_ms().max(0) as u64;
        let card = card::limit_card(hit, &ctx.agent_label, pane_id, operation_id.is_some(), env.clock());
        if let Some(created) = card::raise(env.questions(), card, now) {
            self.cards.insert(pane_id.to_string(), CardRef { card_id: created.id, project: ctx.project, operation_id });
        }
    }

    fn clear(&mut self, env: &dyn Env, pane_id: &str, hit: &LimitHit, why: Cleared) {
        env.mark(pane_id, &format!("limit:cleared {} {why:?}", hit.rule_id));
        let Some(card) = self.cards.remove(pane_id) else { return };
        let now = env.clock().now_ms().max(0) as u64;
        // The card goes away with the limit. (Dismissing an answered card is a no-op.)
        let _ = env.questions().dismiss(&card.card_id, now);
        if let Some(operation_id) = card.operation_id {
            self.unpark(env, pane_id, &card.project, &operation_id);
        }
    }

    /// The agent carried on: the run goes back to what its pane says. Only a run parked at a limit.
    fn unpark(&self, env: &dyn Env, pane_id: &str, project: &str, operation_id: &str) {
        let Ok(run) = crate::operations::get_operation(project, operation_id) else { return };
        let parked = run.status == crate::operations::OperationStatus::WaitingInput
            && run.required_action.as_ref().and_then(|a| a.get("kind")).and_then(Value::as_str) == Some(USAGE_LIMIT_KIND);
        if !parked {
            return;
        }
        let state = env.pane(pane_id).map(|p| p.status).unwrap_or_else(|| PaneStatus::Running.as_str().to_string());
        if let Ok(snapshot) =
            crate::operations::mark_operation_observation(project, operation_id, Some(state), None, crate::operations::StateSource::Inferred)
        {
            env.publish(&snapshot);
        }
    }

    /// A person answered a card.
    pub fn resolved(&mut self, env: &dyn Env, question: &Question) {
        let now = env.clock().now_ms().max(0) as u64;
        match card::decision(question, now) {
            card::Decision::Remind { pane_id, agent_label, window, due_ms } => {
                self.reminders.retain(|r| r.pane_id != pane_id);
                self.reminders.push(Reminder { pane_id: pane_id.clone(), agent_label, window, due_ms });
                self.save_reminders();
                env.mark(&pane_id, &format!("limit:remind due={due_ms}"));
            }
            card::Decision::StopRun { pane_id } => self.stop_run(env, &pane_id),
            card::Decision::Nothing => {}
        }
        if let Some(pane) = question.pane_id.as_deref() {
            if self.cards.get(pane).is_some_and(|c| c.card_id == question.id) {
                self.cards.remove(pane);
            }
        }
    }

    /// "Stop here": the run ends as cancelled with the limit on it. The pane is left alone.
    fn stop_run(&mut self, env: &dyn Env, pane_id: &str) {
        let Some(ctx) = env.pane(pane_id) else { return };
        let Ok(Some(run)) = crate::operations::active_operation_for_pane(&ctx.project, pane_id) else { return };
        let context = run
            .required_action
            .as_ref()
            .map(|a| json!({ "window": a.get("window"), "reset_at_ms": a.get("reset_at_ms"), "reset_text": a.get("reset_text") }))
            .unwrap_or(Value::Null);
        if let Ok(stopped) = crate::operations::mark_operation_stopped_at_limit(&ctx.project, &run.operation_id, context) {
            env.mark(pane_id, "limit:stopped-by-person");
            env.publish(&stopped);
        }
    }

    fn remind_due(&mut self, env: &dyn Env, now: u64) {
        let (due, later): (Vec<Reminder>, Vec<Reminder>) = std::mem::take(&mut self.reminders).into_iter().partition(|r| r.due_ms <= now);
        self.reminders = later;
        if due.is_empty() {
            return;
        }
        self.save_reminders();
        for reminder in due {
            // The pane is gone, or the agent already carried on by itself: nothing to remind about.
            let Some(ctx) = env.pane(&reminder.pane_id) else { continue };
            let carried_on = self.tracker.standing_of(&reminder.pane_id) == Standing::Clear && ctx.status == PaneStatus::Running.as_str();
            if carried_on {
                continue;
            }
            let card = card::reminder_card(&reminder.agent_label, &reminder.pane_id, reminder.window.clone());
            card::raise(env.questions(), card, now);
        }
    }

    /// The limit acted on for a pane, while it is still on screen.
    pub fn parked_action(&self, env: &dyn Env, pane_id: &str) -> Option<Value> {
        let hit = self.tracker.parked(pane_id)?;
        let ctx = env.pane(pane_id)?;
        Some(limit_action(&ctx, pane_id, hit, env.clock()))
    }

    /// The pane's one-line status while a limit is on screen: waiting at a usage limit (`Waiting`),
    /// or the vendor throttling and the CLI retrying by itself (`Working`, never a card).
    pub fn status_line(&self, env: &dyn Env, pane_id: &str) -> Option<(crate::tui::ActivityKind, String)> {
        use crate::tui::ActivityKind;
        if let Some(hit) = self.tracker.parked(pane_id) {
            let ctx = env.pane(pane_id)?;
            let at = hit.reset_at_ms.and_then(|ms| i64::try_from(ms).ok()).map(|ms| reset::when_text(ms, env.clock()));
            let text = match at {
                Some(at) => format!("{} hit its usage limit · resets {at}", ctx.agent_label),
                None => format!("{} hit its usage limit", ctx.agent_label),
            };
            return Some((ActivityKind::Waiting, text));
        }
        self.tracker.transient(pane_id)?;
        let ctx = env.pane(pane_id)?;
        Some((ActivityKind::Working, format!("{} is busy at its end · retrying", ctx.agent_label)))
    }
}

/// The answer routes (`answer_prompt`) call this first: a usage limit is not a prompt. It has no
/// choices, and the vendor's own limit menu offers paid ones, so nothing is ever typed for it,
/// whatever the caller sends.
pub fn refuse_answer(action: &Value) -> Option<crate::operations::OperationError> {
    if action.get("kind").and_then(Value::as_str) != Some(USAGE_LIMIT_KIND) {
        return None;
    }
    let mut error = crate::operations::OperationError::new(
        "USAGE_LIMIT_NOT_ANSWERABLE",
        "the agent stopped at its usage limit; there is nothing to answer, and Puppet Master never answers the agent's own limit menu. Nothing was sent",
        false,
    );
    error.context = json!({ "sent_input": false, "kind": USAGE_LIMIT_KIND });
    Some(error)
}

/// The run's `required_action` for a usage limit. Typed, with no options: nothing can answer it.
pub fn limit_action(ctx: &PaneCtx, pane_id: &str, hit: &LimitHit, clock: &dyn Clock) -> Value {
    let at = hit.reset_at_ms.and_then(|ms| i64::try_from(ms).ok()).map(|ms| reset::when_text(ms, clock));
    let summary = match &at {
        Some(at) => format!("{} ran out of usage. It can continue at {at}.", ctx.agent_label),
        None => format!("{} ran out of usage. It didn't say when it can continue.", ctx.agent_label),
    };
    json!({
        "kind": USAGE_LIMIT_KIND,
        "pane_id": pane_id,
        "agent": ctx.agent_id,
        "window": hit.window,
        "reset_at_ms": hit.reset_at_ms,
        "reset_text": hit.reset_text,
        "self_resume_armed": hit.self_resume_armed,
        "summary": summary,
        "options": [],
    })
}

// ---- the live app ------------------------------------------------------------------------------

struct LiveEnv;

impl Env for LiveEnv {
    fn pane(&self, pane_id: &str) -> Option<PaneCtx> {
        let registry = crate::bridge::live_registry()?;
        let info = registry.lock().list().into_iter().find(|p| p.id == pane_id)?;
        let project = crate::project_path::normalize_project_path(std::path::Path::new(&info.cwd)).ok()?.to_string_lossy().to_string();
        let agent_label = crate::tui::descriptors()
            .into_iter()
            .find(|d| d.id == info.agent_type)
            .map(|d| d.label)
            .unwrap_or_else(|| info.agent_type.clone());
        Some(PaneCtx { agent_id: info.agent_type, agent_label, project, status: info.status })
    }

    fn questions(&self) -> &QuestionStore {
        crate::questions::store()
    }

    fn clock(&self) -> &dyn Clock {
        &SystemClock
    }

    fn publish(&self, snapshot: &OperationSnapshot) {
        // Not in unit tests: reaching the app handle's window code links Tauri's window layer into
        // the test binary, which then fails to start on Windows (docs/REPO_MAP.md).
        #[cfg(not(test))]
        if let (Some(registry), Some(app)) = (crate::bridge::live_registry(), crate::bridge::live_app()) {
            let _ = crate::bridge::publish_operation(snapshot, &registry, &app);
        }
        #[cfg(test)]
        let _ = snapshot;
    }

    fn mark(&self, pane_id: &str, text: &str) {
        crate::flight::mark(pane_id, text);
    }
}

static ENGINE: Lazy<Mutex<Engine>> = Lazy::new(|| {
    // Unit tests never read or write the real app-data folder.
    #[cfg(test)]
    return Mutex::new(Engine::default());
    #[cfg(not(test))]
    Mutex::new(Engine::with_reminders_file(crate::app_paths::app_data_dir().join("handoff-reminders.json")))
});

/// `pane_activity` calls this for a pane whose screen changed (at most once a second). The result
/// tells a run supervisor whether the screen is a usage limit: such a screen is never a run's answer.
pub fn observe_pane(pane_id: &str, agent_type: &str, screen: &str) -> Standing {
    let profile = crate::tui::profile_for(agent_type);
    if profile.limits().is_none() {
        return Standing::Clear;
    }
    ENGINE.lock().observe(&LiveEnv, pane_id, profile.as_ref(), screen)
}

/// A run's turn ended on `screen`: like [`observe_pane`], but a usage limit on the final screen
/// counts without the `stable_ms` hold. `Parked` means the run was parked at its limit (or already
/// was) and must not be settled from this screen.
pub fn observe_pane_at_turn_end(pane_id: &str, agent_type: &str, screen: &str) -> Standing {
    let profile = crate::tui::profile_for(agent_type);
    if profile.limits().is_none() {
        return Standing::Clear;
    }
    ENGINE.lock().observe_final(&LiveEnv, pane_id, profile.as_ref(), screen)
}

/// Once a second, from the pane-activity ticker.
pub fn tick(alive: &dyn Fn(&str) -> bool) {
    let changed = ENGINE.lock().tick(&LiveEnv, alive);
    for pane_id in changed {
        crate::pane_activity::invalidate(&pane_id);
    }
}

/// A person answered a limit card (`questions::QuestionStore::answer`).
pub fn on_resolved(question: &Question) {
    if question.limit.is_some() {
        ENGINE.lock().resolved(&LiveEnv, question);
    }
}

/// The run's `required_action` while the pane's screen still shows the limit it was parked on. The
/// pane-status observer (`bridge::observe`) keeps the run parked with it instead of reading the pane
/// as idle and un-parking it.
pub fn active_limit_action(pane_id: &str) -> Option<Value> {
    ENGINE.lock().parked_action(&LiveEnv, pane_id)
}

/// The pane's status line while a limit is on screen (waiting at a usage limit, or retrying a
/// transient rate limit), for `pane_activity`.
pub fn status_line(pane_id: &str) -> Option<(crate::tui::ActivityKind, String)> {
    ENGINE.lock().status_line(&LiveEnv, pane_id)
}
