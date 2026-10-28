//! Turns an install spec plus "what this PC already has" into the exact steps to run.
//!
//! Pure: nothing here starts a process or touches the registry, so every rule is a unit test. The
//! plan is built twice, once to show the user and again when they press Run; the second plan must
//! produce the same command text or the run is refused (`INSTALL_COMMAND_CHANGED`).

use crate::tui::install::{is_pypi_package, is_python_version, InstallSpec, InstallTable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TOOLS_TOML: &str = include_str!("tools.toml");

/// How a step installs something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Winget,
    Npm,
    Uv,
    Script,
}

/// What the program will run. Always argv (no shell string) except the vendor script, which is
/// `irm '<url>' | iex` built from a URL the spec validator restricted to inert characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Argv { exe: String, args: Vec<String> },
    Script { url: String },
}

/// One command the user is asked to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// `git` / `node` for a prerequisite, `None` for the agent itself.
    pub tool: Option<String>,
    pub label: String,
    pub method: Method,
    pub action: Action,
    /// The exact text shown to the user and compared again at Run.
    pub command: String,
    pub source: String,
    pub high_risk: bool,
    pub note: String,
}

/// The whole answer for one agent: steps in the order they must run, or the reason there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub agent_id: String,
    pub label: String,
    pub steps: Vec<Step>,
    pub blocked: Option<String>,
}

/// What this PC has. Tests build their own; `detect` reads the real PATH.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    /// Program to run for winget (`winget`, or a full path when only the app alias exists).
    pub winget: Option<String>,
    /// Tool ids (`git`, `node`) whose programs are all on PATH.
    pub present: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolDef {
    label: String,
    programs: Vec<String>,
    install: InstallTable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsFile {
    tools: BTreeMap<String, ToolDef>,
}

fn tool_defs() -> BTreeMap<String, ToolDef> {
    toml::from_str::<ToolsFile>(TOOLS_TOML).map(|f| f.tools).unwrap_or_default()
}

/// First directory in `path_value` (a `;`/`:` separated list) that holds `name`. Uses
/// `symlink_metadata` so a Windows app-execution alias such as winget.exe, which `is_file`
/// reports as missing, still counts.
pub fn find_in_path(name: &str, path_value: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_value)
        .map(|dir| dir.join(name))
        .find(|candidate| std::fs::symlink_metadata(candidate).is_ok())
}

impl Tools {
    pub fn detect() -> Self {
        let path = std::env::var_os("PATH").unwrap_or_default();
        Self::detect_in(&path, std::env::var_os("LOCALAPPDATA").map(PathBuf::from).as_deref())
    }

    /// `detect` against an explicit PATH and `%LOCALAPPDATA%`.
    pub fn detect_in(path: &std::ffi::OsStr, local_app_data: Option<&Path>) -> Self {
        let winget = if find_in_path("winget.exe", path).is_some() {
            Some("winget".to_string())
        } else {
            // The Store installs winget as an app alias in WindowsApps, which some processes
            // (a service, a shortcut launch) do not have on PATH.
            local_app_data
                .map(|dir| dir.join("Microsoft").join("WindowsApps").join("winget.exe"))
                .filter(|alias| std::fs::symlink_metadata(alias).is_ok())
                .map(|alias| alias.to_string_lossy().into_owned())
        };
        let present = tool_defs()
            .into_iter()
            .filter(|(_, def)| def.programs.iter().all(|p| find_in_path(p, path).is_some()))
            .map(|(id, _)| id)
            .collect();
        Self { winget, present }
    }

    fn has(&self, tool: &str) -> bool {
        self.present.contains(tool)
    }
}

/// `winget install --id <id> -e --source winget`. No `--accept-*` flags: the agreements are shown in
/// the visible window and the person answers them.
fn winget_args(id: &str) -> Vec<String> {
    ["install", "--id", id, "-e", "--source", "winget"].iter().map(|s| s.to_string()).collect()
}

fn npm_args(package: &str) -> Vec<String> {
    ["install", "-g", package].iter().map(|s| s.to_string()).collect()
}

/// `tool install [--python <v>] <package>`. The package and version were checked by the spec
/// validator; `python` is checked again here so a bad value can never reach the argv.
fn uv_args(package: &str, python: Option<&str>) -> Option<Vec<String>> {
    if !is_pypi_package(package) {
        return None;
    }
    let mut args: Vec<String> = ["tool", "install"].iter().map(|s| s.to_string()).collect();
    if let Some(version) = python {
        if !is_python_version(version) {
            return None;
        }
        args.push("--python".into());
        args.push(version.into());
    }
    args.push(package.into());
    Some(args)
}

/// `tool uninstall <package>`, the removal that matches `uv_args`.
fn uv_uninstall_args(package: &str) -> Vec<String> {
    ["tool", "uninstall", package].iter().map(|s| s.to_string()).collect()
}

/// `uninstall --id <id> -e --source winget`, the removal that matches `winget_args`.
fn winget_uninstall_args(id: &str) -> Vec<String> {
    ["uninstall", "--id", id, "-e", "--source", "winget"].iter().map(|s| s.to_string()).collect()
}

/// `uninstall -g <package>`, the removal that matches `npm_args`.
fn npm_uninstall_args(package: &str) -> Vec<String> {
    ["uninstall", "-g", package].iter().map(|s| s.to_string()).collect()
}

/// Text for the sheet and the pane banner. Every token passed the spec validator, so plain
/// spaces are enough; the real run quotes each token again (`command::powershell_script`).
fn display(action: &Action) -> String {
    match action {
        Action::Argv { exe, args } => {
            let program = if exe.contains(['\\', '/']) { "winget" } else { exe.as_str() };
            let mut parts = vec![program.to_string()];
            parts.extend(args.iter().cloned());
            parts.join(" ")
        }
        Action::Script { url } => format!("irm '{url}' | iex"),
    }
}

fn step(label: &str, tool: Option<&str>, spec: &InstallSpec, method: &str, tools: &Tools) -> Option<Step> {
    let (method_kind, action, high_risk) = match method {
        "winget" => {
            let exe = tools.winget.clone()?;
            (Method::Winget, Action::Argv { exe, args: winget_args(spec.winget.as_deref()?) }, false)
        }
        "npm" => (
            Method::Npm,
            Action::Argv { exe: "npm".into(), args: npm_args(spec.npm.as_deref()?) },
            false,
        ),
        "uv" => (
            Method::Uv,
            Action::Argv { exe: "uv".into(), args: uv_args(spec.uv.as_deref()?, spec.python.as_deref())? },
            false,
        ),
        "script" => (Method::Script, Action::Script { url: spec.script_url.clone()? }, true),
        // Never guess: a method this file does not know gets no step at all.
        _ => return None,
    };
    Some(Step {
        tool: tool.map(str::to_string),
        label: label.to_string(),
        method: method_kind,
        command: display(&action),
        action,
        source: spec.source.clone(),
        high_risk,
        note: spec.note.clone(),
    })
}

/// Tools a method needs that this PC lacks. `npm` always needs Node.js; `uv` always needs uv.
fn missing_tools(spec: &InstallSpec, method: &str, tools: &Tools) -> Vec<String> {
    let mut need: Vec<&str> = spec.requires.iter().map(String::as_str).collect();
    if method == "npm" && !need.contains(&"node") {
        need.push("node");
    }
    if method == "uv" && !need.contains(&"uv") {
        need.push("uv");
    }
    need.into_iter().filter(|t| !tools.has(t)).map(str::to_string).collect()
}

/// The install step for a missing tool, through winget only.
fn tool_step(tool: &str, tools: &Tools) -> Option<Step> {
    let defs = tool_defs();
    let def = defs.get(tool)?;
    let spec = def.install.windows.as_ref()?;
    step(&def.label, Some(tool), spec, "winget", tools)
}

/// The plan for one agent.
///
/// Preference: a method that is ready now (winget, then npm, then uv, in the spec's order); then
/// one that is ready once a missing tool (Node.js, uv) is installed through winget; the vendor
/// script last, because it is the one method that downloads and runs code.
pub fn plan_install(agent_id: &str, label: &str, spec: Option<&InstallSpec>, tools: &Tools) -> Plan {
    let blocked = |reason: &str| Plan {
        agent_id: agent_id.into(),
        label: label.into(),
        steps: Vec::new(),
        blocked: Some(reason.to_string()),
    };
    let Some(spec) = spec else {
        return blocked("Puppet Master does not know how to install this one. Use the command below.");
    };
    let methods = spec.ordered_methods();
    let mut ready_script: Option<Step> = None;
    let mut with_prereqs: Option<Vec<Step>> = None;
    for method in methods {
        let Some(main) = step(label, None, spec, method, tools) else { continue };
        let missing = missing_tools(spec, method, tools);
        if method == "script" {
            // Script needs only PowerShell. A required tool can still be missing (rare).
            if missing.is_empty() && ready_script.is_none() {
                ready_script = Some(main);
            }
            continue;
        }
        if missing.is_empty() {
            return Plan { agent_id: agent_id.into(), label: label.into(), steps: vec![main], blocked: None };
        }
        if with_prereqs.is_none() {
            let prereqs: Option<Vec<Step>> = missing.iter().map(|t| tool_step(t, tools)).collect();
            if let Some(mut steps) = prereqs {
                steps.push(main);
                with_prereqs = Some(steps);
            }
        }
    }
    if let Some(steps) = with_prereqs {
        return Plan { agent_id: agent_id.into(), label: label.into(), steps, blocked: None };
    }
    if let Some(script) = ready_script {
        return Plan { agent_id: agent_id.into(), label: label.into(), steps: vec![script], blocked: None };
    }
    if tools.winget.is_none() {
        return blocked(
            "This PC cannot install apps from the command line (winget is missing). Use the command below, or download it from the website.",
        );
    }
    blocked("No way to install this one from here. Use the command below.")
}

/// Plain words for "this was not installed by a method we can remove".
const INSTALLED_ANOTHER_WAY: &str = "This was installed another way. Remove it where you installed it.";

/// The removal for a program that the Store installed, from the CURRENT spec.
///
/// `record_method` and `record_package` come from what the Store recorded at install time. The
/// spec must still name that same method and that same package, otherwise the plan is blocked: a
/// catalog change must never point a removal at a different program. One step only, never a
/// script, never a prerequisite (Node.js, Git and uv stay on the PC).
pub fn plan_uninstall(
    agent_id: &str,
    label: &str,
    spec: Option<&InstallSpec>,
    record_method: &str,
    record_package: &str,
    tools: &Tools,
) -> Plan {
    let blocked = |reason: &str| Plan {
        agent_id: agent_id.into(),
        label: label.into(),
        steps: Vec::new(),
        blocked: Some(reason.to_string()),
    };
    let Some(spec) = spec else { return blocked(INSTALLED_ANOTHER_WAY) };
    let (method, now) = match record_method {
        "winget" => (Method::Winget, spec.winget.as_deref()),
        "npm" => (Method::Npm, spec.npm.as_deref()),
        "uv" => (Method::Uv, spec.uv.as_deref()),
        _ => return blocked(INSTALLED_ANOTHER_WAY),
    };
    if now != Some(record_package) {
        return blocked(INSTALLED_ANOTHER_WAY);
    }
    let action = match method {
        Method::Winget => match &tools.winget {
            Some(exe) => Action::Argv { exe: exe.clone(), args: winget_uninstall_args(record_package) },
            None => {
                return blocked(
                    "This PC cannot remove apps from the command line (winget is missing). Remove it where you installed it.",
                )
            }
        },
        Method::Npm => Action::Argv { exe: "npm".into(), args: npm_uninstall_args(record_package) },
        Method::Uv => Action::Argv { exe: "uv".into(), args: uv_uninstall_args(record_package) },
        Method::Script => return blocked(INSTALLED_ANOTHER_WAY),
    };
    Plan {
        agent_id: agent_id.into(),
        label: label.into(),
        steps: vec![Step {
            tool: None,
            label: label.into(),
            method,
            command: display(&action),
            action,
            source: spec.source.clone(),
            high_risk: false,
            note: String::new(),
        }],
        blocked: None,
    }
}

#[cfg(test)]
mod tests;
