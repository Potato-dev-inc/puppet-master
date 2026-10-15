//! Missions v0 (Preview): a goal plus checks plus a budget (DESIGN-MISSIONS).
//!
//! Puppet Master makes one separate copy of the project, asks one agent at a time to work towards
//! the goal, and runs the checks after every turn. Not green yet: the agent gets the failures and
//! continues. No progress: the next backup agent takes over from a brief. Worse: the copy's files
//! go back to the best turn. Green: the test-change guard compares the tests with the start, and
//! if they were weakened the mission stops and asks the person. Nothing ever merges on its own: a
//! finished mission is a separate copy waiting for the person's review.
//!
//! Off by default (Settings › General › Labs). Visible panes only, a budget is required, and the
//! test-change guard is always on.
//!
//! Layout: `model` (types), `store` (disk), `ledger` (MISSION.md), `detect` (the sheet's checks),
//! `verdict` (checks + counts + test changes after a turn), `decide` (the pure decision table),
//! `prompt` (what each turn is asked), `engine` (the loop, `RoundIo`), `io` (the live world),
//! `cards` (attention), `view` (what the UI and MCP read), `routes` (bridge + MCP handles).

pub mod cards;
pub mod decide;
pub mod detect;
pub mod engine;
mod io;
pub mod ledger;
pub mod model;
pub mod prompt;
pub mod routes;
pub mod store;
pub mod verdict;
pub mod view;
#[cfg(test)]
mod tests;

use crate::agent_runs::checks::Check;
use crate::operations::OperationError;
use engine::{PersonAction, RoundIo};
use model::*;
use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// The Labs switch (public settings key). Off by default.
pub const SETTING: &str = "missions_enabled";

pub fn enabled(settings: &serde_json::Value) -> bool {
    settings.get(SETTING).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

fn err(code: &str, message: &str) -> OperationError {
    OperationError::new(code, message, false)
}

/// What a caller asks for (the start sheet or `run_agent` with `until_checks_pass`).
#[derive(Debug, Clone)]
pub struct StartRequest {
    pub project: String,
    pub objective: String,
    pub checks: Vec<MissionCheck>,
    pub agents: Vec<AgentChoice>,
    /// `None`: on when the goal names tests, else off.
    pub may_change_tests: Option<bool>,
    pub rollback: bool,
    pub budget_turns: Option<u32>,
    pub budget_minutes: Option<u32>,
    pub budget_parent: Option<String>,
    pub round_timeout_ms: u64,
    pub check_timeout_ms: u64,
    pub headless: bool,
    pub isolation: Option<String>,
    /// Checkpoints are on in Settings (Undo).
    pub undo_on: bool,
    pub started_by: Starter,
    pub idempotency_key: Option<String>,
}

impl StartRequest {
    pub fn new(project: &str, objective: &str, checks: Vec<MissionCheck>, agents: Vec<AgentChoice>) -> StartRequest {
        StartRequest {
            project: project.to_string(),
            objective: objective.to_string(),
            checks,
            agents,
            may_change_tests: None,
            rollback: true,
            budget_turns: None,
            budget_minutes: None,
            budget_parent: None,
            round_timeout_ms: DEFAULT_ROUND_TIMEOUT_MS,
            check_timeout_ms: DEFAULT_CHECK_TIMEOUT_MS,
            headless: false,
            isolation: None,
            undo_on: true,
            started_by: Starter::Desktop,
            idempotency_key: None,
        }
    }
}

static GOAL_NAMES_TESTS: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"(?i)\b(tests?|specs?|snapshots?)\b").expect("regex"));

/// "Your goal mentions tests, so agents may change them."
pub fn goal_names_tests(objective: &str) -> bool {
    GOAL_NAMES_TESTS.is_match(objective)
}

fn git_project(project: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .current_dir(project)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Two at once while missions are a Preview. Unit tests run many missions in parallel in one
/// process, so they don't share the limit.
fn max_active() -> usize {
    if cfg!(test) {
        usize::MAX
    } else {
        MAX_ACTIVE
    }
}

/// Every mission that is not finished, app-wide.
pub fn active_count() -> usize {
    store::index().iter().filter(|e| e.state.active()).count()
}

/// Checks a start request (the sheet's validation lines and the MCP error codes).
pub fn validate(req: &StartRequest) -> Result<(), OperationError> {
    if req.objective.trim().is_empty() {
        return Err(err("INVALID_TASK", "Say what should be true when it's done."));
    }
    if req.objective.chars().count() > MAX_OBJECTIVE {
        return Err(err("INVALID_TASK", "The goal is too long (2000 characters at most)."));
    }
    if req.headless {
        return Err(err("MISSION_HEADLESS", "Missions run in visible panes."));
    }
    if req.isolation.as_deref().is_some_and(|i| i.trim() == "none") {
        return Err(err("INVALID_ARGUMENT", "Missions always work in a separate copy; isolation \"none\" can't be used."));
    }
    if req.checks.is_empty() {
        return Err(err("MISSION_CHECKS_REQUIRED", "Pick at least one check. A mission ends only when its checks pass."));
    }
    if !req.checks.iter().any(|c| matches!(c.check, Check::CommandExits { .. })) {
        return Err(err("MISSION_CHECKS_REQUIRED", "Add a check that runs something, like Tests."));
    }
    let checks: Vec<Check> = req.checks.iter().map(|c| c.check.clone()).collect();
    crate::agent_runs::checks::validate(std::path::Path::new(&req.project), &checks).map_err(|m| err("INVALID_ARGUMENT", &m))?;
    if req.budget_turns.is_none() && req.budget_minutes.is_none() {
        return Err(err("MISSION_BUDGET_REQUIRED", "Set a budget in turns or time."));
    }
    crate::budgets::validate(&crate::budgets::Asked { turns: req.budget_turns, minutes: req.budget_minutes, parent: None })?;
    if req.agents.is_empty() {
        return Err(err("INVALID_ARGUMENT", "Pick an agent to work on it."));
    }
    if req.agents.len() > MAX_BACKUPS + 1 {
        return Err(err("INVALID_ARGUMENT", "At most 4 backup agents."));
    }
    let known: Vec<String> = crate::tui::descriptors().into_iter().map(|d| d.id.to_string()).collect();
    for agent in &req.agents {
        if !known.iter().any(|k| *k == agent.agent_type) {
            return Err(err("UNKNOWN_AGENT_TYPE", &format!("{} is not an agent Puppet Master knows.", agent.agent_type)));
        }
    }
    if !std::path::Path::new(&req.project).is_dir() || !git_project(&req.project) {
        return Err(err("MISSION_NOT_GIT", "Missions need a git project, because they work in a separate copy."));
    }
    if req.rollback && (!req.undo_on || !crate::checkpoints::git_available()) {
        return Err(err(
            "MISSION_UNDO_OFF",
            "Missions need Undo to go back when a turn makes things worse. Undo is off for this folder. Switch \"Go back when checks get worse\" off to start anyway.",
        ));
    }
    if active_count() >= max_active() {
        return Err(err("MISSION_LIMIT", "Two missions are already running. Stop or finish one first."));
    }
    Ok(())
}

/// Creates the mission and starts its engine with `io`. Returns the mission as saved (planning).
pub fn start_with(req: StartRequest, io: Arc<dyn RoundIo>) -> Result<Mission, OperationError> {
    if let Some(key) = req.idempotency_key.as_deref().filter(|k| !k.trim().is_empty()) {
        if let Some(found) = store::list(&req.project).into_iter().find(|m| m.idempotency_key.as_deref() == Some(key)) {
            return Ok(found);
        }
    }
    validate(&req)?;
    let now = store::now_ms();
    let id = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
    let may_change_tests = req.may_change_tests.unwrap_or_else(|| goal_names_tests(&req.objective));
    let mut mission = Mission {
        id,
        project: req.project.clone(),
        objective: req.objective.trim().to_string(),
        checks: req.checks,
        agents: req.agents,
        may_change_tests,
        rollback: req.rollback,
        budget_turns: req.budget_turns,
        budget_minutes: req.budget_minutes,
        budget_parent: req.budget_parent.clone(),
        round_timeout_ms: req.round_timeout_ms.clamp(60_000, 4 * 3_600_000),
        check_timeout_ms: req.check_timeout_ms.clamp(10_000, crate::agent_runs::checks::MAX_COMMAND_TIMEOUT_MS),
        worktree: None,
        state: MissionState::Planning,
        needs: None,
        sentence: None,
        paused_from: None,
        pause_requested: false,
        rounds: Vec::new(),
        baseline: None,
        roster: Roster::default(),
        expected_tree: None,
        outside_paths: Vec::new(),
        events: Vec::new(),
        started_by: req.started_by,
        tests_accepted: None,
        idempotency_key: req.idempotency_key,
        created_at_ms: now,
        updated_at_ms: now,
        ended_at_ms: None,
        rev: 0,
    };
    // The mission's own budget: the root every agent's turns are drawn from.
    crate::budgets::admit(
        &mission.project,
        &mission.handle(),
        &crate::budgets::Asked { turns: mission.budget_turns, minutes: mission.budget_minutes, parent: req.budget_parent },
        crate::budgets::Defaults::default(),
        now,
    )?;
    if may_change_tests && goal_names_tests(&mission.objective) {
        mission.push_event(now, "note", "v", "Your goal mentions tests, so agents may change them. Changes are listed when it's done.", None);
    }
    store::save(&mut mission).map_err(|e| err("MISSION_STORE_FAILED", &e))?;
    let who = match &mission.started_by {
        Starter::Desktop => crate::audit::Who::Desktop,
        Starter::Agent { session, .. } => crate::audit::Who::Agent(session.clone().unwrap_or_default()),
    };
    crate::audit::record(
        crate::audit::Kind::MissionStarted,
        who,
        &format!("Started the mission \"{}\"", engine::short(&mission.objective)),
        serde_json::json!({ "mission": mission.handle(), "agents": mission.agents.iter().map(|a| a.agent_type.clone()).collect::<Vec<_>>() }),
    );
    engine::spawn(io, mission.project.clone(), mission.id.clone());
    Ok(mission)
}

/// The live world. Unit tests never reach it (it would link the whole windowing runtime into the
/// test exe): there a stand-in that starts nothing is used.
#[cfg(not(test))]
fn live() -> Arc<dyn RoundIo> {
    Arc::new(io::LiveIo)
}

#[cfg(test)]
fn live() -> Arc<dyn RoundIo> {
    Arc::new(tests::NullIo)
}

pub fn start(req: StartRequest) -> Result<Mission, OperationError> {
    start_with(req, live())
}

/// The mission behind an id or a `mission-<id>` handle.
pub fn find(id_or_handle: &str) -> Option<Mission> {
    let id = id_or_handle.strip_prefix(HANDLE_PREFIX).unwrap_or(id_or_handle);
    let project = store::project_of(id)?;
    store::load(&project, id).ok()
}

fn not_found(id: &str) -> OperationError {
    OperationError::new("MISSION_NOT_FOUND", format!("There is no mission {id}."), false)
}

/// Stop now: the current turn is stopped, the copy and notes stay. `by` is `you`, `device:<id>` or
/// `agent:<session>`.
pub fn stop(id: &str, by: &str) -> Result<Mission, OperationError> {
    let m = find(id).ok_or_else(|| not_found(id))?;
    if m.state.terminal() {
        return Ok(m);
    }
    match engine::control_of(&m.id) {
        Some(ctl) => {
            *ctl.stop_by.lock() = Some(by.to_string());
            ctl.stop.store(true, Ordering::SeqCst);
            // Stop the turn right away, not when the engine next looks.
            if let Some(handle) = m.rounds.last().filter(|r| r.end.is_none()).and_then(|r| r.handle.clone()) {
                live().cancel_round(&m.project, &handle);
            }
            ctl.wake();
            Ok(wait_until(&m, |now| now.state.terminal(), 10_000))
        }
        None => {
            // No engine (it ended, or the app just started): stop the record itself.
            store::update(&m.project, &m.id, |m| {
                m.state = MissionState::Stopped;
                m.needs = None;
                m.sentence = Some("Stopped by you".into());
                m.push_event(store::now_ms(), "stopped", ":", "Stopped by you", None);
                ((), true)
            })
            .map_err(|_| not_found(id))?;
            find(id).ok_or_else(|| not_found(id))
        }
    }
}

/// "Pause after this turn" (desktop and phone only; orchestrators stop instead).
pub fn pause(id: &str) -> Result<Mission, OperationError> {
    set_pause(id, 1)
}

pub fn resume(id: &str) -> Result<Mission, OperationError> {
    let m = find(id).ok_or_else(|| not_found(id))?;
    if m.state != MissionState::Paused {
        // Not paused yet: "never mind" the pause asked for after this turn.
        return set_pause(id, 2);
    }
    act(&m, PersonAction::Resume)
}

fn set_pause(id: &str, want: u8) -> Result<Mission, OperationError> {
    let m = find(id).ok_or_else(|| not_found(id))?;
    if m.state.terminal() {
        return Ok(m);
    }
    match engine::control_of(&m.id) {
        Some(ctl) => {
            let rev = m.rev;
            ctl.pause.store(want, Ordering::SeqCst);
            ctl.wake();
            Ok(wait_until(&m, |now| now.rev > rev, 5_000))
        }
        None => {
            store::update(&m.project, &m.id, |m| {
                match (want, m.state) {
                    (1, MissionState::NeedsYou) => {
                        m.paused_from = Some(MissionState::NeedsYou);
                        m.state = MissionState::Paused;
                    }
                    (1, MissionState::Paused) => return ((), false),
                    (1, _) => m.pause_requested = true,
                    _ => m.pause_requested = false,
                }
                ((), true)
            })
            .map_err(|_| not_found(id))?;
            find(id).ok_or_else(|| not_found(id))
        }
    }
}

/// The person's answer to a card: `undo_tests`, `accept_tests`, `more_turns`, `hand_to_me`,
/// `try_again`. Never called by an agent.
pub fn decide_card(id: &str, choice: &str, by: &str) -> Result<Mission, OperationError> {
    let m = find(id).ok_or_else(|| not_found(id))?;
    let action = match choice {
        "undo_tests" => PersonAction::UndoTestChanges { by: by.to_string() },
        "accept_tests" => PersonAction::AcceptTestChanges { by: by.to_string() },
        "more_turns" => PersonAction::TryMoreTurns,
        "hand_to_me" => PersonAction::HandToMe,
        "try_again" => PersonAction::TryAgain,
        "stop" => return stop(id, by),
        other => return Err(err("INVALID_ARGUMENT", &format!("unknown choice {other}"))),
    };
    act(&m, action)
}

fn act(m: &Mission, action: PersonAction) -> Result<Mission, OperationError> {
    let ctl = match engine::control_of(&m.id) {
        Some(ctl) => ctl,
        None => {
            // The engine ended while waiting (or never ran since a restart): start it again.
            engine::spawn(live(), m.project.clone(), m.id.clone());
            engine::control_of(&m.id).ok_or_else(|| err("MISSION_BUSY", "The mission is busy. Try again in a moment."))?
        }
    };
    let rev = m.rev;
    ctl.set_action(action);
    Ok(wait_until(m, |now| now.rev > rev, 5_000))
}

// ---- change notification ---------------------------------------------------------------------

static CHANGED: Lazy<(Mutex<u64>, Condvar)> = Lazy::new(|| (Mutex::new(0), Condvar::new()));

/// A mission was saved: wake waiters and tell the desktop.
pub fn notify_changed(m: &Mission) {
    {
        let (lock, cv) = &*CHANGED;
        *lock.lock() += 1;
        cv.notify_all();
    }
    #[cfg(not(test))]
    if let Some(app) = crate::bridge::live_app() {
        use tauri::Emitter;
        let _ = app.emit("missions://changed", serde_json::json!({ "id": m.id, "state": m.state.as_str(), "rev": m.rev }));
    }
    #[cfg(test)]
    let _ = m;
}

/// Blocks until `done(mission)` or `timeout_ms`; returns the mission as it is then.
pub fn wait_until(m: &Mission, done: impl Fn(&Mission) -> bool, timeout_ms: u64) -> Mission {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let (lock, cv) = &*CHANGED;
        let mut generation = lock.lock();
        let seen = *generation;
        let now = store::load(&m.project, &m.id).unwrap_or_else(|_| m.clone());
        if done(&now) || std::time::Instant::now() >= deadline {
            return now;
        }
        if *generation == seen {
            cv.wait_for(&mut generation, deadline.saturating_duration_since(std::time::Instant::now()).min(std::time::Duration::from_millis(500)));
        }
    }
}

// ---- startup and panic -------------------------------------------------------------------------

/// After the operations recovery at startup: every unfinished mission continues where it was. A
/// turn the restart interrupted ends as "turn ended by restart"; its checks run and a new
/// conversation continues (DESIGN-MISSIONS 3.6). A paused mission stays paused.
pub fn resume_all() -> usize {
    resume_all_with(live())
}

pub fn resume_all_with(io: Arc<dyn RoundIo>) -> usize {
    let mut resumed = 0;
    for entry in store::index().into_iter().filter(|e| e.state.active()) {
        if store::load(&entry.project, &entry.id).is_ok_and(|m| !m.state.terminal()) && engine::spawn(io.clone(), entry.project, entry.id).is_some() {
            resumed += 1;
        }
    }
    resumed
}

/// "Stop all agents" (panic) stops every mission too.
pub fn stop_all() -> usize {
    let active: Vec<_> = store::index().into_iter().filter(|e| e.state.active()).collect();
    let mut stopped = 0;
    for entry in active {
        if stop(&entry.id, "you").is_ok() {
            stopped += 1;
        }
    }
    stopped
}
