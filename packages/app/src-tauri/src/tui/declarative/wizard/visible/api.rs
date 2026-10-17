//! `POST /agents/wizard/capture` (SPEC 11.4): the operations of the guided capture. One action with an
//! `op`: `plan`, `start`, `status`, `act`, `show`, `finish`, `captures`, `discard`.
//!
//! `start` is the person's confirmation of the exact program and arguments (the same fingerprint rule
//! as the headless live test), and the only place a capture pane is created.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use super::super::locate::display_path;
use super::super::report::WizardError;
use super::super::session::{sha256_hex, Session, State, Wizard};
use super::capture::{CaptureAction, StepId};
use super::fixtures::masked_json;
use super::host::{CaptureProfile, PaneSpec};
use super::screen::{GRID_COLS, GRID_ROWS};

pub const MAX_ARGS: usize = 8;
pub const MAX_ARG_CHARS: usize = 120;

/// The arguments the person typed: at most 8, each at most 120 characters, no control characters.
pub fn clean_args(body: &Value) -> Result<Vec<String>, WizardError> {
    let Some(raw) = body.get("args") else { return Ok(Vec::new()) };
    if raw.is_null() {
        return Ok(Vec::new());
    }
    let list = raw.as_array().ok_or_else(|| WizardError::bad_request("args must be a list of text"))?;
    if list.len() > MAX_ARGS {
        return Err(WizardError::bad_request(format!("at most {MAX_ARGS} arguments")));
    }
    let mut args = Vec::new();
    for item in list {
        let text = item.as_str().ok_or_else(|| WizardError::bad_request("every argument must be text"))?;
        if text.chars().count() > MAX_ARG_CHARS || text.chars().any(|c| c.is_control()) {
            return Err(WizardError::bad_request(format!(
                "an argument may have at most {MAX_ARG_CHARS} characters and no control characters"
            )));
        }
        args.push(text.to_string());
    }
    Ok(args)
}

/// What the capture pane would start: the program, the arguments and the grid.
#[derive(Debug, Clone)]
pub struct CapturePlan {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub fingerprint: String,
}

pub fn plan(session: &Session, args: &[String]) -> CapturePlan {
    let (program, all) = CaptureProfile::command_line(&session.program, args);
    let canonical = json!({"program": program, "args": all, "grid": [GRID_COLS, GRID_ROWS], "cwd": "<isolated folder>"});
    CapturePlan { program: session.program.clone(), args: args.to_vec(), fingerprint: sha256_hex(canonical.to_string().as_bytes()) }
}

pub fn plan_json(plan: &CapturePlan) -> Value {
    json!({
        "program": {"path": plan.program.display().to_string(), "display": display_path(&plan.program)},
        "args": plan.args,
        "cwd": "<isolated folder>",
        "grid": {"cols": GRID_COLS, "rows": GRID_ROWS},
        "fingerprint": plan.fingerprint,
    })
}

fn bad_state(op: &str, state: State) -> WizardError {
    WizardError::new(
        409,
        "WIZARD_BAD_STATE",
        format!("capture {op} needs the session to be probed first, but it is {}", state.as_str()),
    )
}

fn snapshot(wizard: &Wizard, id: &str, op: &str) -> Result<Session, WizardError> {
    let session = wizard.read(id, |s| s.clone())?;
    if !matches!(session.state, State::Probed | State::Verified | State::Tested) {
        return Err(bad_state(op, session.state));
    }
    Ok(session)
}

fn git(dir: &Path, args: &[&str]) -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).current_dir(dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// A fresh folder with `git init` and one commit, so a trust question appears as in a real project.
/// Returns false when git is missing (the capture still runs; a trust question may not appear).
pub fn make_project(dir: &Path) -> Result<bool, WizardError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| WizardError::new(500, "WIZARD_IO", format!("cannot create the isolated folder: {e}")))?;
    std::fs::write(dir.join("README.md"), "# isolated project\n\nA scratch folder made by the Puppet Master agent wizard.\n")
        .map_err(|e| WizardError::new(500, "WIZARD_IO", format!("cannot write into the isolated folder: {e}")))?;
    let ok = git(dir, &["init", "-q"])
        && git(dir, &["add", "."])
        && git(dir, &["-c", "user.name=wizard", "-c", "user.email=wizard@example.invalid", "commit", "-q", "-m", "start"]);
    Ok(ok)
}

fn run_of(session: &Session) -> Option<Arc<super::runner::CaptureRun>> {
    session.visible.run.lock().clone()
}

fn captured_flag(session: &Session) -> bool {
    session.visible.has_captures()
}

fn status_of(session: &Session) -> Value {
    let mut status = match run_of(session) {
        Some(run) => run.status(),
        None => json!({"state": "none"}),
    };
    status["captured"] = json!(captured_flag(session));
    status
}

/// `POST /agents/wizard/capture`.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::super::api::session_id(body)?;
    let op = body.get("op").and_then(Value::as_str).map(str::trim).filter(|o| !o.is_empty());
    let Some(op) = op else {
        return Err(WizardError::bad_request("op is required: plan, start, status, act, show, finish, captures, debug, test_status, test_act or discard"));
    };
    match op {
        "plan" => {
            let session = snapshot(wizard, &id, "plan")?;
            let args = clean_args(body)?;
            Ok(json!({"plan": plan_json(&plan(&session, &args))}))
        }
        "start" => start(wizard, &id, body),
        "status" => {
            let session = wizard.read(&id, |s| s.clone())?;
            Ok(status_of(&session))
        }
        "act" => {
            let session = wizard.read(&id, |s| s.clone())?;
            let run = run_of(&session).ok_or_else(|| WizardError::new(409, "WIZARD_NO_CAPTURE", "no capture is running; start one first"))?;
            let action = body
                .get("action")
                .and_then(Value::as_str)
                .and_then(CaptureAction::parse)
                .ok_or_else(|| WizardError::bad_request("action must be skip, retry, stuck, ready, try_exit, closed, done or question"))?;
            let step = match body.get("step").and_then(Value::as_str) {
                Some(text) => Some(StepId::parse(text).ok_or_else(|| WizardError::bad_request("step must be start, ready, message, work, permission or exit"))?),
                None => None,
            };
            if action == CaptureAction::Stuck {
                // "It looks stuck": keep what the engine saw before it moves on
                *session.visible.debug.lock() = Some(run.debug_json(false));
            }
            run.act(step, action).map_err(|e| WizardError::new(409, "WIZARD_CAPTURE_REFUSED", e))?;
            Ok(status_of(&session))
        }
        "debug" => {
            let session = wizard.read(&id, |s| s.clone())?;
            let live = run_of(&session).map(|run| run.debug_json(false));
            let kept = session.visible.debug.lock().clone();
            Ok(json!({"debug": live.or(kept)}))
        }
        "test_status" => {
            let session = wizard.read(&id, |s| s.clone())?;
            let status = session.visible.test_status.lock().clone();
            Ok(if status.is_null() { json!({"running": false}) } else { status })
        }
        "show" => {
            let session = wizard.read(&id, |s| s.clone())?;
            if let Some(run) = run_of(&session) {
                run.show();
            } else if let Some((pane, _)) = session.visible.test_pane.lock().clone() {
                // the visible test's pane (the question on it is the person's to answer)
                wizard.env().host.show(&pane);
            }
            Ok(json!({"ok": true}))
        }
        "test_act" => {
            let session = wizard.read(&id, |s| s.clone())?;
            let action = body.get("action").and_then(Value::as_str).unwrap_or("");
            if !matches!(action, "skip" | "stop") {
                return Err(WizardError::bad_request("action must be skip or stop"));
            }
            *session.visible.test_ctl.lock() = Some(action.to_string());
            Ok(json!({"ok": true}))
        }
        "finish" => {
            let session = wizard.read(&id, |s| s.clone())?;
            let run = run_of(&session);
            let Some(run) = run else {
                return Ok(captures_json(&session));
            };
            *session.visible.debug.lock() = Some(run.debug_json(false));
            let set = run.finish();
            *session.visible.captures.lock() = Some(set);
            *session.visible.run.lock() = None;
            Ok(captures_json(&session))
        }
        "captures" => {
            let session = wizard.read(&id, |s| s.clone())?;
            Ok(captures_json(&session))
        }
        "discard" => {
            let session = wizard.read(&id, |s| s.clone())?;
            let host = wizard.env().host;
            session.visible.shut_down(&*host);
            *session.visible.captures.lock() = None;
            Ok(json!({"ok": true}))
        }
        other => Err(WizardError::bad_request(format!("unknown capture op {other}"))),
    }
}

fn captures_json(session: &Session) -> Value {
    match session.visible.captures.lock().as_ref() {
        Some(set) => json!({"captures": masked_json(set, &session.words)}),
        None => json!({"captures": null}),
    }
}

fn start(wizard: &Wizard, id: &str, body: &Value) -> Result<Value, WizardError> {
    if body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return Err(WizardError::new(
            400,
            "WIZARD_CONFIRM_REQUIRED",
            "starting the program in a pane needs the person's confirmation; send confirm: true after they saw the command",
        ));
    }
    let given = body.get("plan").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
    let Some(given) = given else {
        return Err(WizardError::new(
            400,
            "WIZARD_PLAN_REQUIRED",
            "send the fingerprint of the command (the `plan.fingerprint` that capture plan returned) that the person saw",
        ));
    };
    let session = snapshot(wizard, id, "start")?;
    let args = clean_args(body)?;
    let wanted = plan(&session, &args);
    if wanted.fingerprint != given {
        return Err(WizardError::new(
            409,
            "WIZARD_PLAN_MISMATCH",
            "the command that would start is not the one the person was shown; show it again and ask for a new confirmation",
        ));
    }
    if let Some(run) = run_of(&session) {
        if !run.is_finished() {
            return Err(WizardError::new(409, "WIZARD_CAPTURE_RUNNING", "a capture is already running for this session; finish or discard it first"));
        }
    }
    let env = wizard.env();
    let project = session.work_dir.join("capture-project");
    let _ = std::fs::remove_dir_all(&project);
    make_project(&project)?;
    let profile_id = format!("wzc_{}", session.id.trim_start_matches("wz_"));
    let profile = Arc::new(CaptureProfile::new(&profile_id, session.program.clone(), args));
    let spec = PaneSpec { profile, cwd: project.clone(), cols: GRID_COLS, rows: GRID_ROWS, window: env.visible.window };
    crate::tui::registry::registry().add_transient(spec.profile.clone()).map_err(|e| WizardError::new(500, "WIZARD_PANE_FAILED", e))?;
    let pane = match env.host.spawn(&spec) {
        Ok(pane) => pane,
        Err(e) => {
            crate::tui::registry::registry().remove_transient(&profile_id);
            return Err(WizardError::new(500, "WIZARD_PANE_FAILED", format!("the pane could not be started: {e}")));
        }
    };
    let run = super::runner::CaptureRun::start(
        env.host.clone(),
        pane,
        profile_id,
        project,
        session.words.clone(),
        (GRID_COLS, GRID_ROWS),
        env.visible.clone(),
        session.cancel.clone(),
        session.auto_answer.then(|| super::runner::AutoScope { work_dir: session.work_dir.clone(), temp_root: env.temp_root.clone() }),
    );
    *session.visible.run.lock() = Some(run);
    Ok(status_of(&session))
}
