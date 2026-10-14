//! Worktree presets: what Puppet Master does so a separate copy starts READY.
//!
//! The config lives in the project, in `<project>/.puppet-master/project.toml`:
//!
//! ```toml
//! [worktree]
//! setup = [["npm", "ci"]]          # run after the copy is made, in the copy; one argv per step
//! copy  = [".env", ".env.local"]   # copied from the project into the copy, never committed
//! run   = ["npm", "run", "dev"]    # the start command (stored and shown; not run by this module)
//! ```
//!
//! `setup` entries may also be plain strings (`"npm ci"`); they are split into an argv. Nothing
//! runs through a shell: `&&`, pipes and redirects are refused with a plain message, so a setup
//! step can never do more than its argv says.
//!
//! The setup is never silent: every step's output goes to `<worktree>.setup.log` next to the
//! worktree, the result (steps, exit codes, the last lines of a failing step, which files were
//! copied and which were not) is recorded on the [`WorktreeRef`](super::WorktreeRef) and shown
//! in the receipt.
//!
//! Copied files are untracked secrets by definition. They are skipped when the file is tracked
//! (the copy already has the committed version), and otherwise written to the repository's
//! `info/exclude` unless an ignore rule already covers them, so neither `git add -A` by an agent
//! nor Puppet Master's own commit can put them in a branch.
//!
//! Conductor's `conductor.json` (`{"scripts": {"setup": "...", "run": "..."}}`) can be imported.

use super::git::git_raw;
use super::proc::{base_command, kill_tree, tail_lines, ToolEnv};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Relative to the project.
pub const PROJECT_TOML: &str = ".puppet-master/project.toml";
/// How long one setup step may run before it is stopped.
pub const DEFAULT_STEP_TIMEOUT_SECS: u64 = 600;
const MAX_COPY_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_COPY_DIR_FILES: usize = 2000;
const TAIL_LINES: usize = 40;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreePreset {
    #[serde(default)]
    pub setup: Vec<Vec<String>>,
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub run: Vec<String>,
    /// Per-step limit in seconds; the default applies when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_timeout_secs: Option<u64>,
}

impl WorktreePreset {
    pub fn is_empty(&self) -> bool {
        self.setup.is_empty() && self.copy.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupStep {
    /// The command as it would be typed (`npm ci`).
    pub command: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// The last lines of its output; only kept for a step that failed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tail: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotCopied {
    pub path: String,
    /// Plain words: why this file was left out.
    pub reason: String,
}

/// What preparing a separate copy did. Recorded on the worktree and shown in the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupReport {
    /// `ok` or `failed`.
    pub status: String,
    #[serde(default)]
    pub copied: Vec<String>,
    #[serde(default)]
    pub not_copied: Vec<NotCopied>,
    #[serde(default)]
    pub steps: Vec<SetupStep>,
    /// File name of the full output, next to the worktree folder (`<name>.setup.log`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    pub duration_ms: u64,
}

impl SetupReport {
    pub fn failed(&self) -> bool {
        self.status == "failed"
    }

    /// One sentence for the receipt and the spawn view.
    pub fn sentence(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for step in &self.steps {
            parts.push(if step.ok {
                format!("ran {}", step.command)
            } else if step.timed_out {
                format!("{} took too long and was stopped", step.command)
            } else {
                format!("{} stopped with an error", step.command)
            });
        }
        if !self.copied.is_empty() {
            parts.push(format!("copied {} (never committed)", self.copied.join(", ")));
        }
        if parts.is_empty() {
            return "Nothing to set up.".into();
        }
        let joined = parts.join("; ");
        let mut chars = joined.chars();
        let text = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
        if self.failed() {
            format!("Setting up the copy failed: {joined}.")
        } else {
            format!("{text}.")
        }
    }
}

// ---------------------------------------------------------------------------- reading and writing

pub fn toml_path(project: &Path) -> PathBuf {
    project.join(PROJECT_TOML)
}

fn argv_of(value: &toml::Value) -> Result<Vec<String>, String> {
    match value {
        toml::Value::String(text) => parse_command_line(text),
        toml::Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string).ok_or_else(|| "a command must be text".to_string()))
            .collect(),
        _ => Err("a command must be text or a list of words".into()),
    }
}

/// The preset of `project`; an empty one when the file or the `[worktree]` table is missing.
pub fn load(project: &Path) -> Result<WorktreePreset, String> {
    let path = toml_path(project);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(WorktreePreset::default()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn parse(text: &str) -> Result<WorktreePreset, String> {
    let root: toml::Table = text.parse().map_err(|error| format!("not valid TOML: {error}"))?;
    let Some(table) = root.get("worktree").and_then(toml::Value::as_table) else {
        return Ok(WorktreePreset::default());
    };
    let mut preset = WorktreePreset::default();
    if let Some(setup) = table.get("setup") {
        let steps = setup.as_array().ok_or("worktree.setup must be a list")?;
        for step in steps {
            let argv = argv_of(step)?;
            if !argv.is_empty() {
                preset.setup.push(argv);
            }
        }
    }
    if let Some(copy) = table.get("copy") {
        preset.copy = copy
            .as_array()
            .ok_or("worktree.copy must be a list")?
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
            .filter(|item| !item.is_empty())
            .collect();
    }
    if let Some(run) = table.get("run") {
        preset.run = argv_of(run)?;
    }
    preset.setup_timeout_secs = table
        .get("setup_timeout_secs")
        .and_then(toml::Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
        .filter(|n| *n > 0);
    Ok(preset)
}

/// Write `preset` into the `[worktree]` table, keeping every other table of the file.
pub fn save(project: &Path, preset: &WorktreePreset) -> Result<(), String> {
    for item in &preset.copy {
        safe_relative(item).ok_or_else(|| format!("\"{item}\" cannot be copied: use a path inside the project"))?;
    }
    let path = toml_path(project);
    let mut root: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => text.parse().map_err(|error| format!("{} is not valid TOML: {error}", path.display()))?,
        Err(_) => toml::Table::new(),
    };
    let mut table = toml::Table::new();
    let words = |argv: &[String]| toml::Value::Array(argv.iter().cloned().map(toml::Value::String).collect());
    if !preset.setup.is_empty() {
        table.insert("setup".into(), toml::Value::Array(preset.setup.iter().map(|s| words(s)).collect()));
    }
    if !preset.copy.is_empty() {
        table.insert("copy".into(), words(&preset.copy));
    }
    if !preset.run.is_empty() {
        table.insert("run".into(), words(&preset.run));
    }
    if let Some(secs) = preset.setup_timeout_secs {
        table.insert("setup_timeout_secs".into(), toml::Value::Integer(secs as i64));
    }
    if table.is_empty() {
        root.remove("worktree");
    } else {
        root.insert("worktree".into(), toml::Value::Table(table));
    }
    crate::git_exclude::ensure_project_storage_excluded(project);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let text = toml::to_string_pretty(&root).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("could not write {}: {error}", path.display()))
}

// ------------------------------------------------------------------------------------- commands

/// Split a typed command (`npm ci`, `pnpm run "build all"`) into words. Quotes group words; shell
/// features are refused because nothing here runs through a shell.
pub fn parse_command_line(text: &str) -> Result<Vec<String>, String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.trim().chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None => match ch {
                '"' | '\'' => {
                    quote = Some(ch);
                    started = true;
                }
                c if c.is_whitespace() => {
                    if started || !current.is_empty() {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                '&' | '|' | ';' | '<' | '>' | '`' => {
                    return Err(format!(
                        "\"{ch}\" is not supported here: put each command on its own line, without pipes, && or redirects"
                    ))
                }
                '$' if chars.peek() == Some(&'(') => {
                    return Err("$(...) is not supported here: put each command on its own line".into())
                }
                c => current.push(c),
            },
        }
    }
    if quote.is_some() {
        return Err("a quote was opened but never closed".into());
    }
    if started || !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

/// The command as it would be typed.
pub fn display(argv: &[String]) -> String {
    argv.iter()
        .map(|word| if word.is_empty() || word.contains(char::is_whitespace) { format!("\"{word}\"") } else { word.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `conductor.json` in the project, as a preset, when it holds a setup or run script that can run
/// without a shell. `Err` explains what could not be imported.
pub fn read_conductor(project: &Path) -> Option<Result<WorktreePreset, String>> {
    let text = std::fs::read_to_string(project.join("conductor.json")).ok()?;
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(error) => return Some(Err(format!("conductor.json is not valid JSON: {error}"))),
    };
    let scripts = json.get("scripts")?;
    let mut preset = WorktreePreset::default();
    if let Some(setup) = scripts.get("setup").and_then(serde_json::Value::as_str) {
        // `a && b` becomes two steps; anything else a shell would do is refused.
        for part in setup.split("&&") {
            match parse_command_line(part) {
                Ok(argv) if !argv.is_empty() => preset.setup.push(argv),
                Ok(_) => {}
                Err(error) => return Some(Err(format!("conductor.json setup: {error}"))),
            }
        }
    }
    if let Some(run) = scripts.get("run").and_then(serde_json::Value::as_str) {
        match parse_command_line(run) {
            Ok(argv) => preset.run = argv,
            Err(error) => return Some(Err(format!("conductor.json run: {error}"))),
        }
    }
    (!preset.setup.is_empty() || !preset.run.is_empty()).then_some(Ok(preset))
}

// ------------------------------------------------------------------------------------- applying

/// `path` as a clean relative path inside the project, or `None` when it could escape it or touch
/// git's or Puppet Master's own folders.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim().trim_start_matches("./");
    if trimmed.is_empty() {
        return None;
    }
    let candidate = Path::new(trimmed);
    let mut clean = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => {
                let name = part.to_string_lossy();
                if name.eq_ignore_ascii_case(".git") {
                    return None;
                }
                clean.push(part);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    let first = clean.components().next()?.as_os_str().to_string_lossy().into_owned();
    if first.eq_ignore_ascii_case(".puppet-master") {
        return None;
    }
    Some(clean)
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Everything `apply` needs to know about the copy.
pub struct Target<'a> {
    /// The origin project folder (where the files are copied from).
    pub project: &'a Path,
    /// The folder the agent works in, inside the new worktree.
    pub cwd: &'a Path,
    /// `""` or `sub/dir/`: the project's offset inside its repository.
    pub prefix: &'a str,
    /// The repository's shared `info/exclude`.
    pub exclude_file: &'a Path,
    /// Where the full output of the steps goes.
    pub log_file: &'a Path,
    pub env: &'a ToolEnv,
}

fn copy_tree(from: &Path, to: &Path, budget: &mut usize) -> std::io::Result<Vec<PathBuf>> {
    let mut done = Vec::new();
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == ".git" || name == "node_modules" || name == ".puppet-master" {
                continue;
            }
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            done.extend(copy_tree(&entry.path(), &to.join(&name), budget)?);
        }
    } else if *budget > 0 {
        *budget -= 1;
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
        done.push(to.to_path_buf());
    }
    Ok(done)
}

fn is_tracked(cwd: &Path, rel: &str) -> bool {
    git_raw(cwd, &["ls-files", "--error-unmatch", "--", rel], &[]).map(|out| out.ok).unwrap_or(false)
}

fn is_ignored(cwd: &Path, rel: &str) -> bool {
    git_raw(cwd, &["check-ignore", "-q", "--", rel], &[]).map(|out| out.ok).unwrap_or(false)
}

const EXCLUDE_HEADER: &str = "# Puppet Master: files copied into separate copies (never committed)";

fn append_exclude(file: &Path, lines: &[String]) {
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    let missing: Vec<&String> = lines.iter().filter(|line| !existing.lines().any(|l| l.trim() == line.as_str())).collect();
    if missing.is_empty() {
        return;
    }
    let mut next = existing.clone();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !existing.lines().any(|l| l.trim() == EXCLUDE_HEADER) {
        next.push_str(EXCLUDE_HEADER);
        next.push('\n');
    }
    for line in missing {
        next.push_str(line);
        next.push('\n');
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(file, next) {
        tracing::warn!(%error, file = %file.display(), "could not exclude copied files");
    }
}

fn copy_files(target: &Target<'_>, wanted: &[String]) -> (Vec<String>, Vec<NotCopied>) {
    let mut copied = Vec::new();
    let mut not_copied = Vec::new();
    let mut to_exclude: Vec<String> = Vec::new();
    for item in wanted {
        let skip = |reason: &str| NotCopied { path: item.clone(), reason: reason.into() };
        let Some(rel) = safe_relative(item) else {
            not_copied.push(skip("not a path inside the project"));
            continue;
        };
        let rel_text = slashed(&rel);
        let from = target.project.join(&rel);
        let to = target.cwd.join(&rel);
        if !from.exists() {
            not_copied.push(skip("not in the project folder"));
            continue;
        }
        if from.is_file() && std::fs::metadata(&from).map(|m| m.len() > MAX_COPY_FILE_BYTES).unwrap_or(false) {
            not_copied.push(skip("too large to copy"));
            continue;
        }
        if is_tracked(target.cwd, &rel_text) {
            not_copied.push(skip("already in the project's history, so the copy has it"));
            continue;
        }
        let mut budget = MAX_COPY_DIR_FILES;
        match copy_tree(&from, &to, &mut budget) {
            Ok(_) => {
                copied.push(rel_text.clone());
                if !is_ignored(target.cwd, &rel_text) {
                    let pattern = if from.is_dir() { format!("/{}{rel_text}/", target.prefix) } else { format!("/{}{rel_text}", target.prefix) };
                    to_exclude.push(pattern);
                }
            }
            Err(error) => not_copied.push(skip(&format!("could not copy it: {error}"))),
        }
    }
    if !to_exclude.is_empty() {
        append_exclude(target.exclude_file, &to_exclude);
    }
    (copied, not_copied)
}

fn log_tail(path: &Path, from: u64, lines: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(64 * 1024).max(from);
    let _ = file.seek(SeekFrom::Start(start));
    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    tail_lines(&String::from_utf8_lossy(&buf), lines)
}

fn run_step(target: &Target<'_>, argv: &[String], limit: Duration) -> SetupStep {
    use std::io::Write;
    let command = display(argv);
    let started = Instant::now();
    let finish = |ok: bool, code: Option<i32>, timed_out: bool, tail: String| SetupStep {
        command: command.clone(),
        ok,
        exit_code: code,
        timed_out,
        tail,
        duration_ms: started.elapsed().as_millis() as u64,
    };
    let Some(program) = argv.first() else {
        return finish(false, None, false, "the command is empty".into());
    };
    let mut log = match std::fs::OpenOptions::new().create(true).append(true).open(target.log_file) {
        Ok(file) => file,
        Err(error) => return finish(false, None, false, format!("could not open the log: {error}")),
    };
    let from = log.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = writeln!(log, "$ {command}");
    let Some(resolved) = target.env.resolve(program) else {
        let message = format!("{program} was not found on this PC");
        let _ = writeln!(log, "{message}");
        return finish(false, None, false, message);
    };
    let (out, err) = match (log.try_clone(), log.try_clone()) {
        (Ok(out), Ok(err)) => (out, err),
        _ => return finish(false, None, false, "could not write the log".into()),
    };
    let mut child = match base_command(&resolved, &argv[1..], target.cwd, target.env)
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let message = format!("could not start {program}: {error}");
            let _ = writeln!(log, "{message}");
            return finish(false, None, false, message);
        }
    };
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let ok = status.success();
                let _ = writeln!(log, "[exit {}]", status.code().map_or("?".into(), |c| c.to_string()));
                let tail = if ok { String::new() } else { log_tail(target.log_file, from, TAIL_LINES) };
                return finish(ok, status.code(), false, tail);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                kill_tree(&mut child);
                let _ = writeln!(log, "[stopped after {} s]", limit.as_secs());
                return finish(false, None, true, log_tail(target.log_file, from, TAIL_LINES));
            }
            Err(error) => {
                return finish(false, None, false, format!("lost the process: {error}"));
            }
        }
    }
}

/// Copy the files, then run the setup steps one after the other (stopping at the first failure).
/// `None` when the preset asks for nothing.
pub fn apply(target: &Target<'_>, preset: &WorktreePreset) -> Option<SetupReport> {
    if preset.is_empty() {
        return None;
    }
    let started = Instant::now();
    let (copied, not_copied) = copy_files(target, &preset.copy);
    let limit = Duration::from_secs(preset.setup_timeout_secs.unwrap_or(DEFAULT_STEP_TIMEOUT_SECS));
    let mut steps: Vec<SetupStep> = Vec::new();
    let mut status = "ok";
    for argv in &preset.setup {
        let step = run_step(target, argv, limit);
        let ok = step.ok;
        steps.push(step);
        if !ok {
            status = "failed";
            break;
        }
    }
    Some(SetupReport {
        status: status.into(),
        copied,
        not_copied,
        steps,
        log: (!preset.setup.is_empty())
            .then(|| target.log_file.file_name().map(|n| n.to_string_lossy().into_owned()))
            .flatten(),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

// --------------------------------------------------------------------------- the Copies tab

/// The preset as text lines, for the Project view's Copies tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PresetView {
    /// One command per line (`npm ci`).
    pub setup: Vec<String>,
    pub copy: Vec<String>,
    /// The start command (`npm run dev`), or empty.
    pub run: String,
    /// What `conductor.json` in the project would give, when the project has one.
    pub conductor: Option<ConductorView>,
    /// The file exists but could not be read; the reason in plain words.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConductorView {
    pub setup: Vec<String>,
    pub run: String,
    /// Set when its scripts cannot be imported (they use shell features).
    pub unsupported: Option<String>,
}

fn lines_of(preset: &WorktreePreset) -> (Vec<String>, String) {
    (preset.setup.iter().map(|argv| display(argv)).collect(), display(&preset.run))
}

pub fn view(project: &Path) -> PresetView {
    let (preset, problem) = match load(project) {
        Ok(preset) => (preset, None),
        Err(error) => (WorktreePreset::default(), Some(error)),
    };
    let (setup, run) = lines_of(&preset);
    let conductor = match read_conductor(project) {
        Some(Ok(found)) => {
            let (setup, run) = lines_of(&found);
            Some(ConductorView { setup, run, unsupported: None })
        }
        Some(Err(message)) => Some(ConductorView { setup: Vec::new(), run: String::new(), unsupported: Some(message) }),
        None => None,
    };
    PresetView { setup, copy: preset.copy, run, conductor, problem }
}

/// Save what the Copies tab holds. Commands are checked first; nothing is written when one is not
/// something that can run without a shell.
pub fn set_from_text(project: &Path, setup: &[String], copy: &[String], run: &str) -> Result<PresetView, String> {
    let mut preset = load(project).unwrap_or_default();
    preset.setup = Vec::new();
    for line in setup.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        preset.setup.push(parse_command_line(line).map_err(|e| format!("\"{line}\": {e}"))?);
    }
    preset.copy = copy.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
    preset.run = if run.trim().is_empty() { Vec::new() } else { parse_command_line(run).map_err(|e| format!("\"{run}\": {e}"))? };
    save(project, &preset)?;
    Ok(view(project))
}

/// Use the setup and start command of `conductor.json`, keeping the files list.
pub fn import_conductor(project: &Path) -> Result<PresetView, String> {
    let found = read_conductor(project).ok_or("There is no conductor.json in this project.")??;
    let mut preset = load(project).unwrap_or_default();
    if !found.setup.is_empty() {
        preset.setup = found.setup;
    }
    if !found.run.is_empty() {
        preset.run = found.run;
    }
    save(project, &preset)?;
    Ok(view(project))
}
