//! Missions on the bridge and on MCP.
//!
//! - `/missions/*`: the desktop's and the phone's routes (list, detect, get, start, stop, pause,
//!   resume, decide). Person-only: an MCP session is refused, except that it may list and read.
//! - MCP has no mission tools (DESIGN-MISSIONS 5, option C): `run_agent` with `until_checks_pass`
//!   starts one and returns a `mission-<id>` handle that `wait_agents`, `inspect_agent`,
//!   `cancel_agent` and `agent_transcript` understand. Orchestrators can stop a mission, never pause
//!   it, and can't steer it: `followup_task` / `send_message` / `answer_prompt` on a mission handle
//!   say to steer the current agent's own run instead.

use super::model::*;
use super::view::view;
use super::{StartRequest, HANDLE_PREFIX};
use crate::agent_runs::checks::Check;
use crate::operations::OperationError;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn err(code: &str, message: impl Into<String>) -> OperationError {
    OperationError::new(code, message, false)
}

fn settings(app: &tauri::AppHandle) -> Value {
    crate::settings_store::read_public_settings(app)
}

fn missions_off() -> OperationError {
    err("MISSIONS_OFF", "Missions are off. A person can turn them on in Settings › General › Labs.")
}

/// A name a person reads for a check given as an argv.
pub fn name_for(command: &[String]) -> String {
    let text = command.join(" ").to_ascii_lowercase();
    if text.contains("tsc") || text.contains("typecheck") || text.contains("mypy") || text.contains("pyright") {
        "Type check".into()
    } else if text.contains("lint") || text.contains("clippy") || text.contains("ruff") || text.contains("eslint") {
        "Lint".into()
    } else if text.contains("build") {
        "Build".into()
    } else {
        "Tests".into()
    }
}

fn named_checks(checks: Vec<Check>) -> Vec<MissionCheck> {
    let mut out: Vec<MissionCheck> = Vec::new();
    for check in checks {
        let base = match &check {
            Check::CommandExits { command, .. } => name_for(command),
            _ => "File check".to_string(),
        };
        let taken = out.iter().filter(|c| c.name == base || c.name.starts_with(&format!("{base} "))).count();
        let name = if taken == 0 { base } else { format!("{base} {}", taken + 1) };
        out.push(MissionCheck { name, check });
    }
    out
}

pub fn is_mission_handle(handle: &str) -> bool {
    handle.starts_with(HANDLE_PREFIX)
}

/// The MCP status of a mission: `completed` = done (or handed to the person), `needs_input` =
/// waiting for the person, `cancelled` = stopped, `failed` = ended, else `running`.
pub fn status_of(m: &Mission) -> &'static str {
    match m.state {
        MissionState::Green | MissionState::HandedOff => "completed",
        MissionState::NeedsYou => "needs_input",
        MissionState::Stopped => "cancelled",
        MissionState::Failed => "failed",
        _ => "running",
    }
}

/// The object `run_agent` / `wait_agents` / `inspect_agent` / `cancel_agent` return for a mission.
pub fn agent_object(m: &Mission) -> Value {
    let v = view(m);
    let status = status_of(m);
    let mut out = json!({
        "handle": m.handle(),
        "kind": "mission",
        "status": status,
        "stage": m.state.as_str(),
        "state_text": v.state_text,
        "turn": v.turn,
        "current_run": v.current_run,
        "mission": v,
    });
    match m.state {
        MissionState::Green => {
            let branch = m.worktree.as_ref().map(|w| w.branch.clone()).unwrap_or_default();
            out["result"] = json!(format!(
                "{} The separate copy is ready for the person to review on branch {branch}. Nothing was merged.",
                m.sentence.clone().unwrap_or_default()
            ));
        }
        MissionState::HandedOff => out["result"] = json!("handed to the person"),
        MissionState::NeedsYou => out["required_action"] = json!(m.sentence.clone().unwrap_or_else(|| "A person needs to decide.".into())),
        MissionState::Failed => out["error"] = json!({ "code": "MISSION_ENDED", "message": m.sentence.clone().unwrap_or_default() }),
        _ => {}
    }
    out
}

/// `run_agent` with `until_checks_pass: true`.
pub fn mcp_start(
    request: crate::agent_runs::AgentRunRequest,
    app: &tauri::AppHandle,
    default_project: &str,
    session: Option<&str>,
) -> Result<Value, OperationError> {
    let settings = settings(app);
    if !super::enabled(&settings) {
        return Err(missions_off());
    }
    let project = request.project_path.clone().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| default_project.to_string());
    let project = crate::project_path::prepare_project_path(std::path::Path::new(&project))
        .map_err(|e| err("INVALID_PROJECT_PATH", e))?
        .to_string_lossy()
        .into_owned();
    if request.pane_id.is_some() || request.worker_id.is_some() || request.handle.is_some() || request.agent_run_id.is_some() {
        return Err(err("INVALID_ARGUMENT", "A mission starts its own agents in a separate copy; pane_id, worker_id and handle can't be used with until_checks_pass."));
    }
    if request.allowed_commands.as_ref().is_some_and(|c| !c.is_empty()) || request.allow_shell {
        return Err(err("INVALID_ARGUMENT", "Missions never approve anything by themselves; allowed_commands and allow_shell can't be used with until_checks_pass."));
    }
    let mut agents = vec![AgentChoice { agent_type: request.agent_type.clone(), account: request.account.clone(), model: request.model.clone() }];
    for backup in request.backup_agents.clone().unwrap_or_default() {
        match AgentChoice::parse(&backup) {
            Some(choice) => agents.push(choice),
            None => return Err(err("INVALID_ARGUMENT", format!("backup_agents: {backup:?} is not an agent"))),
        }
    }
    let label = session
        .and_then(|s| crate::mcp_sessions::session_client_name(s))
        .unwrap_or_else(|| "an agent".to_string());
    let mut req = StartRequest::new(&project, &request.task, named_checks(request.checks.clone().unwrap_or_default()), agents);
    req.budget_turns = request.budget_turns;
    req.budget_minutes = request.budget_minutes;
    req.budget_parent = request.budget_parent.clone().filter(|p| !p.trim().is_empty());
    req.headless = request.headless;
    req.isolation = request.isolation.clone();
    req.undo_on = settings.get("checkpoints_enabled").and_then(Value::as_bool).unwrap_or(true);
    req.started_by = Starter::Agent { session: session.map(str::to_string), label };
    req.idempotency_key = request.idempotency_key.clone();
    if request.timeout_ms > 0 && request.timeout_ms != 900_000 {
        req.round_timeout_ms = request.timeout_ms;
    }
    let m = super::start(req)?;
    Ok(agent_object(&m))
}

#[derive(Debug, Deserialize)]
struct WaitBody {
    handles: Vec<String>,
    #[serde(default)]
    until: Option<Vec<String>>,
    #[serde(default = "default_wait", alias = "timeout_ms")]
    wait_ms: u64,
}

fn default_wait() -> u64 {
    30_000
}

fn mission_or_404(handle: &str) -> Result<Mission, OperationError> {
    super::find(handle).ok_or_else(|| err("AGENT_NOT_FOUND", format!("There is no mission {handle}.")))
}

fn wait(body: &[u8]) -> Result<Value, OperationError> {
    let body: WaitBody = serde_json::from_slice(body).map_err(|e| err("INVALID_ARGUMENT", e.to_string()))?;
    if body.wait_ms > 300_000 {
        return Err(err("INVALID_TIMEOUT", "wait_ms must be between 0 and 300000"));
    }
    if !body.handles.iter().all(|h| is_mission_handle(h)) {
        return Err(err("INVALID_ARGUMENT", "Wait for missions in their own wait_agents call (mission handles can't be mixed with run handles)."));
    }
    let until: Vec<String> = body.until.unwrap_or_else(|| vec!["completed".into(), "failed".into(), "cancelled".into(), "needs_input".into()]);
    let started = std::time::Instant::now();
    let missions: Vec<Mission> = body.handles.iter().map(|h| mission_or_404(h)).collect::<Result<_, _>>()?;
    let mut done: Vec<Mission> = Vec::new();
    // Any mode: return when one of them is in a wanted state, or the time is up.
    let deadline = started + std::time::Duration::from_millis(body.wait_ms);
    loop {
        done.clear();
        for m in &missions {
            done.push(super::store::load(&m.project, &m.id).unwrap_or_else(|_| m.clone()));
        }
        let hit = done.iter().any(|m| until.iter().any(|u| u == status_of(m)));
        if hit || std::time::Instant::now() >= deadline {
            let reason = if hit { "matched_state" } else { "timeout" };
            return Ok(json!({
                "agents": done.iter().map(agent_object).collect::<Vec<_>>(),
                "reason": reason,
                "mode": "any",
                "waited_ms": started.elapsed().as_millis() as u64,
                "requested_wait_ms": body.wait_ms,
            }));
        }
        let first = &done[0];
        super::wait_until(first, |now| now.rev != first.rev, deadline.saturating_duration_since(std::time::Instant::now()).as_millis().min(1_000) as u64);
    }
}

fn transcript(m: &Mission) -> Value {
    let entries: Vec<Value> = m
        .rounds
        .iter()
        .map(|r| {
            json!({
                "turn": r.n,
                "agent": m.agent_label(r.agent),
                "run": r.handle,
                "checks": r.verdict.as_ref().map(|v| v.short()),
                "what_happened": r.decision.as_ref().map(|d| d.line.clone()),
                "started_at_ms": r.started_at_ms,
                "ended_at_ms": r.ended_at_ms,
            })
        })
        .collect();
    json!({ "handle": m.handle(), "kind": "mission", "entries": entries, "events": m.events })
}

fn owner_ok(m: &Mission, session: Option<&str>) -> Result<(), OperationError> {
    match (&m.started_by, session) {
        (_, None) => Ok(()),
        (Starter::Agent { session: Some(owner), .. }, Some(caller)) if owner == caller => Ok(()),
        _ => Err(err("AGENT_NOT_OWNED", "Only the person, or the agent that started this mission, can stop it.")),
    }
}

/// Mission handles on the `/agents/*` routes. `None`: not a mission request.
pub fn intercept(
    method: &str,
    segments: &[&str],
    body: &[u8],
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
    session: Option<&str>,
) -> Option<Result<Value, OperationError>> {
    let handle_in_body = || serde_json::from_slice::<Value>(body).ok().and_then(|v| v.get("handle").and_then(Value::as_str).map(str::to_string));
    match (method, segments) {
        ("POST", ["agents", "run"]) => {
            let value: Value = serde_json::from_slice(body).ok()?;
            if value.get("until_checks_pass").and_then(Value::as_bool) != Some(true) {
                return None;
            }
            let request: crate::agent_runs::AgentRunRequest = match serde_json::from_value(value) {
                Ok(r) => r,
                Err(e) => return Some(Err(err("INVALID_ARGUMENT", e.to_string()))),
            };
            let default_project = registry.lock().project_path.clone();
            Some(mcp_start(request, app, &default_project, session))
        }
        ("POST", ["agents", "wait"]) => {
            let value: Value = serde_json::from_slice(body).ok()?;
            let handles = value.get("handles")?.as_array()?;
            if !handles.iter().any(|h| h.as_str().is_some_and(is_mission_handle)) {
                return None;
            }
            Some(wait(body))
        }
        ("POST", ["agents", "cancel" | "close" | "interrupt"]) => {
            let handle = handle_in_body().filter(|h| is_mission_handle(h))?;
            Some(mission_or_404(&handle).and_then(|m| {
                owner_ok(&m, session)?;
                let by = session.map(|s| format!("agent:{s}")).unwrap_or_else(|| "you".into());
                super::stop(&m.id, &by).map(|m| agent_object(&m))
            }))
        }
        ("POST", ["agents", "followup" | "send" | "answer"]) => {
            let handle = handle_in_body().filter(|h| is_mission_handle(h))?;
            let current = super::find(&handle).and_then(|m| view(&m).current_run);
            Some(Err(OperationError::new(
                "MISSION_NOT_STEERABLE",
                format!(
                    "This is a mission. Steer its current agent with the handle in inspect_agent's current_run{}, or stop the mission.",
                    current.map(|c| format!(" ({c})")).unwrap_or_default()
                ),
                false,
            )))
        }
        ("GET", ["agents", handle]) if is_mission_handle(handle) => Some(mission_or_404(handle).map(|m| agent_object(&m))),
        ("GET", ["agents", handle, "transcript"]) if is_mission_handle(handle) => Some(mission_or_404(handle).map(|m| transcript(&m))),
        _ => None,
    }
}

// ---- /missions/* (desktop, phone) --------------------------------------------------------------

/// The start sheet's body.
#[derive(Debug, Deserialize)]
pub struct StartBody {
    pub project_path: String,
    pub objective: String,
    /// `[{name, command: [argv]}]`.
    pub checks: Vec<SheetCheck>,
    pub agents: Vec<String>,
    #[serde(default)]
    pub may_change_tests: Option<bool>,
    #[serde(default = "yes")]
    pub rollback: bool,
    #[serde(default)]
    pub budget_turns: Option<u32>,
    #[serde(default)]
    pub budget_minutes: Option<u32>,
    #[serde(default)]
    pub round_timeout_min: Option<u64>,
    #[serde(default)]
    pub check_timeout_min: Option<u64>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct SheetCheck {
    pub name: String,
    pub command: Vec<String>,
}

pub fn start_from_sheet(body: StartBody, settings: &Value) -> Result<Mission, OperationError> {
    if !super::enabled(settings) {
        return Err(missions_off());
    }
    let check_timeout = body.check_timeout_min.unwrap_or(5).clamp(1, 10) * 60_000;
    let checks = body
        .checks
        .into_iter()
        .map(|c| MissionCheck { name: c.name, check: Check::CommandExits { command: c.command, expect_exit: 0, timeout_ms: Some(check_timeout) } })
        .collect();
    let agents = body.agents.iter().filter_map(|a| AgentChoice::parse(a)).collect();
    let mut req = StartRequest::new(&body.project_path, &body.objective, checks, agents);
    req.may_change_tests = body.may_change_tests;
    req.rollback = body.rollback;
    req.budget_turns = body.budget_turns.filter(|t| *t > 0);
    req.budget_minutes = body.budget_minutes.filter(|m| *m > 0);
    req.check_timeout_ms = check_timeout;
    req.round_timeout_ms = body.round_timeout_min.unwrap_or(30).clamp(1, 240) * 60_000;
    req.undo_on = settings.get("checkpoints_enabled").and_then(Value::as_bool).unwrap_or(true);
    super::start(req)
}

pub fn list_views(project: Option<&str>) -> Value {
    let missions: Vec<Mission> = match project.filter(|p| !p.trim().is_empty()) {
        Some(p) => super::store::list(p),
        None => {
            let mut all: Vec<Mission> = super::store::index()
                .into_iter()
                .filter_map(|e| super::store::load(&e.project, &e.id).ok())
                .collect();
            all.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
            all
        }
    };
    json!({ "missions": missions.iter().map(view).collect::<Vec<_>>() })
}

pub fn detect_value(project: &str) -> Value {
    json!({ "checks": super::detect::detect(std::path::Path::new(project)) })
}

/// The mission and its notes (MISSION.md as it is in the copy).
pub fn get_value(id: &str) -> Result<Value, OperationError> {
    let m = mission_or_404(id)?;
    let notes = m.worktree.as_ref().map(|w| super::ledger::notes(std::path::Path::new(&w.cwd))).unwrap_or_default();
    let ledger = m.worktree.as_ref().map(|w| super::ledger::path(std::path::Path::new(&w.cwd)).to_string_lossy().into_owned());
    let mut value = serde_json::to_value(view(&m)).unwrap_or_default();
    value["notes"] = json!(notes);
    value["ledger_path"] = json!(ledger);
    Ok(value)
}

fn person_only(session: Option<&str>) -> Result<(), OperationError> {
    match session {
        None => Ok(()),
        Some(_) => Err(err("PERSON_ONLY", "Only a person can do this from the app or the phone.")),
    }
}

/// `/missions/*` on the bridge. `None` when the request is not a missions route.
pub fn respond(
    method: &str,
    segments: &[&str],
    project: Option<&str>,
    body: &Value,
    app: &tauri::AppHandle,
    session: Option<&str>,
    by: &str,
) -> Option<Result<Value, OperationError>> {
    let result = match (method, segments) {
        ("GET", ["missions"]) => Ok(list_views(project)),
        ("GET", ["missions", "detect"]) => Ok(detect_value(project.unwrap_or_default())),
        ("GET", ["missions", id]) => get_value(id),
        ("POST", ["missions", "start"]) => person_only(session).and_then(|_| {
            let body: StartBody = serde_json::from_value(body.clone()).map_err(|e| err("INVALID_ARGUMENT", e.to_string()))?;
            start_from_sheet(body, &settings(app)).map(|m| serde_json::to_value(view(&m)).unwrap_or_default())
        }),
        ("POST", ["missions", id, "stop"]) => person_only(session).and_then(|_| super::stop(id, by)).map(|m| serde_json::to_value(view(&m)).unwrap_or_default()),
        ("POST", ["missions", id, "pause"]) => person_only(session).and_then(|_| super::pause(id)).map(|m| serde_json::to_value(view(&m)).unwrap_or_default()),
        ("POST", ["missions", id, "resume"]) => person_only(session).and_then(|_| super::resume(id)).map(|m| serde_json::to_value(view(&m)).unwrap_or_default()),
        ("POST", ["missions", id, "decide"]) => person_only(session).and_then(|_| {
            let choice = body.get("choice").and_then(Value::as_str).unwrap_or_default();
            super::decide_card(id, choice, by).map(|m| serde_json::to_value(view(&m)).unwrap_or_default())
        }),
        _ => return None,
    };
    Some(result)
}
