//! Hand-off at the limit: detect it, tell the person, and keep going on another agent when they tap.
//!
//! When an agent's plan runs out, Puppet Master notices from the screen and raises a card. The card
//! can offer `Continue on <agent>`: a person's tap moves the task to another agent in the same
//! folder (`keep_going`, which never runs under this engine's lock). Nothing here moves a task by
//! itself, resumes an agent, or presses a key in an agent's pane.
//!
//! - `limits`: the `[limits]` table (rules as data per agent profile) and its validation.
//! - `detect`: the generic detector, one screen in and at most one [`LimitHit`] out, with the guards
//!   against false matches (an agent quoting the limit text is not a limit).
//! - `reset`: "resets 5pm" / "try again at 9:45 PM" read as a moment, in this machine's zone.
//! - `watch`: the per-pane state machine (a limit must hold still before it counts).
//! - `card`: the Inbox cards ("Claude Code ran out of usage. It can continue at 14:05.") and what
//!   answering them asks for: "Continue on ...", "Remind me at 14:05", "Stop here".
//! - `keep_going`: the hand-over to another agent, the come-back check and the records behind them.
//! - this file: the engine that ties them to live panes, runs and the questions store.
//!
//! What this never does (each is tested):
//! - type into a pane, or press anything the agent itself would use to continue. Claude's own
//!   "continue automatically at usage limit" wait is left exactly as it is; the `/rate-limit-options`
//!   menu is never answered. "Remind me" and "Stop here" are bookkeeping inside Puppet Master; a
//!   hand-over ends the old agent's process by its pid and starts a new run.
//! - treat a transient rate limit as a limit: a retry is a status line, never a card.
//! - offer another login of the same vendor.
//!
//! Naming: this feature is "hand-off", never "relay" (`team/transport/relay.rs` is the team relay).

pub mod card;
pub mod detect;
pub mod keep_going;
pub mod limits;
pub mod reset;
pub mod watch;

#[cfg(test)]
mod card_tests;
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
    /// The account (id) the pane runs under; `None` = the agent's own login.
    pub account: Option<String>,
    pub project: String,
    pub status: String,
}

impl PaneCtx {
    /// The folder whose `.puppet-master/operations` holds this pane's runs. A pane that works in a
    /// separate copy (`<project>/.puppet-master/worktrees/<id>`) keeps its runs in the project the copy
    /// came from, so that is where they are looked up (`worktrees::origin_of_cwd`, as keep going does).
    fn runs_folder(&self) -> String {
        crate::worktrees::origin_of_cwd(&self.project).unwrap_or_else(|| self.project.clone())
    }
}

/// Everything the engine touches outside itself, so a test can run it without an app.
pub trait Env {
    fn pane(&self, pane_id: &str) -> Option<PaneCtx>;
    fn questions(&self) -> &QuestionStore;
    fn clock(&self) -> &dyn Clock;
    fn publish(&self, snapshot: &OperationSnapshot);
    /// A flight-recorder decision marker.
    fn mark(&self, pane_id: &str, text: &str);
    /// A line for the usage-limit log behind "Your plans" (`crate::plans_report`). Default: none.
    fn log_limit(&self, _entry: crate::plans_report::limit_log::Entry) {}
    /// What the limit card offers for `pane_id` (Keep going). `limited` looks up the engine's own limit
    /// notes: this runs inside the engine's lock, so an implementation must use it and not call the
    /// engine again. Default: nothing is offered.
    fn keep_going_offer(
        &self,
        _pane_id: &str,
        _ctx: &PaneCtx,
        _hit: &LimitHit,
        _limited: &dyn Fn(&str, Option<&str>) -> Option<Option<u64>>,
    ) -> keep_going::model::KeepGoingOffer {
        keep_going::model::KeepGoingOffer::NotHere
    }
    /// A person's answer asked for a hand-over. Must return at once and do the work elsewhere: it is
    /// called inside the engine's lock. Default: nothing happens.
    fn keep_going(&self, _request: keep_going::live::Request) {}
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

/// An agent (and the account it ran under) that hit its usage limit, remembered by the pane that hit it.
/// It is still known after that pane is closed, so a companion that moved away from it asks before it
/// moves back. The account matters: a limit on one plan does not block the same agent on another plan.
/// Each pane keeps its own note: a pane that carries on or closes forgets only its own note.
struct LimitNote {
    pane_id: String,
    agent_id: String,
    account: Option<String>,
    reset_at_ms: Option<u64>,
    since_ms: u64,
}

impl LimitNote {
    /// Whether this limit still holds at `now_ms`: until its reset, or an hour after a limit with no reset.
    fn live_at(&self, now_ms: u64) -> bool {
        match self.reset_at_ms {
            Some(reset) => reset > now_ms,
            None => self.since_ms.saturating_add(card::REMIND_FALLBACK_MS) > now_ms,
        }
    }
}

/// The detection engine: the tracker, the card each pane raised and the reminders people asked for.
#[derive(Default)]
pub struct Engine {
    tracker: Tracker,
    cards: HashMap<String, CardRef>,
    reminders: Vec<Reminder>,
    reminders_path: Option<PathBuf>,
    /// One note per pane that is at a limit (see `LimitNote`).
    limited_agents: Vec<LimitNote>,
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

    /// Whether `agent_id` under `account` (`None` = its own login) is known to be at a usage limit at
    /// `now_ms`. `Some(reset)`: the limit, with the latest moment a live limit of it says it ends (`None`
    /// = none said so). A limit ends at its reset, an hour after a limit that gave none, and when the
    /// pane that saw it carries on or closes. Other accounts of the same agent are not looked at.
    pub fn agent_limit(&self, agent_id: &str, account: Option<&str>, now_ms: u64) -> Option<Option<u64>> {
        let mut found = false;
        let mut latest: Option<u64> = None;
        for note in self.limited_agents.iter().filter(|n| n.agent_id == agent_id && n.account.as_deref() == account && n.live_at(now_ms)) {
            found = true;
            latest = latest.max(note.reset_at_ms);
        }
        found.then_some(latest)
    }

    fn fire(&mut self, env: &dyn Env, pane_id: &str, hit: &LimitHit) {
        let Some(ctx) = env.pane(pane_id) else { return };
        let since_ms = env.clock().now_ms().max(0) as u64;
        // One note per pane: a repeated detection on the same pane replaces its own note only.
        self.limited_agents.retain(|note| note.pane_id != pane_id && note.live_at(since_ms));
        self.limited_agents.push(LimitNote {
            pane_id: pane_id.to_string(),
            agent_id: ctx.agent_id.clone(),
            account: ctx.account.clone(),
            reset_at_ms: hit.reset_at_ms,
            since_ms,
        });
        env.mark(
            pane_id,
            &format!("limit:detected {} {} reset={}", hit.rule_id, hit.window.as_deref().unwrap_or("-"), hit.reset_text.as_deref().unwrap_or("-")),
        );
        env.log_limit(crate::plans_report::limit_log::Entry::Start {
            at_ms: env.clock().now_ms().max(0) as u64,
            pane: pane_id.to_string(),
            agent: ctx.agent_id.clone(),
            account: ctx.account.clone(),
            reset_at_ms: hit.reset_at_ms,
        });
        let project = ctx.runs_folder();
        let run = crate::operations::active_operation_for_pane(&project, pane_id).ok().flatten();
        let mut operation_id = None;
        if let Some(run) = &run {
            let action = limit_action(&ctx, pane_id, hit, env.clock());
            if let Ok(parked) = crate::operations::mark_operation_usage_limit(&project, &run.operation_id, action) {
                env.publish(&parked);
                operation_id = Some(run.operation_id.clone());
            }
        }
        let now = env.clock().now_ms().max(0) as u64;
        let offer = env.keep_going_offer(pane_id, &ctx, hit, &|agent, account| self.agent_limit(agent, account, now));
        let card = card::limit_card(hit, &ctx.agent_label, pane_id, operation_id.is_some(), env.clock(), &offer);
        if let Some(created) = card::raise(env.questions(), card, now) {
            self.cards.insert(pane_id.to_string(), CardRef { card_id: created.id, project, operation_id });
        }
    }

    fn clear(&mut self, env: &dyn Env, pane_id: &str, hit: &LimitHit, why: Cleared) {
        env.mark(pane_id, &format!("limit:cleared {} {why:?}", hit.rule_id));
        env.log_limit(crate::plans_report::limit_log::Entry::End { at_ms: env.clock().now_ms().max(0) as u64, pane: pane_id.to_string() });
        if why != Cleared::Replaced {
            self.limited_agents.retain(|note| note.pane_id != pane_id);
        }
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
        let answered_by = question.answer.as_ref().map(|answer| answer.answered_by.clone()).unwrap_or_else(|| "desktop".to_string());
        match card::decision(question, now) {
            card::Decision::Remind { pane_id, agent_label, window, due_ms } => {
                self.reminders.retain(|r| r.pane_id != pane_id);
                self.reminders.push(Reminder { pane_id: pane_id.clone(), agent_label, window, due_ms });
                self.save_reminders();
                env.mark(&pane_id, &format!("limit:remind due={due_ms}"));
            }
            card::Decision::StopRun { pane_id } => self.stop_run(env, &pane_id),
            // Keep going: queued for a thread, never run under this lock (`keep_going::live`).
            card::Decision::Continue { pane_id, target } => {
                env.keep_going(keep_going::live::Request::Continue { pane_id, target, who: keep_going::live::who_of(&answered_by) })
            }
            card::Decision::ComeBack { pane_id } => {
                env.keep_going(keep_going::live::Request::ComeBack { pane_id, who: keep_going::live::who_of(&answered_by) })
            }
            card::Decision::Stay { pane_id } => env.keep_going(keep_going::live::Request::Stay { pane_id }),
            card::Decision::Retry { pane_id, target } => {
                env.keep_going(keep_going::live::Request::Retry { pane_id, target, who: keep_going::live::who_of(&answered_by) })
            }
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
        let Ok(Some(run)) = crate::operations::active_operation_for_pane(&ctx.runs_folder(), pane_id) else { return };
        let context = run
            .required_action
            .as_ref()
            .map(|a| json!({ "window": a.get("window"), "reset_at_ms": a.get("reset_at_ms"), "reset_text": a.get("reset_text") }))
            .unwrap_or(Value::Null);
        if let Ok(stopped) = crate::operations::mark_operation_stopped_at_limit(&ctx.runs_folder(), &run.operation_id, context) {
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
        Some(PaneCtx { agent_id: info.agent_type, agent_label, account: info.account, project, status: info.status })
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

    fn log_limit(&self, entry: crate::plans_report::limit_log::Entry) {
        // Unit tests never write to the real app-data folder.
        #[cfg(not(test))]
        {
            let _ = crate::plans_report::limit_log::append(&crate::plans_report::limit_log::default_path(), &entry);
        }
        #[cfg(test)]
        let _ = entry;
    }

    fn keep_going_offer(
        &self,
        pane_id: &str,
        _ctx: &PaneCtx,
        _hit: &LimitHit,
        limited: &dyn Fn(&str, Option<&str>) -> Option<Option<u64>>,
    ) -> keep_going::model::KeepGoingOffer {
        keep_going::live::offer_for(pane_id, limited)
    }

    fn keep_going(&self, request: keep_going::live::Request) {
        keep_going::live::spawn(request);
    }
}

/// End-to-end tests only (`dev-fake-tui`): forget every agent limit note. A note lasts until its reset, so a limit an
/// earlier test of the same process left behind would otherwise decide which agent a later test is offered.
#[cfg(feature = "dev-fake-tui")]
pub fn forget_limit_notes_for_e2e() {
    ENGINE.lock().limited_agents.clear();
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
    #[cfg(not(test))]
    {
        // The usage-limit log begins with the first tick, so "Your plans" can say since when it counts.
        static BEGUN: std::sync::Once = std::sync::Once::new();
        BEGUN.call_once(|| {
            let now = SystemClock.now_ms().max(0) as u64;
            crate::plans_report::limit_log::ensure_begun(&crate::plans_report::limit_log::default_path(), now);
        });
    }
    let changed = ENGINE.lock().tick(&LiveEnv, alive);
    for pane_id in changed {
        crate::pane_activity::invalidate(&pane_id);
    }
    // Keep going's come-back check runs after the engine's lock is released.
    keep_going::live::tick(SystemClock.now_ms().max(0) as u64);
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

/// A limit is on this pane's screen right now and has been acted on (for "Your plans": a limit with no
/// recorded end that is still on a live pane lasts until now).
pub fn is_parked(pane_id: &str) -> bool {
    ENGINE.lock().tracker.parked(pane_id).is_some()
}

/// Whether the agent under `account` (`None` = its own login) is known to be at a usage limit now, with
/// the reset moment when it printed one (epoch ms). Seen by the detector on any of its panes, kept after
/// that pane is closed.
pub fn agent_limit(agent_id: &str, account: Option<&str>) -> Option<Option<u64>> {
    let now = SystemClock.now_ms().max(0) as u64;
    ENGINE.lock().agent_limit(agent_id, account, now)
}

/// The pane's status line while a limit is on screen (waiting at a usage limit, or retrying a
/// transient rate limit), for `pane_activity`.
pub fn status_line(pane_id: &str) -> Option<(crate::tui::ActivityKind, String)> {
    // A pane whose task was handed to another agent says so before anything about its limit.
    if let Some(line) = keep_going::live::status_line(pane_id) {
        return Some(line);
    }
    ENGINE.lock().status_line(&LiveEnv, pane_id)
}
