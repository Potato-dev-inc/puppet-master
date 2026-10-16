//! Resolve (SPEC 4.1): turn the typed command into a program on disk, try `--version`, and open a
//! session. Reuses `declarative::resolve` so the wizard finds a program exactly the way a finished
//! definition will (the same PATH rules, the same npm-shim handling).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

use crate::tui::declarative::resolve::{resolve_in, searchable_dirs, Platform, ResolveError};
use crate::tui::declarative::schema::BinaryTable;

use super::exec::{self, RunSpec};
use super::report::WizardError;
use super::session::{make_work_dir, ProgramKind, Session, State, VersionInfo, Wizard, WizardEnv};

const MAX_CANDIDATES: usize = 20;
const MAX_CANDIDATE_DEPTH: usize = 6;
const MAX_VISITED: usize = 5_000;

/// A bare program name: letters, digits and `. _ - +`, at most 64, not starting with `.`. A trailing
/// `.exe/.cmd/.bat/.ps1/.com` is dropped. Same rule as "Add as generic agent".
pub fn command_name(raw: &str) -> Result<String, WizardError> {
    let command = raw.trim();
    let safe = !command.is_empty()
        && command.len() <= 64
        && command.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
        && !command.starts_with('.');
    if !safe {
        return Err(WizardError::new(
            400,
            "AGENT_DEF_BAD_BINARY",
            "enter just the command name, such as `mycoder` (letters, digits, . _ - +), without a folder or arguments",
        ));
    }
    let stem = command
        .rsplit_once('.')
        .filter(|(_, ext)| ["exe", "cmd", "bat", "ps1", "com"].contains(&ext.to_ascii_lowercase().as_str()))
        .map_or(command, |(s, _)| s);
    if stem.is_empty() {
        return Err(WizardError::bad_request("the command name is empty"));
    }
    Ok(stem.to_string())
}

/// `kiro-cli` becomes `kiro_cli`: a valid `agent.id` (lowercase, starts with a letter, at most 32).
pub fn id_suggestion(command: &str) -> String {
    let mut id: String = command
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !id.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        id.insert(0, 'a');
    }
    id.truncate(32);
    while id.len() < 2 {
        id.push('x');
    }
    id
}

/// A `[binary]` table that finds `command` and nothing else (no shim globs: those are the drafter's job).
pub fn probe_table(command: &str) -> BinaryTable {
    BinaryTable {
        names: vec![command.to_string()],
        windows_extensions: ["exe", "com", "cmd", "bat"].iter().map(|s| s.to_string()).collect(),
        shim_native_globs: Vec::new(),
        allow_cmd_shim: false,
        version_args: vec!["--version".to_string()],
        install_hint: String::new(),
        login_hint: String::new(),
    }
}

#[derive(Debug, Clone)]
pub struct Located {
    pub path: PathBuf,
    pub kind: ProgramKind,
    pub native_candidates: Vec<String>,
}

/// Find `command` in `dirs`. A native program wins; an npm wrapper is returned as a `Shim` with the
/// executables found beside it.
pub fn locate(command: &str, dirs: &[PathBuf]) -> Result<Located, WizardError> {
    match resolve_in(&probe_table(command), dirs, Platform::current()) {
        Ok(path) => Ok(Located { path, kind: ProgramKind::Native, native_candidates: Vec::new() }),
        Err(ResolveError::ShimNeedsNative { shim }) => {
            let candidates = native_candidates(&shim);
            Ok(Located { path: shim, kind: ProgramKind::Shim, native_candidates: candidates })
        }
        Err(e @ ResolveError::NotFound { .. }) => Err(WizardError::new(
            404,
            e.code(),
            format!("`{command}` was not found on PATH. Install it, or start Puppet Master from a terminal where `{command}` works, then try again"),
        )),
    }
}

/// `*.exe` files under `<shim folder>/node_modules` (depth and count limited), relative to the shim
/// folder with `/` separators: what a drafter needs to write `shim_native_globs`.
pub fn native_candidates(shim: &Path) -> Vec<String> {
    let Some(root) = shim.parent() else { return Vec::new() };
    let start = root.join("node_modules");
    let mut found = Vec::new();
    let mut visited = 0usize;
    let mut stack = vec![(start, 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            visited += 1;
            if visited > MAX_VISITED || found.len() >= MAX_CANDIDATES {
                found.sort();
                return found;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth < MAX_CANDIDATE_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if kind.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
                if let Ok(rel) = path.strip_prefix(root) {
                    found.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    found.sort();
    found
}

static VERSION_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+(?:\.\d+)+").expect("version regex"));

/// The first `1.2.3`-looking number in the text.
pub fn version_number(text: &str) -> Option<String> {
    VERSION_NUMBER.find(text).map(|m| m.as_str().to_string())
}

/// "~\AppData\..." for a path under the home folder: what the UI shows and nothing is sent anywhere.
pub fn display_path(path: &Path) -> String {
    match crate::project_path::home_dir() {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
            Err(_) => path.display().to_string(),
        },
        None => path.display().to_string(),
    }
}

fn cost_note(command: &str) -> String {
    format!(
        "The live test runs {command} 3 times with tiny messages (about 3 small messages per try). It uses your own {command} account or plan."
    )
}

/// `POST /agents/wizard/start`.
pub fn start(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let raw = body.get("command").and_then(Value::as_str).unwrap_or("");
    if raw.trim().is_empty() {
        return Err(WizardError::bad_request("command is required"));
    }
    let command = command_name(raw)?;
    let env = wizard.env();
    let dirs = search_dirs(&env);
    let located = locate(&command, &dirs)?;

    let id = Wizard::new_id();
    let work_dir = make_work_dir(&env.temp_root, &id)?;
    let version = read_version(&located.path, &work_dir, &env);
    let session = Session {
        id: id.clone(),
        command: command.clone(),
        id_suggestion: id_suggestion(&command),
        program: located.path.clone(),
        kind: located.kind,
        version: version.clone(),
        native_candidates: located.native_candidates.clone(),
        dirs,
        work_dir: work_dir.clone(),
        state: State::Resolved,
        touched: SystemTime::now(),
        probe: None,
        draft: None,
        samples: Vec::new(),
        tested: None,
        busy: None,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let suggestion = session.id_suggestion.clone();
    if let Err(e) = wizard.insert(session) {
        super::session::remove_work_dir(&work_dir);
        return Err(e);
    }
    let limits = &env.limits;
    Ok(json!({
        "session": id,
        "state": State::Resolved.as_str(),
        "command": command,
        "id_suggestion": suggestion,
        "program": {
            "path": located.path.display().to_string(),
            "display": display_path(&located.path),
            "kind": located.kind.as_str(),
        },
        "version": {"ok": version.ok, "args": ["--version"], "text": version.text, "number": version.number},
        "native_candidates": located.native_candidates,
        "cost_note": cost_note(&command),
        "limits": {
            "probe_timeout_ms": limits.probe_timeout.as_millis() as u64,
            "probe_max_bytes": limits.probe_max_bytes,
            "probe_max_runs": limits.probe_max_runs,
            "draft_max_bytes": crate::tui::declarative::validate::MAX_DEFINITION_BYTES,
        },
    }))
}

/// Directories searched for programs: the environment's, or this process's `PATH` without the
/// current folder.
pub fn search_dirs(env: &WizardEnv) -> Vec<PathBuf> {
    match &env.path_dirs {
        Some(dirs) => dirs.clone(),
        None => {
            let cwd: Vec<PathBuf> = std::env::current_dir().into_iter().collect();
            searchable_dirs(&std::env::var_os("PATH").unwrap_or_default(), &cwd)
        }
    }
}

fn read_version(program: &Path, work_dir: &Path, env: &WizardEnv) -> VersionInfo {
    let args = vec!["--version".to_string()];
    let vars = probe_env();
    let mut spec = RunSpec::new(program, &args, work_dir);
    spec.env = &vars;
    spec.timeout = env.limits.version_timeout;
    spec.max_bytes = 8 * 1024;
    let out = exec::run(&spec);
    let text = exec::clean(&format!("{}\n{}", out.stdout, out.stderr));
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let first = exec::cut_chars(first, 200);
    let ok = out.exit == Some(0) && !first.is_empty();
    VersionInfo { ok, number: version_number(&first), text: first }
}

/// Environment added to every probe: no colour, no terminal tricks.
pub fn probe_env() -> Vec<(String, String)> {
    vec![("NO_COLOR".to_string(), "1".to_string()), ("TERM".to_string(), "dumb".to_string())]
}
