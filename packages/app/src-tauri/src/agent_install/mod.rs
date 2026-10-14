//! "Set up agents" that installs: the doctor's Install button.
//!
//! Flow, all driven by the person at the desk (these are Tauri commands only; no bridge route or
//! MCP tool reaches them, so an agent can never trigger an install):
//!
//! 1. `plan` reads the agent's install spec (profile data, `tui::install`) and what this PC has
//!    (`plan::Tools`), and returns the exact commands, in order, with where each comes from.
//! 2. The UI shows them and asks. Nothing runs yet.
//! 3. `start` runs one step in a VISIBLE PowerShell pane. It rebuilds the plan itself and refuses
//!    unless the command text equals what the person saw (`INSTALL_COMMAND_CHANGED`), and unless
//!    a high-risk step (a vendor script) was confirmed a second time.
//! 4. `status` reads the pane for the done marker; `finish` refreshes PATH for this process
//!    (`path_refresh`) and re-runs the health check for that agent.
//!
//! Nothing here names an agent: commands come from `TuiProfile::install_spec`. Puppet Master never
//! types into the install pane and never sees a password; signing in is a separate visible pane
//! the agent itself opens.

pub mod command;
pub mod path_refresh;
pub mod plan;

use parking_lot::Mutex;
use plan::{Plan, Step, Tools};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

use crate::pty::PaneRegistry;

/// Install panes this module started, so `status` reads only those.
static INSTALL_PANES: once_cell::sync::Lazy<Mutex<HashSet<String>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashSet::new()));

/// Host part of a URL, for "this comes from example.com".
pub fn source_host(url: &str) -> String {
    url.strip_prefix("https://")
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// JSON the UI shows in the confirm sheet.
pub fn plan_view(plan: &Plan, install_hint: Option<&str>) -> Value {
    let steps: Vec<Value> = plan
        .steps
        .iter()
        .enumerate()
        .map(|(index, s)| step_view(index, s))
        .collect();
    json!({
        "agent_id": plan.agent_id,
        "label": plan.label,
        "available": plan.blocked.is_none() && !plan.steps.is_empty(),
        "reason": plan.blocked,
        "hint": install_hint,
        "steps": steps,
    })
}

fn step_view(index: usize, s: &Step) -> Value {
    json!({
        "index": index,
        "label": s.label,
        "tool": s.tool,
        "method": s.method,
        "command": s.command,
        "source": s.source,
        "source_host": source_host(&s.source),
        "high_risk": s.high_risk,
        "note": s.note,
    })
}

/// The current plan for `agent_id`, on this PC, right now.
pub fn plan_for(agent_id: &str, tools: &Tools) -> Result<(Plan, Option<String>), String> {
    let descriptor = crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id.eq_ignore_ascii_case(agent_id))
        .ok_or_else(|| format!("INSTALL_UNKNOWN_AGENT: {agent_id} is not an agent Puppet Master knows"))?;
    let spec = crate::tui::profile_for(descriptor.id).install_spec();
    let plan = plan::plan_install(descriptor.id, &descriptor.label, spec.as_ref(), tools);
    Ok((plan, descriptor.install_hint))
}

/// `agents_install_plan`.
pub fn plan(agent_id: &str) -> Result<Value, String> {
    let (plan, hint) = plan_for(agent_id, &Tools::detect())?;
    Ok(plan_view(&plan, hint.as_deref()))
}

/// Checks a run request against a freshly built plan. Pure, so the refusals are unit tests.
pub fn check_request<'a>(
    plan: &'a Plan,
    step_index: usize,
    shown_command: &str,
    acknowledged_risk: bool,
) -> Result<&'a Step, String> {
    if let Some(reason) = &plan.blocked {
        return Err(format!("INSTALL_UNAVAILABLE: {reason}"));
    }
    let step = plan
        .steps
        .get(step_index)
        .ok_or_else(|| format!("INSTALL_NO_SUCH_STEP: there is no step {step_index} for {}", plan.label))?;
    if step.command != shown_command {
        return Err("INSTALL_COMMAND_CHANGED: the command is not the one you were shown. Open Install again.".into());
    }
    if step.high_risk && !acknowledged_risk {
        return Err("INSTALL_NEEDS_CONFIRMATION: this runs a script from the internet. Confirm that you trust the source.".into());
    }
    Ok(step)
}

/// `agents_install_run`: runs one confirmed step in a visible PowerShell pane. Returns the pane id.
pub fn start(
    app: &tauri::AppHandle,
    registry: &Arc<Mutex<PaneRegistry>>,
    agent_id: &str,
    step_index: usize,
    shown_command: &str,
    acknowledged_risk: bool,
) -> Result<Value, String> {
    use tauri::Emitter;
    let (plan, _) = plan_for(agent_id, &Tools::detect())?;
    let step = check_request(&plan, step_index, shown_command, acknowledged_risk)?.clone();
    let script = command::powershell_script(&step);

    // A neutral folder: an install must not sync project memory into the active project.
    let dir = std::env::temp_dir().join("puppet-master-doctor");
    let _ = std::fs::create_dir_all(&dir);
    let pane_id = crate::pty::registry_spawn_pane(
        registry,
        app,
        crate::pty::SpawnPaneArgs {
            agent_type: "powershell".into(),
            cwd: Some(dir.to_string_lossy().into_owned()),
            cols: None,
            rows: None,
            extra_args: Some(vec!["-Command".into(), script]),
            pane_id: None,
            read_only: false,
            account: None,
        },
    )?;
    INSTALL_PANES.lock().insert(pane_id.clone());
    crate::ambient::store::set_user_title(&pane_id, &format!("Install {}", step.label));
    crate::ambient::announce_title_change(app, &pane_id);
    let (cols, rows) = registry
        .lock()
        .list()
        .into_iter()
        .find(|p| p.id == pane_id)
        .map(|p| (p.cols, p.rows))
        .unwrap_or((120, 30));
    crate::cockpit::present_pane(app, &pane_id, "powershell", cols, rows, crate::cockpit::Origin::CockpitUi);
    let _ = app.emit("pty://panes-changed", ());
    tracing::info!(agent = agent_id, step = step_index, command = %step.command, pane = %pane_id, "agent install: started in a visible pane");
    Ok(json!({ "pane_id": pane_id, "command": step.command, "label": step.label }))
}

/// `agents_install_status`: is the install pane's command finished, and with what exit code?
pub fn status(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Value {
    if !INSTALL_PANES.lock().contains(pane_id) {
        return json!({ "known": false, "done": false, "exit_code": null, "message": null });
    }
    match crate::pty::registry::read_buffer(registry, pane_id, 400) {
        Ok(text) => {
            let code = command::parse_done(&text);
            json!({
                "known": true,
                "done": code.is_some(),
                "exit_code": code,
                "message": code.and_then(command::explain_exit),
            })
        }
        // The window was closed before the command finished.
        Err(_) => json!({ "known": true, "done": true, "exit_code": null, "message": "The install window was closed.", "closed": true }),
    }
}

/// `agents_install_finish`: refresh PATH for this process, then check the agent again.
pub fn finish(agent_id: &str) -> Result<Value, String> {
    let refresh = path_refresh::refresh_process_path();
    let probe = crate::agent_health::collect_probes()
        .into_iter()
        .find(|p| p.id.eq_ignore_ascii_case(agent_id))
        .ok_or_else(|| format!("INSTALL_UNKNOWN_AGENT: {agent_id} is not an agent Puppet Master knows"))?;
    let health = crate::agent_health::check_probe(&probe, crate::agent_health::VERSION_TIMEOUT);
    Ok(json!({
        "path_added": refresh.added,
        "health": health,
        // Installed on disk, yet still not found: a restart gives every process the new PATH.
        "needs_restart": !health.installed,
    }))
}

#[cfg(test)]
mod tests;
