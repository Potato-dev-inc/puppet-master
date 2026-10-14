//! Simple mode ("Plain mode" in the design notes): the guards behind the plain face of the cockpit.
//! The face itself is React (`components/simple/`); what MUST hold no matter what the UI does lives
//! here, in Rust:
//!
//! - **A copy, always.** Every Simple task starts through [`run_request`], which sets
//!   `isolation: "worktree"`. There is no argument that turns it off. The agent works in its own
//!   copy of the project; her folder only changes when she presses Keep.
//! - **Broad folders are refused** ([`guard`]): the user profile, Desktop, Documents, Downloads,
//!   OneDrive, a drive root, Windows, Program Files, and any folder that is too big to copy.
//! - **No Git words.** A new project is a new folder under `Documents\PM`, made a repository
//!   quietly so "Go back" (checkpoints) and "Keep" (worktree merge) work. A folder she already has
//!   is only accepted when it already is a repository; copying other folders is future work.
//!
//! The pure parts ([`guard::verdict`], [`plan`], [`run_request`]) are tested without an app.

pub mod guard;
pub mod preview;
#[cfg(test)]
mod tests;

use crate::worktrees::folder;
use guard::{KnownDirs, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What the "Make something" flow sends.
#[derive(Debug, Clone, Deserialize)]
pub struct StartInput {
    /// The project's name (becomes the folder name for a new project).
    pub name: String,
    /// The prompt she saw before pressing Start.
    pub prompt: String,
    pub agent: String,
    /// A folder she already has. `None`: make a new folder.
    #[serde(default)]
    pub existing_folder: Option<String>,
}

/// Why a start was refused. `code` is stable (the UI turns it into plain words); `message` is for logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// The refusal for a broad-folder verdict, or `None` when the folder is fine.
pub fn refusal_for(verdict: Verdict, path: &str) -> Option<Refusal> {
    if verdict.is_ok() {
        return None;
    }
    Some(Refusal::new(
        if verdict == Verdict::Empty { "FOLDER_MISSING" } else { "FOLDER_TOO_BROAD" },
        format!("{path} is not a folder Simple mode works in ({}).", verdict.as_str()),
    ))
}

/// A folder name from what she typed: no characters Windows forbids, no reserved names, short.
pub fn slug(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut out = words.join(" ");
    if out.chars().count() > 40 {
        out = out.chars().take(40).collect::<String>().trim_end().to_string();
    }
    let out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    let upper = out.to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")) && upper.as_bytes()[3].is_ascii_digit());
    if out.is_empty() {
        "My project".to_string()
    } else if reserved {
        format!("{out} project")
    } else {
        out
    }
}

/// `root\name`, or `root\name 2`, `name 3`… when taken.
pub fn unique_folder(root: &Path, name: &str) -> PathBuf {
    let base = slug(name);
    let first = root.join(&base);
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|n| root.join(format!("{base} {n}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| root.join(format!("{base} {}", uuid::Uuid::new_v4().simple())))
}

/// The one place a Simple task request is built. `isolation` is always `worktree`: a copy.
pub fn run_request(origin: &Path, prompt: &str, agent: &str) -> Value {
    json!({
        "task": prompt,
        "project_path": origin.to_string_lossy(),
        "agent_type": agent,
        "headless": false,
        "background": true,
        "keep_pane": true,
        "read_only": false,
        "isolation": "worktree",
        "context_policy": "fresh",
        "wait_ms": 1000,
        // The Simple floor: nothing that skips the agent's own questions.
        "allow_shell": false,
        "trust_workspace": false,
        "allow_broad": false,
    })
}

/// Where the task will run, decided before anything is created.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The folder the copy is made from (and where Keep puts the result).
    pub origin: PathBuf,
    /// `true`: the folder does not exist yet.
    pub is_new: bool,
    pub request: Value,
}

pub fn plan(input: &StartInput, known: &KnownDirs, projects_root: &Path) -> Result<Plan, Refusal> {
    if input.prompt.trim().is_empty() {
        return Err(Refusal::new("PROMPT_EMPTY", "There is nothing to ask the agent."));
    }
    if input.agent.trim().is_empty() {
        return Err(Refusal::new("AGENT_MISSING", "No agent was chosen."));
    }
    let existing = input.existing_folder.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let (origin, is_new) = match existing {
        Some(path) => {
            let path = folder::path_of(path);
            let text = path.to_string_lossy().into_owned();
            if let Some(refusal) = refusal_for(guard::verdict(&text, known), &text) {
                return Err(refusal);
            }
            if !path.is_dir() {
                return Err(Refusal::new("FOLDER_MISSING", format!("{text} is not a folder.")));
            }
            (path, false)
        }
        None => {
            let root_text = projects_root.to_string_lossy();
            if root_text.trim().is_empty() {
                return Err(Refusal::new("FOLDER_MISSING", "There is no place to make a project."));
            }
            (unique_folder(projects_root, &input.name), true)
        }
    };
    // The same check on the final folder: a name can never walk out of the projects folder.
    let origin_text = origin.to_string_lossy().into_owned();
    if let Some(refusal) = refusal_for(guard::verdict(&origin_text, known), &origin_text) {
        return Err(refusal);
    }
    Ok(Plan { request: run_request(&origin, &input.prompt, &input.agent), origin, is_new })
}

/// Make the folder a repository (new project), or check that it already is one. Returns the
/// refusal when the folder is not something PM can copy yet.
pub fn prepare(plan: &Plan) -> Result<(), Refusal> {
    if plan.is_new {
        std::fs::create_dir_all(&plan.origin)
            .map_err(|e| Refusal::new("FOLDER_CREATE_FAILED", format!("could not make {}: {e}", plan.origin.display())))?;
        folder::init(&plan.origin).map_err(|e| Refusal::new(&e.code, e.message))?;
        return Ok(());
    }
    if !guard::small_enough(&plan.origin) {
        return Err(Refusal::new("FOLDER_TOO_BIG", "This folder is too big to copy."));
    }
    match folder::git_state(&plan.origin) {
        "repo" => Ok(()),
        _ => Err(Refusal::new(
            "FOLDER_NEEDS_SETUP",
            "PM cannot copy this folder yet. Only a new project, or a folder that already has saved versions, works.",
        )),
    }
}

/// Where Simple projects are made: `<home>\Documents\PM`. `PM_SIMPLE_PROJECTS_ROOT` moves it (live
/// checks and tests use a scratch folder); it is still checked like every other folder.
pub fn projects_root() -> PathBuf {
    if let Some(root) = std::env::var_os("PM_SIMPLE_PROJECTS_ROOT").filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    PathBuf::from(guard::projects_root_for(&KnownDirs::from_env().home))
}

/// Open `file` with the program the computer picks for it (a page opens in the browser).
pub fn open_in_default_app(file: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(file)
            .creation_flags(0x0800_0000)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("OPEN_FAILED: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(file).spawn().map(|_| ()).map_err(|e| format!("OPEN_FAILED: {e}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(file).spawn().map(|_| ()).map_err(|e| format!("OPEN_FAILED: {e}"))
    }
}
