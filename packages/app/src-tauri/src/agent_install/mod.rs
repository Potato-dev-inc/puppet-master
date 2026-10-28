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
use std::path::Path;
use std::sync::Arc;

use crate::agent_health::Probe;
use crate::agent_store::catalog::{self, CatalogEntry};
use crate::pty::PaneRegistry;
use crate::tui::declarative::parse_definition;
use crate::tui::install::InstallSpec;
use crate::tui::VersionCommand;

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

fn unknown_agent(agent_id: &str) -> String {
    format!("INSTALL_UNKNOWN_AGENT: {agent_id} is not an agent Puppet Master knows")
}

/// The current plan for `agent_id`, on this PC, right now. A registered agent uses its profile's
/// spec. An id that is not registered but is in the Store catalog uses its catalog entry.
pub fn plan_for(agent_id: &str, tools: &Tools) -> Result<(Plan, Option<String>), String> {
    let Some(descriptor) = crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id.eq_ignore_ascii_case(agent_id))
    else {
        let entry = catalog::get(agent_id).ok_or_else(|| unknown_agent(agent_id))?;
        let spec = catalog::install_spec(agent_id);
        return Ok((store_plan(entry, spec.as_ref(), tools), None));
    };
    let spec = crate::tui::profile_for(descriptor.id).install_spec();
    let plan = plan::plan_install(descriptor.id, &descriptor.label, spec.as_ref(), tools);
    Ok((plan, descriptor.install_hint))
}

/// The install plan for a catalog entry that is not registered. The label is `CatalogEntry::label`
/// (the definition's name, else the vendor's). `spec` is its Windows install spec, or `None` (then the
/// plan says why it is blocked).
pub fn store_plan(entry: &CatalogEntry, spec: Option<&InstallSpec>, tools: &Tools) -> Plan {
    plan::plan_install(&entry.entry.store.id, &entry.label(), spec, tools)
}

/// The health probe for a catalog entry that is not registered. The program is looked up the way
/// its definition says (`resolve_binary`, which skips the project folder), then checked like any agent.
pub fn store_probe(entry: &CatalogEntry, here: &Path) -> Option<Probe> {
    let def = parse_definition(entry.definition_text.as_deref()?).ok()?;
    let program = crate::tui::resolve_binary(&def.binary, here).ok();
    let words = |text: &str| Some(text.trim().to_string()).filter(|t| !t.is_empty());
    Some(Probe {
        id: entry.entry.store.id.clone(),
        label: entry.label(),
        installed: program.is_some(),
        version_command: program.map(|program| VersionCommand {
            program: program.to_string_lossy().into_owned(),
            args: def.binary.version_args.clone(),
        }),
        install_hint: words(def.binary.install_hint.as_str()),
        install_fallback: None,
        login_hint: words(def.binary.login_hint.as_str()),
        can_test: false,
    })
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
    let (plan, _) = plan_for(agent_id, &Tools::detect())?;
    start_plan(app, registry, &plan, step_index, shown_command, acknowledged_risk, "Install")
}

/// Runs one step of a plan the person confirmed, in a visible PowerShell pane. The plan must be
/// built fresh from the current state; the step's command must equal `shown_command`. `verb` starts
/// the pane title ("Install", "Uninstall"), so the step label reads "Install <label>".
pub fn start_plan(
    app: &tauri::AppHandle,
    registry: &Arc<Mutex<PaneRegistry>>,
    plan: &Plan,
    step_index: usize,
    shown_command: &str,
    acknowledged_risk: bool,
    verb: &str,
) -> Result<Value, String> {
    use tauri::Emitter;
    let step = check_request(plan, step_index, shown_command, acknowledged_risk)?.clone();
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
            guard_env: None,
        },
    )?;
    INSTALL_PANES.lock().insert(pane_id.clone());
    crate::ambient::store::set_user_title(&pane_id, &format!("{verb} {}", step.label));
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
    tracing::info!(agent = plan.agent_id.as_str(), step = step_index, command = %step.command, pane = %pane_id, "agent install: started in a visible pane");
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
    let probe = match crate::agent_health::collect_probes()
        .into_iter()
        .find(|p| p.id.eq_ignore_ascii_case(agent_id))
    {
        Some(probe) => probe,
        // Not registered (a Store agent that is not added yet): check it the way its definition says.
        None => {
            let here = std::env::current_dir().unwrap_or_default();
            catalog::get(agent_id)
                .and_then(|entry| store_probe(entry, &here))
                .ok_or_else(|| unknown_agent(agent_id))?
        }
    };
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
#[cfg(all(test, windows))]
mod live_tests;
